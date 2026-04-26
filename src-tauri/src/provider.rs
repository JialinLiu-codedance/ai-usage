use crate::{
    errors::{ProviderError, ProviderErrorKind},
    models::{
        AuthMode, ProbeCredentials, QuotaSnapshot, QuotaWindow, PROVIDER_ANTHROPIC, PROVIDER_KIMI,
        PROVIDER_MINIMAX, PROVIDER_OPENAI,
    },
};
use chrono::{DateTime, Duration, TimeZone, Utc};
use reqwest::{
    header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE, COOKIE},
    Client, StatusCode,
};
use serde::{de::Error as _, Deserialize, Deserializer};
use serde_json::json;

const DEFAULT_PROBE_URL: &str = "https://chatgpt.com/backend-api/codex/responses";
const DEFAULT_API_URL: &str = "https://api.openai.com/v1/responses";
const DEFAULT_ANTHROPIC_USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const DEFAULT_KIMI_USAGE_URL: &str = "https://api.kimi.com/coding/v1/usages";
const MINIMAX_GLOBAL_USAGE_URLS: [&str; 3] = [
    "https://api.minimax.io/v1/api/openplatform/coding_plan/remains",
    "https://api.minimax.io/v1/coding_plan/remains",
    "https://www.minimax.io/v1/api/openplatform/coding_plan/remains",
];
const MINIMAX_CN_USAGE_URLS: [&str; 2] = [
    "https://api.minimaxi.com/v1/api/openplatform/coding_plan/remains",
    "https://api.minimaxi.com/v1/coding_plan/remains",
];
const ANTHROPIC_OAUTH_USAGE_BETA: &str = "oauth-2025-04-20";
const CODEX_CLI_VERSION: &str = "0.104.0";
const CODEX_CLI_USER_AGENT: &str = "codex_cli_rs/0.104.0";
const DEFAULT_MODEL: &str = "gpt-5.3-codex";
const DEFAULT_INSTRUCTIONS: &str = "You are Codex. Respond briefly.";
const MINIMAX_CODING_PLAN_WINDOW_MINUTES: u32 = 300;
const MINIMAX_CODING_PLAN_WINDOW_MS: f64 = 5.0 * 60.0 * 60.0 * 1000.0;
const MINIMAX_CODING_PLAN_WINDOW_TOLERANCE_MS: f64 = 10.0 * 60.0 * 1000.0;

#[derive(Debug, Clone)]
struct RawWindow {
    used_percent: f64,
    window_minutes: Option<u32>,
    reset_after_seconds: Option<i64>,
}

#[derive(Debug, Clone)]
struct KimiQuota {
    used: f64,
    limit: f64,
    reset_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
struct KimiLimitCandidate {
    quota: KimiQuota,
    window_minutes: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MiniMaxEndpoint {
    Global,
    Cn,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsageResponse {
    five_hour: Option<AnthropicUsageWindow>,
    seven_day: Option<AnthropicUsageWindow>,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsageWindow {
    #[serde(alias = "used_percentage")]
    utilization: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_optional_reset_at")]
    resets_at: Option<DateTime<Utc>>,
}

fn deserialize_optional_reset_at<'de, D>(deserializer: D) -> Result<Option<DateTime<Utc>>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    let Some(value) = value else {
        return Ok(None);
    };

    match value {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::String(raw) => {
            let raw = raw.trim();
            if raw.is_empty() {
                return Ok(None);
            }
            DateTime::parse_from_rfc3339(raw)
                .map(|time| Some(time.with_timezone(&Utc)))
                .map_err(D::Error::custom)
        }
        serde_json::Value::Number(raw) => {
            let seconds = raw
                .as_i64()
                .or_else(|| raw.as_f64().map(|value| value as i64))
                .ok_or_else(|| D::Error::custom("reset timestamp is not a valid number"))?;
            Utc.timestamp_opt(seconds, 0)
                .single()
                .ok_or_else(|| D::Error::custom("reset timestamp is out of range"))
                .map(Some)
        }
        _ => Err(D::Error::custom(
            "reset timestamp must be an RFC3339 string or epoch seconds",
        )),
    }
}

pub async fn fetch_quota(
    account_id: &str,
    account_name: &str,
    base_url_override: Option<&str>,
    credentials: &ProbeCredentials,
) -> Result<QuotaSnapshot, ProviderError> {
    match credentials.provider.as_str() {
        PROVIDER_OPENAI => {
            fetch_openai_quota(account_id, account_name, base_url_override, credentials).await
        }
        PROVIDER_ANTHROPIC => {
            fetch_anthropic_quota(account_id, account_name, base_url_override, credentials).await
        }
        PROVIDER_KIMI => {
            fetch_kimi_quota(account_id, account_name, base_url_override, credentials).await
        }
        PROVIDER_MINIMAX => {
            fetch_minimax_quota(account_id, account_name, base_url_override, credentials).await
        }
        provider => Err(ProviderError::new(
            ProviderErrorKind::Unknown,
            format!(
                "{} 账号已绑定，但当前版本暂未实现额度刷新",
                provider_display_label(provider)
            ),
        )),
    }
}

async fn fetch_openai_quota(
    account_id: &str,
    account_name: &str,
    base_url_override: Option<&str>,
    credentials: &ProbeCredentials,
) -> Result<QuotaSnapshot, ProviderError> {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|error| {
            ProviderError::new(
                ProviderErrorKind::Unknown,
                format!("创建 HTTP 客户端失败: {error}"),
            )
        })?;

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

    let mut request = client.post(target).headers(headers).json(&payload);

