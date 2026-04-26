use crate::{
    models::{OAuthPhase, OAuthStatus, StoredOAuthTokens},
    secrets, settings,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Duration, TimeZone, Utc};
use serde::Deserialize;
use serde_json::json;
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::{Arc, OnceLock},
};
use tauri::AppHandle;
use tokio::sync::{Mutex, RwLock};
use url::Url;

const OPENAI_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const OPENAI_AUTHORIZE_URL: &str = "https://auth.openai.com/oauth/authorize";
const OPENAI_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const OPENAI_SCOPES: &str = "openid profile email offline_access";
const OPENAI_REFRESH_SCOPES: &str = "openid profile email";
const REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const ANTHROPIC_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const ANTHROPIC_AUTHORIZE_URL: &str = "https://claude.ai/oauth/authorize";
const ANTHROPIC_TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
const ANTHROPIC_REDIRECT_URI: &str = "https://platform.claude.com/oauth/code/callback";
const ANTHROPIC_SCOPES: &str =
    "org:create_api_key user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload";
const KIMI_CLIENT_ID: &str = "17e5f671-d194-4dfb-9706-5516cb48c098";
const KIMI_TOKEN_URL: &str = "https://auth.kimi.com/api/oauth/token";
const KIMI_CLI_CREDENTIALS_PATH: &str = ".kimi/credentials/kimi-code.json";
const TOKEN_REFRESH_SKEW: Duration = Duration::minutes(3);

static REFRESH_LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();

#[derive(Clone)]
struct OAuthSession {
    provider: OAuthProvider,
    state: String,
    code_verifier: String,
    target_account_id: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OAuthProvider {
    OpenAI,
    Anthropic,
}

impl OAuthProvider {
    fn key(self) -> &'static str {
        match self {
            Self::OpenAI => crate::models::PROVIDER_OPENAI,
            Self::Anthropic => crate::models::PROVIDER_ANTHROPIC,
        }
    }

    fn display_name(self) -> &'static str {
        match self {
            Self::OpenAI => "OpenAI",
            Self::Anthropic => "Anthropic",
        }
    }

    fn token_url(self) -> &'static str {
        match self {
            Self::OpenAI => OPENAI_TOKEN_URL,
            Self::Anthropic => ANTHROPIC_TOKEN_URL,
        }
    }

    fn from_key(provider: &str) -> Result<Self, String> {
        match provider {
            crate::models::PROVIDER_OPENAI => Ok(Self::OpenAI),
            crate::models::PROVIDER_ANTHROPIC => Ok(Self::Anthropic),
            _ => Err(format!("暂不支持 {provider} OAuth 刷新")),
        }
    }
}

#[derive(Default)]
pub struct OAuthStore {
    pub status: RwLock<OAuthStatus>,
    session: RwLock<Option<OAuthSession>>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    id_token: Option<String>,
    expires_in: i64,
    scope: Option<String>,
    account: Option<AnthropicAccountInfo>,
    organization: Option<AnthropicOrganizationInfo>,
}

