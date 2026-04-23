use crate::models::{AppSettings, ProbeCredentials};
use keyring::Entry;

const SERVICE_NAME: &str = "com.liujialin.ai-usage";
const SECRET_ACCOUNT: &str = "default";

pub fn load_secret(settings: &AppSettings) -> Result<Option<ProbeCredentials>, String> {
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
