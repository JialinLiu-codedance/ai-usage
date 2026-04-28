use crate::{
    models::{
        AppSettings, AuthMode, ClaudeApiFormat, ClaudeAuthField, ClaudeModelRoute,
        ClaudeProxyCapability, ClaudeProxyProfileSummary, ConnectedAccount, LocalProxyMatchResult,
        LocalProxySettingsState, LocalProxyStatus, PROVIDER_ANTHROPIC, PROVIDER_CUSTOM,
        PROVIDER_GLM, PROVIDER_KIMI, PROVIDER_MINIMAX, PROVIDER_QWEN, PROVIDER_XIAOMI,
    },
    secrets, settings,
};
use async_stream::try_stream;
use axum::{
    body::{self, Body, Bytes},
    extract::Request,
    http::{header::CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures_util::stream::BoxStream;
use futures_util::TryStreamExt;
use reqwest::Client;
use serde_json::{json, Value};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tauri::AppHandle;
use tokio::{
    net::TcpListener,
    sync::{oneshot, Mutex},
};
use tauri::async_runtime::JoinHandle;

const DEFAULT_ANTHROPIC_VERSION: &str = "2023-06-01";
const REQUEST_BODY_LIMIT_BYTES: usize = 10 * 1024 * 1024;

pub fn is_claude_compatible_provider(provider: &str) -> bool {
    matches!(
        provider,
        PROVIDER_ANTHROPIC
            | PROVIDER_GLM
            | PROVIDER_MINIMAX
            | PROVIDER_KIMI
            | PROVIDER_QWEN
            | PROVIDER_XIAOMI
            | PROVIDER_CUSTOM
    )
}

pub fn match_model_route<'a>(
    model: &str,
    routes: &'a [ClaudeModelRoute],
) -> Option<&'a ClaudeModelRoute> {
    routes
        .iter()
        .find(|route| route.enabled && model_matches_pattern(model, &route.model_pattern))
}

pub fn build_local_proxy_settings_state(
    settings: &AppSettings,
) -> Result<LocalProxySettingsState, String> {
    let capabilities = settings
        .accounts
        .iter()
        .map(|account| build_capability_for_account(settings, account))
        .collect();

    Ok(LocalProxySettingsState {
        config: settings.claude_proxy.clone(),
        capabilities,
    })
}

pub fn test_model_match(settings: &AppSettings, model: &str) -> Result<LocalProxyMatchResult, String> {
    let trimmed = model.trim();
    if trimmed.is_empty() {
        return Ok(LocalProxyMatchResult {
            matched: false,
            route_id: None,
            model_pattern: None,
            account_id: None,
            display_name: None,
            error: Some("请输入模型名".into()),
        });
    }

    let Some(route) = match_model_route(trimmed, &settings.claude_proxy.routes) else {
        return Ok(LocalProxyMatchResult {
            matched: false,
            route_id: None,
            model_pattern: None,
            account_id: None,
            display_name: None,
            error: Some("未匹配到模型路由".into()),
        });
    };

    let Some(account) = settings
        .accounts
        .iter()
        .find(|account| account.account_id == route.account_id)
    else {
        return Ok(LocalProxyMatchResult {
            matched: false,
            route_id: Some(route.id.clone()),
            model_pattern: Some(route.model_pattern.clone()),
            account_id: Some(route.account_id.clone()),
            display_name: None,
            error: Some("路由目标账号不存在".into()),
        });
    };

    Ok(LocalProxyMatchResult {
        matched: true,
        route_id: Some(route.id.clone()),
        model_pattern: Some(route.model_pattern.clone()),
        account_id: Some(account.account_id.clone()),
        display_name: Some(account.account_name.clone()),
        error: None,
    })
}

#[derive(Default)]
pub struct LocalProxyManager {
    inner: Mutex<LocalProxyRuntime>,
}

#[derive(Default)]
struct LocalProxyRuntime {
    handle: Option<LocalProxyHandle>,
    metrics: Arc<Mutex<RuntimeMetrics>>,
}