    if uses_chatgpt_internal(credentials) {
        request = request.header("Host", "chatgpt.com");
    }

    let response = request.send().await.map_err(map_request_error)?;

    if let Some(snapshot) = parse_snapshot_if_present(account_id, account_name, response.headers())?
    {
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

async fn fetch_anthropic_quota(
    account_id: &str,
    account_name: &str,
    base_url_override: Option<&str>,
    credentials: &ProbeCredentials,
) -> Result<QuotaSnapshot, ProviderError> {
    if !matches!(credentials.auth_mode, AuthMode::OAuth) {
        return Err(ProviderError::new(
            ProviderErrorKind::Unknown,
            "Anthropic 额度刷新当前仅支持 OAuth 账号",
        ));
    }

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|error| {
            ProviderError::new(
                ProviderErrorKind::Unknown,
                format!("创建 HTTP 客户端失败: {error}"),
            )
        })?;

    let response = client
        .get(resolve_anthropic_usage_url(base_url_override))
        .headers(build_anthropic_usage_headers(credentials)?)
        .send()
        .await
        .map_err(map_request_error)?;

    if response.status() == StatusCode::UNAUTHORIZED || response.status() == StatusCode::FORBIDDEN {
        return Err(ProviderError::new(
            ProviderErrorKind::Unauthorized,
            "Anthropic OAuth 认证失败，请重新授权",
        ));
    }

    if response.status() == StatusCode::TOO_MANY_REQUESTS {
        return Err(ProviderError::new(
            ProviderErrorKind::Unknown,
            "Anthropic 使用量接口被限流，请稍后重试",
        ));
    }

    if !response.status().is_success() {
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| String::from("无法读取响应体"));
        return Err(ProviderError::new(
            ProviderErrorKind::Unknown,
            format!("Anthropic 使用量接口返回 {status}：{}", compact_body(&body)),
        ));
    }

    let body = response.text().await.map_err(|error| {
        ProviderError::new(
            ProviderErrorKind::InvalidResponse,
            format!("读取 Anthropic 使用量响应失败: {error}"),
        )
    })?;
    parse_anthropic_usage_snapshot(account_id, account_name, &body)
}

async fn fetch_kimi_quota(
    account_id: &str,
    account_name: &str,
    base_url_override: Option<&str>,
    credentials: &ProbeCredentials,
) -> Result<QuotaSnapshot, ProviderError> {
    if !matches!(credentials.auth_mode, AuthMode::OAuth) {
        return Err(ProviderError::new(
            ProviderErrorKind::Unknown,
            "Kimi 额度刷新当前仅支持从 Kimi CLI 导入的 OAuth 账号",
        ));
    }

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|error| {
            ProviderError::new(
                ProviderErrorKind::Unknown,
                format!("创建 HTTP 客户端失败: {error}"),
            )
        })?;

    let response = client
        .get(resolve_kimi_usage_url(base_url_override))
        .headers(build_kimi_usage_headers(credentials)?)
        .send()
        .await
        .map_err(map_request_error)?;

    if response.status() == StatusCode::UNAUTHORIZED || response.status() == StatusCode::FORBIDDEN {
        return Err(ProviderError::new(
            ProviderErrorKind::Unauthorized,
            "Kimi OAuth 认证失败，请重新导入 Kimi CLI 登录态",
        ));
    }

    if response.status() == StatusCode::TOO_MANY_REQUESTS {
        return Err(ProviderError::new(
            ProviderErrorKind::Unknown,
            "Kimi 使用量接口被限流，请稍后重试",
        ));
    }

    if !response.status().is_success() {
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| String::from("无法读取响应体"));
        return Err(ProviderError::new(
            ProviderErrorKind::Unknown,
            format!("Kimi 使用量接口返回 {status}：{}", compact_body(&body)),
        ));
    }

    let body = response.text().await.map_err(|error| {
        ProviderError::new(
            ProviderErrorKind::InvalidResponse,
            format!("读取 Kimi 使用量响应失败: {error}"),
        )
    })?;
    parse_kimi_usage_snapshot(account_id, account_name, &body)
}

async fn fetch_minimax_quota(
    account_id: &str,
    account_name: &str,
    base_url_override: Option<&str>,
    credentials: &ProbeCredentials,
) -> Result<QuotaSnapshot, ProviderError> {
    if !matches!(credentials.auth_mode, AuthMode::ApiKey) {
        return Err(ProviderError::new(
            ProviderErrorKind::Unknown,
            "MiniMax 额度刷新当前仅支持 API Key 账号",
        ));
    }

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|error| {
            ProviderError::new(
                ProviderErrorKind::Unknown,
                format!("创建 HTTP 客户端失败: {error}"),
            )
        })?;

    let mut first_error = None;
    for endpoint in [MiniMaxEndpoint::Global, MiniMaxEndpoint::Cn] {
        for url in minimax_usage_urls(base_url_override, endpoint) {
            match request_minimax_usage(&client, &url, credentials).await {
                Ok(body) => {
                    match parse_minimax_usage_snapshot(account_id, account_name, &body, endpoint) {
                        Ok(snapshot) => return Ok(snapshot),
                        Err(error) => {
                            if first_error.is_none() {
                                first_error = Some(error);
                            }
                        }
                    }
                }
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }
    }

    Err(first_error.unwrap_or_else(|| {
        ProviderError::new(
            ProviderErrorKind::MissingHeaders,
            "MiniMax 使用量接口没有返回可识别的额度窗口",
        )
    }))
}

