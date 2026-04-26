use crate::{
    models::{
        default_account_id, AppSettings, AuthMode, ConnectedAccount, SaveSettingsInput,
        PROVIDER_GLM, PROVIDER_KIMI, PROVIDER_MINIMAX, PROVIDER_OPENAI,
    },
    secrets, storage,
};
use std::collections::HashSet;
use tauri::AppHandle;

const SETTINGS_FILE: &str = "settings.json";

pub fn load_settings(app: &AppHandle) -> Result<AppSettings, String> {
    let mut settings = storage::read_json::<AppSettings>(app, SETTINGS_FILE)?.unwrap_or_default();
    hydrate_connected_accounts(&mut settings)?;
    settings.secret_configured = secret_configured_for_settings(&settings)?;
    Ok(settings)
}

pub fn save_settings(app: &AppHandle, input: SaveSettingsInput) -> Result<AppSettings, String> {
    let existing = load_settings(app)?;
    let mut settings = AppSettings {
        account_id: sanitize_account_id(input.account_id),
        account_name: input.account_name,
        auth_mode: input.auth_mode,
        base_url_override: sanitize_optional(input.base_url_override),
        chatgpt_account_id: sanitize_optional(input.chatgpt_account_id),
        accounts: existing.accounts,
        refresh_interval_minutes: input.refresh_interval_minutes.max(1),
        low_quota_threshold_percent: input.low_quota_threshold_percent.clamp(0.0, 100.0),
        notify_on_low_quota: input.notify_on_low_quota,
        notify_on_reset: input.notify_on_reset,
        reset_notify_lead_minutes: input.reset_notify_lead_minutes.max(1),
        secret_configured: false,
    };

    if let Some(secret) = input
        .auth_secret
        .and_then(|value| sanitize_optional(Some(value)))
    {
        secrets::save_secret(&secret)?;
    }

    if matches!(settings.auth_mode, AuthMode::OAuth) {
        sync_active_account_metadata(&mut settings);
    }

    write_settings(app, &settings)
}

pub fn delete_account(app: &AppHandle, account_id: &str) -> Result<AppSettings, String> {
    let mut settings = load_settings(app)?;
    delete_account_from_settings(&mut settings, account_id);
    write_settings(app, &settings)
}

pub fn write_settings(app: &AppHandle, settings: &AppSettings) -> Result<AppSettings, String> {
    storage::write_json(app, SETTINGS_FILE, settings)?;
    load_settings(app)
}

pub fn normalize_account_id(input: &str) -> String {
    sanitize_account_id(input.to_string())
}

pub(crate) fn upsert_oauth_account(
    settings: &mut AppSettings,
    target_account_id: Option<String>,
    email: Option<String>,
    chatgpt_account_id: Option<String>,
) -> String {
    upsert_provider_oauth_account(
        settings,
        PROVIDER_OPENAI,
        target_account_id,
        email,
        chatgpt_account_id,
    )
}

pub(crate) fn upsert_provider_oauth_account(
    settings: &mut AppSettings,
    provider: &str,
    target_account_id: Option<String>,
    email: Option<String>,
    provider_account_id: Option<String>,
) -> String {
    let provider = normalize_provider(provider);
    let account_id = resolve_oauth_account_id(
        settings,
        &provider,
        target_account_id.as_deref(),
        email.as_deref(),
        provider_account_id.as_deref(),
    );
    let account_name = email
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .or_else(|| {
            settings
                .accounts
                .iter()
                .find(|account| account.account_id == account_id)
                .map(|account| account.account_name.clone())
        })
        .unwrap_or_else(|| default_account_name_for_provider(&provider).into());

    let next_account = ConnectedAccount {
        account_id: account_id.clone(),
        account_name: account_name.clone(),
        provider: provider.clone(),
        auth_mode: AuthMode::OAuth,
        chatgpt_account_id: provider_account_id.clone(),
        secret_configured: true,
    };

    if let Some(existing) = settings
        .accounts
        .iter_mut()
        .find(|account| account.account_id == account_id)
    {
        *existing = next_account;
    } else {
        settings.accounts.push(next_account);
    }

    settings.account_id = account_id.clone();
    settings.account_name = account_name;
    settings.auth_mode = AuthMode::OAuth;
    settings.chatgpt_account_id = provider_account_id;
    settings.secret_configured = true;
    account_id
}