struct LocalProxyHandle {
    shutdown_tx: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

#[derive(Clone)]
struct ProxyServerState {
    app: AppHandle,
    metrics: Arc<Mutex<RuntimeMetrics>>,
}

#[derive(Default)]
struct RuntimeMetrics {
    running: bool,
    address: String,
    port: u16,
    active_connections: usize,
    total_requests: u64,
    successful_requests: u64,
    failed_requests: u64,
    last_error: Option<String>,
    started_at: Option<Instant>,
}

impl LocalProxyManager {
    pub async fn start(&self, app: AppHandle) -> Result<LocalProxyStatus, String> {
        let settings = settings::load_settings(&app)?;
        let config = settings.claude_proxy.clone();
        let bind_address = format!("{}:{}", config.listen_address, config.listen_port);
        let listener = TcpListener::bind(&bind_address)
            .await
            .map_err(|error| format!("启动本地代理失败: {error}"))?;

        let mut runtime = self.inner.lock().await;
        if runtime
            .metrics
            .lock()
            .await
            .running
        {
            return build_status(&app, &runtime.metrics).await;
        }

        {
            let mut metrics = runtime.metrics.lock().await;
            metrics.running = true;
            metrics.address = config.listen_address.clone();
            metrics.port = config.listen_port;
            metrics.active_connections = 0;
            metrics.total_requests = 0;
            metrics.successful_requests = 0;
            metrics.failed_requests = 0;
            metrics.last_error = None;
            metrics.started_at = Some(Instant::now());
        }

        let metrics = runtime.metrics.clone();
        let state = ProxyServerState {
            app: app.clone(),
            metrics: metrics.clone(),
        };
        let router = Router::new()
            .route("/health", get(health_check))
            .route("/v1/messages", post(handle_messages))
            .with_state(state);
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

        let task = tauri::async_runtime::spawn(async move {
            let server = axum::serve(listener, router).with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            });
            if let Err(error) = server.await {
                let mut guard = metrics.lock().await;
                guard.last_error = Some(format!("本地代理异常退出: {error}"));
            }
            let mut guard = metrics.lock().await;
            guard.running = false;
            guard.active_connections = 0;
            guard.started_at = None;
        });

        runtime.handle = Some(LocalProxyHandle { shutdown_tx, task });
        build_status(&app, &runtime.metrics).await
    }

    pub async fn stop(&self, app: AppHandle) -> Result<LocalProxyStatus, String> {
        let mut runtime = self.inner.lock().await;
        if let Some(handle) = runtime.handle.take() {
            let _ = handle.shutdown_tx.send(());
            let _ = handle.task.await;
        }
        {
            let mut metrics = runtime.metrics.lock().await;
            metrics.running = false;
            metrics.active_connections = 0;
            metrics.started_at = None;
        }
        build_status(&app, &runtime.metrics).await
    }

    pub async fn status(&self, app: &AppHandle) -> Result<LocalProxyStatus, String> {
        let runtime = self.inner.lock().await;
        build_status(app, &runtime.metrics).await
    }
}

fn build_capability_for_account(
    settings: &AppSettings,
    account: &ConnectedAccount,
) -> ClaudeProxyCapability {
    let compatible = is_claude_compatible_provider(&account.provider);
    let defaults = default_profile_for_provider(&account.provider);
    let stored = settings.claude_proxy_profiles.get(&account.account_id);
    let direct_secret_configured = account_supports_direct_proxy(account);
    let profile = ClaudeProxyProfileSummary {
        base_url: stored
            .and_then(|profile| profile.base_url.clone())
            .or(defaults.base_url),
        api_format: stored.map(|profile| profile.api_format).unwrap_or(defaults.api_format),
        auth_field: stored
            .map(|profile| profile.auth_field)
            .unwrap_or(defaults.auth_field),
        secret_configured: stored
            .map(|profile| profile.secret_configured)
            .unwrap_or(false)
            || direct_secret_configured,
    };
    let missing_fields = if compatible {
        missing_profile_fields(&profile)
    } else {
        Vec::new()
    };
    let can_connect = compatible && missing_fields.is_empty();

    ClaudeProxyCapability {
        account_id: account.account_id.clone(),
        provider: account.provider.clone(),
        display_name: account.account_name.clone(),
        is_claude_compatible_provider: compatible,
        can_direct_connect: can_connect,
        missing_fields,
        profile: profile.clone(),
        resolved_profile: can_connect.then_some(profile),
    }
}