fn provider_display_label(provider: &str) -> &'static str {
    match provider {
        PROVIDER_ANTHROPIC => "Anthropic",
        PROVIDER_KIMI => "Kimi",
        PROVIDER_MINIMAX => "MiniMax",
        _ => "OpenAI",
    }
}

pub async fn test_connection(
    account_id: &str,
    account_name: &str,
    base_url_override: Option<&str>,
    credentials: &ProbeCredentials,
) -> Result<String, ProviderError> {
    let snapshot = fetch_quota(account_id, account_name, base_url_override, credentials).await?;
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

fn parse_snapshot(
    account_id: &str,
    account_name: &str,
    headers: &HeaderMap,
) -> Result<QuotaSnapshot, ProviderError> {
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
        account_id: account_id.to_string(),
        account_name: account_name.to_string(),
        five_hour,
        seven_day,
        fetched_at,
        source: "probe_headers".into(),
    })
}

fn parse_snapshot_if_present(
    account_id: &str,
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

    parse_snapshot(account_id, account_name, headers).map(Some)
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
                (
                    Some(to_window(primary, fetched_at)),
                    Some(to_window(secondary, fetched_at)),
                )
            } else {
                (
                    Some(to_window(secondary, fetched_at)),
                    Some(to_window(primary, fetched_at)),
                )
            }
        }
    }
}

fn classify_single(
    window: RawWindow,
    fetched_at: DateTime<Utc>,
) -> (Option<QuotaWindow>, Option<QuotaWindow>) {
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
        .map_err(|_| {
            ProviderError::new(
                ProviderErrorKind::InvalidResponse,
                format!("{key} 不是合法文本"),
            )
        })?
        .parse::<f64>()
        .map_err(|_| {
            ProviderError::new(
                ProviderErrorKind::InvalidResponse,
                format!("{key} 不是合法数字"),
            )
        })?;

    Ok(Some(parsed))
}

fn get_header_i64(headers: &HeaderMap, key: &str) -> Result<Option<i64>, ProviderError> {
    let Some(value) = headers.get(key) else {
        return Ok(None);
    };

    let parsed = value
        .to_str()
        .map_err(|_| {
            ProviderError::new(
                ProviderErrorKind::InvalidResponse,
                format!("{key} 不是合法文本"),
            )
        })?
        .parse::<i64>()
        .map_err(|_| {
            ProviderError::new(
                ProviderErrorKind::InvalidResponse,
                format!("{key} 不是合法整数"),
            )
        })?;

    Ok(Some(parsed))
}

fn get_header_u32(headers: &HeaderMap, key: &str) -> Result<Option<u32>, ProviderError> {
    let Some(value) = headers.get(key) else {
        return Ok(None);
    };

    let parsed = value
        .to_str()
        .map_err(|_| {
            ProviderError::new(
                ProviderErrorKind::InvalidResponse,
                format!("{key} 不是合法文本"),
            )
        })?
        .parse::<u32>()
        .map_err(|_| {
            ProviderError::new(
                ProviderErrorKind::InvalidResponse,
                format!("{key} 不是合法整数"),
            )
        })?;

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

fn build_headers(
    credentials: &ProbeCredentials,
    target_url: &str,
) -> Result<HeaderMap, ProviderError> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert("User-Agent", HeaderValue::from_static(CODEX_CLI_USER_AGENT));

    match credentials.auth_mode {
        AuthMode::ApiKey => {
            headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
            headers.insert(AUTHORIZATION, format_auth_header(credentials)?);
        }
        AuthMode::OAuth | AuthMode::SessionToken => {
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
                ProviderError::new(
                    ProviderErrorKind::InvalidResponse,
                    "chatgpt_account_id 非法",
                )
            })?;
            headers.insert("chatgpt-account-id", value);
        }
    } else if target_url.contains("chatgpt.com") {
        headers.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));
    }

    Ok(headers)
}

fn build_anthropic_usage_headers(
    credentials: &ProbeCredentials,
) -> Result<HeaderMap, ProviderError> {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert("User-Agent", HeaderValue::from_static(CODEX_CLI_USER_AGENT));
    headers.insert(
        "anthropic-beta",
        HeaderValue::from_static(ANTHROPIC_OAUTH_USAGE_BETA),
    );
    headers.insert(AUTHORIZATION, format_auth_header(credentials)?);
    Ok(headers)
}

fn build_kimi_usage_headers(credentials: &ProbeCredentials) -> Result<HeaderMap, ProviderError> {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    headers.insert("User-Agent", HeaderValue::from_static("ai-usage"));
    headers.insert(AUTHORIZATION, format_auth_header(credentials)?);
    Ok(headers)
}

fn build_minimax_usage_headers(credentials: &ProbeCredentials) -> Result<HeaderMap, ProviderError> {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert("User-Agent", HeaderValue::from_static("ai-usage"));
    headers.insert(AUTHORIZATION, format_auth_header(credentials)?);
    Ok(headers)
}

fn uses_chatgpt_internal(credentials: &ProbeCredentials) -> bool {
    !matches!(credentials.auth_mode, AuthMode::ApiKey)
}

fn resolve_target_url(base_url_override: Option<&str>, auth_mode: &AuthMode) -> String {
    if let Some(raw) = base_url_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if raw.ends_with("/responses") {
            return raw.to_string();
        }
        if raw.contains("/backend-api/codex") || raw.contains("/v1") {
            return raw.to_string();
        }
        return match auth_mode {
            AuthMode::ApiKey => format!("{}/v1/responses", raw.trim_end_matches('/')),
            AuthMode::OAuth | AuthMode::SessionToken | AuthMode::Cookie => {
                format!("{}/backend-api/codex/responses", raw.trim_end_matches('/'))
            }
        };
    }

    match auth_mode {
        AuthMode::ApiKey => DEFAULT_API_URL.to_string(),
        AuthMode::OAuth | AuthMode::SessionToken | AuthMode::Cookie => {
            DEFAULT_PROBE_URL.to_string()
        }
    }
}

