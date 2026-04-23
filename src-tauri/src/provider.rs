use crate::{
    errors::{ProviderError, ProviderErrorKind},
    models::{ProbeCredentials, QuotaSnapshot, QuotaWindow},
};
use chrono::{DateTime, Duration, Utc};
use reqwest::{
    header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue},
    Client, StatusCode,
};
use serde_json::json;

const DEFAULT_PROBE_URL: &str = "https://chatgpt.com/backend-api/codex/responses";
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

    let target = base_url_override.unwrap_or(DEFAULT_PROBE_URL);
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

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));
    headers.insert(
        "OpenAI-Beta",
        HeaderValue::from_static("responses=experimental"),
    );
    headers.insert("Originator", HeaderValue::from_static("codex_cli_rs"));
    headers.insert("Version", HeaderValue::from_static(CODEX_CLI_VERSION));
    headers.insert("User-Agent", HeaderValue::from_static(CODEX_CLI_USER_AGENT));
    let auth = format_auth_header(credentials)?;
    headers.insert(AUTHORIZATION, auth);

    if let Some(account_id) = credentials.chatgpt_account_id.as_deref() {
        let value = HeaderValue::from_str(account_id)
            .map_err(|_| ProviderError::new(ProviderErrorKind::InvalidResponse, "chatgpt_account_id 非法"))?;
        headers.insert("chatgpt-account-id", value);
    }

    let response = client
        .post(target)
        .headers(headers)
        .json(&payload)
        .send()
        .await
        .map_err(map_request_error)?;

    if response.status() == StatusCode::UNAUTHORIZED || response.status() == StatusCode::FORBIDDEN {
        return Err(ProviderError::new(
            ProviderErrorKind::Unauthorized,
            "认证失败，请检查当前认证信息",
        ));
    }

    let snapshot = parse_snapshot(account_name, response.headers())?;
    Ok(snapshot)
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
    let value = match credentials.auth_mode {
        crate::models::AuthMode::ApiKey | crate::models::AuthMode::SessionToken => {
            format!("Bearer {}", credentials.secret.trim())
        }
        crate::models::AuthMode::Cookie => {
            return HeaderValue::from_str(credentials.secret.trim()).map_err(|_| {
                ProviderError::new(ProviderErrorKind::InvalidResponse, "Cookie 认证值格式非法")
            });
        }
    };

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
