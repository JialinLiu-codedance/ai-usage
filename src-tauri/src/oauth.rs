use crate::{
    models::{OAuthPhase, OAuthStatus, StoredOAuthTokens},
    secrets, settings,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{Duration, Utc};
use serde::Deserialize;
use std::{
    collections::HashMap,
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
const TOKEN_REFRESH_SKEW: Duration = Duration::minutes(3);

static REFRESH_LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();

#[derive(Clone)]
struct OAuthSession {
    state: String,
    code_verifier: String,
    target_account_id: Option<String>,
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
    let session = OAuthSession {
        state: random_hex(32)?,
        code_verifier: random_hex(64)?,
        target_account_id: target_account_id
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
    };
    let code_challenge = code_challenge(&session.code_verifier);
    let auth_url = format!(
        "{OPENAI_AUTHORIZE_URL}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256&id_token_add_organizations=true&codex_cli_simplified_flow=true",
        urlencoding::encode(OPENAI_CLIENT_ID),
        urlencoding::encode(REDIRECT_URI),
        urlencoding::encode(OPENAI_SCOPES),
        urlencoding::encode(&session.state),
        urlencoding::encode(&code_challenge),
    );

    {
        let mut guard = store.session.write().await;
        *guard = Some(session);
    }

    {
        let mut guard = store.status.write().await;
        *guard = OAuthStatus {
            phase: OAuthPhase::Running,
            message: Some("OAuth 已就绪，请完成授权后把浏览器最终回调 URL 粘贴回来".into()),
            email: None,
            auth_url: Some(auth_url.clone()),
        };
    }

    Ok(auth_url)
}

pub async fn complete_openai_oauth(
    app: &AppHandle,
    store: &OAuthStore,
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

    let (code, state) = parse_callback_input(callback_input)?;
    if let Some(state) = state {
        if state != session.state {
            return Err("OAuth state 校验失败，请重新发起授权".into());
        }
    }

    let token = exchange_code(&code, &session.code_verifier).await?;
    let (email, chatgpt_account_id) = token
        .id_token
        .as_deref()
        .and_then(parse_id_token)
        .unwrap_or((None, None));

    let mut current = settings::load_settings(app)?;
    let account_id = settings::upsert_oauth_account(
        &mut current,
        session.target_account_id.clone(),
        email.clone(),
        chatgpt_account_id.clone(),
    );
    let stored_tokens = stored_tokens_from_response(
        account_id.clone(),
        token,
        Utc::now(),
        email.clone(),
        chatgpt_account_id
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
                "OAuth 授权成功，已保存访问令牌和刷新令牌".into()
            }
            None => "OAuth 授权成功，已保存访问令牌".into(),
            Some(_) => "OAuth 授权成功，已保存访问令牌".into(),
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
        .ok_or_else(|| "请先完成 OpenAI OAuth 授权".to_string())?;
    let now = Utc::now();
    if !should_refresh_token(existing.expires_at, now, force) {
        return Ok(existing);
    }

    let refresh_lock = refresh_lock_for_account(account_id).await;
    let _guard = refresh_lock.lock().await;

    let existing = secrets::load_oauth_tokens(account_id)?
        .ok_or_else(|| "请先完成 OpenAI OAuth 授权".to_string())?;
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
    let response = refresh_access_token(refresh_token).await?;
    let updated = refreshed_tokens_from_response(existing, response, now);
    secrets::save_oauth_tokens(account_id, &updated)?;
    Ok(updated)
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

async fn exchange_code(code: &str, code_verifier: &str) -> Result<TokenResponse, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|error| format!("创建 OAuth HTTP 客户端失败: {error}"))?;

    let response = client
        .post(OPENAI_TOKEN_URL)
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

async fn refresh_access_token(refresh_token: &str) -> Result<TokenResponse, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|error| format!("创建 OAuth HTTP 客户端失败: {error}"))?;

    let response = client
        .post(OPENAI_TOKEN_URL)
        .header("User-Agent", "codex-cli/0.91.0")
        .form(&refresh_form_data(refresh_token))
        .send()
        .await
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

fn should_refresh_token(
    expires_at: chrono::DateTime<Utc>,
    now: chrono::DateTime<Utc>,
    force: bool,
) -> bool {
    force || expires_at - now <= TOKEN_REFRESH_SKEW
}

fn stored_tokens_from_response(
    account_id: String,
    response: TokenResponse,
    now: chrono::DateTime<Utc>,
    email: Option<String>,
    chatgpt_account_id: Option<String>,
) -> StoredOAuthTokens {
    StoredOAuthTokens {
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
    let (email, chatgpt_account_id) = response
        .id_token
        .as_deref()
        .and_then(parse_id_token)
        .unwrap_or((None, None));
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

fn parse_id_token(id_token: &str) -> Option<(Option<String>, Option<String>)> {
    let payload = id_token.split('.').nth(1)?;
    let decoded = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let claims = serde_json::from_slice::<IdTokenClaims>(&decoded).ok()?;
    Some((
        claims.email,
        claims.openai_auth.and_then(|auth| auth.chatgpt_account_id),
    ))
}

fn random_hex(bytes: usize) -> Result<String, String> {
    let random = (0..bytes).map(|_| rand::random::<u8>()).collect::<Vec<_>>();
    Ok(hex::encode(random))
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