fn resolve_anthropic_usage_url(base_url_override: Option<&str>) -> String {
    let Some(raw) = base_url_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return DEFAULT_ANTHROPIC_USAGE_URL.to_string();
    };

    if raw.ends_with("/api/oauth/usage") {
        raw.to_string()
    } else {
        format!("{}/api/oauth/usage", raw.trim_end_matches('/'))
    }
}

fn resolve_kimi_usage_url(base_url_override: Option<&str>) -> String {
    let Some(raw) = base_url_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return DEFAULT_KIMI_USAGE_URL.to_string();
    };

    if raw.ends_with("/usages") {
        raw.to_string()
    } else if raw.ends_with("/coding/v1") {
        format!("{raw}/usages")
    } else {
        format!("{}/coding/v1/usages", raw.trim_end_matches('/'))
    }
}

fn minimax_usage_urls(base_url_override: Option<&str>, endpoint: MiniMaxEndpoint) -> Vec<String> {
    if let Some(raw) = base_url_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let url = if raw.ends_with("/coding_plan/remains") {
            raw.to_string()
        } else if raw.contains("/v1/") {
            raw.to_string()
        } else {
            format!(
                "{}/v1/api/openplatform/coding_plan/remains",
                raw.trim_end_matches('/')
            )
        };
        return vec![url];
    }

    match endpoint {
        MiniMaxEndpoint::Global => MINIMAX_GLOBAL_USAGE_URLS
            .iter()
            .map(|url| (*url).to_string())
            .collect(),
        MiniMaxEndpoint::Cn => MINIMAX_CN_USAGE_URLS
            .iter()
            .map(|url| (*url).to_string())
            .collect(),
    }
}

async fn request_minimax_usage(
    client: &Client,
    url: &str,
    credentials: &ProbeCredentials,
) -> Result<String, ProviderError> {
    let response = client
        .get(url)
        .headers(build_minimax_usage_headers(credentials)?)
        .send()
        .await
        .map_err(map_request_error)?;

    if response.status() == StatusCode::UNAUTHORIZED || response.status() == StatusCode::FORBIDDEN {
        return Err(ProviderError::new(
            ProviderErrorKind::Unauthorized,
            "MiniMax API Key 认证失败，请检查后重新保存",
        ));
    }

    if response.status() == StatusCode::TOO_MANY_REQUESTS {
        return Err(ProviderError::new(
            ProviderErrorKind::Unknown,
            "MiniMax 使用量接口被限流，请稍后重试",
        ));
    }

    if !response.status().is_success() {
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| String::from("无法读取响应体"));
        return Err(ProviderError::new(
            ProviderErrorKind::Unknown,
            format!("MiniMax 使用量接口返回 {status}：{}", compact_body(&body)),
        ));
    }

    response.text().await.map_err(|error| {
        ProviderError::new(
            ProviderErrorKind::InvalidResponse,
            format!("读取 MiniMax 使用量响应失败: {error}"),
        )
    })
}

fn parse_anthropic_usage_snapshot(
    account_id: &str,
    account_name: &str,
    body: &str,
) -> Result<QuotaSnapshot, ProviderError> {
    let response = serde_json::from_str::<AnthropicUsageResponse>(body).map_err(|error| {
        ProviderError::new(
            ProviderErrorKind::InvalidResponse,
            format!("解析 Anthropic 使用量响应失败: {error}"),
        )
    })?;

    let five_hour = anthropic_usage_window(response.five_hour, 300);
    let seven_day = anthropic_usage_window(response.seven_day, 10080);
    if five_hour.is_none() && seven_day.is_none() {
        return Err(ProviderError::new(
            ProviderErrorKind::MissingHeaders,
            "Anthropic 使用量接口没有返回可识别的额度窗口",
        ));
    }

    Ok(QuotaSnapshot {
        account_id: account_id.to_string(),
        account_name: account_name.to_string(),
        five_hour,
        seven_day,
        fetched_at: Utc::now(),
        source: "anthropic_oauth_usage".into(),
    })
}

fn anthropic_usage_window(
    raw: Option<AnthropicUsageWindow>,
    window_minutes: u32,
) -> Option<QuotaWindow> {
    let raw = raw?;
    let used_percent = raw.utilization?.clamp(0.0, 100.0);
    Some(QuotaWindow {
        used_percent,
        remaining_percent: (100.0 - used_percent).clamp(0.0, 100.0),
        reset_at: raw.resets_at,
        window_minutes: Some(window_minutes),
    })
}