pub(crate) fn upsert_api_key_account(
    settings: &mut AppSettings,
    provider: &str,
    target_account_id: Option<String>,
    account_name: String,
) -> String {
    let provider = normalize_provider(provider);
    let account_id = resolve_oauth_account_id(
        settings,
        &provider,
        target_account_id.as_deref(),
        Some(account_name.as_str()),
        None,
    );
    let account_name = account_name.trim();
    let account_name = if account_name.is_empty() {
        default_account_name_for_provider(&provider).to_string()
    } else {
        account_name.to_string()
    };

    let next_account = ConnectedAccount {
        account_id: account_id.clone(),
        account_name: account_name.clone(),
        provider,
        auth_mode: AuthMode::ApiKey,
        chatgpt_account_id: None,
        secret_configured: true,
    };

    if let Some(existing) = settings
        .accounts
        .iter_mut()
        .find(|account| account.account_id == account_id)
    {
        *existing = next_account;
    } else {
        settings.accounts.push(next_account);
    }

    settings.account_id = account_id.clone();
    settings.account_name = account_name;
    settings.auth_mode = AuthMode::ApiKey;
    settings.chatgpt_account_id = None;
    settings.secret_configured = true;
    account_id
}

pub(crate) fn delete_account_from_settings(settings: &mut AppSettings, account_id: &str) -> bool {
    let normalized = sanitize_account_id(account_id.to_string());
    let previous_len = settings.accounts.len();
    settings
        .accounts
        .retain(|account| account.account_id != normalized);
    let removed = settings.accounts.len() != previous_len;

    if removed
        && !settings
            .accounts
            .iter()
            .any(|account| account.account_id == settings.account_id)
    {
        if let Some(next_account) = settings.accounts.first().cloned() {
            activate_account(settings, &next_account);
        } else {
            reset_account_binding(settings);
        }
    }

    removed
}

fn hydrate_connected_accounts(settings: &mut AppSettings) -> Result<(), String> {
    let active_secret_configured = secret_configured_for_settings(settings)?;
    if active_secret_configured
        && matches!(settings.auth_mode, AuthMode::OAuth)
        && !settings
            .accounts
            .iter()
            .any(|account| account.account_id == settings.account_id)
    {
        settings.accounts.push(ConnectedAccount {
            account_id: settings.account_id.clone(),
            account_name: settings.account_name.clone(),
            provider: "openai".into(),
            auth_mode: settings.auth_mode.clone(),
            chatgpt_account_id: settings.chatgpt_account_id.clone(),
            secret_configured: true,
        });
    }

    let mut hydrated = Vec::with_capacity(settings.accounts.len());
    let mut seen = HashSet::new();
    for mut account in std::mem::take(&mut settings.accounts) {
        account.account_id = sanitize_account_id(account.account_id);
        account.provider = normalize_provider(&account.provider);
        account.account_name = account.account_name.trim().to_string();
        if account.account_name.is_empty() {
            account.account_name = default_account_name_for_provider(&account.provider).into();
        }
        account.secret_configured = secret_configured_for_account(&account)?;
        if account.secret_configured && seen.insert(account.account_id.clone()) {
            hydrated.push(account);
        }
    }
    settings.accounts = hydrated;
    if !settings
        .accounts
        .iter()
        .any(|account| account.account_id == settings.account_id)
    {
        if let Some(next_account) = settings.accounts.first().cloned() {
            activate_account(settings, &next_account);
        }
    }
    Ok(())
}

fn sync_active_account_metadata(settings: &mut AppSettings) {
    let active_id = settings.account_id.clone();
    if let Some(account) = settings
        .accounts
        .iter_mut()
        .find(|account| account.account_id == active_id)
    {
        account.account_name = settings.account_name.clone();
        account.auth_mode = settings.auth_mode.clone();
        account.chatgpt_account_id = settings.chatgpt_account_id.clone();
    }
}

fn secret_configured_for_settings(settings: &AppSettings) -> Result<bool, String> {
    match settings.auth_mode {
        AuthMode::OAuth => secrets::oauth_secret_configured(&settings.account_id),
        _ => secrets::load_secret(settings).map(|secret| secret.is_some()),
    }
}

fn secret_configured_for_account(account: &ConnectedAccount) -> Result<bool, String> {
    match account.auth_mode {
        AuthMode::OAuth => secrets::oauth_secret_configured(&account.account_id),
        AuthMode::ApiKey => {
            secrets::account_secret_configured(&account.account_id).map(|configured| {
                configured
                    || account.secret_configured
                        && !matches!(account.provider.as_str(), PROVIDER_GLM | PROVIDER_MINIMAX)
            })
        }
        _ => Ok(account.secret_configured),
    }
}

