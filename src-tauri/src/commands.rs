use crate::{
    errors::{ProviderError, ProviderErrorKind},
    models::{
        AccountQuotaStatus, AppSettings, AppStatus, AuthMode, ConnectedAccount,
        ConnectionTestResult, QuotaSnapshot, SaveSettingsInput, PROVIDER_ANTHROPIC, PROVIDER_KIMI,
        PROVIDER_OPENAI,
    },
    oauth, panel, provider, secrets, settings,
    state::StateStore,
    storage,
};
use chrono::Utc;
use std::{collections::HashMap, future::Future};
use tauri::{AppHandle, State};

const SNAPSHOTS_FILE: &str = "snapshots.json";

#[tauri::command]
pub async fn get_current_quota(
    app: AppHandle,
    store: State<'_, StateStore>,
) -> Result<AppStatus, String> {
    hydrate_cached_snapshot(&app, &store).await?;
    Ok(store.inner.read().await.clone())
}

#[tauri::command]
pub async fn refresh_quota(
    app: AppHandle,
    store: State<'_, StateStore>,
) -> Result<AppStatus, String> {
    refresh_inner(&app, &store).await?;
    Ok(store.inner.read().await.clone())
}

#[tauri::command]
pub async fn test_connection(app: AppHandle) -> Result<ConnectionTestResult, String> {
    let settings = settings::load_settings(&app)?;
    let message = run_with_oauth_retry(
        &settings,
        || async {
            let credentials = secrets::load_secret(&settings)
                .map_err(|error| ProviderError::new(ProviderErrorKind::Unknown, error))?
                .ok_or_else(|| {
                    ProviderError::new(ProviderErrorKind::Unauthorized, "请先在设置中填写认证值")
                })?;

            provider::test_connection(
                &settings.account_id,
                &settings.account_name,
                settings.base_url_override.as_deref(),
                &credentials,
            )
            .await
        },
        |force| {
            let account_id = settings.account_id.clone();
            async move {
                oauth::ensure_fresh_token(&account_id, force)
                    .await
                    .map(|_| ())
                    .map_err(|error| ProviderError::new(ProviderErrorKind::Unauthorized, error))
            }
        },
    )
    .await
    .map_err(|error| error.message)?;

    Ok(ConnectionTestResult {
        success: true,
        message,
    })
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Result<AppSettings, String> {
    settings::load_settings(&app)
}

#[tauri::command]
pub fn save_settings(app: AppHandle, input: SaveSettingsInput) -> Result<AppSettings, String> {
    settings::save_settings(&app, input)
}

#[tauri::command]
pub async fn import_kimi_account(
    app: AppHandle,
    store: State<'_, StateStore>,
    account_name: Option<String>,
    account_id: Option<String>,
) -> Result<AppSettings, String> {
    let display_name = account_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Kimi Account")
        .to_string();
    let mut current = settings::load_settings(&app)?;
    let account_id = settings::upsert_provider_oauth_account(
        &mut current,
        PROVIDER_KIMI,
        account_id,
        Some(display_name),
        None,
    );
    let tokens = oauth::load_kimi_cli_tokens(account_id.clone())?;
    secrets::save_oauth_tokens(&account_id, &tokens)?;
    let next_settings = settings::write_settings(&app, &current)?;
    hydrate_cached_snapshot(&app, &store).await?;
    Ok(next_settings)
}

#[tauri::command]
pub async fn start_openai_oauth(
    oauth_store: State<'_, oauth::OAuthStore>,
    account_id: Option<String>,
) -> Result<String, String> {
    oauth::start_openai_oauth(&oauth_store, account_id).await
}

#[tauri::command]
pub async fn start_anthropic_oauth(
    oauth_store: State<'_, oauth::OAuthStore>,
    account_id: Option<String>,
) -> Result<String, String> {
    oauth::start_anthropic_oauth(&oauth_store, account_id).await
}

#[tauri::command]
pub async fn get_oauth_status(
    oauth_store: State<'_, oauth::OAuthStore>,
) -> Result<crate::models::OAuthStatus, String> {
    Ok(oauth::oauth_status(&oauth_store).await)
}

#[tauri::command]
pub async fn complete_openai_oauth(
    app: AppHandle,
    oauth_store: State<'_, oauth::OAuthStore>,
    callback_url: String,
) -> Result<crate::models::OAuthStatus, String> {
    oauth::complete_openai_oauth(&app, &oauth_store, &callback_url).await
}

#[tauri::command]
pub async fn complete_anthropic_oauth(
    app: AppHandle,
    oauth_store: State<'_, oauth::OAuthStore>,
    callback_url: String,
) -> Result<crate::models::OAuthStatus, String> {
    oauth::complete_anthropic_oauth(&app, &oauth_store, &callback_url).await
}

#[tauri::command]
pub async fn delete_openai_account(
    app: AppHandle,
    store: State<'_, StateStore>,
    account_id: String,
) -> Result<AppSettings, String> {
    delete_connected_account(app, store, account_id).await
}

#[tauri::command]
pub async fn delete_connected_account(
    app: AppHandle,
    store: State<'_, StateStore>,
    account_id: String,
) -> Result<AppSettings, String> {
    let account_id = settings::normalize_account_id(&account_id);
    let _ = settings::delete_account(&app, &account_id)?;
    secrets::delete_oauth_tokens(&account_id)?;
    delete_account_snapshot(&app, &account_id)?;
    hydrate_cached_snapshot(&app, &store).await?;
    settings::load_settings(&app)
}

#[tauri::command]
pub fn resize_main_panel(app: AppHandle, width: f64, height: f64) -> Result<(), String> {
    panel::resize_main_panel(&app, width, height)
}

pub async fn refresh_inner(app: &AppHandle, store: &StateStore) -> Result<(), String> {
    let settings = settings::load_settings(app)?;

    {
        let mut guard = store.inner.write().await;
        guard.refresh_status = crate::models::RefreshStatus::Refreshing;
        guard.last_error = None;
    }

    let accounts = refreshable_quota_accounts(&settings);
    if accounts.is_empty() {
        let snapshots = read_account_snapshots(app)?;
        let mut guard = store.inner.write().await;
        guard.snapshot = snapshots.get(&settings.account_id).cloned();
        guard.accounts = account_statuses_from_settings_and_snapshots(&settings, &snapshots);
        guard.refresh_status = crate::models::RefreshStatus::Error;
        guard.last_error = Some("当前没有支持额度刷新的账号".into());
        return Err(guard
            .last_error
            .clone()
            .unwrap_or_else(|| "刷新失败".to_string()));
    }

    let mut errors = Vec::new();
    let mut latest_active_snapshot = None;
    for account in accounts {
        let account_settings = settings_for_refresh_account(&settings, &account);
        match fetch_quota_with_oauth_retry(&account_settings).await {
            Ok(snapshot) => {
                if account.account_id == settings.account_id {
                    latest_active_snapshot = Some(snapshot.clone());
                }
                write_account_snapshot(app, &account.account_id, &snapshot)?;
            }
            Err(error) => {
                errors.push(format!("{}: {}", account.account_name, error.message));
            }
        }
    }

    let snapshots = read_account_snapshots(app)?;
    let cached_active_snapshot = snapshots.get(&settings.account_id).cloned();
    let mut guard = store.inner.write().await;
    guard.snapshot = latest_active_snapshot.or(cached_active_snapshot);
    guard.accounts = account_statuses_from_settings_and_snapshots(&settings, &snapshots);
    guard.last_refreshed_at = Some(Utc::now());

    if errors.is_empty() {
        guard.refresh_status = crate::models::RefreshStatus::Ok;
        guard.last_error = None;
        Ok(())
    } else {
        guard.refresh_status = crate::models::RefreshStatus::Error;
        guard.last_error = Some(errors.join("；"));
        Err(guard
            .last_error
            .clone()
            .unwrap_or_else(|| "刷新失败".to_string()))
    }
}

pub async fn hydrate_cached_snapshot(app: &AppHandle, store: &StateStore) -> Result<(), String> {
    let settings = settings::load_settings(app)?;
    if !settings.secret_configured {
        let mut guard = store.inner.write().await;
        guard.snapshot = None;
        guard.accounts = Vec::new();
        guard.refresh_status = crate::models::RefreshStatus::Idle;
        guard.last_error = None;
        guard.last_refreshed_at = None;
        return Ok(());
    }

    let snapshots = read_account_snapshots(app)?;
    let cached = snapshots.get(&settings.account_id).cloned();
    let mut guard = store.inner.write().await;
    guard.snapshot = cached;
    guard.accounts = account_statuses_from_settings_and_snapshots(&settings, &snapshots);
    Ok(())
}

async fn fetch_quota_with_oauth_retry(
    settings: &AppSettings,
) -> Result<QuotaSnapshot, ProviderError> {
    if !account_supports_quota_refresh(settings.active_provider()) {
        return Err(ProviderError::new(
            ProviderErrorKind::Unknown,
            format!(
                "{} 账号已绑定，但当前版本暂未实现额度刷新",
                provider_display_label(settings.active_provider())
            ),
        ));
    }

    run_with_oauth_retry(
        settings,
        || async {
            let credentials = secrets::load_secret(settings)
                .map_err(|error| ProviderError::new(ProviderErrorKind::Unknown, error))?
                .ok_or_else(|| {
                    ProviderError::new(ProviderErrorKind::Unauthorized, "请先在设置中填写认证值")
                })?;

            provider::fetch_quota(
                &settings.account_id,
                &settings.account_name,
                settings.base_url_override.as_deref(),
                &credentials,
            )
            .await
        },
        |force| {
            let account_id = settings.account_id.clone();
            async move {
                oauth::ensure_fresh_token(&account_id, force)
                    .await
                    .map(|_| ())
                    .map_err(|error| ProviderError::new(ProviderErrorKind::Unauthorized, error))
            }
        },
    )
    .await
}

fn refreshable_quota_accounts(settings: &AppSettings) -> Vec<ConnectedAccount> {
    accounts_for_status(settings)
        .into_iter()
        .filter(|account| {
            account.secret_configured && account_supports_quota_refresh(&account.provider)
        })
        .collect()
}

fn settings_for_refresh_account(settings: &AppSettings, account: &ConnectedAccount) -> AppSettings {
    let mut next = settings.clone();
    next.account_id = account.account_id.clone();
    next.account_name = account.account_name.clone();
    next.auth_mode = account.auth_mode.clone();
    next.chatgpt_account_id = account.chatgpt_account_id.clone();
    next.secret_configured = account.secret_configured;
    next
}

fn account_supports_quota_refresh(provider: &str) -> bool {
    matches!(
        provider,
        PROVIDER_OPENAI | PROVIDER_ANTHROPIC | PROVIDER_KIMI
    )
}

fn provider_display_label(provider: &str) -> &'static str {
    match provider {
        crate::models::PROVIDER_ANTHROPIC => "Anthropic",
        crate::models::PROVIDER_KIMI => "Kimi",
        _ => "OpenAI",
    }
}