fn default_profile_for_provider(provider: &str) -> ClaudeProxyProfileSummary {
    let base_url = match provider {
        PROVIDER_ANTHROPIC => Some("https://api.anthropic.com".into()),
        PROVIDER_GLM => Some("https://open.bigmodel.cn/api/anthropic".into()),
        PROVIDER_MINIMAX => Some("https://api.minimaxi.com/anthropic".into()),
        _ => None,
    };

    ClaudeProxyProfileSummary {
        base_url,
        api_format: ClaudeApiFormat::Anthropic,
        auth_field: ClaudeAuthField::AnthropicAuthToken,
        secret_configured: false,
    }
}

fn account_supports_direct_proxy(account: &ConnectedAccount) -> bool {
    matches!(account.provider.as_str(), PROVIDER_GLM | PROVIDER_MINIMAX)
        && matches!(account.auth_mode, AuthMode::ApiKey)
        && account.secret_configured
}

fn missing_profile_fields(profile: &ClaudeProxyProfileSummary) -> Vec<String> {
    let mut missing = Vec::new();

    if profile
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        missing.push("base_url".into());
    }

    if !profile.secret_configured {
        missing.push("api_key_or_token".into());
    }

    missing
}

fn model_matches_pattern(model: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    if let Some(prefix) = pattern.strip_suffix('*') {
        return model.starts_with(prefix);
    }

    if let Some(suffix) = pattern.strip_prefix('*') {
        return model.ends_with(suffix);
    }

    model == pattern
}

async fn build_status(
    app: &AppHandle,
    metrics: &Arc<Mutex<RuntimeMetrics>>,
) -> Result<LocalProxyStatus, String> {
    let settings = settings::load_settings(app)?;
    let guard = metrics.lock().await;
    let uptime_seconds = guard
        .started_at
        .map(|started| started.elapsed().as_secs())
        .unwrap_or(0);
    let address = if guard.address.is_empty() {
        settings.claude_proxy.listen_address.clone()
    } else {
        guard.address.clone()
    };
    let port = if guard.port == 0 {
        settings.claude_proxy.listen_port
    } else {
        guard.port
    };
    let success_rate = if guard.total_requests == 0 {
        0.0
    } else {
        (guard.successful_requests as f64 / guard.total_requests as f64) * 100.0
    };

    Ok(LocalProxyStatus {
        running: guard.running,
        address,
        port,
        active_connections: guard.active_connections,
        total_requests: guard.total_requests,
        successful_requests: guard.successful_requests,
        failed_requests: guard.failed_requests,
        success_rate,
        uptime_seconds,
        last_error: guard.last_error.clone(),
    })
}

async fn health_check() -> impl IntoResponse {
    Json(json!({
        "status": "ok",
    }))
}

