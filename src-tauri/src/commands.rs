use crate::{
    errors::{ProviderError, ProviderErrorKind},
    models::{
        AppSettings, AppStatus, AuthMode, ConnectionTestResult, QuotaSnapshot, SaveSettingsInput,
    },
    oauth, provider, secrets, settings,
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
pub async fn start_openai_oauth(
    oauth_store: State<'_, oauth::OAuthStore>,
) -> Result<String, String> {
    oauth::start_openai_oauth(&oauth_store).await
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

pub async fn refresh_inner(app: &AppHandle, store: &StateStore) -> Result<(), String> {
    let settings = settings::load_settings(app)?;

    {
        let mut guard = store.inner.write().await;
        guard.refresh_status = crate::models::RefreshStatus::Refreshing;
        guard.last_error = None;
    }

    match fetch_quota_with_oauth_retry(&settings).await {
        Ok(snapshot) => {
            write_account_snapshot(app, &settings.account_id, &snapshot)?;
            let mut guard = store.inner.write().await;
            guard.snapshot = Some(snapshot);
            guard.refresh_status = crate::models::RefreshStatus::Ok;
            guard.last_refreshed_at = Some(Utc::now());
            guard.last_error = None;
            Ok(())
        }
        Err(error) => {
            let mut guard = store.inner.write().await;
            guard.refresh_status = crate::models::RefreshStatus::Error;
            guard.last_error = Some(error.message);
            Err(guard
                .last_error
                .clone()
                .unwrap_or_else(|| "刷新失败".to_string()))
        }
    }
}

pub async fn hydrate_cached_snapshot(app: &AppHandle, store: &StateStore) -> Result<(), String> {
    let settings = settings::load_settings(app)?;
    let cached = read_account_snapshot(app, &settings.account_id)?;
    let mut guard = store.inner.write().await;
    if should_replace_cached_snapshot(&guard.snapshot, &settings.account_id) {
        guard.snapshot = cached;
    }
    Ok(())
}

async fn fetch_quota_with_oauth_retry(
    settings: &AppSettings,
) -> Result<QuotaSnapshot, ProviderError> {
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

fn read_account_snapshot(
    app: &AppHandle,
    account_id: &str,
) -> Result<Option<QuotaSnapshot>, String> {
    let snapshots = storage::read_json::<HashMap<String, QuotaSnapshot>>(app, SNAPSHOTS_FILE)?;
    Ok(snapshots.and_then(|mut values| values.remove(account_id)))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::{ProviderError, ProviderErrorKind};

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
}
