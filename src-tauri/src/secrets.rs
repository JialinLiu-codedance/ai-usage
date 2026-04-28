use crate::models::{
    default_account_id, AppSettings, AuthMode, ProbeCredentials, StoredOAuthTokens, PROVIDER_OPENAI,
};
use base64::{engine::general_purpose, Engine as _};
use keyring::Entry;

const SERVICE_NAME: &str = "com.liujialin.ai-usage";
const SECRET_ACCOUNT: &str = "default";
const ACCOUNT_SECRET_PREFIX: &str = "account-secret:";
const GH_CLI_KEYCHAIN_SERVICE: &str = "gh:github.com";

pub fn load_secret(settings: &AppSettings) -> Result<Option<ProbeCredentials>, String> {
    if matches!(settings.auth_mode, AuthMode::OAuth) {
        return Ok(
            load_oauth_tokens(&settings.account_id)?.map(|tokens| ProbeCredentials {
                provider: settings.active_provider().to_string(),
                auth_mode: settings.auth_mode.clone(),
                secret: tokens.access_token,
                chatgpt_account_id: tokens
                    .chatgpt_account_id
                    .or_else(|| settings.chatgpt_account_id.clone()),
            }),
        );
    }

    let active_provider = settings.active_provider().to_string();
    if let Some(secret) = load_account_secret(&settings.account_id)? {
        return Ok(Some(ProbeCredentials {
            provider: active_provider,
            auth_mode: settings.auth_mode.clone(),
            secret,
            chatgpt_account_id: settings.chatgpt_account_id.clone(),
        }));
    }
    if active_provider != PROVIDER_OPENAI {
        return Ok(None);
    }

    let entry = Entry::new(SERVICE_NAME, SECRET_ACCOUNT)
        .map_err(|error| format!("初始化 Keychain 失败: {error}"))?;
    match entry.get_password() {
        Ok(secret) => {
            if secret.trim().is_empty() {
                return Ok(None);
            }

            Ok(Some(ProbeCredentials {
                provider: settings.active_provider().to_string(),
                auth_mode: settings.auth_mode.clone(),
                secret,
                chatgpt_account_id: settings.chatgpt_account_id.clone(),
            }))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(format!("读取 Keychain 失败: {error}")),
    }
}

pub fn save_secret(secret: &str) -> Result<(), String> {
    let entry = Entry::new(SERVICE_NAME, SECRET_ACCOUNT)
        .map_err(|error| format!("初始化 Keychain 失败: {error}"))?;
    entry
        .set_password(secret)
        .map_err(|error| format!("写入 Keychain 失败: {error}"))
}

pub fn load_account_secret(account_id: &str) -> Result<Option<String>, String> {
    let entry = Entry::new(SERVICE_NAME, &account_secret_keychain_account(account_id))
        .map_err(|error| format!("初始化账号 Keychain 失败: {error}"))?;
    match entry.get_password() {
        Ok(secret) => {
            if secret.trim().is_empty() {
                return Ok(None);
            }
            Ok(Some(secret))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(format!("读取账号 Keychain 失败: {error}")),
    }
}

pub fn save_account_secret(account_id: &str, secret: &str) -> Result<(), String> {
    let entry = Entry::new(SERVICE_NAME, &account_secret_keychain_account(account_id))
        .map_err(|error| format!("初始化账号 Keychain 失败: {error}"))?;
    entry
        .set_password(secret)
        .map_err(|error| format!("写入账号 Keychain 失败: {error}"))
}

pub fn delete_account_secret(account_id: &str) -> Result<(), String> {
    let entry = Entry::new(SERVICE_NAME, &account_secret_keychain_account(account_id))
        .map_err(|error| format!("初始化账号 Keychain 失败: {error}"))?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(format!("删除账号 Keychain 凭证失败: {error}")),
    }
}

pub fn account_secret_configured(account_id: &str) -> Result<bool, String> {
    Ok(load_account_secret(account_id)?.is_some())
}

pub fn load_github_cli_token() -> Result<Option<String>, String> {
    load_macos_generic_password_by_service(GH_CLI_KEYCHAIN_SERVICE)
        .map(|token| token.and_then(|value| normalize_github_cli_token(&value)))
}

pub fn load_oauth_tokens(account_id: &str) -> Result<Option<StoredOAuthTokens>, String> {
    let entry = Entry::new(SERVICE_NAME, &oauth_keychain_account(account_id))
        .map_err(|error| format!("初始化 OAuth Keychain 失败: {error}"))?;
    match entry.get_password() {
        Ok(secret) => {
            if secret.trim().is_empty() {
                return Ok(None);
            }
            serde_json::from_str::<StoredOAuthTokens>(&secret)
                .map(Some)
                .map_err(|error| format!("解析 OAuth Keychain 凭证失败: {error}"))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(format!("读取 OAuth Keychain 失败: {error}")),
    }
}

pub fn save_oauth_tokens(account_id: &str, tokens: &StoredOAuthTokens) -> Result<(), String> {
    let entry = Entry::new(SERVICE_NAME, &oauth_keychain_account(account_id))
        .map_err(|error| format!("初始化 OAuth Keychain 失败: {error}"))?;
    let encoded =
        serde_json::to_string(tokens).map_err(|error| format!("序列化 OAuth 凭证失败: {error}"))?;
    entry
        .set_password(&encoded)
        .map_err(|error| format!("写入 OAuth Keychain 失败: {error}"))
}

pub fn delete_oauth_tokens(account_id: &str) -> Result<(), String> {
    let entry = Entry::new(SERVICE_NAME, &oauth_keychain_account(account_id))
        .map_err(|error| format!("初始化 OAuth Keychain 失败: {error}"))?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(format!("删除 OAuth Keychain 凭证失败: {error}")),
    }
}

pub fn oauth_secret_configured(account_id: &str) -> Result<bool, String> {
    Ok(load_oauth_tokens(account_id)?.is_some())
}

fn oauth_keychain_account(account_id: &str) -> String {
    let trimmed = account_id.trim();
    let id = if trimmed.is_empty() {
        default_account_id()
    } else {
        trimmed.to_string()
    };
    format!("openai-oauth:{id}")
}

fn account_secret_keychain_account(account_id: &str) -> String {
    let trimmed = account_id.trim();
    let id = if trimmed.is_empty() {
        default_account_id()
    } else {
        trimmed.to_string()
    };
    format!("{ACCOUNT_SECRET_PREFIX}{id}")
}

#[cfg(target_os = "macos")]
fn load_macos_generic_password_by_service(service: &str) -> Result<Option<String>, String> {
    let output = std::process::Command::new("security")
        .args(["find-generic-password", "-s", service, "-w"])
        .output()
        .map_err(|error| format!("读取 GitHub CLI Keychain 失败: {error}"))?;

    if !output.status.success() {
        return Ok(None);
    }

    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if token.is_empty() {
        Ok(None)
    } else {
        Ok(Some(token))
    }
}

#[cfg(not(target_os = "macos"))]
fn load_macos_generic_password_by_service(_service: &str) -> Result<Option<String>, String> {
    Ok(None)
}

fn normalize_github_cli_token(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(encoded) = trimmed.strip_prefix("go-keyring-base64:") {
        let decoded = general_purpose::STANDARD.decode(encoded.trim()).ok()?;
        let token = String::from_utf8(decoded).ok()?;
        let token = token.trim().to_string();
        if token.is_empty() {
            None
        } else {
            Some(token)
        }
    } else {
        Some(trimmed.to_string())
    }
}