async fn handle_messages(
    axum::extract::State(state): axum::extract::State<ProxyServerState>,
    request: Request,
) -> Response {
    begin_request(&state.metrics).await;

    let (parts, body) = request.into_parts();
    let headers = parts.headers;
    let body_bytes = match body::to_bytes(body, REQUEST_BODY_LIMIT_BYTES).await {
        Ok(bytes) => bytes,
        Err(error) => {
            finish_request(&state.metrics, false, Some(format!("读取请求体失败: {error}"))).await;
            return error_response(StatusCode::BAD_REQUEST, "请求体无效");
        }
    };

    let request_body: Value = match serde_json::from_slice(&body_bytes) {
        Ok(value) => value,
        Err(error) => {
            finish_request(&state.metrics, false, Some(format!("解析请求体失败: {error}"))).await;
            return error_response(StatusCode::BAD_REQUEST, "请求 JSON 无效");
        }
    };

    let model = request_body
        .get("model")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(model) = model else {
        finish_request(&state.metrics, false, Some("请求缺少 model".into())).await;
        return error_response(StatusCode::BAD_REQUEST, "请求缺少 model");
    };

    let settings = match settings::load_settings(&state.app) {
        Ok(settings) => settings,
        Err(error) => {
            finish_request(&state.metrics, false, Some(error.clone())).await;
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, &error);
        }
    };
    let matched = match test_model_match(&settings, model) {
        Ok(result) => result,
        Err(error) => {
            finish_request(&state.metrics, false, Some(error.clone())).await;
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, &error);
        }
    };
    if !matched.matched {
        let message = matched
            .error
            .unwrap_or_else(|| "未匹配到模型路由".into());
        finish_request(&state.metrics, false, Some(message.clone())).await;
        return error_response(StatusCode::BAD_REQUEST, &message);
    }

    let Some(account_id) = matched.account_id.as_deref() else {
        finish_request(&state.metrics, false, Some("路由目标账号无效".into())).await;
        return error_response(StatusCode::BAD_REQUEST, "路由目标账号无效");
    };
    let account = match settings
        .accounts
        .iter()
        .find(|account| account.account_id == account_id)
    {
        Some(account) => account,
        None => {
            finish_request(&state.metrics, false, Some("路由目标账号不存在".into())).await;
            return error_response(StatusCode::BAD_REQUEST, "路由目标账号不存在");
        }
    };
    let profile = match resolve_runtime_profile(&settings, account) {
        Ok(profile) => profile,
        Err(error) => {
            finish_request(&state.metrics, false, Some(error.clone())).await;
            return error_response(StatusCode::BAD_REQUEST, &error);
        }
    };

    if !matches!(profile.api_format, ClaudeApiFormat::Anthropic) {
        let message = "当前版本仅支持 Anthropic Messages 兼容供应商";
        finish_request(&state.metrics, false, Some(message.into())).await;
        return error_response(StatusCode::BAD_REQUEST, message);
    }

    let upstream_url = build_upstream_messages_url(&profile.base_url);
    let client = match Client::builder().timeout(Duration::from_secs(180)).build() {
        Ok(client) => client,
        Err(error) => {
            finish_request(&state.metrics, false, Some(format!("创建 HTTP 客户端失败: {error}"))).await;
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "创建代理客户端失败");
        }
    };

    let mut upstream_request = client
        .post(upstream_url)
        .header(CONTENT_TYPE, "application/json")
        .body(body_bytes.clone());
    for (name, value) in forwardable_request_headers(&headers, profile.auth_field) {
        upstream_request = upstream_request.header(name, value);
    }
    for (name, value) in auth_headers(&profile) {
        upstream_request = upstream_request.header(name, value);
    }

    let upstream_response = match upstream_request.send().await {
        Ok(response) => response,
        Err(error) => {
            finish_request(&state.metrics, false, Some(format!("转发请求失败: {error}"))).await;
            return error_response(StatusCode::BAD_GATEWAY, "转发请求失败");
        }
    };

    let status = upstream_response.status();
    let headers = copy_response_headers(upstream_response.headers());
    let is_streaming = upstream_response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("text/event-stream"));

    if is_streaming {
        let metrics = state.metrics.clone();
        let stream: BoxStream<'static, Result<Bytes, std::io::Error>> = Box::pin(try_stream! {
            let mut stream = upstream_response.bytes_stream();
            loop {
                match stream.try_next().await {
                    Ok(Some(chunk)) => yield chunk,
                    Ok(None) => {
                        finish_request(&metrics, status.is_success(), None).await;
                        break;
                    }
                    Err(error) => {
                        finish_request(&metrics, false, Some(format!("读取上游流失败: {error}"))).await;
                        let io_error = std::io::Error::other(error.to_string());
                        Err(io_error)?;
                    }
                }
            }
        });
        return (status, headers, Body::from_stream(stream)).into_response();
    }

    let response_bytes = match upstream_response.bytes().await {
        Ok(bytes) => bytes,
        Err(error) => {
            finish_request(&state.metrics, false, Some(format!("读取上游响应失败: {error}"))).await;
            return error_response(StatusCode::BAD_GATEWAY, "读取上游响应失败");
        }
    };
    finish_request(
        &state.metrics,
        status.is_success(),
        (!status.is_success()).then(|| format!("上游返回状态码 {status}")),
    )
    .await;

    (status, headers, response_bytes).into_response()
}

fn resolve_runtime_profile(
    settings: &AppSettings,
    account: &ConnectedAccount,
) -> Result<ResolvedClaudeProxyProfile, String> {
    if !is_claude_compatible_provider(&account.provider) {
        return Err("当前账号暂不支持 Claude 调用模式".into());
    }

    let defaults = default_profile_for_provider(&account.provider);
    let stored = settings.claude_proxy_profiles.get(&account.account_id);
    let base_url = stored
        .and_then(|profile| profile.base_url.clone())
        .or(defaults.base_url)
        .ok_or_else(|| "请补充 Claude 代理 BASE_URL".to_string())?;
    let api_format = stored.map(|profile| profile.api_format).unwrap_or(defaults.api_format);
    let auth_field = stored
        .map(|profile| profile.auth_field)
        .unwrap_or(defaults.auth_field);

    let secret = if stored
        .map(|profile| profile.secret_configured)
        .unwrap_or(false)
    {
        secrets::load_claude_proxy_secret(&account.account_id)?
    } else if account_supports_direct_proxy(account) {
        secrets::load_account_secret(&account.account_id)?
    } else {
        None
    }
    .ok_or_else(|| "请补充 Claude 代理 API Key".to_string())?;

    Ok(ResolvedClaudeProxyProfile {
        base_url,
        api_format,
        auth_field,
        api_key_or_token: secret,
    })
}

