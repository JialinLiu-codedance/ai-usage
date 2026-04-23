use crate::{
    errors::{ProviderError, ProviderErrorKind},
    models::{AuthMode, ProbeCredentials, QuotaSnapshot, QuotaWindow},
};
use chrono::{DateTime, Duration, Utc};
use reqwest::{
    header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, COOKIE, HeaderMap, HeaderValue},
    Client, StatusCode,
};
use serde_json::json;

const DEFAULT_PROBE_URL: &str = "https://chatgpt.com/backend-api/codex/responses";
const DEFAULT_API_URL: &str = "https://api.openai.com/v1/responses";
const CODEX_CLI_VERSION: &str = "0.104.0";
const CODEX_CLI_USER_AGENT: &str = "codex_cli_rs/0.104.0";
const DEFAULT_MODEL: &str = "gpt-5.1-codex";
const DEFAULT_INSTRUCTIONS: &str = "You are Codex. Respond briefly.";

#[derive(Debug, Clone)]
struct RawWindow {
    used_percent: f64,
    window_minutes: Option<u32>,
    reset_after_seconds: Option<i64>,
}

pub async fn fetch_quota(
    account_name: &str,
    base_url_override: Option<&str>,
    credentials: &ProbeCredentials,
) -> Result<QuotaSnapshot, ProviderError> {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|error| ProviderError::new(ProviderErrorKind::Unknown, format!("创建 HTTP 客户端失败: {error}")))?;

    let target = resolve_target_url(base_url_override, &credentials.auth_mode);
    let payload = json!({
        "model": DEFAULT_MODEL,
        "input": [
            {
                "role": "user",
                "content": [
                    {
                        "type": "input_text",
                        "text": "hi"
                    }
                ]
            }
        ],
        "stream": true,
        "store": false,
        "instructions": DEFAULT_INSTRUCTIONS
    });

    let headers = build_headers(credentials, &target)?;

    let mut request = client
        .post(target)
        .headers(headers)
        .json(&payload);

    if uses_chatgpt_internal(credentials) {
        request = request.header("Host", "chatgpt.com");
    }

    let response = request.send().await.map_err(map_request_error)?;

    if let Some(snapshot) = parse_snapshot_if_present(account_name, response.headers())? {
        return Ok(snapshot);
    }

    if response.status() == StatusCode::UNAUTHORIZED || response.status() == StatusCode::FORBIDDEN {
        return Err(ProviderError::new(
            ProviderErrorKind::Unauthorized,
            "认证失败，请检查当前认证信息",
        ));
    }

    if response.status() == StatusCode::TOO_MANY_REQUESTS {
        return Err(ProviderError::new(
            ProviderErrorKind::MissingHeaders,
            "请求被限流，且没有返回可识别的 x-codex-* 响应头",
        ));
    }

    let status = response.status();
    let body = response
        .text()
        .await
        .unwrap_or_else(|_| String::from("无法读取响应体"));

    Err(ProviderError::new(
        ProviderErrorKind::Unknown,
        format!("上游返回 {status}：{}", compact_body(&body)),
    ))
}

pub async fn test_connection(
    account_name: &str,
    base_url_override: Option<&str>,
    credentials: &ProbeCredentials,
) -> Result<String, ProviderError> {
    let snapshot = fetch_quota(account_name, base_url_override, credentials).await?;
    let five = snapshot
        .five_hour
        .as_ref()
        .map(|window| format!("{:.0}%", window.remaining_percent))
        .unwrap_or_else(|| "未知".into());
    let seven = snapshot
        .seven_day
        .as_ref()
        .map(|window| format!("{:.0}%", window.remaining_percent))
        .unwrap_or_else(|| "未知".into());
    Ok(format!("连接成功，5H 剩余 {five}，7D 剩余 {seven}"))
}