async fn run_with_oauth_retry<T, Probe, ProbeFut, Refresh, RefreshFut>(
    settings: &AppSettings,
    mut probe: Probe,
    mut refresh: Refresh,
) -> Result<T, ProviderError>
where
    Probe: FnMut() -> ProbeFut,
    ProbeFut: Future<Output = Result<T, ProviderError>>,
    Refresh: FnMut(bool) -> RefreshFut,
    RefreshFut: Future<Output = Result<(), ProviderError>>,
{
    if matches!(settings.auth_mode, AuthMode::OAuth) {
        refresh(false).await?;
    }

    let first = probe().await;
    let error = match first {
        Ok(value) => return Ok(value),
        Err(error) => error,
    };

    if !matches!(settings.auth_mode, AuthMode::OAuth)
        || !should_force_refresh_after_probe_error(&error, false)
    {
        return Err(error);
    }

    refresh(true).await?;
    probe().await
}

fn should_force_refresh_after_probe_error(error: &ProviderError, already_forced: bool) -> bool {
    !already_forced && matches!(error.kind, ProviderErrorKind::Unauthorized)
}

fn should_replace_cached_snapshot(current: &Option<QuotaSnapshot>, account_id: &str) -> bool {
    current
        .as_ref()
        .map(|snapshot| snapshot.account_id != account_id)
        .unwrap_or(true)
}