async fn begin_request(metrics: &Arc<Mutex<RuntimeMetrics>>) {
    let mut guard = metrics.lock().await;
    guard.active_connections += 1;
    guard.total_requests += 1;
}

async fn finish_request(
    metrics: &Arc<Mutex<RuntimeMetrics>>,
    success: bool,
    error: Option<String>,
) {
    let mut guard = metrics.lock().await;
    if guard.active_connections > 0 {
        guard.active_connections -= 1;
    }
    if success {
        guard.successful_requests += 1;
    } else {
        guard.failed_requests += 1;
    }
    if let Some(error) = error {
        guard.last_error = Some(error);
    }
}

fn forwardable_request_headers(
    source: &HeaderMap,
    _auth_field: ClaudeAuthField,
) -> Vec<(HeaderName, HeaderValue)> {
    let mut headers = Vec::new();

    if let Some(value) = source.get("anthropic-version").cloned() {
        headers.push((HeaderName::from_static("anthropic-version"), value));
    } else {
        headers.push((
            HeaderName::from_static("anthropic-version"),
            HeaderValue::from_static(DEFAULT_ANTHROPIC_VERSION),
        ));
    }

    if let Some(value) = source.get("anthropic-beta").cloned() {
        headers.push((HeaderName::from_static("anthropic-beta"), value));
    }
    if let Some(value) = source.get("accept").cloned() {
        headers.push((HeaderName::from_static("accept"), value));
    }
    if let Some(value) = source.get("user-agent").cloned() {
        headers.push((HeaderName::from_static("user-agent"), value));
    }

    headers
}

fn auth_headers(profile: &ResolvedClaudeProxyProfile) -> Vec<(HeaderName, HeaderValue)> {
    match profile.auth_field {
        ClaudeAuthField::AnthropicAuthToken => vec![(
            HeaderName::from_static("authorization"),
            HeaderValue::from_str(&format!("Bearer {}", profile.api_key_or_token))
                .unwrap_or_else(|_| HeaderValue::from_static("")),
        )],
        ClaudeAuthField::AnthropicApiKey => vec![(
            HeaderName::from_static("x-api-key"),
            HeaderValue::from_str(&profile.api_key_or_token)
                .unwrap_or_else(|_| HeaderValue::from_static("")),
        )],
    }
}

fn copy_response_headers(source: &HeaderMap) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (name, value) in source.iter() {
        if name.as_str().eq_ignore_ascii_case("content-length")
            || name.as_str().eq_ignore_ascii_case("connection")
            || name.as_str().eq_ignore_ascii_case("transfer-encoding")
        {
            continue;
        }
        headers.insert(name.clone(), value.clone());
    }
    headers
}

fn build_upstream_messages_url(base_url: &str) -> String {
    if base_url.ends_with("/v1/messages") {
        return base_url.to_string();
    }
    format!("{}/v1/messages", base_url.trim_end_matches('/'))
}

fn error_response(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(json!({
            "error": {
                "type": "proxy_error",
                "message": message,
            }
        })),
    )
        .into_response()
}