fn parse_snapshot(account_name: &str, headers: &HeaderMap) -> Result<QuotaSnapshot, ProviderError> {
    let fetched_at = Utc::now();
    let primary = read_window(headers, "primary")?;
    let secondary = read_window(headers, "secondary")?;

    if primary.is_none() && secondary.is_none() {
        return Err(ProviderError::new(
            ProviderErrorKind::MissingHeaders,
            "接口可达，但没有返回可识别的 x-codex-* 响应头",
        ));
    }

    let (five_hour, seven_day) = normalize_windows(primary, secondary, fetched_at);

    Ok(QuotaSnapshot {
        account_name: account_name.to_string(),
        five_hour,
        seven_day,
        fetched_at,
        source: "probe_headers".into(),
    })
}

fn parse_snapshot_if_present(
    account_name: &str,
    headers: &HeaderMap,
) -> Result<Option<QuotaSnapshot>, ProviderError> {
    let has_any = headers.contains_key("x-codex-primary-used-percent")
        || headers.contains_key("x-codex-primary-window-minutes")
        || headers.contains_key("x-codex-primary-reset-after-seconds")
        || headers.contains_key("x-codex-secondary-used-percent")
        || headers.contains_key("x-codex-secondary-window-minutes")
        || headers.contains_key("x-codex-secondary-reset-after-seconds");

    if !has_any {
        return Ok(None);
    }

    parse_snapshot(account_name, headers).map(Some)
}

fn read_window(headers: &HeaderMap, prefix: &str) -> Result<Option<RawWindow>, ProviderError> {
    let used_key = format!("x-codex-{prefix}-used-percent");
    let reset_key = format!("x-codex-{prefix}-reset-after-seconds");
    let window_key = format!("x-codex-{prefix}-window-minutes");

    let used_percent = get_header_f64(headers, &used_key)?;
    let reset_after_seconds = get_header_i64(headers, &reset_key)?;
    let window_minutes = get_header_u32(headers, &window_key)?;

    if used_percent.is_none() && reset_after_seconds.is_none() && window_minutes.is_none() {
        return Ok(None);
    }

    Ok(Some(RawWindow {
        used_percent: used_percent.unwrap_or(0.0).clamp(0.0, 100.0),
        window_minutes,
        reset_after_seconds,
    }))
}

fn normalize_windows(
    primary: Option<RawWindow>,
    secondary: Option<RawWindow>,
    fetched_at: DateTime<Utc>,
) -> (Option<QuotaWindow>, Option<QuotaWindow>) {
    match (primary, secondary) {
        (None, None) => (None, None),
        (Some(primary), None) => classify_single(primary, fetched_at),
        (None, Some(secondary)) => classify_single(secondary, fetched_at),
        (Some(primary), Some(secondary)) => {
            let primary_minutes = primary.window_minutes.unwrap_or(10080);
            let secondary_minutes = secondary.window_minutes.unwrap_or(300);
            if primary_minutes <= secondary_minutes {
                (Some(to_window(primary, fetched_at)), Some(to_window(secondary, fetched_at)))
            } else {
                (Some(to_window(secondary, fetched_at)), Some(to_window(primary, fetched_at)))
            }
        }
    }
}

fn classify_single(window: RawWindow, fetched_at: DateTime<Utc>) -> (Option<QuotaWindow>, Option<QuotaWindow>) {
    match window.window_minutes {
        Some(minutes) if minutes <= 360 => (Some(to_window(window, fetched_at)), None),
        Some(_) => (None, Some(to_window(window, fetched_at))),
        None => (None, Some(to_window(window, fetched_at))),
    }
}

fn to_window(raw: RawWindow, fetched_at: DateTime<Utc>) -> QuotaWindow {
    let reset_at = raw
        .reset_after_seconds
        .map(|seconds| fetched_at + Duration::seconds(seconds.max(0)));

    if let Some(reset_at) = reset_at {
        if reset_at <= fetched_at {
            return QuotaWindow {
                used_percent: 0.0,
                remaining_percent: 100.0,
                reset_at: Some(reset_at),
                window_minutes: raw.window_minutes,
            };
        }
    }

    QuotaWindow {
        used_percent: raw.used_percent,
        remaining_percent: (100.0 - raw.used_percent).clamp(0.0, 100.0),
        reset_at,
        window_minutes: raw.window_minutes,
    }
}