#[derive(Debug, Deserialize)]
struct KimiCliCredentials {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: Option<serde_json::Value>,
    expires_in: Option<i64>,
    scope: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicAccountInfo {
    uuid: Option<String>,
    email_address: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicOrganizationInfo {
    uuid: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IdTokenClaims {
    email: Option<String>,
    #[serde(rename = "https://api.openai.com/auth")]
    openai_auth: Option<OpenAIAuthClaims>,
}

#[derive(Debug, Deserialize)]
struct OpenAIAuthClaims {
    chatgpt_account_id: Option<String>,
}

pub async fn start_openai_oauth(
    store: &OAuthStore,
    target_account_id: Option<String>,
) -> Result<String, String> {
    start_provider_oauth(store, OAuthProvider::OpenAI, target_account_id).await
}

pub async fn start_anthropic_oauth(
    store: &OAuthStore,
    target_account_id: Option<String>,
) -> Result<String, String> {
    start_provider_oauth(store, OAuthProvider::Anthropic, target_account_id).await
}

async fn start_provider_oauth(
    store: &OAuthStore,
    provider: OAuthProvider,
    target_account_id: Option<String>,
) -> Result<String, String> {
    let session = OAuthSession {
        provider,
        state: random_hex(32)?,
        code_verifier: random_code_verifier(provider)?,
        target_account_id: target_account_id
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
    };
    let code_challenge = code_challenge(&session.code_verifier);
    let auth_url = build_authorization_url(&provider, &session.state, &code_challenge);

    {
        let mut guard = store.session.write().await;
        *guard = Some(session);
    }

    {
        let mut guard = store.status.write().await;
        *guard = OAuthStatus {
            phase: OAuthPhase::Running,
            message: Some(format!(
                "{} OAuth 已就绪，请完成授权后把浏览器最终回调 URL 粘贴回来",
                provider.display_name()
            )),
            email: None,
            auth_url: Some(auth_url.clone()),
        };
    }

    Ok(auth_url)
}

fn build_authorization_url(provider: &OAuthProvider, state: &str, code_challenge: &str) -> String {
    match provider {
        OAuthProvider::OpenAI => format!(
            "{OPENAI_AUTHORIZE_URL}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256&id_token_add_organizations=true&codex_cli_simplified_flow=true",
            urlencoding::encode(OPENAI_CLIENT_ID),
            urlencoding::encode(REDIRECT_URI),
            urlencoding::encode(OPENAI_SCOPES),
            urlencoding::encode(state),
            urlencoding::encode(code_challenge),
        ),
        OAuthProvider::Anthropic => format!(
            "{ANTHROPIC_AUTHORIZE_URL}?code=true&client_id={}&response_type=code&redirect_uri={}&scope={}&code_challenge={}&code_challenge_method=S256&state={}",
            urlencoding::encode(ANTHROPIC_CLIENT_ID),
            urlencoding::encode(ANTHROPIC_REDIRECT_URI),
            urlencoding::encode(ANTHROPIC_SCOPES),
            urlencoding::encode(code_challenge),
            urlencoding::encode(state),
        ),
    }
}

pub async fn complete_openai_oauth(
    app: &AppHandle,
    store: &OAuthStore,
    callback_input: &str,
) -> Result<OAuthStatus, String> {
    complete_provider_oauth(app, store, OAuthProvider::OpenAI, callback_input).await
}

pub async fn complete_anthropic_oauth(
    app: &AppHandle,
    store: &OAuthStore,
    callback_input: &str,
) -> Result<OAuthStatus, String> {
    complete_provider_oauth(app, store, OAuthProvider::Anthropic, callback_input).await
}

async fn complete_provider_oauth(
    app: &AppHandle,
    store: &OAuthStore,
    provider: OAuthProvider,
    callback_input: &str,
) -> Result<OAuthStatus, String> {
    let callback_input = callback_input.trim();
    if callback_input.is_empty() {
        return Err("请粘贴回调完整 URL 或 code".into());
    }

    let session = {
        let guard = store.session.read().await;
        guard
            .clone()
            .ok_or_else(|| "当前没有待完成的 OAuth 会话，请先点击开始 OAuth 授权".to_string())?
    };
    if session.provider != provider {
        return Err(format!(
            "当前待完成的是 {} OAuth，请重新发起授权",
            session.provider.display_name()
        ));
    }

    let (code, state) = parse_callback_input(callback_input)?;
    if let Some(state) = state {
        if state != session.state {
            return Err("OAuth state 校验失败，请重新发起授权".into());
        }
    }

    let token = exchange_code(provider, &code, &session.code_verifier).await?;
    let (email, provider_account_id) = oauth_account_metadata(provider, &token);

    let mut current = settings::load_settings(app)?;
    let account_id = settings::upsert_provider_oauth_account(
        &mut current,
        provider.key(),
        session.target_account_id.clone(),
        email.clone(),
        provider_account_id.clone(),
    );
    let stored_tokens = stored_tokens_from_response(
        provider,
        account_id.clone(),
        token,
        Utc::now(),
        email.clone(),
        provider_account_id
            .clone()
            .or_else(|| current.chatgpt_account_id.clone()),
    );
    secrets::save_oauth_tokens(&account_id, &stored_tokens)?;
    let _ = settings::write_settings(app, &current)?;

    {
        let mut guard = store.session.write().await;
        *guard = None;
    }

    let status = OAuthStatus {
        phase: OAuthPhase::Success,
        message: Some(match stored_tokens.refresh_token.as_deref() {
            Some(refresh_token) if !refresh_token.trim().is_empty() => {
                format!(
                    "{} OAuth 授权成功，已保存访问令牌和刷新令牌",
                    provider.display_name()
                )
            }
            None => format!("{} OAuth 授权成功，已保存访问令牌", provider.display_name()),
            Some(_) => format!("{} OAuth 授权成功，已保存访问令牌", provider.display_name()),
        }),
        email,
        auth_url: None,
    };

    {
        let mut guard = store.status.write().await;
        *guard = status.clone();
    }

    Ok(status)
}

fn parse_callback_input(input: &str) -> Result<(String, Option<String>), String> {
    if input.starts_with("http://") || input.starts_with("https://") {
        let parsed = Url::parse(input).map_err(|error| format!("回调 URL 解析失败: {error}"))?;
        let params: HashMap<String, String> = parsed.query_pairs().into_owned().collect();
        let code = params
            .get("code")
            .cloned()
            .ok_or_else(|| "回调 URL 中缺少 code 参数".to_string())?;
        let state = params.get("state").cloned();
        return Ok((code, state));
    }

    if let Some((code, state)) = input.split_once('#') {
        let code = code.trim();
        let state = state.trim();
        if code.is_empty() {
            return Err("授权 Code 为空".into());
        }
        return Ok((
            code.to_string(),
            if state.is_empty() {
                None
            } else {
                Some(state.to_string())
            },
        ));
    }

    Ok((input.to_string(), None))
}

pub async fn oauth_status(store: &OAuthStore) -> OAuthStatus {
    store.status.read().await.clone()
}

pub async fn ensure_fresh_token(
    account_id: &str,
    force: bool,
) -> Result<StoredOAuthTokens, String> {
    let existing = secrets::load_oauth_tokens(account_id)?
        .ok_or_else(|| "请先完成账号授权或导入".to_string())?;
    let now = Utc::now();
    if !should_refresh_token(existing.expires_at, now, force) {
        return Ok(existing);
    }

    let refresh_lock = refresh_lock_for_account(account_id).await;
    let _guard = refresh_lock.lock().await;

    let existing = secrets::load_oauth_tokens(account_id)?
        .ok_or_else(|| "请先完成账号授权或导入".to_string())?;
    let now = Utc::now();
    if !should_refresh_token(existing.expires_at, now, force) {
        return Ok(existing);
    }

    let refresh_token = existing
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "当前 OAuth 凭证缺少刷新令牌，请重新授权".to_string())?;
    if existing.provider == crate::models::PROVIDER_KIMI {
        let response = refresh_kimi_access_token(refresh_token).await?;
        let updated = merge_refreshed_tokens(existing, response, now, None, None);
        secrets::save_oauth_tokens(account_id, &updated)?;
        return Ok(updated);
    }

    let provider = OAuthProvider::from_key(&existing.provider)?;
    let response = refresh_access_token(provider, refresh_token).await?;
    let updated = refreshed_tokens_from_response(existing, response, now);
    secrets::save_oauth_tokens(account_id, &updated)?;
    Ok(updated)
}

pub fn load_kimi_cli_tokens(account_id: String) -> Result<StoredOAuthTokens, String> {
    let path = kimi_cli_credentials_path()?;
    let raw = fs::read_to_string(&path)
        .map_err(|error| format!("读取 Kimi CLI 凭证失败（{}）: {error}", path.display()))?;
    let credentials = serde_json::from_str::<KimiCliCredentials>(&raw)
        .map_err(|error| format!("解析 Kimi CLI 凭证失败: {error}"))?;
    if credentials.access_token.trim().is_empty() {
        return Err("Kimi CLI 凭证缺少 access_token，请先运行 kimi /login".into());
    }

    let now = Utc::now();
    Ok(StoredOAuthTokens {
        provider: crate::models::PROVIDER_KIMI.into(),
        account_id,
        access_token: credentials.access_token,
        refresh_token: credentials.refresh_token,
        id_token: None,
        expires_at: kimi_cli_expires_at(
            credentials.expires_at.as_ref(),
            credentials.expires_in,
            now,
        )?,
        email: None,
        chatgpt_account_id: None,
        scope: credentials.scope,
    })
}

fn kimi_cli_credentials_path() -> Result<PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "无法定位 HOME 目录".to_string())?;
    Ok(PathBuf::from(home).join(KIMI_CLI_CREDENTIALS_PATH))
}