fn read_account_snapshots(app: &AppHandle) -> Result<HashMap<String, QuotaSnapshot>, String> {
    Ok(
        storage::read_json::<HashMap<String, QuotaSnapshot>>(app, SNAPSHOTS_FILE)?
            .unwrap_or_default(),
    )
}

fn write_account_snapshot(
    app: &AppHandle,
    account_id: &str,
    snapshot: &QuotaSnapshot,
) -> Result<(), String> {
    let mut snapshots = storage::read_json::<HashMap<String, QuotaSnapshot>>(app, SNAPSHOTS_FILE)?
        .unwrap_or_default();
    snapshots.insert(account_id.to_string(), snapshot.clone());
    storage::write_json(app, SNAPSHOTS_FILE, &snapshots)
}

fn delete_account_snapshot(app: &AppHandle, account_id: &str) -> Result<(), String> {
    let mut snapshots = read_account_snapshots(app)?;
    snapshots.remove(account_id);
    storage::write_json(app, SNAPSHOTS_FILE, &snapshots)
}

fn account_statuses_from_settings_and_snapshots(
    settings: &AppSettings,
    snapshots: &HashMap<String, QuotaSnapshot>,
) -> Vec<AccountQuotaStatus> {
    accounts_for_status(settings)
        .into_iter()
        .map(|account| account_status_from_snapshot(&account, snapshots.get(&account.account_id)))
        .collect()
}

