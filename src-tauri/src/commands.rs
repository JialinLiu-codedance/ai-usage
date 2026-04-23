use crate::{
    models::{AppSettings, AppStatus, ConnectionTestResult, SaveSettingsInput},
    provider,
    secrets,
    settings,
    state::StateStore,
    storage,
};
use chrono::Utc;
use tauri::{AppHandle, State};

const SNAPSHOT_FILE: &str = "snapshot.json";

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
    let credentials = secrets::load_secret(&settings)?
        .ok_or_else(|| "请先在设置中填写认证值".to_string())?;

    let message = provider::test_connection(
        &settings.account_name,
        settings.base_url_override.as_deref(),
        &credentials,
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

pub async fn refresh_inner(app: &AppHandle, store: &StateStore) -> Result<(), String> {
    let settings = settings::load_settings(app)?;
    let credentials = secrets::load_secret(&settings)?
        .ok_or_else(|| "请先在设置中填写认证值".to_string())?;

    {
        let mut guard = store.inner.write().await;
        guard.refresh_status = crate::models::RefreshStatus::Refreshing;
        guard.last_error = None;
    }

    match provider::fetch_quota(
        &settings.account_name,
        settings.base_url_override.as_deref(),
        &credentials,
    )
    .await
    {
        Ok(snapshot) => {
            storage::write_json(app, SNAPSHOT_FILE, &snapshot)?;
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
    let cached = storage::read_json(app, SNAPSHOT_FILE)?;
    let mut guard = store.inner.write().await;
    if guard.snapshot.is_none() {
        guard.snapshot = cached;
    }
    Ok(())
}