fn kimi_cli_expires_at(
    expires_at: Option<&serde_json::Value>,
    expires_in: Option<i64>,
    now: DateTime<Utc>,
) -> Result<DateTime<Utc>, String> {
    if let Some(value) = expires_at {
        if let Some(seconds) = json_number(value) {
            let mut whole_seconds = seconds.floor() as i64;
            let mut nanos = ((seconds - whole_seconds as f64) * 1_000_000_000.0).round() as u32;
            if nanos >= 1_000_000_000 {
                whole_seconds += 1;
                nanos = 0;
            }
            return Utc
                .timestamp_opt(whole_seconds, nanos)
                .single()
                .ok_or_else(|| "Kimi CLI expires_at 超出可识别范围".to_string());
        }
    }

    Ok(now + Duration::seconds(expires_in.unwrap_or(3600).max(0)))
}

async fn refresh_lock_for_account(account_id: &str) -> Arc<Mutex<()>> {
    let key = normalize_account_id(account_id);
    let locks = REFRESH_LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = locks.lock().await;
    guard
        .entry(key)
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

fn normalize_account_id(account_id: &str) -> String {
    let trimmed = account_id.trim();
    if trimmed.is_empty() {
        crate::models::default_account_id()
    } else {
        trimmed.to_string()
    }
}

async fn exchange_code(
    provider: OAuthProvider,
    code: &str,
    code_verifier: &str,
) -> Result<TokenResponse, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|error| format!("创建 OAuth HTTP 客户端失败: {error}"))?;

    let request = client.post(provider.token_url());
    let response = match provider {
        OAuthProvider::OpenAI => {
            request
                .header("User-Agent", "codex-cli/0.91.0")
                .form(&[
                    ("grant_type", "authorization_code"),
                    ("client_id", OPENAI_CLIENT_ID),
                    ("code", code),
                    ("redirect_uri", REDIRECT_URI),
                    ("code_verifier", code_verifier),
                ])
                .send()
                .await
        }
        OAuthProvider::Anthropic => {
            request
                .header("Accept", "application/json, text/plain, */*")
                .header("Content-Type", "application/json")
                .header("User-Agent", "axios/1.13.6")
                .json(&json!({
                    "grant_type": "authorization_code",
                    "client_id": ANTHROPIC_CLIENT_ID,
                    "code": code,
                    "redirect_uri": ANTHROPIC_REDIRECT_URI,
                    "code_verifier": code_verifier,
                }))
                .send()
                .await
        }
    }
    .map_err(|error| format!("OAuth token 交换失败: {error}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("OAuth token 交换失败: {status} {}", body.trim()));
    }

    response
        .json::<TokenResponse>()
        .await
        .map_err(|error| format!("解析 OAuth token 响应失败: {error}"))
}