fn resolve_oauth_account_id(
    settings: &AppSettings,
    provider: &str,
    target_account_id: Option<&str>,
    email: Option<&str>,
    provider_account_id: Option<&str>,
) -> String {
    if let Some(target) = target_account_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return sanitize_account_id(target.to_string());
    }

    if let Some(provider_account_id) = provider_account_id {
        if let Some(account) = settings.accounts.iter().find(|account| {
            account.provider == provider
                && account.chatgpt_account_id.as_deref() == Some(provider_account_id)
        }) {
            return account.account_id.clone();
        }
    }

    if let Some(email) = email.map(str::trim).filter(|value| !value.is_empty()) {
        if let Some(account) = settings.accounts.iter().find(|account| {
            account.provider == provider && account.account_name.eq_ignore_ascii_case(email)
        }) {
            return account.account_id.clone();
        }
    }

    let raw_base = provider_account_id
        .or(email)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(provider);
    let base = if provider == PROVIDER_OPENAI {
        sanitize_generated_account_id(raw_base)
    } else {
        sanitize_generated_account_id(&format!("{provider}-{raw_base}"))
    };
    unique_account_id(settings, &base)
}

fn unique_account_id(settings: &AppSettings, base: &str) -> String {
    let existing = settings
        .accounts
        .iter()
        .map(|account| account.account_id.as_str())
        .collect::<HashSet<_>>();
    if !existing.contains(base) {
        return base.to_string();
    }

    for index in 2.. {
        let candidate = format!("{base}-{index}");
        if !existing.contains(candidate.as_str()) {
            return candidate;
        }
    }

    unreachable!("unbounded account id suffix search should always return")
}

fn sanitize_generated_account_id(input: &str) -> String {
    let mut output = String::new();
    let mut previous_dash = false;
    for character in input.trim().chars() {
        if character.is_ascii_alphanumeric() || character == '_' || character == '-' {
            output.push(character.to_ascii_lowercase());
            previous_dash = false;
        } else if !previous_dash {
            output.push('-');
            previous_dash = true;
        }
    }

    let trimmed = output.trim_matches('-').to_string();
    if trimmed.is_empty() {
        default_account_id()
    } else {
        trimmed
    }
}

fn activate_account(settings: &mut AppSettings, account: &ConnectedAccount) {
    settings.account_id = account.account_id.clone();
    settings.account_name = account.account_name.clone();
    settings.auth_mode = account.auth_mode.clone();
    settings.chatgpt_account_id = account.chatgpt_account_id.clone();
    settings.secret_configured = account.secret_configured;
}

fn reset_account_binding(settings: &mut AppSettings) {
    settings.account_id = default_account_id();
    settings.account_name = "OpenAI Account".into();
    settings.auth_mode = AuthMode::ApiKey;
    settings.chatgpt_account_id = None;
    settings.secret_configured = false;
}

fn normalize_provider(provider: &str) -> String {
    let trimmed = provider.trim();
    if trimmed.is_empty() {
        PROVIDER_OPENAI.into()
    } else {
        trimmed.to_ascii_lowercase()
    }
}

fn default_account_name_for_provider(provider: &str) -> &'static str {
    match provider {
        crate::models::PROVIDER_ANTHROPIC => "Anthropic Account",
        PROVIDER_GLM => "GLM Account",
        PROVIDER_KIMI => "Kimi Account",
        PROVIDER_MINIMAX => "MiniMax Account",
        _ => "OpenAI Account",
    }
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