struct ResolvedClaudeProxyProfile {
    base_url: String,
    api_format: ClaudeApiFormat,
    auth_field: ClaudeAuthField,
    api_key_or_token: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AppSettings, AuthMode, ClaudeApiFormat, ClaudeProxyProfileSettings};
    use std::collections::HashMap;

    fn account(id: &str, provider: &str, auth_mode: AuthMode) -> ConnectedAccount {
        ConnectedAccount {
            account_id: id.into(),
            account_name: format!("{provider}-{id}"),
            provider: provider.into(),
            auth_mode,
            chatgpt_account_id: None,
            secret_configured: true,
        }
    }

    #[test]
    fn claude_compatible_provider_list_matches_plan() {
        for provider in [
            PROVIDER_ANTHROPIC,
            PROVIDER_GLM,
            PROVIDER_MINIMAX,
            PROVIDER_KIMI,
            PROVIDER_QWEN,
            PROVIDER_XIAOMI,
            PROVIDER_CUSTOM,
        ] {
            assert!(is_claude_compatible_provider(provider), "{provider} should be compatible");
        }

        assert!(!is_claude_compatible_provider("openai"));
        assert!(!is_claude_compatible_provider("copilot"));
    }

    #[test]
    fn route_match_uses_first_enabled_match() {
        let routes = vec![
            ClaudeModelRoute {
                id: "first".into(),
                model_pattern: "claude-*".into(),
                account_id: "a".into(),
                enabled: true,
            },
            ClaudeModelRoute {
                id: "second".into(),
                model_pattern: "claude-sonnet-*".into(),
                account_id: "b".into(),
                enabled: true,
            },
        ];

        let matched = match_model_route("claude-sonnet-4-5", &routes).expect("expected route");
        assert_eq!(matched.id, "first");
    }

    #[test]
    fn direct_capability_defaults_glm_and_minimax_only() {
        let settings = AppSettings {
            accounts: vec![
                account("glm-1", PROVIDER_GLM, AuthMode::ApiKey),
                account("minimax-1", PROVIDER_MINIMAX, AuthMode::ApiKey),
                account("anthropic-1", PROVIDER_ANTHROPIC, AuthMode::OAuth),
                account("kimi-1", PROVIDER_KIMI, AuthMode::OAuth),
            ],
            claude_proxy_profiles: HashMap::<String, ClaudeProxyProfileSettings>::new(),
            ..AppSettings::default()
        };

        let state = build_local_proxy_settings_state(&settings).expect("settings state");
        let glm = state
            .capabilities
            .iter()
            .find(|capability| capability.account_id == "glm-1")
            .expect("glm capability");
        assert!(glm.can_direct_connect);
        assert_eq!(
            glm.resolved_profile.as_ref().map(|profile| profile.base_url.clone()),
            Some(Some("https://open.bigmodel.cn/api/anthropic".into()))
        );

        let minimax = state
            .capabilities
            .iter()
            .find(|capability| capability.account_id == "minimax-1")
            .expect("minimax capability");
        assert!(minimax.can_direct_connect);
        assert_eq!(
            minimax.resolved_profile.as_ref().map(|profile| profile.base_url.clone()),
            Some(Some("https://api.minimaxi.com/anthropic".into()))
        );

        let anthropic = state
            .capabilities
            .iter()
            .find(|capability| capability.account_id == "anthropic-1")
            .expect("anthropic capability");
        assert!(anthropic.is_claude_compatible_provider);
        assert!(!anthropic.can_direct_connect);
        assert_eq!(anthropic.missing_fields, vec!["api_key_or_token".to_string()]);

        let kimi = state
            .capabilities
            .iter()
            .find(|capability| capability.account_id == "kimi-1")
            .expect("kimi capability");
        assert!(kimi.is_claude_compatible_provider);
        assert!(!kimi.can_direct_connect);
        assert_eq!(kimi.profile.api_format, ClaudeApiFormat::Anthropic);
    }

    #[test]
    fn stored_profile_makes_anthropic_account_connectable() {
        let mut profiles = HashMap::<String, ClaudeProxyProfileSettings>::new();
        profiles.insert(
            "anthropic-1".into(),
            ClaudeProxyProfileSettings {
                base_url: Some("https://proxy.example.com".into()),
                api_format: ClaudeApiFormat::Anthropic,
                auth_field: ClaudeAuthField::AnthropicApiKey,
                secret_configured: true,
            },
        );
        let settings = AppSettings {
            accounts: vec![account("anthropic-1", PROVIDER_ANTHROPIC, AuthMode::OAuth)],
            claude_proxy_profiles: profiles,
            ..AppSettings::default()
        };

        let state = build_local_proxy_settings_state(&settings).expect("settings state");
        let anthropic = &state.capabilities[0];
        assert!(anthropic.can_direct_connect);
        assert!(anthropic.missing_fields.is_empty());
        assert_eq!(anthropic.profile.auth_field, ClaudeAuthField::AnthropicApiKey);
    }
}