async fn refresh_access_token(
    provider: OAuthProvider,
    refresh_token: &str,
) -> Result<TokenResponse, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|error| format!("创建 OAuth HTTP 客户端失败: {error}"))?;

    let request = client.post(provider.token_url());
    let response = match provider {
        OAuthProvider::OpenAI => {
            request
                .header("User-Agent", "codex-cli/0.91.0")
                .form(&refresh_form_data(refresh_token))
                .send()
                .await
        }
        OAuthProvider::Anthropic => {
            request
                .header("Accept", "application/json, text/plain, */*")
                .header("Content-Type", "application/json")
                .header("User-Agent", "axios/1.13.6")
                .json(&json!({
                    "grant_type": "refresh_token",
                    "refresh_token": refresh_token,
                    "client_id": ANTHROPIC_CLIENT_ID,
                }))
                .send()
                .await
        }
    }
    .map_err(|error| format!("OAuth token 刷新失败: {error}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("OAuth token 刷新失败: {status} {}", body.trim()));
    }

    response
        .json::<TokenResponse>()
        .await
        .map_err(|error| format!("解析 OAuth token 刷新响应失败: {error}"))
}

async fn refresh_kimi_access_token(refresh_token: &str) -> Result<TokenResponse, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|error| format!("创建 Kimi OAuth HTTP 客户端失败: {error}"))?;

    let response = client
        .post(KIMI_TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .form(&HashMap::from([
            ("client_id".to_string(), KIMI_CLIENT_ID.to_string()),
            ("grant_type".to_string(), "refresh_token".to_string()),
            ("refresh_token".to_string(), refresh_token.to_string()),
        ]))
        .send()
        .await
        .map_err(|error| format!("Kimi OAuth token 刷新失败: {error}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "Kimi OAuth token 刷新失败: {status} {}",
            body.trim()
        ));
    }

    response
        .json::<TokenResponse>()
        .await
        .map_err(|error| format!("解析 Kimi OAuth token 刷新响应失败: {error}"))
}