fn get_header_f64(headers: &HeaderMap, key: &str) -> Result<Option<f64>, ProviderError> {
    let Some(value) = headers.get(key) else {
        return Ok(None);
    };

    let parsed = value
        .to_str()
        .map_err(|_| ProviderError::new(ProviderErrorKind::InvalidResponse, format!("{key} 不是合法文本")))?
        .parse::<f64>()
        .map_err(|_| ProviderError::new(ProviderErrorKind::InvalidResponse, format!("{key} 不是合法数字")))?;

    Ok(Some(parsed))
}

fn get_header_i64(headers: &HeaderMap, key: &str) -> Result<Option<i64>, ProviderError> {
    let Some(value) = headers.get(key) else {
        return Ok(None);
    };

    let parsed = value
        .to_str()
        .map_err(|_| ProviderError::new(ProviderErrorKind::InvalidResponse, format!("{key} 不是合法文本")))?
        .parse::<i64>()
        .map_err(|_| ProviderError::new(ProviderErrorKind::InvalidResponse, format!("{key} 不是合法整数")))?;

    Ok(Some(parsed))
}

fn get_header_u32(headers: &HeaderMap, key: &str) -> Result<Option<u32>, ProviderError> {
    let Some(value) = headers.get(key) else {
        return Ok(None);
    };

    let parsed = value
        .to_str()
        .map_err(|_| ProviderError::new(ProviderErrorKind::InvalidResponse, format!("{key} 不是合法文本")))?
        .parse::<u32>()
        .map_err(|_| ProviderError::new(ProviderErrorKind::InvalidResponse, format!("{key} 不是合法整数")))?;

    Ok(Some(parsed))
}

fn format_auth_header(credentials: &ProbeCredentials) -> Result<HeaderValue, ProviderError> {
    let value = format!("Bearer {}", credentials.secret.trim());
    HeaderValue::from_str(&value).map_err(|_| {
        ProviderError::new(
            ProviderErrorKind::InvalidResponse,
            "认证头构造失败，请检查认证值格式",
        )
    })
}

fn map_request_error(error: reqwest::Error) -> ProviderError {
    if error.is_timeout() {
        return ProviderError::new(ProviderErrorKind::Timeout, "网络连接超时，请稍后重试");
    }

    if error.is_connect() || error.is_request() {
        return ProviderError::new(ProviderErrorKind::Network, format!("网络请求失败: {error}"));
    }

    ProviderError::new(ProviderErrorKind::Unknown, format!("请求失败: {error}"))
}

fn build_headers(credentials: &ProbeCredentials, target_url: &str) -> Result<HeaderMap, ProviderError> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert("User-Agent", HeaderValue::from_static(CODEX_CLI_USER_AGENT));

    match credentials.auth_mode {
        AuthMode::ApiKey => {
            headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
            headers.insert(AUTHORIZATION, format_auth_header(credentials)?);
        }
        AuthMode::SessionToken => {
            headers.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));
            headers.insert(
                "OpenAI-Beta",
                HeaderValue::from_static("responses=experimental"),
            );
            headers.insert("Originator", HeaderValue::from_static("codex_cli_rs"));
            headers.insert("Version", HeaderValue::from_static(CODEX_CLI_VERSION));
            headers.insert(AUTHORIZATION, format_auth_header(credentials)?);
        }
        AuthMode::Cookie => {
            headers.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));
            headers.insert(
                "OpenAI-Beta",
                HeaderValue::from_static("responses=experimental"),
            );
            headers.insert("Originator", HeaderValue::from_static("codex_cli_rs"));
            headers.insert("Version", HeaderValue::from_static(CODEX_CLI_VERSION));
            headers.insert(
                COOKIE,
                HeaderValue::from_str(credentials.secret.trim()).map_err(|_| {
                    ProviderError::new(ProviderErrorKind::InvalidResponse, "Cookie 认证值格式非法")
                })?,
            );
        }
    }

    if uses_chatgpt_internal(credentials) {
        if let Some(account_id) = credentials.chatgpt_account_id.as_deref() {
            let value = HeaderValue::from_str(account_id).map_err(|_| {
                ProviderError::new(ProviderErrorKind::InvalidResponse, "chatgpt_account_id 非法")
            })?;
            headers.insert("chatgpt-account-id", value);
        }
    } else if target_url.contains("chatgpt.com") {
        headers.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));
    }

    Ok(headers)
}