fn accounts_for_status(settings: &AppSettings) -> Vec<ConnectedAccount> {
    if !settings.accounts.is_empty() {
        return settings.accounts.clone();
    }
    if !settings.secret_configured {
        return Vec::new();
    }
    vec![ConnectedAccount {
        account_id: settings.account_id.clone(),
        account_name: settings.account_name.clone(),
        provider: "openai".into(),
        auth_mode: settings.auth_mode.clone(),
        chatgpt_account_id: settings.chatgpt_account_id.clone(),
        secret_configured: settings.secret_configured,
    }]
}

fn account_status_from_snapshot(
    account: &ConnectedAccount,
    snapshot: Option<&QuotaSnapshot>,
) -> AccountQuotaStatus {
    AccountQuotaStatus {
        account_id: account.account_id.clone(),
        account_name: display_account_name_for_status(account, snapshot),
        provider: account.provider.clone(),
        five_hour: snapshot.and_then(|snapshot| snapshot.five_hour.clone()),
        seven_day: snapshot.and_then(|snapshot| snapshot.seven_day.clone()),
        fetched_at: snapshot.map(|snapshot| snapshot.fetched_at),
        source: snapshot.map(|snapshot| snapshot.source.clone()),
    }
}

fn display_account_name_for_status(
    account: &ConnectedAccount,
    snapshot: Option<&QuotaSnapshot>,
) -> String {
    let account_name = account.account_name.trim();
    if !account_name.is_empty() {
        return account_name.to_string();
    }
    snapshot
        .map(|snapshot| snapshot.account_name.trim())
        .filter(|name| !name.is_empty())
        .unwrap_or("OpenAI Account")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::{ProviderError, ProviderErrorKind};
    use crate::models::QuotaWindow;

    #[test]
    fn unauthorized_retry_happens_once() {
        let error = ProviderError::new(ProviderErrorKind::Unauthorized, "expired");

        assert!(should_force_refresh_after_probe_error(&error, false));
        assert!(!should_force_refresh_after_probe_error(&error, true));
    }

    #[test]
    fn non_unauthorized_error_does_not_force_refresh() {
        let error = ProviderError::new(ProviderErrorKind::Network, "offline");

        assert!(!should_force_refresh_after_probe_error(&error, false));
    }

    #[tokio::test]
    async fn oauth_probe_retries_once_after_unauthorized() {
        let settings = AppSettings {
            auth_mode: AuthMode::OAuth,
            ..AppSettings::default()
        };
        let mut probes = 0;
        let mut refreshes = Vec::new();

        let result = run_with_oauth_retry(
            &settings,
            || {
                probes += 1;
                async move {
                    if probes == 1 {
                        Err(ProviderError::new(
                            ProviderErrorKind::Unauthorized,
                            "expired",
                        ))
                    } else {
                        Ok("ok")
                    }
                }
            },
            |force| {
                refreshes.push(force);
                async { Ok(()) }
            },
        )
        .await
        .unwrap();

        assert_eq!(result, "ok");
        assert_eq!(probes, 2);
        assert_eq!(refreshes, vec![false, true]);
    }

    #[tokio::test]
    async fn oauth_probe_does_not_retry_unauthorized_twice() {
        let settings = AppSettings {
            auth_mode: AuthMode::OAuth,
            ..AppSettings::default()
        };
        let mut probes = 0;
        let mut refreshes = Vec::new();

        let error = run_with_oauth_retry(
            &settings,
            || {
                probes += 1;
                async {
                    Err::<(), _>(ProviderError::new(
                        ProviderErrorKind::Unauthorized,
                        "expired",
                    ))
                }
            },
            |force| {
                refreshes.push(force);
                async { Ok(()) }
            },
        )
        .await
        .unwrap_err();

        assert!(matches!(error.kind, ProviderErrorKind::Unauthorized));
        assert_eq!(probes, 2);
        assert_eq!(refreshes, vec![false, true]);
    }

    #[test]
    fn cached_snapshot_should_replace_different_account() {
        let current = Some(QuotaSnapshot {
            account_id: "first".into(),
            account_name: "First".into(),
            five_hour: None,
            seven_day: None,
            fetched_at: chrono::Utc::now(),
            source: "probe_headers".into(),
        });
        assert!(should_replace_cached_snapshot(&current, "second"));
    }

    #[test]
    fn account_statuses_include_every_connected_account() {
        let settings = AppSettings {
            accounts: vec![
                crate::models::ConnectedAccount {
                    account_id: "first".into(),
                    account_name: "first@example.com".into(),
                    provider: "openai".into(),
                    auth_mode: AuthMode::OAuth,
                    chatgpt_account_id: None,
                    secret_configured: true,
                },
                crate::models::ConnectedAccount {
                    account_id: "second".into(),
                    account_name: "second@example.com".into(),
                    provider: "openai".into(),
                    auth_mode: AuthMode::OAuth,
                    chatgpt_account_id: None,
                    secret_configured: true,
                },
            ],
            ..AppSettings::default()
        };
        let mut snapshots = HashMap::new();
        snapshots.insert(
            "first".into(),
            QuotaSnapshot {
                account_id: "first".into(),
                account_name: "first@example.com".into(),
                five_hour: Some(QuotaWindow {
                    used_percent: 4.0,
                    remaining_percent: 96.0,
                    reset_at: None,
                    window_minutes: Some(300),
                }),
                seven_day: None,
                fetched_at: chrono::Utc::now(),
                source: "probe_headers".into(),
            },
        );

        let statuses = account_statuses_from_settings_and_snapshots(&settings, &snapshots);

        assert_eq!(statuses.len(), 2);
        assert_eq!(statuses[0].account_name, "first@example.com");
        assert_eq!(
            statuses[0]
                .five_hour
                .as_ref()
                .map(|window| window.remaining_percent),
            Some(96.0)
        );
        assert_eq!(statuses[1].account_name, "second@example.com");
        assert!(statuses[1].five_hour.is_none());
    }

    #[test]
    fn refreshable_accounts_include_all_supported_accounts_even_when_anthropic_is_active() {
        let settings = AppSettings {
            account_id: "anthropic".into(),
            accounts: vec![
                crate::models::ConnectedAccount {
                    account_id: "first".into(),
                    account_name: "first@example.com".into(),
                    provider: crate::models::PROVIDER_OPENAI.into(),
                    auth_mode: AuthMode::OAuth,
                    chatgpt_account_id: None,
                    secret_configured: true,
                },
                crate::models::ConnectedAccount {
                    account_id: "anthropic".into(),
                    account_name: "claude@example.com".into(),
                    provider: crate::models::PROVIDER_ANTHROPIC.into(),
                    auth_mode: AuthMode::OAuth,
                    chatgpt_account_id: None,
                    secret_configured: true,
                },
                crate::models::ConnectedAccount {
                    account_id: "second".into(),
                    account_name: "second@example.com".into(),
                    provider: crate::models::PROVIDER_OPENAI.into(),
                    auth_mode: AuthMode::OAuth,
                    chatgpt_account_id: None,
                    secret_configured: true,
                },
            ],
            ..AppSettings::default()
        };

        let account_ids = refreshable_quota_accounts(&settings)
            .into_iter()
            .map(|account| account.account_id)
            .collect::<Vec<_>>();

        assert_eq!(account_ids, vec!["first", "anthropic", "second"]);
    }

    #[test]
    fn refreshable_accounts_include_kimi_accounts() {
        let settings = AppSettings {
            accounts: vec![crate::models::ConnectedAccount {
                account_id: "kimi-work".into(),
                account_name: "Kimi Work".into(),
                provider: crate::models::PROVIDER_KIMI.into(),
                auth_mode: AuthMode::OAuth,
                chatgpt_account_id: None,
                secret_configured: true,
            }],
            ..AppSettings::default()
        };

        let account_ids = refreshable_quota_accounts(&settings)
            .into_iter()
            .map(|account| account.account_id)
            .collect::<Vec<_>>();

        assert_eq!(account_ids, vec!["kimi-work"]);
    }

    #[test]
    fn settings_for_refresh_account_targets_that_account() {
        let settings = AppSettings {
            account_id: "active".into(),
            account_name: "active@example.com".into(),
            auth_mode: AuthMode::OAuth,
            accounts: vec![crate::models::ConnectedAccount {
                account_id: "second".into(),
                account_name: "second@example.com".into(),
                provider: crate::models::PROVIDER_OPENAI.into(),
                auth_mode: AuthMode::OAuth,
                chatgpt_account_id: Some("acct-second".into()),
                secret_configured: true,
            }],
            ..AppSettings::default()
        };

        let next = settings_for_refresh_account(&settings, &settings.accounts[0]);

        assert_eq!(next.account_id, "second");
        assert_eq!(next.account_name, "second@example.com");
        assert_eq!(next.chatgpt_account_id.as_deref(), Some("acct-second"));
        assert_eq!(next.accounts.len(), 1);
    }
}