fn should_refresh_token(
    expires_at: chrono::DateTime<Utc>,
    now: chrono::DateTime<Utc>,
    force: bool,
) -> bool {
    force || expires_at - now <= TOKEN_REFRESH_SKEW
}

fn stored_tokens_from_response(
    provider: OAuthProvider,
    account_id: String,
    response: TokenResponse,
    now: chrono::DateTime<Utc>,
    email: Option<String>,
    chatgpt_account_id: Option<String>,
) -> StoredOAuthTokens {
    StoredOAuthTokens {
        provider: provider.key().into(),
        account_id,
        access_token: response.access_token,
        refresh_token: response.refresh_token,
        id_token: response.id_token,
        expires_at: now + Duration::seconds(response.expires_in.max(0)),
        email,
        chatgpt_account_id,
        scope: response.scope,
    }
}

fn merge_refreshed_tokens(
    existing: StoredOAuthTokens,
    response: TokenResponse,
    now: chrono::DateTime<Utc>,
    email: Option<String>,
    chatgpt_account_id: Option<String>,
) -> StoredOAuthTokens {
    StoredOAuthTokens {
        provider: existing.provider,
        account_id: existing.account_id,
        access_token: response.access_token,
        refresh_token: response.refresh_token.or(existing.refresh_token),
        id_token: response.id_token.or(existing.id_token),
        expires_at: now + Duration::seconds(response.expires_in.max(0)),
        email: email.or(existing.email),
        chatgpt_account_id: chatgpt_account_id.or(existing.chatgpt_account_id),
        scope: response.scope.or(existing.scope),
    }
}

fn refreshed_tokens_from_response(
    existing: StoredOAuthTokens,
    response: TokenResponse,
    now: chrono::DateTime<Utc>,
) -> StoredOAuthTokens {
    let provider = OAuthProvider::from_key(&existing.provider).unwrap_or(OAuthProvider::OpenAI);
    let (email, chatgpt_account_id) = oauth_account_metadata(provider, &response);
    merge_refreshed_tokens(existing, response, now, email, chatgpt_account_id)
}

fn refresh_form_data(refresh_token: &str) -> HashMap<String, String> {
    HashMap::from([
        ("grant_type".into(), "refresh_token".into()),
        ("refresh_token".into(), refresh_token.to_string()),
        ("client_id".into(), OPENAI_CLIENT_ID.into()),
        ("scope".into(), OPENAI_REFRESH_SCOPES.into()),
    ])
}