fn uses_chatgpt_internal(credentials: &ProbeCredentials) -> bool {
    !matches!(credentials.auth_mode, AuthMode::ApiKey)
}

fn resolve_target_url(base_url_override: Option<&str>, auth_mode: &AuthMode) -> String {
    if let Some(raw) = base_url_override.map(str::trim).filter(|value| !value.is_empty()) {
        if raw.ends_with("/responses") {
            return raw.to_string();
        }
        if raw.contains("/backend-api/codex") || raw.contains("/v1") {
            return raw.to_string();
        }
        return match auth_mode {
            AuthMode::ApiKey => format!("{}/v1/responses", raw.trim_end_matches('/')),
            AuthMode::SessionToken | AuthMode::Cookie => {
                format!("{}/backend-api/codex/responses", raw.trim_end_matches('/'))
            }
        };
    }

    match auth_mode {
        AuthMode::ApiKey => DEFAULT_API_URL.to_string(),
        AuthMode::SessionToken | AuthMode::Cookie => DEFAULT_PROBE_URL.to_string(),
    }
}

fn compact_body(body: &str) -> String {
    let normalized = body.split_whitespace().collect::<Vec<_>>().join(" ");
    normalized.chars().take(200).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderName;

    fn headers(entries: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (key, value) in entries {
            map.insert(
                HeaderName::from_bytes(key.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        map
    }

    #[test]
    fn resolves_default_url_by_auth_mode() {
        assert_eq!(resolve_target_url(None, &AuthMode::ApiKey), DEFAULT_API_URL);
        assert_eq!(resolve_target_url(None, &AuthMode::SessionToken), DEFAULT_PROBE_URL);
        assert_eq!(resolve_target_url(None, &AuthMode::Cookie), DEFAULT_PROBE_URL);
    }

    #[test]
    fn normalizes_windows_by_minutes() {
        let map = headers(&[
            ("x-codex-primary-used-percent", "70"),
            ("x-codex-primary-window-minutes", "10080"),
            ("x-codex-primary-reset-after-seconds", "3600"),
            ("x-codex-secondary-used-percent", "25"),
            ("x-codex-secondary-window-minutes", "300"),
            ("x-codex-secondary-reset-after-seconds", "1800"),
        ]);

        let snapshot = parse_snapshot("acc", &map).unwrap();
        assert_eq!(
            snapshot.five_hour.as_ref().unwrap().remaining_percent.round() as i64,
            75
        );
        assert_eq!(
            snapshot.seven_day.as_ref().unwrap().remaining_percent.round() as i64,
            30
        );
    }

    #[test]
    fn returns_none_when_no_quota_headers() {
        let map = HeaderMap::new();
        assert!(parse_snapshot_if_present("acc", &map).unwrap().is_none());
    }

    #[test]
    fn cookie_mode_uses_cookie_header() {
        let credentials = ProbeCredentials {
            auth_mode: AuthMode::Cookie,
            secret: "foo=bar".into(),
            chatgpt_account_id: Some("acct".into()),
        };
        let headers = build_headers(&credentials, DEFAULT_PROBE_URL).unwrap();
        assert_eq!(headers.get(COOKIE).unwrap(), "foo=bar");
        assert!(headers.get(AUTHORIZATION).is_none());
        assert_eq!(headers.get("chatgpt-account-id").unwrap(), "acct");
    }
}