fn parse_kimi_usage_snapshot(
    account_id: &str,
    account_name: &str,
    body: &str,
) -> Result<QuotaSnapshot, ProviderError> {
    let data = serde_json::from_str::<serde_json::Value>(body).map_err(|error| {
        ProviderError::new(
            ProviderErrorKind::InvalidResponse,
            format!("解析 Kimi 使用量响应失败: {error}"),
        )
    })?;

    let candidates = kimi_limit_candidates(&data);
    let session_index = pick_kimi_session_index(&candidates);
    let five_hour = session_index
        .and_then(|index| candidates.get(index))
        .map(|candidate| kimi_quota_to_window(&candidate.quota, candidate.window_minutes));

    let seven_day = data
        .get("usage")
        .and_then(parse_kimi_quota)
        .map(|quota| kimi_quota_to_window(&quota, Some(10080)))
        .or_else(|| {
            pick_kimi_weekly_index(&candidates, session_index)
                .and_then(|index| candidates.get(index))
                .map(|candidate| kimi_quota_to_window(&candidate.quota, candidate.window_minutes))
        });

    if five_hour.is_none() && seven_day.is_none() {
        return Err(ProviderError::new(
            ProviderErrorKind::MissingHeaders,
            "Kimi 使用量接口没有返回可识别的额度窗口",
        ));
    }

    Ok(QuotaSnapshot {
        account_id: account_id.to_string(),
        account_name: account_name.to_string(),
        five_hour,
        seven_day,
        fetched_at: Utc::now(),
        source: "kimi_code_usage".into(),
    })
}

fn parse_minimax_usage_snapshot(
    account_id: &str,
    account_name: &str,
    body: &str,
    endpoint: MiniMaxEndpoint,
) -> Result<QuotaSnapshot, ProviderError> {
    let payload = serde_json::from_str::<serde_json::Value>(body).map_err(|error| {
        ProviderError::new(
            ProviderErrorKind::InvalidResponse,
            format!("解析 MiniMax 使用量响应失败: {error}"),
        )
    })?;

    let data = payload
        .get("data")
        .filter(|value| value.is_object())
        .unwrap_or(&payload);
    validate_minimax_base_response(data, &payload)?;
    let remains = minimax_model_remains(data, &payload).ok_or_else(|| {
        ProviderError::new(
            ProviderErrorKind::MissingHeaders,
            "MiniMax 使用量接口没有返回 model_remains",
        )
    })?;
    let item = pick_minimax_model_remain(remains).ok_or_else(|| {
        ProviderError::new(
            ProviderErrorKind::MissingHeaders,
            "MiniMax 使用量接口没有返回可识别的额度窗口",
        )
    })?;
    let total = minimax_number_by_keys(
        item,
        &["current_interval_total_count", "currentIntervalTotalCount"],
    )
    .filter(|value| *value > 0.0)
    .ok_or_else(|| {
        ProviderError::new(
            ProviderErrorKind::MissingHeaders,
            "MiniMax 使用量接口缺少 current_interval_total_count",
        )
    })?;
    let remaining = minimax_number_by_keys(
        item,
        &[
            "current_interval_remaining_count",
            "currentIntervalRemainingCount",
            "current_interval_remains_count",
            "currentIntervalRemainsCount",
            "current_interval_remain_count",
            "currentIntervalRemainCount",
            "remaining_count",
            "remainingCount",
            "remains_count",
            "remainsCount",
            "remaining",
            "remains",
            "left_count",
            "leftCount",
        ],
    )
    .or_else(|| {
        minimax_number_by_keys(
            item,
            &["current_interval_usage_count", "currentIntervalUsageCount"],
        )
    });
    let explicit_used = minimax_number_by_keys(
        item,
        &[
            "current_interval_used_count",
            "currentIntervalUsedCount",
            "used_count",
            "used",
        ],
    );
    let used = explicit_used
        .or_else(|| remaining.map(|remaining| total - remaining))
        .map(|value| value.clamp(0.0, total))
        .ok_or_else(|| {
            ProviderError::new(
                ProviderErrorKind::MissingHeaders,
                "MiniMax 使用量接口缺少可识别的已用或剩余额度",
            )
        })?;

    let reset_at = minimax_epoch_to_datetime(minimax_value_by_keys(item, &["end_time", "endTime"]))
        .or_else(|| {
            minimax_remains_duration(item)
                .and_then(|duration| Utc::now().checked_add_signed(duration))
        });
    let window_minutes = minimax_window_minutes(item).or(Some(MINIMAX_CODING_PLAN_WINDOW_MINUTES));
    let used_percent = ((used / total) * 100.0).clamp(0.0, 100.0);
    let five_hour = Some(QuotaWindow {
        used_percent,
        remaining_percent: (100.0 - used_percent).clamp(0.0, 100.0),
        reset_at,
        window_minutes,
    });

    Ok(QuotaSnapshot {
        account_id: account_id.to_string(),
        account_name: account_name.to_string(),
        five_hour,
        seven_day: None,
        fetched_at: Utc::now(),
        source: match endpoint {
            MiniMaxEndpoint::Global => "minimax_coding_plan",
            MiniMaxEndpoint::Cn => "minimax_coding_plan_cn",
        }
        .into(),
    })
}

fn validate_minimax_base_response(
    data: &serde_json::Value,
    payload: &serde_json::Value,
) -> Result<(), ProviderError> {
    let base_resp = data
        .get("base_resp")
        .or_else(|| data.get("baseResp"))
        .or_else(|| payload.get("base_resp"))
        .or_else(|| payload.get("baseResp"));
    let Some(base_resp) = base_resp else {
        return Ok(());
    };
    let status_code = minimax_number_by_keys(base_resp, &["status_code", "statusCode"]);
    if status_code.unwrap_or(0.0) == 0.0 {
        return Ok(());
    }
    let status_message =
        minimax_string_by_keys(base_resp, &["status_msg", "statusMsg"]).unwrap_or_default();
    let normalized = status_message.to_ascii_lowercase();
    if status_code == Some(1004.0)
        || normalized.contains("cookie")
        || normalized.contains("login")
        || normalized.contains("log in")
    {
        return Err(ProviderError::new(
            ProviderErrorKind::Unauthorized,
            "MiniMax API Key 认证失败，请检查后重新保存",
        ));
    }

    Err(ProviderError::new(
        ProviderErrorKind::Unknown,
        if status_message.trim().is_empty() {
            format!(
                "MiniMax API 返回错误状态 {}",
                status_code.unwrap_or_default().round()
            )
        } else {
            format!("MiniMax API 返回错误：{status_message}")
        },
    ))
}