fn json_number(value: &serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::Number(number) => number.as_f64(),
        serde_json::Value::String(raw) => raw.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn parse_id_token(id_token: &str) -> Option<(Option<String>, Option<String>)> {
    let payload = id_token.split('.').nth(1)?;
    let decoded = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let claims = serde_json::from_slice::<IdTokenClaims>(&decoded).ok()?;
    Some((
        claims.email,
        claims.openai_auth.and_then(|auth| auth.chatgpt_account_id),
    ))
}

fn oauth_account_metadata(
    provider: OAuthProvider,
    response: &TokenResponse,
) -> (Option<String>, Option<String>) {
    match provider {
        OAuthProvider::OpenAI => response
            .id_token
            .as_deref()
            .and_then(parse_id_token)
            .unwrap_or((None, None)),
        OAuthProvider::Anthropic => (
            response
                .account
                .as_ref()
                .and_then(|account| account.email_address.clone()),
            response
                .account
                .as_ref()
                .and_then(|account| account.uuid.clone())
                .or_else(|| {
                    response
                        .organization
                        .as_ref()
                        .and_then(|organization| organization.uuid.clone())
                }),
        ),
    }
}

fn random_hex(bytes: usize) -> Result<String, String> {
    Ok(hex::encode(random_bytes(bytes)))
}

fn random_code_verifier(provider: OAuthProvider) -> Result<String, String> {
    match provider {
        OAuthProvider::OpenAI => random_hex(64),
        OAuthProvider::Anthropic => Ok(URL_SAFE_NO_PAD.encode(random_bytes(32))),
    }
}

fn random_bytes(bytes: usize) -> Vec<u8> {
    (0..bytes).map(|_| rand::random::<u8>()).collect::<Vec<_>>()
}

fn code_challenge(verifier: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn stored_oauth_tokens_json_round_trips() {
        let tokens = StoredOAuthTokens {
            provider: crate::models::PROVIDER_OPENAI.into(),
            account_id: "default".into(),
            access_token: "access".into(),
            refresh_token: Some("refresh".into()),
            id_token: Some("id".into()),
            expires_at: Utc.with_ymd_and_hms(2026, 4, 24, 10, 0, 0).unwrap(),
            email: Some("john@example.com".into()),
            chatgpt_account_id: Some("chatgpt-account".into()),
            scope: Some(OPENAI_REFRESH_SCOPES.into()),
        };

        let encoded = serde_json::to_string(&tokens).unwrap();
        let decoded: StoredOAuthTokens = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded, tokens);
    }

    #[test]
    fn should_refresh_token_respects_force_and_skew() {
        let now = Utc.with_ymd_and_hms(2026, 4, 24, 10, 0, 0).unwrap();

        assert!(should_refresh_token(now - Duration::seconds(1), now, false));
        assert!(should_refresh_token(now + Duration::minutes(2), now, false));
        assert!(!should_refresh_token(
            now + Duration::minutes(4),
            now,
            false
        ));
        assert!(should_refresh_token(now + Duration::hours(1), now, true));
    }

    #[test]
    fn refresh_response_without_refresh_token_preserves_existing_refresh_token() {
        let now = Utc.with_ymd_and_hms(2026, 4, 24, 10, 0, 0).unwrap();
        let existing = StoredOAuthTokens {
            provider: crate::models::PROVIDER_OPENAI.into(),
            account_id: "default".into(),
            access_token: "old-access".into(),
            refresh_token: Some("old-refresh".into()),
            id_token: Some("old-id".into()),
            expires_at: now,
            email: Some("old@example.com".into()),
            chatgpt_account_id: Some("old-account".into()),
            scope: Some("old-scope".into()),
        };
        let response = TokenResponse {
            access_token: "new-access".into(),
            refresh_token: None,
            id_token: None,
            expires_in: 3600,
            scope: Some(OPENAI_REFRESH_SCOPES.into()),
            account: None,
            organization: None,
        };

        let next = merge_refreshed_tokens(existing, response, now, None, None);

        assert_eq!(next.access_token, "new-access");
        assert_eq!(next.refresh_token.as_deref(), Some("old-refresh"));
        assert_eq!(next.id_token.as_deref(), Some("old-id"));
        assert_eq!(next.expires_at, now + Duration::seconds(3600));
    }

    #[test]
    fn refresh_response_with_id_token_updates_account_metadata() {
        let now = Utc.with_ymd_and_hms(2026, 4, 24, 10, 0, 0).unwrap();
        let existing = StoredOAuthTokens {
            provider: crate::models::PROVIDER_OPENAI.into(),
            account_id: "default".into(),
            access_token: "old-access".into(),
            refresh_token: Some("old-refresh".into()),
            id_token: Some("old-id".into()),
            expires_at: now,
            email: Some("old@example.com".into()),
            chatgpt_account_id: Some("old-account".into()),
            scope: Some("old-scope".into()),
        };
        let response = TokenResponse {
            access_token: "new-access".into(),
            refresh_token: None,
            id_token: Some(make_id_token("new@example.com", "new-account")),
            expires_in: 3600,
            scope: Some(OPENAI_REFRESH_SCOPES.into()),
            account: None,
            organization: None,
        };

        let next = refreshed_tokens_from_response(existing, response, now);

        assert_eq!(next.email.as_deref(), Some("new@example.com"));
        assert_eq!(next.chatgpt_account_id.as_deref(), Some("new-account"));
    }

    #[tokio::test]
    async fn refresh_locks_are_shared_per_account() {
        let first = refresh_lock_for_account("default").await;
        let second = refresh_lock_for_account("default").await;
        let other = refresh_lock_for_account("other").await;

        assert!(std::sync::Arc::ptr_eq(&first, &second));
        assert!(!std::sync::Arc::ptr_eq(&first, &other));
    }

    #[test]
    fn refresh_form_data_uses_openai_refresh_grant() {
        let form = refresh_form_data("refresh-token");

        assert_eq!(form.get("grant_type"), Some(&"refresh_token".to_string()));
        assert_eq!(
            form.get("refresh_token"),
            Some(&"refresh-token".to_string())
        );
        assert_eq!(form.get("client_id"), Some(&OPENAI_CLIENT_ID.to_string()));
        assert_eq!(form.get("scope"), Some(&OPENAI_REFRESH_SCOPES.to_string()));
    }

    #[test]
    fn anthropic_authorization_url_uses_claude_oauth_config() {
        let auth_url = build_authorization_url(&OAuthProvider::Anthropic, "state-1", "challenge-1");
        let parsed = Url::parse(&auth_url).unwrap();
        let params: HashMap<String, String> = parsed.query_pairs().into_owned().collect();

        assert_eq!(
            parsed.as_str().split('?').next(),
            Some(ANTHROPIC_AUTHORIZE_URL)
        );
        assert_eq!(
            params.get("client_id"),
            Some(&ANTHROPIC_CLIENT_ID.to_string())
        );
        assert_eq!(
            params.get("redirect_uri"),
            Some(&ANTHROPIC_REDIRECT_URI.to_string())
        );
        assert_eq!(params.get("scope"), Some(&ANTHROPIC_SCOPES.to_string()));
        assert_eq!(
            params.get("code_challenge"),
            Some(&"challenge-1".to_string())
        );
        assert_eq!(
            params.get("code_challenge_method"),
            Some(&"S256".to_string())
        );
        assert_eq!(params.get("state"), Some(&"state-1".to_string()));
    }

    #[test]
    fn parse_callback_input_accepts_anthropic_code_state_fragment() {
        let (code, state) = parse_callback_input("anthropic-code#state-1").unwrap();

        assert_eq!(code, "anthropic-code");
        assert_eq!(state.as_deref(), Some("state-1"));
    }

    #[test]
    fn anthropic_token_response_metadata_uses_account_email_and_uuid() {
        let response = TokenResponse {
            access_token: "access".into(),
            refresh_token: Some("refresh".into()),
            id_token: None,
            expires_in: 3600,
            scope: Some(ANTHROPIC_SCOPES.into()),
            account: Some(AnthropicAccountInfo {
                uuid: Some("acct-uuid".into()),
                email_address: Some("claude@example.com".into()),
            }),
            organization: Some(AnthropicOrganizationInfo {
                uuid: Some("org-uuid".into()),
            }),
        };

        let (email, account_id) = oauth_account_metadata(OAuthProvider::Anthropic, &response);

        assert_eq!(email.as_deref(), Some("claude@example.com"));
        assert_eq!(account_id.as_deref(), Some("acct-uuid"));
    }

    fn make_id_token(email: &str, chatgpt_account_id: &str) -> String {
        let payload = serde_json::json!({
            "email": email,
            "https://api.openai.com/auth": {
                "chatgpt_account_id": chatgpt_account_id
            }
        });
        format!(
            "header.{}.sig",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap())
        )
    }
}