fn sanitize_account_id(input: String) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        crate::models::default_account_id()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{default_account_id, ConnectedAccount};

    fn openai_account(id: &str, name: &str, chatgpt_account_id: Option<&str>) -> ConnectedAccount {
        ConnectedAccount {
            account_id: id.into(),
            account_name: name.into(),
            provider: "openai".into(),
            auth_mode: AuthMode::OAuth,
            chatgpt_account_id: chatgpt_account_id.map(str::to_string),
            secret_configured: true,
        }
    }

    #[test]
    fn oauth_completion_adds_second_account_without_replacing_first() {
        let mut settings = AppSettings {
            account_id: "first".into(),
            account_name: "first@example.com".into(),
            auth_mode: AuthMode::OAuth,
            accounts: vec![openai_account(
                "first",
                "first@example.com",
                Some("acct-first"),
            )],
            ..AppSettings::default()
        };

        let second_id = upsert_oauth_account(
            &mut settings,
            None,
            Some("second@example.com".into()),
            Some("acct-second".into()),
        );

        assert_eq!(second_id, "acct-second");
        assert_eq!(settings.account_id, "acct-second");
        assert_eq!(settings.account_name, "second@example.com");
        assert_eq!(settings.accounts.len(), 2);
        assert_eq!(settings.accounts[0].account_name, "first@example.com");
        assert_eq!(settings.accounts[1].account_name, "second@example.com");
    }

    #[test]
    fn oauth_completion_reauthorizes_target_account_in_place() {
        let mut settings = AppSettings {
            account_id: "first".into(),
            account_name: "first@example.com".into(),
            auth_mode: AuthMode::OAuth,
            accounts: vec![
                openai_account("first", "first@example.com", Some("acct-first")),
                openai_account("second", "second@example.com", Some("acct-second")),
            ],
            ..AppSettings::default()
        };

        let account_id = upsert_oauth_account(
            &mut settings,
            Some("first".into()),
            Some("first-new@example.com".into()),
            Some("acct-first-new".into()),
        );

        assert_eq!(account_id, "first");
        assert_eq!(settings.account_id, "first");
        assert_eq!(settings.accounts.len(), 2);
        assert_eq!(settings.accounts[0].account_name, "first-new@example.com");
        assert_eq!(
            settings.accounts[0].chatgpt_account_id.as_deref(),
            Some("acct-first-new")
        );
        assert_eq!(settings.accounts[1].account_name, "second@example.com");
    }

    #[test]
    fn anthropic_oauth_completion_stores_provider_and_prefixed_account_id() {
        let mut settings = AppSettings::default();

        let account_id = upsert_provider_oauth_account(
            &mut settings,
            crate::models::PROVIDER_ANTHROPIC,
            None,
            Some("claude@example.com".into()),
            Some("acct-uuid".into()),
        );

        assert_eq!(account_id, "anthropic-acct-uuid");
        assert_eq!(
            settings.active_provider(),
            crate::models::PROVIDER_ANTHROPIC
        );
        assert_eq!(settings.account_name, "claude@example.com");
        assert_eq!(settings.accounts.len(), 1);
        assert_eq!(
            settings.accounts[0].provider,
            crate::models::PROVIDER_ANTHROPIC
        );
    }

    #[test]
    fn kimi_oauth_imports_support_multiple_named_accounts() {
        let mut settings = AppSettings::default();

        let work_id = upsert_provider_oauth_account(
            &mut settings,
            crate::models::PROVIDER_KIMI,
            None,
            Some("Kimi Work".into()),
            None,
        );
        let personal_id = upsert_provider_oauth_account(
            &mut settings,
            crate::models::PROVIDER_KIMI,
            None,
            Some("Kimi Personal".into()),
            None,
        );

        assert_eq!(work_id, "kimi-kimi-work");
        assert_eq!(personal_id, "kimi-kimi-personal");
        assert_eq!(settings.accounts.len(), 2);
        assert_eq!(
            settings
                .accounts
                .iter()
                .map(|account| (account.provider.as_str(), account.account_name.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (crate::models::PROVIDER_KIMI, "Kimi Work"),
                (crate::models::PROVIDER_KIMI, "Kimi Personal"),
            ]
        );
        assert_eq!(settings.active_provider(), crate::models::PROVIDER_KIMI);
    }

    #[test]
    fn upsert_api_key_account_appends_multiple_minimax_accounts() {
        let mut settings = AppSettings::default();

        let first_id = upsert_api_key_account(
            &mut settings,
            crate::models::PROVIDER_MINIMAX,
            None,
            "MiniMax Work".into(),
        );
        let second_id = upsert_api_key_account(
            &mut settings,
            crate::models::PROVIDER_MINIMAX,
            None,
            "MiniMax Personal".into(),
        );

        assert_ne!(first_id, second_id);
        assert_eq!(settings.accounts.len(), 2);
        assert_eq!(
            settings.accounts[0].provider,
            crate::models::PROVIDER_MINIMAX
        );
        assert_eq!(settings.accounts[0].auth_mode, AuthMode::ApiKey);
        assert_eq!(settings.accounts[0].account_name, "MiniMax Work");
        assert!(settings.accounts[0].secret_configured);
        assert_eq!(settings.account_id, second_id);
        assert_eq!(settings.account_name, "MiniMax Personal");
        assert_eq!(settings.auth_mode, AuthMode::ApiKey);
    }

    #[test]
    fn upsert_api_key_account_updates_target_minimax_account() {
        let mut settings = AppSettings::default();

        let account_id = upsert_api_key_account(
            &mut settings,
            crate::models::PROVIDER_MINIMAX,
            None,
            "MiniMax Work".into(),
        );
        let updated_id = upsert_api_key_account(
            &mut settings,
            crate::models::PROVIDER_MINIMAX,
            Some(account_id.clone()),
            "MiniMax Renamed".into(),
        );

        assert_eq!(updated_id, account_id);
        assert_eq!(settings.accounts.len(), 1);
        assert_eq!(settings.accounts[0].account_name, "MiniMax Renamed");
        assert_eq!(settings.account_id, account_id);
        assert_eq!(settings.account_name, "MiniMax Renamed");
    }

    #[test]
    fn upsert_api_key_account_appends_multiple_glm_accounts() {
        let mut settings = AppSettings::default();

        let first_id = upsert_api_key_account(
            &mut settings,
            crate::models::PROVIDER_GLM,
            None,
            "GLM Work".into(),
        );
        let second_id = upsert_api_key_account(
            &mut settings,
            crate::models::PROVIDER_GLM,
            None,
            "GLM Personal".into(),
        );

        assert_ne!(first_id, second_id);
        assert_eq!(settings.accounts.len(), 2);
        assert_eq!(
            settings
                .accounts
                .iter()
                .map(|account| (
                    account.provider.as_str(),
                    account.auth_mode.clone(),
                    account.account_name.as_str()
                ))
                .collect::<Vec<_>>(),
            vec![
                (crate::models::PROVIDER_GLM, AuthMode::ApiKey, "GLM Work"),
                (
                    crate::models::PROVIDER_GLM,
                    AuthMode::ApiKey,
                    "GLM Personal"
                ),
            ]
        );
        assert_eq!(settings.account_id, second_id);
        assert_eq!(settings.account_name, "GLM Personal");
    }

    #[test]
    fn upsert_api_key_account_updates_target_glm_account() {
        let mut settings = AppSettings::default();

        let account_id = upsert_api_key_account(
            &mut settings,
            crate::models::PROVIDER_GLM,
            None,
            "GLM Work".into(),
        );
        let updated_id = upsert_api_key_account(
            &mut settings,
            crate::models::PROVIDER_GLM,
            Some(account_id.clone()),
            "GLM Renamed".into(),
        );

        assert_eq!(updated_id, account_id);
        assert_eq!(settings.accounts.len(), 1);
        assert_eq!(settings.accounts[0].provider, crate::models::PROVIDER_GLM);
        assert_eq!(settings.accounts[0].account_name, "GLM Renamed");
        assert_eq!(settings.account_id, account_id);
        assert_eq!(settings.account_name, "GLM Renamed");
    }

    #[test]
    fn deleting_active_account_selects_remaining_account_and_preserves_preferences() {
        let mut settings = AppSettings {
            account_id: "second".into(),
            account_name: "second@example.com".into(),
            auth_mode: AuthMode::OAuth,
            refresh_interval_minutes: 30,
            low_quota_threshold_percent: 25.0,
            accounts: vec![
                openai_account("first", "first@example.com", Some("acct-first")),
                openai_account("second", "second@example.com", Some("acct-second")),
            ],
            ..AppSettings::default()
        };

        assert!(delete_account_from_settings(&mut settings, "second"));

        assert_eq!(settings.account_id, "first");
        assert_eq!(settings.account_name, "first@example.com");
        assert_eq!(settings.refresh_interval_minutes, 30);
        assert_eq!(settings.low_quota_threshold_percent, 25.0);
        assert_eq!(settings.accounts.len(), 1);
        assert_eq!(settings.accounts[0].account_id, "first");
    }

    #[test]
    fn deleting_last_account_resets_account_binding_only() {
        let mut settings = AppSettings {
            account_id: "only".into(),
            account_name: "only@example.com".into(),
            auth_mode: AuthMode::OAuth,
            refresh_interval_minutes: 60,
            accounts: vec![openai_account(
                "only",
                "only@example.com",
                Some("acct-only"),
            )],
            ..AppSettings::default()
        };

        assert!(delete_account_from_settings(&mut settings, "only"));

        assert_eq!(settings.account_id, default_account_id());
        assert_eq!(settings.account_name, "OpenAI Account");
        assert!(matches!(settings.auth_mode, AuthMode::ApiKey));
        assert_eq!(settings.refresh_interval_minutes, 60);
        assert!(settings.accounts.is_empty());
    }
}