fn minimax_model_remains<'a>(
    data: &'a serde_json::Value,
    payload: &'a serde_json::Value,
) -> Option<&'a Vec<serde_json::Value>> {
    data.get("model_remains")
        .or_else(|| payload.get("model_remains"))
        .or_else(|| data.get("modelRemains"))
        .or_else(|| payload.get("modelRemains"))
        .and_then(|value| value.as_array())
        .filter(|items| !items.is_empty())
}

fn pick_minimax_model_remain(items: &[serde_json::Value]) -> Option<&serde_json::Value> {
    items
        .iter()
        .find(|item| {
            minimax_number_by_keys(
                item,
                &["current_interval_total_count", "currentIntervalTotalCount"],
            )
            .map(|total| total > 0.0)
            .unwrap_or(false)
        })
        .or_else(|| items.first())
}

fn minimax_window_minutes(item: &serde_json::Value) -> Option<u32> {
    let start = minimax_epoch_to_ms(minimax_value_by_keys(item, &["start_time", "startTime"]))?;
    let end = minimax_epoch_to_ms(minimax_value_by_keys(item, &["end_time", "endTime"]))?;
    if end <= start {
        return None;
    }
    let minutes = ((end - start) as f64 / 60_000.0).round();
    if !minutes.is_finite() || minutes <= 0.0 || minutes > f64::from(u32::MAX) {
        None
    } else {
        Some(minutes as u32)
    }
}

fn minimax_remains_duration(item: &serde_json::Value) -> Option<Duration> {
    let remains = minimax_number_by_keys(item, &["remains_time", "remainsTime"])?;
    if !remains.is_finite() || remains <= 0.0 {
        return None;
    }
    let max_window_ms = MINIMAX_CODING_PLAN_WINDOW_MS + MINIMAX_CODING_PLAN_WINDOW_TOLERANCE_MS;
    let seconds_ms = remains * 1000.0;
    let millis_ms = remains;
    let inferred_ms = if seconds_ms <= max_window_ms {
        seconds_ms
    } else if millis_ms <= max_window_ms {
        millis_ms
    } else {
        seconds_ms
    };
    Some(Duration::milliseconds(inferred_ms.round() as i64))
}

fn minimax_epoch_to_datetime(value: Option<&serde_json::Value>) -> Option<DateTime<Utc>> {
    let ms = minimax_epoch_to_ms(value)?;
    Utc.timestamp_millis_opt(ms).single()
}

fn minimax_epoch_to_ms(value: Option<&serde_json::Value>) -> Option<i64> {
    let raw = json_number(value?)?;
    if !raw.is_finite() {
        return None;
    }
    let ms = if raw.abs() < 10_000_000_000.0 {
        raw * 1000.0
    } else {
        raw
    };
    Some(ms.round() as i64)
}

fn minimax_number_by_keys(value: &serde_json::Value, keys: &[&str]) -> Option<f64> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(json_number))
}

fn minimax_string_by_keys(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

fn minimax_value_by_keys<'a>(
    value: &'a serde_json::Value,
    keys: &[&str],
) -> Option<&'a serde_json::Value> {
    keys.iter().find_map(|key| value.get(*key))
}

fn kimi_limit_candidates(data: &serde_json::Value) -> Vec<KimiLimitCandidate> {
    let Some(limits) = data.get("limits").and_then(|value| value.as_array()) else {
        return Vec::new();
    };

    limits
        .iter()
        .filter_map(|item| {
            let detail = item.get("detail").unwrap_or(item);
            let quota = parse_kimi_quota(detail)?;
            let window_minutes = item.get("window").and_then(parse_kimi_window_minutes);
            Some(KimiLimitCandidate {
                quota,
                window_minutes,
            })
        })
        .collect()
}

fn parse_kimi_quota(value: &serde_json::Value) -> Option<KimiQuota> {
    let limit = json_number(value.get("limit")?)?;
    if limit <= 0.0 {
        return None;
    }

    let used = value.get("used").and_then(json_number).or_else(|| {
        let remaining = json_number(value.get("remaining")?)?;
        Some(limit - remaining)
    })?;
    let reset_at = ["resetTime", "reset_at", "resetAt", "reset_time"]
        .iter()
        .find_map(|key| value.get(*key).and_then(parse_reset_at_value));

    Some(KimiQuota {
        used,
        limit,
        reset_at,
    })
}

fn parse_kimi_window_minutes(value: &serde_json::Value) -> Option<u32> {
    let duration = json_number(value.get("duration")?)?;
    if duration <= 0.0 {
        return None;
    }

    let unit = value
        .get("timeUnit")
        .or_else(|| value.get("time_unit"))
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_ascii_uppercase();

    let minutes = if unit.contains("MINUTE") {
        duration
    } else if unit.contains("HOUR") {
        duration * 60.0
    } else if unit.contains("DAY") {
        duration * 24.0 * 60.0
    } else if unit.contains("SECOND") {
        duration / 60.0
    } else {
        return None;
    };

    if !minutes.is_finite() || minutes <= 0.0 || minutes > f64::from(u32::MAX) {
        return None;
    }
    Some(minutes.round() as u32)
}

