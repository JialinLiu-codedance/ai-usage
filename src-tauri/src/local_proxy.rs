use crate::{
    commands::build_reverse_proxy_status,
    copilot_oauth::CopilotAuthState,
    local_proxy_sse::{strip_sse_field, take_sse_block},
    local_proxy_streaming_responses::create_anthropic_sse_stream_from_responses,
    local_proxy_transform_chat::{anthropic_to_openai, openai_to_anthropic},
    local_proxy_transform_responses::{anthropic_to_responses, responses_to_anthropic},
    models::{
        AppSettings, AuthMode, ClaudeApiFormat, ClaudeAuthField, ClaudeModelRoute,
        ClaudeProxyCapability, ClaudeProxyProfileSummary, ConnectedAccount, LocalProxyMatchResult,
        LocalProxySettingsState, LocalProxyStatus, ProxyTargetKind, ProxyTargetStatus,
        ReverseProxyStatus, PROVIDER_ANTHROPIC, PROVIDER_COPILOT, PROVIDER_CUSTOM, PROVIDER_GLM,
        PROVIDER_KIMI, PROVIDER_MINIMAX, PROVIDER_OPENAI, PROVIDER_QWEN, PROVIDER_XIAOMI,
    },
    oauth,
    secrets, settings,
};
use axum::{
    body::{self, Bytes},
    extract::Request,
    http::{header::CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use reqwest::Client;
use serde_json::{json, Value};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager};
use tokio::{
    net::TcpListener,
    sync::{oneshot, Mutex},
};
use tauri::async_runtime::JoinHandle;

const DEFAULT_ANTHROPIC_VERSION: &str = "2023-06-01";
const REQUEST_BODY_LIMIT_BYTES: usize = 10 * 1024 * 1024;
pub const REVERSE_TARGET_COPILOT: &str = "reverse:copilot";
pub const REVERSE_TARGET_OPENAI: &str = "reverse:openai";

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
    reverse_status: &ReverseProxyStatus,
) -> Result<LocalProxySettingsState, String> {
    let mut capabilities = settings
        .accounts
        .iter()
        .filter(|account| !matches!(account.provider.as_str(), PROVIDER_OPENAI | PROVIDER_COPILOT))
        .map(|account| build_capability_for_account(settings, account, reverse_status))
        .collect::<Vec<_>>();
    capabilities.push(build_reverse_capability(
        REVERSE_TARGET_COPILOT,
        PROVIDER_COPILOT,
        "GitHub Copilot",
        reverse_status.copilot_ready,
    ));
    capabilities.push(build_reverse_capability(
        REVERSE_TARGET_OPENAI,
        PROVIDER_OPENAI,
        "ChatGPT (Codex OAuth)",
        reverse_status.openai_ready,
    ));

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

    let display_name = if route.account_id == REVERSE_TARGET_COPILOT {
        Some("GitHub Copilot".to_string())
    } else if route.account_id == REVERSE_TARGET_OPENAI {
        Some("ChatGPT (Codex OAuth)".to_string())
    } else {
        settings
            .accounts
            .iter()
            .find(|account| account.account_id == route.account_id)
            .map(|account| account.account_name.clone())
    };
    if display_name.is_none() {
        return Ok(LocalProxyMatchResult {
            matched: false,
            route_id: Some(route.id.clone()),
            model_pattern: Some(route.model_pattern.clone()),
            account_id: Some(route.account_id.clone()),
            display_name: None,
            error: Some("路由目标账号不存在".into()),
        });
    }

    Ok(LocalProxyMatchResult {
        matched: true,
        route_id: Some(route.id.clone()),
        model_pattern: Some(route.model_pattern.clone()),
        account_id: Some(route.account_id.clone()),
        display_name,
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
    _reverse_status: &ReverseProxyStatus,
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
        kind: ProxyTargetKind::DirectAccount,
        provider: account.provider.clone(),
        display_name: account.account_name.clone(),
        is_claude_compatible_provider: compatible,
        can_direct_connect: can_connect,
        missing_fields,
        status: if can_connect {
            ProxyTargetStatus::DirectReady
        } else if compatible {
            ProxyTargetStatus::NeedsProfile
        } else {
            ProxyTargetStatus::Unsupported
        },
        profile: profile.clone(),
        resolved_profile: can_connect.then_some(profile),
    }
}

fn build_reverse_capability(
    id: &str,
    provider: &str,
    display_name: &str,
    ready: bool,
) -> ClaudeProxyCapability {
    ClaudeProxyCapability {
        account_id: id.to_string(),
        kind: if provider == PROVIDER_COPILOT {
            ProxyTargetKind::ReverseCopilot
        } else {
            ProxyTargetKind::ReverseOpenai
        },
        provider: provider.to_string(),
        display_name: display_name.to_string(),
        is_claude_compatible_provider: true,
        can_direct_connect: ready,
        missing_fields: if ready {
            Vec::new()
        } else {
            vec!["reverse_proxy".into()]
        },
        status: if ready {
            ProxyTargetStatus::ReverseReady
        } else {
            ProxyTargetStatus::ReversePending
        },
        profile: ClaudeProxyProfileSummary {
            base_url: None,
            api_format: ClaudeApiFormat::Anthropic,
            auth_field: ClaudeAuthField::AnthropicAuthToken,
            secret_configured: ready,
        },
        resolved_profile: None,
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
    let reverse_status = match build_reverse_proxy_status(
        &settings,
        &state.app.state::<CopilotAuthState>(),
    )
    .await
    {
        Ok(status) => status,
        Err(error) => {
            finish_request(&state.metrics, false, Some(error.clone())).await;
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, &error);
        }
    };
    let target = if account_id == REVERSE_TARGET_COPILOT {
        match resolve_reverse_copilot_target(&settings, &state.app).await {
            Ok(target) => target,
            Err(error) => {
                finish_request(&state.metrics, false, Some(error.clone())).await;
                return error_response(StatusCode::BAD_REQUEST, &error);
            }
        }
    } else if account_id == REVERSE_TARGET_OPENAI {
        match resolve_reverse_openai_target(&settings, &state.app).await {
            Ok(target) => target,
            Err(error) => {
                finish_request(&state.metrics, false, Some(error.clone())).await;
                return error_response(StatusCode::BAD_REQUEST, &error);
            }
        }
    } else {
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
        match resolve_runtime_profile(&settings, account) {
            Ok(profile) => ResolvedProxyTarget::Direct(profile),
            Err(error) => {
                finish_request(&state.metrics, false, Some(error.clone())).await;
                return error_response(StatusCode::BAD_REQUEST, &error);
            }
        }
    };

    let client = match Client::builder().timeout(Duration::from_secs(180)).build() {
        Ok(client) => client,
        Err(error) => {
            finish_request(&state.metrics, false, Some(format!("创建 HTTP 客户端失败: {error}"))).await;
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "创建代理客户端失败");
        }
    };
    let wants_stream = request_body
        .get("stream")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let response = match forward_request(
        &client,
        &headers,
        body_bytes,
        request_body,
        target,
        wants_stream,
        &reverse_status,
    )
    .await
    {
        Ok(response) => response,
        Err(error) => {
            finish_request(&state.metrics, false, Some(error.clone())).await;
            return error_response(StatusCode::BAD_GATEWAY, &error);
        }
    };
    finish_request(&state.metrics, true, None).await;
    response
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

enum ResolvedProxyTarget {
    Direct(ResolvedClaudeProxyProfile),
    ReverseCopilot {
        token: String,
    },
    ReverseOpenai {
        token: String,
        chatgpt_account_id: Option<String>,
    },
}

async fn resolve_reverse_copilot_target(
    settings: &AppSettings,
    app: &AppHandle,
) -> Result<ResolvedProxyTarget, String> {
    if !settings.reverse_proxy.enabled {
        return Err("反向代理未开启".into());
    }
    let copilot_state = app.state::<CopilotAuthState>();
    let manager = copilot_state.0.read().await;
    let account_id = if let Some(account_id) = settings.reverse_proxy.default_copilot_account_id.clone() {
        account_id
    } else {
        manager
            .list_accounts()
            .await
            .into_iter()
            .next()
            .map(|account| account.id)
            .ok_or_else(|| "请先在反向代理中设置默认 Copilot 账号".to_string())?
    };
    let token = manager
        .get_valid_token_for_account(&account_id)
        .await
        .map_err(|error| error.to_string())?;
    Ok(ResolvedProxyTarget::ReverseCopilot { token })
}

async fn resolve_reverse_openai_target(
    settings: &AppSettings,
    _app: &AppHandle,
) -> Result<ResolvedProxyTarget, String> {
    if !settings.reverse_proxy.enabled {
        return Err("反向代理未开启".into());
    }
    let account_id = settings
        .reverse_proxy
        .default_openai_account_id
        .clone()
        .or_else(|| {
            settings
                .accounts
                .iter()
                .find(|account| account.provider == PROVIDER_OPENAI && matches!(account.auth_mode, AuthMode::OAuth))
                .map(|account| account.account_id.clone())
        })
        .ok_or_else(|| "请先在反向代理中设置默认 OpenAI 账号".to_string())?;
    let tokens = oauth::ensure_fresh_token(&account_id, false)
        .await
        .map_err(|error| error.to_string())?;
    Ok(ResolvedProxyTarget::ReverseOpenai {
        token: tokens.access_token,
        chatgpt_account_id: tokens.chatgpt_account_id,
    })
}

async fn forward_request(
    client: &Client,
    incoming_headers: &HeaderMap,
    original_body_bytes: Bytes,
    original_body: Value,
    target: ResolvedProxyTarget,
    wants_stream: bool,
    _reverse_status: &ReverseProxyStatus,
) -> Result<Response, String> {
    match target {
        ResolvedProxyTarget::Direct(profile) => {
            forward_direct_request(client, incoming_headers, original_body_bytes, profile).await
        }
        ResolvedProxyTarget::ReverseCopilot { token } => {
            forward_reverse_copilot_request(client, original_body, token, wants_stream).await
        }
        ResolvedProxyTarget::ReverseOpenai {
            token,
            chatgpt_account_id,
        } => forward_reverse_openai_request(client, original_body, token, chatgpt_account_id, wants_stream).await,
    }
}

async fn forward_direct_request(
    client: &Client,
    incoming_headers: &HeaderMap,
    original_body_bytes: Bytes,
    profile: ResolvedClaudeProxyProfile,
) -> Result<Response, String> {
    if !matches!(profile.api_format, ClaudeApiFormat::Anthropic) {
        return Err("当前版本仅支持 Anthropic Messages 兼容供应商".into());
    }

    let upstream_url = build_upstream_messages_url(&profile.base_url);
    let mut upstream_request = client
        .post(upstream_url)
        .header(CONTENT_TYPE, "application/json")
        .body(original_body_bytes);
    for (name, value) in forwardable_request_headers(incoming_headers, profile.auth_field) {
        upstream_request = upstream_request.header(name, value);
    }
    for (name, value) in auth_headers(&profile) {
        upstream_request = upstream_request.header(name, value);
    }

    let upstream_response = upstream_request
        .send()
        .await
        .map_err(|error| format!("转发请求失败: {error}"))?;
    let status = upstream_response.status();
    let headers = copy_response_headers(upstream_response.headers());
    let response_bytes = upstream_response
        .bytes()
        .await
        .map_err(|error| format!("读取上游响应失败: {error}"))?;
    Ok((status, headers, response_bytes).into_response())
}

async fn forward_reverse_copilot_request(
    client: &Client,
    original_body: Value,
    token: String,
    wants_stream: bool,
) -> Result<Response, String> {
    let mut upstream_body = anthropic_to_openai(original_body).map_err(|error| error.to_string())?;
    upstream_body["stream"] = json!(false);

    let upstream_response = client
        .post("https://api.githubcopilot.com/chat/completions")
        .header(CONTENT_TYPE, "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .header("Editor-Version", "vscode/1.110.1")
        .header("Editor-Plugin-Version", "copilot-chat/0.38.2")
        .header("Copilot-Integration-Id", "vscode-chat")
        .header("User-Agent", "GitHubCopilotChat/0.38.2")
        .header("X-Github-Api-Version", "2025-10-01")
        .header("openai-intent", "conversation-agent")
        .header("x-initiator", "user")
        .header("x-interaction-type", "conversation-agent")
        .header("x-vscode-user-agent-library-version", "electron-fetch")
        .json(&upstream_body)
        .send()
        .await
        .map_err(|error| format!("Copilot 反向请求失败: {error}"))?;

    let status = upstream_response.status();
    let body = upstream_response
        .json::<Value>()
        .await
        .map_err(|error| format!("解析 Copilot 响应失败: {error}"))?;
    if !status.is_success() {
        return Err(format!("Copilot 上游返回 {status}: {}", body));
    }
    let anthropic = openai_to_anthropic(body).map_err(|error| error.to_string())?;
    Ok(anthropic_response_to_client(anthropic, wants_stream))
}

async fn forward_reverse_openai_request(
    client: &Client,
    original_body: Value,
    token: String,
    chatgpt_account_id: Option<String>,
    wants_stream: bool,
) -> Result<Response, String> {
    let mut upstream_body = anthropic_to_responses(original_body, true).map_err(|error| error.to_string())?;

    let mut request = client
        .post("https://chatgpt.com/backend-api/codex/responses")
        .header(CONTENT_TYPE, "application/json")
        .header("Accept", "text/event-stream")
        .header("User-Agent", "codex_cli_rs/0.104.0")
        .header("Authorization", format!("Bearer {token}"))
        .header("OpenAI-Beta", "responses=experimental")
        .header("Originator", "codex_cli_rs")
        .header("Version", "0.104.0");
    if let Some(account_id) = chatgpt_account_id.as_deref() {
        request = request.header("chatgpt-account-id", account_id);
    }
    let upstream_response = request
        .json(&upstream_body)
        .send()
        .await
        .map_err(|error| format!("ChatGPT 反向请求失败: {error}"))?;

    let status = upstream_response.status();
    if !status.is_success() {
        let body = upstream_response
            .text()
            .await
            .unwrap_or_else(|_| String::from("无法读取响应体"));
        return Err(format!("ChatGPT 上游返回 {status}: {}", body));
    }

    if wants_stream {
        let stream = create_anthropic_sse_stream_from_responses(upstream_response.bytes_stream());
        let body = axum::body::Body::from_stream(stream);
        return Ok((
            StatusCode::OK,
            [(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"))],
            body,
        )
            .into_response());
    }

    let body = upstream_response
        .text()
        .await
        .map_err(|error| format!("读取 ChatGPT SSE 响应失败: {error}"))?;
    let response = responses_sse_to_response_value(&body)?;
    let anthropic = responses_to_anthropic(response).map_err(|error| error.to_string())?;
    Ok(anthropic_response_to_client(anthropic, false))
}

fn anthropic_response_to_client(body: Value, wants_stream: bool) -> Response {
    if !wants_stream {
        return (
            StatusCode::OK,
            Json(body),
        )
            .into_response();
    }

    let sse = anthropic_json_to_sse(&body);
    (
        StatusCode::OK,
        [(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"))],
        sse,
    )
        .into_response()
}

fn anthropic_json_to_sse(body: &Value) -> String {
    let mut events = Vec::new();
    let message_start = json!({
        "type": "message_start",
        "message": {
            "id": body.get("id").and_then(|v| v.as_str()).unwrap_or(""),
            "type": "message",
            "role": "assistant",
            "model": body.get("model").and_then(|v| v.as_str()).unwrap_or(""),
            "usage": body.get("usage").cloned().unwrap_or_else(|| json!({"input_tokens": 0, "output_tokens": 0}))
        }
    });
    events.push(format!("event: message_start\ndata: {}\n\n", serde_json::to_string(&message_start).unwrap()));

    if let Some(content) = body.get("content").and_then(|v| v.as_array()) {
        for (index, block) in content.iter().enumerate() {
            let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("text");
            match block_type {
                "text" => {
                    let start = json!({
                        "type": "content_block_start",
                        "index": index,
                        "content_block": { "type": "text", "text": "" }
                    });
                    events.push(format!("event: content_block_start\ndata: {}\n\n", serde_json::to_string(&start).unwrap()));
                    let delta = json!({
                        "type": "content_block_delta",
                        "index": index,
                        "delta": {
                            "type": "text_delta",
                            "text": block.get("text").and_then(|v| v.as_str()).unwrap_or("")
                        }
                    });
                    events.push(format!("event: content_block_delta\ndata: {}\n\n", serde_json::to_string(&delta).unwrap()));
                    let stop = json!({ "type": "content_block_stop", "index": index });
                    events.push(format!("event: content_block_stop\ndata: {}\n\n", serde_json::to_string(&stop).unwrap()));
                }
                "tool_use" => {
                    let start = json!({
                        "type": "content_block_start",
                        "index": index,
                        "content_block": {
                            "type": "tool_use",
                            "id": block.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                            "name": block.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                            "input": {}
                        }
                    });
                    events.push(format!("event: content_block_start\ndata: {}\n\n", serde_json::to_string(&start).unwrap()));
                    let delta = json!({
                        "type": "content_block_delta",
                        "index": index,
                        "delta": {
                            "type": "input_json_delta",
                            "partial_json": serde_json::to_string(&block.get("input").cloned().unwrap_or_else(|| json!({}))).unwrap_or_else(|_| "{}".to_string())
                        }
                    });
                    events.push(format!("event: content_block_delta\ndata: {}\n\n", serde_json::to_string(&delta).unwrap()));
                    let stop = json!({ "type": "content_block_stop", "index": index });
                    events.push(format!("event: content_block_stop\ndata: {}\n\n", serde_json::to_string(&stop).unwrap()));
                }
                _ => {}
            }
        }
    }

    let message_delta = json!({
        "type": "message_delta",
        "delta": {
            "stop_reason": body.get("stop_reason").cloned().unwrap_or(Value::Null),
            "stop_sequence": Value::Null
        },
        "usage": body.get("usage").cloned().unwrap_or_else(|| json!({"input_tokens": 0, "output_tokens": 0}))
    });
    events.push(format!("event: message_delta\ndata: {}\n\n", serde_json::to_string(&message_delta).unwrap()));
    let message_stop = json!({ "type": "message_stop" });
    events.push(format!("event: message_stop\ndata: {}\n\n", serde_json::to_string(&message_stop).unwrap()));
    events.join("")
}

fn responses_sse_to_response_value(body: &str) -> Result<Value, String> {
    let mut buffer = body.to_string();
    let mut completed_response: Option<Value> = None;
    let mut output_items = Vec::new();

    while let Some(block) = take_sse_block(&mut buffer) {
        let mut event_name = "";
        let mut data_lines: Vec<&str> = Vec::new();

        for line in block.lines() {
            if let Some(evt) = strip_sse_field(line, "event") {
                event_name = evt.trim();
            } else if let Some(d) = strip_sse_field(line, "data") {
                data_lines.push(d);
            }
        }

        if data_lines.is_empty() {
            continue;
        }

        let data_str = data_lines.join("\n");
        if data_str.trim() == "[DONE]" {
            continue;
        }

        let data: Value = serde_json::from_str(&data_str)
            .map_err(|error| format!("解析 ChatGPT SSE 事件失败: {error}"))?;

        match event_name {
            "response.output_item.done" => {
                if let Some(item) = data.get("item") {
                    output_items.push(item.clone());
                }
            }
            "response.completed" => {
                completed_response = Some(data.get("response").cloned().unwrap_or(data));
            }
            "response.failed" => {
                let message = data
                    .pointer("/response/error/message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("response.failed event received");
                return Err(message.to_string());
            }
            _ => {}
        }
    }

    let mut response =
        completed_response.ok_or_else(|| "上游 SSE 缺少 response.completed 事件".to_string())?;

    if !output_items.is_empty() {
        if let Some(obj) = response.as_object_mut() {
            obj.insert("output".to_string(), Value::Array(output_items));
        } else {
            return Err("response.completed payload 不是 JSON object".to_string());
        }
    }

    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AppSettings, AuthMode, ClaudeApiFormat, ClaudeProxyProfileSettings};
    use std::collections::HashMap;

    fn reverse_status(enabled: bool, copilot_ready: bool, openai_ready: bool) -> ReverseProxyStatus {
        ReverseProxyStatus {
            enabled,
            copilot_ready,
            openai_ready,
            available_copilot_accounts: usize::from(copilot_ready),
            available_openai_accounts: usize::from(openai_ready),
        }
    }

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

        let state = build_local_proxy_settings_state(&settings, &reverse_status(false, false, false))
            .expect("settings state");
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

        let state = build_local_proxy_settings_state(&settings, &reverse_status(false, false, false))
            .expect("settings state");
        let anthropic = &state.capabilities[0];
        assert!(anthropic.can_direct_connect);
        assert!(anthropic.missing_fields.is_empty());
        assert_eq!(anthropic.profile.auth_field, ClaudeAuthField::AnthropicApiKey);
    }

    #[test]
    fn reverse_targets_switch_from_pending_to_ready_with_reverse_proxy_status() {
        let settings = AppSettings::default();

        let pending = build_local_proxy_settings_state(&settings, &reverse_status(false, false, false))
            .expect("pending state");
        let copilot_pending = pending
            .capabilities
            .iter()
            .find(|capability| capability.account_id == REVERSE_TARGET_COPILOT)
            .expect("copilot reverse target");
        assert_eq!(copilot_pending.kind, ProxyTargetKind::ReverseCopilot);
        assert_eq!(copilot_pending.status, ProxyTargetStatus::ReversePending);
        assert!(!copilot_pending.can_direct_connect);

        let ready = build_local_proxy_settings_state(&settings, &reverse_status(true, true, true))
            .expect("ready state");
        let openai_ready = ready
            .capabilities
            .iter()
            .find(|capability| capability.account_id == REVERSE_TARGET_OPENAI)
            .expect("openai reverse target");
        assert_eq!(openai_ready.kind, ProxyTargetKind::ReverseOpenai);
        assert_eq!(openai_ready.status, ProxyTargetStatus::ReverseReady);
        assert!(openai_ready.can_direct_connect);
    }

    #[test]
    fn local_proxy_model_capabilities_do_not_duplicate_openai_reverse_accounts() {
        let settings = AppSettings {
            accounts: vec![
                account("openai-1", PROVIDER_OPENAI, AuthMode::OAuth),
                account("openai-2", PROVIDER_OPENAI, AuthMode::OAuth),
            ],
            ..AppSettings::default()
        };

        let state = build_local_proxy_settings_state(&settings, &reverse_status(true, true, true))
            .expect("settings state");

        let openai_capabilities = state
            .capabilities
            .iter()
            .filter(|capability| capability.provider == PROVIDER_OPENAI)
            .collect::<Vec<_>>();

        assert_eq!(openai_capabilities.len(), 1);
        assert_eq!(openai_capabilities[0].account_id, REVERSE_TARGET_OPENAI);
        assert_eq!(openai_capabilities[0].status, ProxyTargetStatus::ReverseReady);
    }

    #[test]
    fn responses_sse_to_response_value_collects_output_items() {
        let sse = r#"event: response.output_item.done
data: {"type":"response.output_item.done","item":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"hello"}]}}

event: response.completed
data: {"type":"response.completed","response":{"id":"resp_1","status":"completed","model":"gpt-5.4","output":[],"usage":{"input_tokens":10,"output_tokens":2}}}

"#;

        let response = responses_sse_to_response_value(sse).expect("sse should aggregate");

        assert_eq!(response["id"], "resp_1");
        assert_eq!(response["output"][0]["type"], "message");
        assert_eq!(response["output"][0]["content"][0]["text"], "hello");
    }
}
