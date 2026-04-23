use crate::{
    models::{AppSettings, SaveSettingsInput},
    secrets,
    storage,
};
use tauri::AppHandle;

const SETTINGS_FILE: &str = "settings.json";

pub fn load_settings(app: &AppHandle) -> Result<AppSettings, String> {
    let mut settings = storage::read_json::<AppSettings>(app, SETTINGS_FILE)?.unwrap_or_default();
    settings.secret_configured = secrets::load_secret(&settings)?.is_some();
    Ok(settings)
}

pub fn save_settings(app: &AppHandle, input: SaveSettingsInput) -> Result<AppSettings, String> {
    let mut settings = AppSettings {
        account_name: input.account_name,
        auth_mode: input.auth_mode,
        base_url_override: sanitize_optional(input.base_url_override),
        chatgpt_account_id: sanitize_optional(input.chatgpt_account_id),
        refresh_interval_minutes: input.refresh_interval_minutes.max(1),
        low_quota_threshold_percent: input.low_quota_threshold_percent.clamp(0.0, 100.0),
        notify_on_low_quota: input.notify_on_low_quota,
        notify_on_reset: input.notify_on_reset,
        reset_notify_lead_minutes: input.reset_notify_lead_minutes.max(1),
        secret_configured: false,
    };

    if let Some(secret) = input.auth_secret.and_then(|value| sanitize_optional(Some(value))) {
        secrets::save_secret(&secret)?;
    }

    settings.secret_configured = secrets::load_secret(&settings)?.is_some();
    storage::write_json(app, SETTINGS_FILE, &settings)?;
    Ok(settings)
}

fn sanitize_optional(input: Option<String>) -> Option<String> {
    input.and_then(|value| {
        let trimmed = value.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}