fn pick_kimi_session_index(candidates: &[KimiLimitCandidate]) -> Option<usize> {
    candidates
        .iter()
        .position(|candidate| candidate.window_minutes == Some(300))
        .or_else(|| {
            candidates
                .iter()
                .enumerate()
                .filter_map(|(index, candidate)| {
                    let minutes = candidate.window_minutes?;
                    Some((index, minutes))
                })
                .min_by_key(|(_, minutes)| *minutes)
                .map(|(index, _)| index)
        })
}

fn pick_kimi_weekly_index(
    candidates: &[KimiLimitCandidate],
    session_index: Option<usize>,
) -> Option<usize> {
    candidates
        .iter()
        .enumerate()
        .filter(|(index, _)| Some(*index) != session_index)
        .max_by_key(|(_, candidate)| candidate.window_minutes.unwrap_or(0))
        .map(|(index, _)| index)
}

fn kimi_quota_to_window(quota: &KimiQuota, window_minutes: Option<u32>) -> QuotaWindow {
    let used_percent = ((quota.used / quota.limit) * 100.0).clamp(0.0, 100.0);
    QuotaWindow {
        used_percent,
        remaining_percent: (100.0 - used_percent).clamp(0.0, 100.0),
        reset_at: quota.reset_at,
        window_minutes,
    }
}

