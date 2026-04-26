use crate::models::{
    default_account_id, AppSettings, AuthMode, ProbeCredentials, StoredOAuthTokens,
};
use keyring::Entry;

const SERVICE_NAME: &str = "com.liujialin.ai-usage";
const SECRET_ACCOUNT: &str = "default";

pub fn load_secret(settings: &AppSettings) -> Result<Option<ProbeCredentials>, String> {
    if matches!(settings.auth_mode, AuthMode::OAuth) {
        return Ok(
            load_oauth_tokens(&settings.account_id)?.map(|tokens| ProbeCredentials {
                auth_mode: settings.auth_mode.clone(),
                secret: tokens.access_token,
                chatgpt_account_id: tokens
                    .chatgpt_account_id
                    .or_else(|| settings.chatgpt_account_id.clone()),
            }),
        );
    }

    let entry = Entry::new(SERVICE_NAME, SECRET_ACCOUNT)
        .map_err(|error| format!("初始化 Keychain 失败: {error}"))?;
    match entry.get_password() {
        Ok(secret) => {
            if secret.trim().is_empty() {
                return Ok(None);
            }

            Ok(Some(ProbeCredentials {
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