fn json_number(value: &serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::Number(number) => number.as_f64(),
        serde_json::Value::String(raw) => raw.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn parse_reset_at_value(value: &serde_json::Value) -> Option<DateTime<Utc>> {
    match value {
        serde_json::Value::String(raw) => DateTime::parse_from_rfc3339(raw.trim())
            .ok()
            .map(|time| time.with_timezone(&Utc)),
        serde_json::Value::Number(number) => {
            let seconds = number
                .as_i64()
                .or_else(|| number.as_f64().map(|value| value as i64))?;
            Utc.timestamp_opt(seconds, 0).single()
        }
        _ => None,
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
        assert_eq!(
            resolve_target_url(None, &AuthMode::SessionToken),
            DEFAULT_PROBE_URL
        );
        assert_eq!(
            resolve_target_url(None, &AuthMode::Cookie),
            DEFAULT_PROBE_URL
        );
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

        let snapshot = parse_snapshot("default", "acc", &map).unwrap();
        assert_eq!(
            snapshot
                .five_hour
                .as_ref()
                .unwrap()
                .remaining_percent
                .round() as i64,
            75
        );
        assert_eq!(
            snapshot
                .seven_day
                .as_ref()
                .unwrap()
                .remaining_percent
                .round() as i64,
            30
        );
    }

    #[test]
    fn returns_none_when_no_quota_headers() {
        let map = HeaderMap::new();
        assert!(parse_snapshot_if_present("default", "acc", &map)
            .unwrap()
            .is_none());
    }

    #[test]
    fn cookie_mode_uses_cookie_header() {
        let credentials = ProbeCredentials {
            provider: PROVIDER_OPENAI.into(),
            auth_mode: AuthMode::Cookie,
            secret: "foo=bar".into(),
            chatgpt_account_id: Some("acct".into()),
        };
        let headers = build_headers(&credentials, DEFAULT_PROBE_URL).unwrap();
        assert_eq!(headers.get(COOKIE).unwrap(), "foo=bar");
        assert!(headers.get(AUTHORIZATION).is_none());
        assert_eq!(headers.get("chatgpt-account-id").unwrap(), "acct");
    }

    #[test]
    fn anthropic_usage_response_maps_to_existing_quota_windows() {
        let body = r#"{
            "five_hour": {
                "utilization": 34,
                "resets_at": "2026-04-26T10:30:00Z"
            },
            "seven_day": {
                "utilization": 78.5,
                "resets_at": "2026-04-30T00:00:00Z"
            }
        }"#;

        let snapshot =
            parse_anthropic_usage_snapshot("claude", "claude@example.com", body).unwrap();

        assert_eq!(snapshot.account_id, "claude");
        assert_eq!(snapshot.account_name, "claude@example.com");
        assert_eq!(snapshot.source, "anthropic_oauth_usage");
        let five = snapshot.five_hour.unwrap();
        assert_eq!(five.used_percent, 34.0);
        assert_eq!(five.remaining_percent, 66.0);
        assert_eq!(five.window_minutes, Some(300));
        assert_eq!(
            five.reset_at.unwrap().to_rfc3339(),
            "2026-04-26T10:30:00+00:00"
        );
        let seven = snapshot.seven_day.unwrap();
        assert_eq!(seven.used_percent, 78.5);
        assert_eq!(seven.remaining_percent, 21.5);
        assert_eq!(seven.window_minutes, Some(10080));
        assert_eq!(
            seven.reset_at.unwrap().to_rfc3339(),
            "2026-04-30T00:00:00+00:00"
        );
    }

    #[test]
    fn anthropic_headers_use_oauth_bearer_and_beta_usage_api() {
        let credentials = ProbeCredentials {
            provider: PROVIDER_ANTHROPIC.into(),
            auth_mode: AuthMode::OAuth,
            secret: "access-token".into(),
            chatgpt_account_id: None,
        };

        let headers = build_anthropic_usage_headers(&credentials).unwrap();

        assert_eq!(headers.get(AUTHORIZATION).unwrap(), "Bearer access-token");
        assert_eq!(headers.get("anthropic-beta").unwrap(), "oauth-2025-04-20");
        assert_eq!(headers.get(ACCEPT).unwrap(), "application/json");
    }

    #[test]
    fn anthropic_usage_response_accepts_statusline_field_names_and_epoch_reset() {
        let body = r#"{
            "five_hour": {
                "used_percentage": 12.5,
                "resets_at": 1777199400
            }
        }"#;

        let snapshot = parse_anthropic_usage_snapshot("claude", "Claude", body).unwrap();
        let five = snapshot.five_hour.unwrap();

        assert_eq!(five.used_percent, 12.5);
        assert_eq!(five.remaining_percent, 87.5);
        assert_eq!(
            five.reset_at.unwrap().to_rfc3339(),
            "2026-04-26T10:30:00+00:00"
        );
    }

    #[test]
    fn kimi_usage_response_maps_weekly_usage_and_five_hour_limit() {
        let body = r#"{
            "usage": {
                "limit": "100",
                "remaining": "87",
                "resetTime": "2026-04-30T00:34:33.277563Z"
            },
            "limits": [
                {
                    "window": {
                        "duration": 300,
                        "timeUnit": "TIME_UNIT_MINUTE"
                    },
                    "detail": {
                        "limit": "100",
                        "remaining": "100",
                        "resetTime": "2026-04-26T13:34:33.277563Z"
                    }
                }
            ],
            "user": {
                "membership": {
                    "level": "LEVEL_INTERMEDIATE"
                }
            }
        }"#;

        let snapshot = parse_kimi_usage_snapshot("kimi-work", "Kimi Work", body).unwrap();

        assert_eq!(snapshot.account_id, "kimi-work");
        assert_eq!(snapshot.account_name, "Kimi Work");
        assert_eq!(snapshot.source, "kimi_code_usage");
        let five = snapshot.five_hour.unwrap();
        assert_eq!(five.used_percent, 0.0);
        assert_eq!(five.remaining_percent, 100.0);
        assert_eq!(five.window_minutes, Some(300));
        assert_eq!(
            five.reset_at.unwrap().to_rfc3339(),
            "2026-04-26T13:34:33.277563+00:00"
        );
        let seven = snapshot.seven_day.unwrap();
        assert_eq!(seven.used_percent, 13.0);
        assert_eq!(seven.remaining_percent, 87.0);
        assert_eq!(seven.window_minutes, Some(10080));
        assert_eq!(
            seven.reset_at.unwrap().to_rfc3339(),
            "2026-04-30T00:34:33.277563+00:00"
        );
    }

    #[test]
    fn kimi_usage_response_uses_largest_limit_as_weekly_when_usage_block_is_missing() {
        let body = r#"{
            "limits": [
                {
                    "window": { "duration": 300, "timeUnit": "TIME_UNIT_MINUTE" },
                    "detail": { "limit": "100", "used": "20", "reset_at": "2026-04-26T13:00:00Z" }
                },
                {
                    "window": { "duration": 7, "time_unit": "TIME_UNIT_DAY" },
                    "detail": { "limit": "200", "remaining": "150", "reset_at": "2026-04-30T00:00:00Z" }
                }
            ]
        }"#;

        let snapshot = parse_kimi_usage_snapshot("kimi", "Kimi", body).unwrap();

        assert_eq!(snapshot.five_hour.unwrap().remaining_percent, 80.0);
        let seven = snapshot.seven_day.unwrap();
        assert_eq!(seven.used_percent, 25.0);
        assert_eq!(seven.remaining_percent, 75.0);
        assert_eq!(seven.window_minutes, Some(10080));
    }

    #[test]
    fn minimax_usage_response_maps_remaining_count_to_five_hour_window() {
        let body = r#"{
            "data": {
                "current_subscribe_title": "MiniMax Coding Plan Plus",
                "model_remains": [
                    {
                        "model_name": "MiniMax-M2",
                        "current_interval_total_count": 300,
                        "current_interval_usage_count": 180,
                        "start_time": 1700000000000,
                        "end_time": 1700018000000
                    }
                ]
            }
        }"#;

        let snapshot = parse_minimax_usage_snapshot(
            "mini-work",
            "MiniMax Work",
            body,
            MiniMaxEndpoint::Global,
        )
        .unwrap();

        assert_eq!(snapshot.account_id, "mini-work");
        assert_eq!(snapshot.account_name, "MiniMax Work");
        assert_eq!(snapshot.source, "minimax_coding_plan");
        let five = snapshot.five_hour.unwrap();
        assert_eq!(five.used_percent, 40.0);
        assert_eq!(five.remaining_percent, 60.0);
        assert_eq!(five.window_minutes, Some(300));
        assert_eq!(
            five.reset_at.unwrap().to_rfc3339(),
            "2023-11-15T03:13:20+00:00"
        );
        assert!(snapshot.seven_day.is_none());
    }

    #[test]
    fn minimax_cn_usage_response_converts_model_calls_to_prompt_percent() {
        let body = r#"{
            "data": {
                "model_remains": [
                    {
                        "currentIntervalTotalCount": 1500,
                        "currentIntervalRemainingCount": 1200,
                        "remainsTime": 3600
                    }
                ]
            }
        }"#;

        let snapshot =
            parse_minimax_usage_snapshot("mini-cn", "MiniMax CN", body, MiniMaxEndpoint::Cn)
                .unwrap();
        let five = snapshot.five_hour.unwrap();

        assert_eq!(snapshot.source, "minimax_coding_plan_cn");
        assert_eq!(five.used_percent, 20.0);
        assert_eq!(five.remaining_percent, 80.0);
        assert_eq!(five.window_minutes, Some(300));
        assert!(five.reset_at.is_some());
    }
}
