use crate::{
    copilot_oauth::CopilotAuthState,
    errors::{ProviderError, ProviderErrorKind},
    git_usage, local_proxy, local_usage,
    models::{
        AccountQuotaStatus, AppSettings, AppStatus, AuthMode, ClaudeProxyProfileInput,
        ConnectedAccount, ConnectionTestResult, GitBranchCandidate, GitBranchManagementState,
        GitBranchProject, GitDefaultBranchSource, GitHubDeviceCodeResponse, GitUsageReport,
        LocalProxySettingsState, LocalProxyStatus, LocalTokenUsageRange, LocalTokenUsageReport,
        ManagedAuthAccount, PrKpiOverview, PrKpiReport, ProbeCredentials, QuotaSnapshot,
        ReverseProxySettingsState, ReverseProxyStatus, SaveLocalProxySettingsInput,
        SaveReverseProxySettingsInput, SaveSettingsInput, UsageRangeRequest,
        CUSTOM_USAGE_WINDOW_DAYS, PROVIDER_ANTHROPIC, PROVIDER_COPILOT, PROVIDER_GLM,
        PROVIDER_KIMI, PROVIDER_MINIMAX, PROVIDER_OPENAI,
    },
    notifications, oauth, pr_kpi, provider, secrets, settings,
    state::StateStore,
    storage,
};
use chrono::{Local, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_autostart::ManagerExt;

const SNAPSHOTS_FILE: &str = "snapshots.json";
const TOKEN_USAGE_CACHE_FILE: &str = "local-token-usage-cache.json";
const TOKEN_USAGE_CACHE_UPDATED_EVENT: &str = "local-token-usage-cache-updated";
const GIT_USAGE_CACHE_FILE: &str = "git-usage-cache.json";
const GIT_USAGE_CACHE_UPDATED_EVENT: &str = "git-usage-cache-updated";
const GIT_BRANCH_MANAGEMENT_CACHE_FILE: &str = "git-branch-management-cache.json";
const GIT_BRANCH_MANAGEMENT_CACHE_UPDATED_EVENT: &str = "git-branch-management-cache-updated";
const PR_KPI_CACHE_FILE: &str = "pr-kpi-cache.json";
const PR_KPI_CACHE_UPDATED_EVENT: &str = "pr-kpi-cache-updated";
static TOKEN_USAGE_CACHE_REFRESHING: AtomicBool = AtomicBool::new(false);
static GIT_USAGE_CACHE_REFRESHING: AtomicBool = AtomicBool::new(false);
static GIT_BRANCH_MANAGEMENT_CACHE_REFRESHING: AtomicBool = AtomicBool::new(false);
static PR_KPI_CACHE_REFRESHING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GitBranchManagementCache {
    pub root_path: String,
    #[serde(with = "crate::app_time::local_datetime_serde")]
    pub generated_at: chrono::DateTime<chrono::Utc>,
    #[serde(default)]
    pub default_branch_override_fingerprint: String,
    #[serde(default)]
    pub projects: Vec<GitBranchProject>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl From<GitBranchManagementCache> for GitBranchManagementState {
    fn from(value: GitBranchManagementCache) -> Self {
        Self {
            root_path: value.root_path,
            generated_at: value.generated_at,
            projects: value.projects,
            warnings: value.warnings,
        }
    }
}

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
    if let Err(error) = refresh_inner(&app, &store).await {
        let status = store.inner.read().await.clone();
        if status.last_error.is_some()
            || matches!(status.refresh_status, crate::models::RefreshStatus::Error)
        {
            return Ok(status);
        }
        return Err(error);
    }
    Ok(store.inner.read().await.clone())
}

async fn run_refresh_operation<T, Operation>(
    store: &StateStore,
    operation: Operation,
) -> Result<T, String>
where
    Operation: Future<Output = Result<T, String>>,
{
    {
        let mut guard = store.inner.write().await;
        guard.refresh_status = crate::models::RefreshStatus::Refreshing;
        guard.last_error = None;
    }

    let result = operation.await;
    if let Err(error) = &result {
        let mut guard = store.inner.write().await;
        if matches!(
            guard.refresh_status,
            crate::models::RefreshStatus::Refreshing
        ) {
            guard.refresh_status = crate::models::RefreshStatus::Error;
        }
        if guard.last_error.is_none() {
            guard.last_error = Some(error.clone());
        }
    }

    result
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

fn autostart_enabled_or_stored(app: &AppHandle, stored_value: bool) -> bool {
    read_autostart_enabled(app).unwrap_or(stored_value)
}

pub(crate) fn read_autostart_enabled(app: &AppHandle) -> Result<bool, String> {
    app.autolaunch()
        .is_enabled()
        .map_err(|error| format!("读取自动启动状态失败: {error}"))
}

pub(crate) fn sync_launch_at_login(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let autostart_manager = app.autolaunch();
    let current = autostart_manager
        .is_enabled()
        .map_err(|error| format!("读取自动启动状态失败: {error}"))?;

    if current == enabled {
        return Ok(());
    }

    if enabled {
        autostart_manager
            .enable()
            .map_err(|error| format!("开启自动启动失败: {error}"))
    } else {
        autostart_manager
            .disable()
            .map_err(|error| format!("关闭自动启动失败: {error}"))
    }
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Result<AppSettings, String> {
    let mut settings = settings::load_settings(&app)?;
    settings.launch_at_login = autostart_enabled_or_stored(&app, settings.launch_at_login);
    Ok(settings)
}

#[tauri::command]
pub fn save_settings(app: AppHandle, input: SaveSettingsInput) -> Result<AppSettings, String> {
    let settings = settings::save_settings(&app, input)?;
    sync_launch_at_login(&app, settings.launch_at_login)?;
    get_settings(app)
}

#[tauri::command]
pub async fn get_local_proxy_settings(
    app: AppHandle,
    copilot_state: State<'_, CopilotAuthState>,
) -> Result<LocalProxySettingsState, String> {
    let settings = settings::load_settings(&app)?;
    let reverse_status = build_reverse_proxy_status(&settings, &copilot_state).await?;
    local_proxy::build_local_proxy_settings_state(&settings, &reverse_status)
}

#[tauri::command]
pub async fn save_local_proxy_settings(
    app: AppHandle,
    copilot_state: State<'_, CopilotAuthState>,
    input: SaveLocalProxySettingsInput,
) -> Result<LocalProxySettingsState, String> {
    let settings = settings::save_local_proxy_settings(&app, input)?;
    let reverse_status = build_reverse_proxy_status(&settings, &copilot_state).await?;
    local_proxy::build_local_proxy_settings_state(&settings, &reverse_status)
}

#[tauri::command]
pub async fn save_claude_proxy_profile(
    app: AppHandle,
    copilot_state: State<'_, CopilotAuthState>,
    input: ClaudeProxyProfileInput,
) -> Result<LocalProxySettingsState, String> {
    let _ = settings::save_claude_proxy_profile(&app, input)?;
    let settings = settings::load_settings(&app)?;
    let reverse_status = build_reverse_proxy_status(&settings, &copilot_state).await?;
    local_proxy::build_local_proxy_settings_state(&settings, &reverse_status)
}

#[tauri::command]
pub async fn get_reverse_proxy_settings(
    app: AppHandle,
    copilot_state: State<'_, CopilotAuthState>,
) -> Result<ReverseProxySettingsState, String> {
    let settings = settings::load_settings(&app)?;
    build_reverse_proxy_settings_state(&settings, &copilot_state).await
}

#[tauri::command]
pub async fn save_reverse_proxy_settings(
    app: AppHandle,
    copilot_state: State<'_, CopilotAuthState>,
    input: SaveReverseProxySettingsInput,
) -> Result<ReverseProxySettingsState, String> {
    let settings = settings::save_reverse_proxy_settings(&app, input)?;
    build_reverse_proxy_settings_state(&settings, &copilot_state).await
}

#[tauri::command]
pub async fn get_reverse_proxy_status(
    app: AppHandle,
    copilot_state: State<'_, CopilotAuthState>,
) -> Result<ReverseProxyStatus, String> {
    let settings = settings::load_settings(&app)?;
    build_reverse_proxy_status(&settings, &copilot_state).await
}

#[tauri::command]
pub async fn copilot_start_device_flow(
    copilot_state: State<'_, CopilotAuthState>,
) -> Result<GitHubDeviceCodeResponse, String> {
    let manager = copilot_state.0.read().await;
    manager
        .start_device_flow()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn copilot_poll_for_account(
    device_code: String,
    copilot_state: State<'_, CopilotAuthState>,
) -> Result<Option<ManagedAuthAccount>, String> {
    let manager = copilot_state.0.read().await;
    manager
        .poll_for_account(&device_code)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn copilot_list_accounts(
    copilot_state: State<'_, CopilotAuthState>,
) -> Result<Vec<ManagedAuthAccount>, String> {
    let manager = copilot_state.0.read().await;
    Ok(manager.list_accounts().await)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn copilot_set_default_account(
    account_id: String,
    copilot_state: State<'_, CopilotAuthState>,
) -> Result<(), String> {
    let manager = copilot_state.0.read().await;
    manager
        .set_default_account(&account_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn copilot_remove_account(
    account_id: String,
    copilot_state: State<'_, CopilotAuthState>,
) -> Result<(), String> {
    let manager = copilot_state.0.read().await;
    manager
        .remove_account(&account_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn copilot_get_auth_status(
    copilot_state: State<'_, CopilotAuthState>,
) -> Result<crate::models::CopilotAuthStatus, String> {
    let manager = copilot_state.0.read().await;
    Ok(manager.get_status().await)
}

async fn build_reverse_proxy_settings_state(
    settings: &AppSettings,
    copilot_state: &State<'_, CopilotAuthState>,
) -> Result<ReverseProxySettingsState, String> {
    let copilot_accounts = {
        let manager = copilot_state.0.read().await;
        manager.list_accounts().await
    };
    let openai_accounts = openai_oauth_managed_accounts(settings);
    let default_copilot_account_id =
        resolve_default_copilot_reverse_account_id(settings, &copilot_accounts);
    let default_openai_account_id =
        resolve_default_openai_reverse_account_id(settings, &openai_accounts);
    Ok(ReverseProxySettingsState {
        enabled: settings.reverse_proxy.enabled,
        copilot_accounts,
        default_copilot_account_id,
        openai_accounts,
        default_openai_account_id,
    })
}

pub(crate) async fn build_reverse_proxy_status(
    settings: &AppSettings,
    copilot_state: &State<'_, CopilotAuthState>,
) -> Result<ReverseProxyStatus, String> {
    let settings_state = build_reverse_proxy_settings_state(settings, copilot_state).await?;
    let copilot_ready = settings.reverse_proxy.enabled
        && settings_state
            .default_copilot_account_id
            .as_ref()
            .is_some_and(|id| {
                settings_state
                    .copilot_accounts
                    .iter()
                    .any(|account| &account.id == id)
            });
    let openai_ready = settings.reverse_proxy.enabled
        && settings_state
            .default_openai_account_id
            .as_ref()
            .is_some_and(|id| {
                settings_state
                    .openai_accounts
                    .iter()
                    .any(|account| &account.id == id)
            });

    Ok(ReverseProxyStatus {
        enabled: settings.reverse_proxy.enabled,
        copilot_ready,
        openai_ready,
        available_copilot_accounts: settings_state.copilot_accounts.len(),
        available_openai_accounts: settings_state.openai_accounts.len(),
    })
}

fn openai_oauth_managed_accounts(settings: &AppSettings) -> Vec<ManagedAuthAccount> {
    settings
        .accounts
        .iter()
        .filter(|account| {
            account.provider == PROVIDER_OPENAI && matches!(account.auth_mode, AuthMode::OAuth)
        })
        .map(|account| ManagedAuthAccount {
            id: account.account_id.clone(),
            login: account.account_name.clone(),
            avatar_url: None,
            authenticated_at: 0,
            domain: None,
        })
        .collect()
}

fn resolve_default_openai_reverse_account_id(
    settings: &AppSettings,
    openai_accounts: &[ManagedAuthAccount],
) -> Option<String> {
    settings
        .reverse_proxy
        .default_openai_account_id
        .clone()
        .or_else(|| openai_accounts.first().map(|account| account.id.clone()))
}

fn resolve_default_copilot_reverse_account_id(
    settings: &AppSettings,
    copilot_accounts: &[ManagedAuthAccount],
) -> Option<String> {
    settings
        .reverse_proxy
        .default_copilot_account_id
        .clone()
        .or_else(|| copilot_accounts.first().map(|account| account.id.clone()))
}

#[tauri::command]
pub async fn get_local_proxy_status(
    app: AppHandle,
    manager: State<'_, local_proxy::LocalProxyManager>,
) -> Result<LocalProxyStatus, String> {
    manager.status(&app).await
}

#[tauri::command]
pub async fn start_local_proxy(
    app: AppHandle,
    manager: State<'_, local_proxy::LocalProxyManager>,
) -> Result<LocalProxyStatus, String> {
    manager.start(app).await
}

#[tauri::command]
pub async fn stop_local_proxy(
    app: AppHandle,
    manager: State<'_, local_proxy::LocalProxyManager>,
) -> Result<LocalProxyStatus, String> {
    manager.stop(app).await
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
pub async fn import_glm_account(
    app: AppHandle,
    store: State<'_, StateStore>,
    account_name: Option<String>,
    api_key: String,
    account_id: Option<String>,
) -> Result<AppSettings, String> {
    let display_name = account_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("GLM Account")
        .to_string();
    let api_key = api_key.trim().to_string();
    if api_key.is_empty() {
        return Err("请填写 GLM API Key".into());
    }

    let mut current = settings::load_settings(&app)?;
    let account_id =
        settings::upsert_api_key_account(&mut current, PROVIDER_GLM, account_id, display_name);
    secrets::save_account_secret(&account_id, &api_key)?;
    let next_settings = settings::write_settings(&app, &current)?;
    hydrate_cached_snapshot(&app, &store).await?;
    Ok(next_settings)
}

#[tauri::command]
pub async fn import_minimax_account(
    app: AppHandle,
    store: State<'_, StateStore>,
    account_name: Option<String>,
    api_key: String,
    account_id: Option<String>,
) -> Result<AppSettings, String> {
    let display_name = account_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("MiniMax Account")
        .to_string();
    let api_key = api_key.trim().to_string();
    if api_key.is_empty() {
        return Err("请填写 MiniMax API Key".into());
    }

    let mut current = settings::load_settings(&app)?;
    let account_id =
        settings::upsert_api_key_account(&mut current, PROVIDER_MINIMAX, account_id, display_name);
    secrets::save_account_secret(&account_id, &api_key)?;
    let next_settings = settings::write_settings(&app, &current)?;
    hydrate_cached_snapshot(&app, &store).await?;
    Ok(next_settings)
}

#[tauri::command]
pub async fn import_copilot_account(
    app: AppHandle,
    store: State<'_, StateStore>,
    account_name: Option<String>,
    github_token: Option<String>,
    account_id: Option<String>,
) -> Result<AppSettings, String> {
    let display_name = account_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Copilot Account")
        .to_string();
    let token = github_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| secrets::load_github_cli_token().ok().flatten())
        .ok_or_else(|| "请填写 GitHub Token，或先运行 gh auth login 后再导入".to_string())?;

    let mut current = settings::load_settings(&app)?;
    let account_id =
        settings::upsert_api_key_account(&mut current, PROVIDER_COPILOT, account_id, display_name);
    secrets::save_account_secret(&account_id, &token)?;
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
    secrets::delete_account_secret(&account_id)?;
    delete_account_snapshot(&app, &account_id)?;
    hydrate_cached_snapshot(&app, &store).await?;
    settings::load_settings(&app)
}

#[tauri::command]
pub fn get_local_token_usage(
    app: AppHandle,
    request: UsageRangeRequest,
) -> Result<LocalTokenUsageReport, String> {
    let resolved = validate_usage_range_request(&request)?;
    let cache = read_local_token_usage_cache(&app)?;
    let max_age_minutes = settings::load_settings(&app)
        .map(|settings| i64::from(settings.refresh_interval_minutes))
        .unwrap_or(15);
    if let ResolvedUsageRange::Custom {
        start_date,
        end_date,
    } = resolved
    {
        let should_refresh = cache
            .as_ref()
            .map(|cache| {
                local_token_usage_cache_is_stale(cache, max_age_minutes)
                    || !cache.covers_custom_range(start_date, end_date)
            })
            .unwrap_or(true);
        if should_refresh {
            start_local_token_usage_cache_refresh(app.clone());
        }

        if let Some(cache) = cache.filter(|cache| cache.covers_custom_range(start_date, end_date)) {
            return Ok(cache.custom_report(start_date, end_date));
        }

        return Ok(local_usage::pending_custom_report(
            start_date,
            end_date,
            Some("Token 用量缓存正在后台生成，完成后会自动更新".into()),
        ));
    }

    let ResolvedUsageRange::Preset(range) = resolved else {
        unreachable!("custom usage range returned above");
    };

    if cache
        .as_ref()
        .map(|cache| local_token_usage_cache_is_stale(cache, max_age_minutes))
        .unwrap_or(true)
    {
        start_local_token_usage_cache_refresh(app.clone());
    }

    if let Some(cache) = cache {
        return Ok(cache.report(range));
    }

    Ok(local_usage::pending_report(
        range,
        Some("Token 用量缓存正在后台生成，完成后会自动更新".into()),
    ))
}

#[tauri::command]
pub async fn refresh_local_token_usage(
    app: AppHandle,
    request: UsageRangeRequest,
) -> Result<LocalTokenUsageReport, String> {
    let resolved = validate_usage_range_request(&request)?;
    if let ResolvedUsageRange::Custom {
        start_date,
        end_date,
    } = resolved
    {
        let cache = refresh_local_token_usage_cache(app, true).await?;
        if !cache.covers_custom_range(start_date, end_date) {
            return Err("Token 用量缓存刷新后仍未准备好".into());
        }
        return Ok(cache.custom_report(start_date, end_date));
    }

    let ResolvedUsageRange::Preset(range) = resolved else {
        unreachable!("custom usage range returned above");
    };
    let cache = refresh_local_token_usage_cache(app, true).await?;
    Ok(cache.report(range))
}

#[tauri::command]
pub fn get_git_usage(app: AppHandle, request: UsageRangeRequest) -> Result<GitUsageReport, String> {
    let resolved = validate_usage_range_request(&request)?;
    let cache = read_git_usage_cache(&app)?;
    let settings = settings::load_settings(&app).unwrap_or_default();
    let max_age_minutes = i64::from(settings.refresh_interval_minutes);
    if let ResolvedUsageRange::Custom {
        start_date,
        end_date,
    } = resolved
    {
        let should_refresh = cache
            .as_ref()
            .map(|cache| {
                git_usage_cache_is_stale(
                    cache,
                    max_age_minutes,
                    &settings.git_usage_root,
                    &git_default_branch_overrides_fingerprint(&settings),
                ) || !cache.covers_custom_range(start_date, end_date)
            })
            .unwrap_or(true);
        if should_refresh {
            start_git_usage_cache_refresh(app.clone());
        }

        if let Some(cache) = cache.filter(|cache| {
            cache.root_path == settings.git_usage_root
                && cache.covers_custom_range(start_date, end_date)
        }) {
            return Ok(cache.custom_report(start_date, end_date));
        }

        return Ok(git_usage::pending_custom_report(
            start_date,
            end_date,
            Some("Git 统计缓存正在后台生成，完成后会自动更新".into()),
        ));
    }

    let ResolvedUsageRange::Preset(range) = resolved else {
        unreachable!("custom usage range returned above");
    };

    let cache_is_stale = cache
        .as_ref()
        .map(|cache| {
            git_usage_cache_is_stale(
                cache,
                max_age_minutes,
                &settings.git_usage_root,
                &git_default_branch_overrides_fingerprint(&settings),
            )
        })
        .unwrap_or(true);

    if cache_is_stale {
        start_git_usage_cache_refresh(app.clone());
    }

    if let Some(cache) = cache.filter(|cache| cache.root_path == settings.git_usage_root) {
        return Ok(cache.report(range));
    }

    Ok(git_usage::pending_report(
        range,
        Some("Git 统计缓存正在后台生成，完成后会自动更新".into()),
    ))
}

#[tauri::command]
pub async fn refresh_git_usage(
    app: AppHandle,
    request: UsageRangeRequest,
) -> Result<GitUsageReport, String> {
    let resolved = validate_usage_range_request(&request)?;
    if let ResolvedUsageRange::Custom {
        start_date,
        end_date,
    } = resolved
    {
        let cache = refresh_git_usage_cache(app, true).await?;
        if !cache.covers_custom_range(start_date, end_date) {
            return Err("Git 统计缓存刷新后仍未准备好".into());
        }
        return Ok(cache.custom_report(start_date, end_date));
    }

    let ResolvedUsageRange::Preset(range) = resolved else {
        unreachable!("custom usage range returned above");
    };
    let cache = refresh_git_usage_cache(app, true).await?;
    Ok(cache.report(range))
}

#[tauri::command]
pub async fn get_git_branch_management(app: AppHandle) -> Result<GitBranchManagementState, String> {
    let settings = settings::load_settings(&app).unwrap_or_default();
    let fingerprint = git_default_branch_overrides_fingerprint(&settings);
    let max_age_minutes = i64::from(settings.refresh_interval_minutes);
    let cache = read_git_branch_management_cache(&app)?;
    let cache_is_stale = cache
        .as_ref()
        .map(|cache| {
            git_branch_management_cache_is_stale(
                cache,
                &settings.git_usage_root,
                &fingerprint,
                max_age_minutes,
            )
        })
        .unwrap_or(true);

    if cache.is_none() {
        start_git_branch_management_cache_refresh(app.clone());
        return Ok(GitBranchManagementState {
            root_path: settings.git_usage_root,
            generated_at: Utc::now(),
            projects: vec![],
            warnings: vec!["Default Branch 管理数据正在后台生成，完成后会自动更新".into()],
        });
    }

    if cache
        .as_ref()
        .is_some_and(|cache| cache.default_branch_override_fingerprint != fingerprint)
    {
        return refresh_git_branch_management(app, false).await;
    }

    if cache_is_stale {
        start_git_branch_management_cache_refresh(app.clone());
    }

    Ok(cache
        .map(Into::into)
        .unwrap_or_else(|| GitBranchManagementState {
            root_path: settings.git_usage_root,
            generated_at: Utc::now(),
            projects: vec![],
            warnings: vec!["Default Branch 管理数据正在后台生成，完成后会自动更新".into()],
        }))
}

#[tauri::command]
pub async fn refresh_git_branch_management(
    app: AppHandle,
    emit_update: bool,
) -> Result<GitBranchManagementState, String> {
    let cache = refresh_git_branch_management_cache(app, emit_update).await?;
    Ok(cache.into())
}

fn build_git_branch_management_state(
    root_path: String,
    overrides: HashMap<String, String>,
    github_token: Option<String>,
) -> Result<GitBranchManagementCache, String> {
    let root = PathBuf::from(&root_path);
    let repositories = git_usage::discover_git_repositories(&root)
        .map_err(|error| format!("扫描本地 Git 仓库失败: {error}"))?;
    let github_client = github_token
        .filter(|value| !value.trim().is_empty())
        .map(|token| pr_kpi::build_github_client(&token))
        .transpose()?;
    let mut warnings = Vec::new();
    let mut projects = Vec::new();

    for repository in repositories {
        let path = repository.to_string_lossy().to_string();
        let name = repository
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("repository")
            .to_string();
        let github_default_branch = match (
            github_client.as_ref(),
            pr_kpi::github_repository_for_path(&repository),
        ) {
            (Some(client), Some((owner, repository_name))) => {
                match pr_kpi::fetch_repository_default_branch(client, &owner, &repository_name) {
                    Ok(branch) => {
                        pr_kpi::resolve_effective_default_branch(&repository, None, Some(&branch))
                            .map(|(reference, _)| reference)
                    }
                    Err(error) => {
                        warnings.push(format!("{name}: {error}"));
                        None
                    }
                }
            }
            _ => None,
        };
        let fallback_default_branch = pr_kpi::resolve_local_default_branch_ref(&repository);
        let override_branch = overrides.get(&path).cloned();
        let github_default_branch_name = github_default_branch
            .as_deref()
            .map(pr_kpi::branch_display_name);
        let effective = pr_kpi::resolve_effective_default_branch(
            &repository,
            override_branch.as_deref(),
            github_default_branch_name.as_deref(),
        );

        projects.push(GitBranchProject {
            name,
            path,
            github_default_branch,
            fallback_default_branch,
            override_branch,
            effective_default_branch: effective.as_ref().map(|(reference, _)| reference.clone()),
            effective_source: effective
                .map(|(_, source)| source)
                .unwrap_or(GitDefaultBranchSource::Missing),
            candidates: pr_kpi::list_branch_candidates(&repository)
                .into_iter()
                .map(|reference| GitBranchCandidate {
                    display_name: pr_kpi::branch_display_name(&reference),
                    reference,
                })
                .collect(),
        });
    }

    projects.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.path.cmp(&right.path))
    });

    Ok(GitBranchManagementCache {
        root_path,
        generated_at: Utc::now(),
        default_branch_override_fingerprint: overrides
            .iter()
            .map(|(path, reference)| format!("{path}={reference}"))
            .collect::<Vec<_>>()
            .join("\n"),
        projects,
        warnings,
    })
}

#[tauri::command]
pub fn get_pr_kpi(app: AppHandle, request: UsageRangeRequest) -> Result<PrKpiReport, String> {
    let resolved = validate_usage_range_request(&request)?;
    let overview = kpi_overview_from_cached_stats(&app, &request)?;
    let cache = read_pr_kpi_cache(&app)?;
    let settings = settings::load_settings(&app).unwrap_or_default();
    let max_age_minutes = i64::from(settings.refresh_interval_minutes);

    if let ResolvedUsageRange::Custom {
        start_date,
        end_date,
    } = resolved
    {
        let should_refresh = cache
            .as_ref()
            .map(|cache| {
                pr_kpi_cache_is_stale(
                    cache,
                    max_age_minutes,
                    &settings.git_usage_root,
                    &git_default_branch_overrides_fingerprint(&settings),
                ) || !cache.covers_custom_range(start_date, end_date)
            })
            .unwrap_or(true);
        if should_refresh {
            start_pr_kpi_cache_refresh(app.clone());
        }

        if let Some(cache) = cache.filter(|cache| {
            cache.root_path == settings.git_usage_root
                && cache.covers_custom_range(start_date, end_date)
        }) {
            return Ok(cache.custom_report(start_date, end_date, overview));
        }

        return Ok(pr_kpi::pending_custom_report(
            start_date,
            end_date,
            overview,
            Some("KPI 分析缓存正在后台生成，完成后会自动更新".into()),
        ));
    }

    let ResolvedUsageRange::Preset(range) = resolved else {
        unreachable!("custom usage range returned above");
    };

    let cache_is_stale = cache
        .as_ref()
        .map(|cache| {
            pr_kpi_cache_is_stale(
                cache,
                max_age_minutes,
                &settings.git_usage_root,
                &git_default_branch_overrides_fingerprint(&settings),
            )
        })
        .unwrap_or(true);
    if cache_is_stale {
        start_pr_kpi_cache_refresh(app.clone());
    }

    if let Some(cache) = cache.filter(|cache| cache.root_path == settings.git_usage_root) {
        return Ok(cache.report(range, overview));
    }

    Ok(pr_kpi::pending_report(
        range,
        overview,
        Some("KPI 分析缓存正在后台生成，完成后会自动更新".into()),
    ))
}

#[tauri::command]
pub async fn refresh_pr_kpi(
    app: AppHandle,
    request: UsageRangeRequest,
) -> Result<PrKpiReport, String> {
    let resolved = validate_usage_range_request(&request)?;
    let cache = refresh_pr_kpi_cache(app.clone(), true).await?;
    let overview = kpi_overview_after_refresh(&app, &request).await?;

    match resolved {
        ResolvedUsageRange::Preset(range) => Ok(cache.report(range, overview)),
        ResolvedUsageRange::Custom {
            start_date,
            end_date,
        } => {
            if !cache.covers_custom_range(start_date, end_date) {
                return Err("KPI 缓存刷新后仍未准备好".into());
            }
            Ok(cache.custom_report(start_date, end_date, overview))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResolvedUsageRange {
    Preset(LocalTokenUsageRange),
    Custom {
        start_date: NaiveDate,
        end_date: NaiveDate,
    },
}

fn validate_usage_range_request(request: &UsageRangeRequest) -> Result<ResolvedUsageRange, String> {
    validate_usage_range_request_with_today(request, Local::now().date_naive())
}

fn validate_usage_range_request_with_today(
    request: &UsageRangeRequest,
    today: NaiveDate,
) -> Result<ResolvedUsageRange, String> {
    match request {
        UsageRangeRequest::Preset { range } => Ok(ResolvedUsageRange::Preset(*range)),
        UsageRangeRequest::Custom {
            start_date,
            end_date,
        } => {
            let start = parse_custom_usage_date(start_date, "开始")?;
            let end = parse_custom_usage_date(end_date, "结束")?;
            if start > end {
                return Err("自定义开始日期不能晚于结束日期".into());
            }
            if end > today {
                return Err("自定义结束日期不能晚于今天".into());
            }
            let earliest_start = custom_usage_window_start(today);
            if start < earliest_start {
                return Err(format!(
                    "自定义开始日期不能早于 {}",
                    earliest_start.format("%Y-%m-%d")
                ));
            }
            Ok(ResolvedUsageRange::Custom {
                start_date: start,
                end_date: end,
            })
        }
    }
}

fn parse_custom_usage_date(value: &str, label: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| format!("自定义{label}日期格式无效，请使用 YYYY-MM-DD"))
}

fn custom_usage_window_start(today: NaiveDate) -> NaiveDate {
    today - chrono::Duration::days(CUSTOM_USAGE_WINDOW_DAYS - 1)
}

pub fn ensure_local_token_usage_cache(app: &AppHandle, max_age_minutes: i64) {
    let should_refresh = read_local_token_usage_cache(app)
        .map(|cache| {
            cache
                .as_ref()
                .map(|cache| local_token_usage_cache_is_stale(cache, max_age_minutes))
                .unwrap_or(true)
        })
        .unwrap_or(true);
    if should_refresh {
        start_local_token_usage_cache_refresh(app.clone());
    }
}

pub fn ensure_git_usage_cache(app: &AppHandle, max_age_minutes: i64, root_path: &str) {
    let branch_override_fingerprint = settings::load_settings(app)
        .map(|settings| git_default_branch_overrides_fingerprint(&settings))
        .unwrap_or_default();
    let should_refresh = read_git_usage_cache(app)
        .map(|cache| {
            cache
                .as_ref()
                .map(|cache| {
                    git_usage_cache_is_stale(
                        cache,
                        max_age_minutes,
                        root_path,
                        &branch_override_fingerprint,
                    )
                })
                .unwrap_or(true)
        })
        .unwrap_or(true);
    if should_refresh {
        start_git_usage_cache_refresh(app.clone());
    }
}

pub fn ensure_pr_kpi_cache(app: &AppHandle, max_age_minutes: i64, root_path: &str) {
    let branch_override_fingerprint = settings::load_settings(app)
        .map(|settings| git_default_branch_overrides_fingerprint(&settings))
        .unwrap_or_default();
    let should_refresh = read_pr_kpi_cache(app)
        .map(|cache| {
            cache
                .as_ref()
                .map(|cache| {
                    pr_kpi_cache_is_stale(
                        cache,
                        max_age_minutes,
                        root_path,
                        &branch_override_fingerprint,
                    )
                })
                .unwrap_or(true)
        })
        .unwrap_or(true);
    if should_refresh {
        start_pr_kpi_cache_refresh(app.clone());
    }
}

pub fn ensure_git_branch_management_cache(
    app: &AppHandle,
    max_age_minutes: i64,
    root_path: &str,
    branch_override_fingerprint: &str,
) {
    let should_refresh = read_git_branch_management_cache(app)
        .map(|cache| {
            cache
                .as_ref()
                .map(|cache| {
                    git_branch_management_cache_is_stale(
                        cache,
                        root_path,
                        branch_override_fingerprint,
                        max_age_minutes,
                    )
                })
                .unwrap_or(true)
        })
        .unwrap_or(true);
    if should_refresh {
        start_git_branch_management_cache_refresh(app.clone());
    }
}

fn start_local_token_usage_cache_refresh(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        match refresh_local_token_usage_cache(app, true).await {
            Ok(_) => {}
            Err(error) => {
                eprintln!("Token usage cache refresh failed: {error}");
            }
        }
    });
}

fn start_git_usage_cache_refresh(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        match refresh_git_usage_cache(app, true).await {
            Ok(_) => {}
            Err(error) => {
                eprintln!("Git usage cache refresh failed: {error}");
            }
        }
    });
}

fn start_pr_kpi_cache_refresh(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        match refresh_pr_kpi_cache(app, true).await {
            Ok(_) => {}
            Err(error) => {
                eprintln!("PR KPI cache refresh failed: {error}");
            }
        }
    });
}

fn start_git_branch_management_cache_refresh(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        match refresh_git_branch_management(app, true).await {
            Ok(_) => {}
            Err(error) => {
                eprintln!("Git branch management cache refresh failed: {error}");
            }
        }
    });
}

async fn refresh_local_token_usage_cache(
    app: AppHandle,
    emit_update: bool,
) -> Result<local_usage::LocalTokenUsageCache, String> {
    let _guard = claim_local_token_usage_refresh()?;
    let cache = tauri::async_runtime::spawn_blocking(local_usage::build_cache)
        .await
        .map_err(|error| format!("Token 用量缓存任务失败: {error}"))??;
    storage::write_json(&app, TOKEN_USAGE_CACHE_FILE, &cache)?;
    if emit_update {
        let _ = app.emit(TOKEN_USAGE_CACHE_UPDATED_EVENT, ());
    }
    Ok(cache)
}

async fn refresh_git_usage_cache(
    app: AppHandle,
    emit_update: bool,
) -> Result<git_usage::GitUsageCache, String> {
    let _guard = claim_git_usage_refresh()?;
    let (root, branch_override_fingerprint) = settings::load_settings(&app)
        .map(|settings| {
            let fingerprint = git_default_branch_overrides_fingerprint(&settings);
            (PathBuf::from(settings.git_usage_root), fingerprint)
        })
        .unwrap_or_else(|_| {
            (
                PathBuf::from(crate::models::default_git_usage_root()),
                String::new(),
            )
        });
    let cache = tauri::async_runtime::spawn_blocking(move || {
        git_usage::build_cache_with_override_fingerprint(root, branch_override_fingerprint)
    })
    .await
    .map_err(|error| format!("Git 统计缓存任务失败: {error}"))??;
    storage::write_json(&app, GIT_USAGE_CACHE_FILE, &cache)?;
    if emit_update {
        let _ = app.emit(GIT_USAGE_CACHE_UPDATED_EVENT, ());
    }
    Ok(cache)
}

async fn refresh_pr_kpi_cache(
    app: AppHandle,
    emit_update: bool,
) -> Result<pr_kpi::PrKpiCache, String> {
    let _guard = claim_pr_kpi_refresh()?;
    let (root, overrides, fingerprint) = settings::load_settings(&app)
        .map(|settings| {
            let fingerprint = git_default_branch_overrides_fingerprint(&settings);
            (
                PathBuf::from(settings.git_usage_root),
                settings.git_default_branch_overrides,
                fingerprint,
            )
        })
        .unwrap_or_else(|_| {
            (
                PathBuf::from(crate::models::default_git_usage_root()),
                HashMap::new(),
                String::new(),
            )
        });
    let github_token = secrets::load_github_cli_token().ok().flatten();
    let cache = tauri::async_runtime::spawn_blocking(move || {
        pr_kpi::build_cache(root, github_token, overrides, fingerprint)
    })
    .await
    .map_err(|error| format!("KPI 分析缓存任务失败: {error}"))??;
    storage::write_json(&app, PR_KPI_CACHE_FILE, &cache)?;
    if emit_update {
        let _ = app.emit(PR_KPI_CACHE_UPDATED_EVENT, ());
    }
    Ok(cache)
}

async fn refresh_git_branch_management_cache(
    app: AppHandle,
    emit_update: bool,
) -> Result<GitBranchManagementCache, String> {
    let _guard = claim_git_branch_management_refresh()?;
    let (root_path, overrides, github_token) = settings::load_settings(&app)
        .map(|settings| {
            (
                settings.git_usage_root,
                settings.git_default_branch_overrides,
                secrets::load_github_cli_token().ok().flatten(),
            )
        })
        .unwrap_or_else(|_| {
            (
                crate::models::default_git_usage_root(),
                HashMap::new(),
                secrets::load_github_cli_token().ok().flatten(),
            )
        });
    let cache = tauri::async_runtime::spawn_blocking(move || {
        build_git_branch_management_state(root_path, overrides, github_token)
    })
    .await
    .map_err(|error| format!("Default Branch 管理缓存任务失败: {error}"))??;
    storage::write_json(&app, GIT_BRANCH_MANAGEMENT_CACHE_FILE, &cache)?;
    if emit_update {
        let _ = app.emit(GIT_BRANCH_MANAGEMENT_CACHE_UPDATED_EVENT, ());
    }
    Ok(cache)
}

#[derive(Debug)]
struct LocalTokenUsageRefreshGuard;

impl Drop for LocalTokenUsageRefreshGuard {
    fn drop(&mut self) {
        TOKEN_USAGE_CACHE_REFRESHING.store(false, Ordering::Release);
    }
}

fn claim_local_token_usage_refresh() -> Result<LocalTokenUsageRefreshGuard, String> {
    if TOKEN_USAGE_CACHE_REFRESHING.swap(true, Ordering::AcqRel) {
        return Err("Token 用量正在刷新，请稍后再试".into());
    }
    Ok(LocalTokenUsageRefreshGuard)
}

#[derive(Debug)]
struct GitUsageRefreshGuard;

impl Drop for GitUsageRefreshGuard {
    fn drop(&mut self) {
        GIT_USAGE_CACHE_REFRESHING.store(false, Ordering::Release);
    }
}

fn claim_git_usage_refresh() -> Result<GitUsageRefreshGuard, String> {
    if GIT_USAGE_CACHE_REFRESHING.swap(true, Ordering::AcqRel) {
        return Err("Git 统计正在刷新，请稍后再试".into());
    }
    Ok(GitUsageRefreshGuard)
}

#[derive(Debug)]
struct GitBranchManagementRefreshGuard;

impl Drop for GitBranchManagementRefreshGuard {
    fn drop(&mut self) {
        GIT_BRANCH_MANAGEMENT_CACHE_REFRESHING.store(false, Ordering::Release);
    }
}

fn claim_git_branch_management_refresh() -> Result<GitBranchManagementRefreshGuard, String> {
    if GIT_BRANCH_MANAGEMENT_CACHE_REFRESHING.swap(true, Ordering::AcqRel) {
        return Err("Default Branch 管理数据正在刷新，请稍后再试".into());
    }
    Ok(GitBranchManagementRefreshGuard)
}

#[derive(Debug)]
struct PrKpiRefreshGuard;

impl Drop for PrKpiRefreshGuard {
    fn drop(&mut self) {
        PR_KPI_CACHE_REFRESHING.store(false, Ordering::Release);
    }
}

fn claim_pr_kpi_refresh() -> Result<PrKpiRefreshGuard, String> {
    if PR_KPI_CACHE_REFRESHING.swap(true, Ordering::AcqRel) {
        return Err("KPI 分析正在刷新，请稍后再试".into());
    }
    Ok(PrKpiRefreshGuard)
}

pub async fn refresh_inner(app: &AppHandle, store: &StateStore) -> Result<(), String> {
    run_refresh_operation(store, async {
        let settings = settings::load_settings(app)?;
        let copilot_state = app.state::<CopilotAuthState>();

        let accounts = refreshable_quota_accounts(&settings);
        let has_managed_copilot = settings
            .reverse_proxy
            .default_copilot_account_id
            .as_ref()
            .is_some_and(|value| !value.trim().is_empty());
        if accounts.is_empty() && !has_managed_copilot {
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
        let mut account_errors = HashMap::new();
        let mut updated_account_ids = HashSet::new();
        let mut latest_active_snapshot = None;
        for account in accounts {
            let account_settings = settings_for_refresh_account(&settings, &account);
            match fetch_quota_with_oauth_retry(&account_settings).await {
                Ok(snapshot) => {
                    if account.account_id == settings.account_id {
                        latest_active_snapshot = Some(snapshot.clone());
                    }
                    write_account_snapshot(app, &account.account_id, &snapshot)?;
                    updated_account_ids.insert(account.account_id.clone());
                }
                Err(error) => {
                    account_errors.insert(account.account_id.clone(), error.message.clone());
                    errors.push(format!("{}: {}", account.account_name, error.message));
                }
            }
        }

        if let Some(snapshot) =
            refresh_managed_copilot_quota(app, &settings, &copilot_state).await?
        {
            write_account_snapshot(app, &snapshot.account_id, &snapshot)?;
            updated_account_ids.insert(snapshot.account_id.clone());
            if settings.account_id == snapshot.account_id {
                latest_active_snapshot = Some(snapshot);
            }
        }

        let snapshots = read_account_snapshots(app)?;
        let cached_active_snapshot = snapshots.get(&settings.account_id).cloned();
        let account_statuses = account_statuses_from_settings_snapshots_and_errors(
            &settings,
            &snapshots,
            &account_errors,
        );
        {
            let mut guard = store.inner.write().await;
            guard.snapshot = latest_active_snapshot.or(cached_active_snapshot);
            guard.accounts = account_statuses.clone();
            guard.last_refreshed_at = Some(Utc::now());

            if errors.is_empty() {
                guard.refresh_status = crate::models::RefreshStatus::Ok;
                guard.last_error = None;
            } else {
                guard.refresh_status = crate::models::RefreshStatus::Error;
                guard.last_error = Some(errors.join("；"));
            }
        }

        if !updated_account_ids.is_empty() {
            let _ = notifications::notify_low_quota_after_refresh(
                app,
                &settings,
                &account_statuses,
                &updated_account_ids,
            );
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("；"))
        }
    })
    .await
}

pub async fn hydrate_cached_snapshot(app: &AppHandle, store: &StateStore) -> Result<(), String> {
    let settings = settings::load_settings(app)?;
    if !settings.secret_configured {
        let mut guard = store.inner.write().await;
        guard.snapshot = None;
        guard.accounts =
            account_statuses_from_settings_and_snapshots(&settings, &read_account_snapshots(app)?);
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
        PROVIDER_OPENAI | PROVIDER_ANTHROPIC | PROVIDER_KIMI | PROVIDER_GLM | PROVIDER_MINIMAX
    )
}

fn provider_display_label(provider: &str) -> &'static str {
    match provider {
        crate::models::PROVIDER_ANTHROPIC => "Anthropic",
        crate::models::PROVIDER_COPILOT => "Copilot",
        crate::models::PROVIDER_GLM => "GLM",
        crate::models::PROVIDER_KIMI => "Kimi",
        crate::models::PROVIDER_MINIMAX => "MiniMax",
        _ => "OpenAI",
    }
}

async fn refresh_managed_copilot_quota(
    _app: &AppHandle,
    settings: &AppSettings,
    copilot_state: &State<'_, CopilotAuthState>,
) -> Result<Option<QuotaSnapshot>, String> {
    let default_account_id = settings.reverse_proxy.default_copilot_account_id.clone();
    let Some(account_id) = default_account_id else {
        return Ok(None);
    };

    let manager = copilot_state.0.read().await;
    let Some(account) = manager.get_account(&account_id).await else {
        return Ok(None);
    };
    let github_token = manager
        .get_github_token_for_account(&account_id)
        .await
        .map_err(|error| error.to_string())?;
    provider::fetch_quota(
        &managed_copilot_snapshot_id(&account.id),
        &account.login,
        None,
        &managed_copilot_probe_credentials(github_token),
    )
    .await
    .map(Some)
    .map_err(|error| {
        if matches!(error.kind, ProviderErrorKind::Unauthorized) {
            "Copilot OAuth 认证失败，请重新登录 GitHub Copilot".into()
        } else {
            error.message
        }
    })
}

pub(crate) fn managed_copilot_snapshot_id(account_id: &str) -> String {
    format!("managed:copilot:{account_id}")
}

fn managed_copilot_probe_credentials(github_token: String) -> ProbeCredentials {
    ProbeCredentials {
        provider: PROVIDER_COPILOT.into(),
        auth_mode: AuthMode::ApiKey,
        secret: github_token,
        chatgpt_account_id: None,
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

fn read_local_token_usage_cache(
    app: &AppHandle,
) -> Result<Option<local_usage::LocalTokenUsageCache>, String> {
    storage::read_json::<local_usage::LocalTokenUsageCache>(app, TOKEN_USAGE_CACHE_FILE)
}

fn read_git_usage_cache(app: &AppHandle) -> Result<Option<git_usage::GitUsageCache>, String> {
    storage::read_json::<git_usage::GitUsageCache>(app, GIT_USAGE_CACHE_FILE)
}

fn read_git_branch_management_cache(
    app: &AppHandle,
) -> Result<Option<GitBranchManagementCache>, String> {
    storage::read_json::<GitBranchManagementCache>(app, GIT_BRANCH_MANAGEMENT_CACHE_FILE)
}

fn read_pr_kpi_cache(app: &AppHandle) -> Result<Option<pr_kpi::PrKpiCache>, String> {
    storage::read_json::<pr_kpi::PrKpiCache>(app, PR_KPI_CACHE_FILE)
}

fn local_token_usage_cache_is_stale(
    cache: &local_usage::LocalTokenUsageCache,
    max_age_minutes: i64,
) -> bool {
    if max_age_minutes <= 0 {
        return true;
    }
    (Utc::now() - cache.generated_at).num_minutes() >= max_age_minutes
}

fn git_usage_cache_is_stale(
    cache: &git_usage::GitUsageCache,
    max_age_minutes: i64,
    root_path: &str,
    branch_override_fingerprint: &str,
) -> bool {
    if cache.root_path != root_path {
        return true;
    }

    if cache.default_branch_override_fingerprint != branch_override_fingerprint {
        return true;
    }

    if !git_usage_cache_has_commit_details(cache) {
        return true;
    }

    if max_age_minutes <= 0 {
        return true;
    }
    (Utc::now() - cache.generated_at).num_minutes() >= max_age_minutes
}

fn git_usage_cache_has_commit_details(cache: &git_usage::GitUsageCache) -> bool {
    [
        &cache.today,
        &cache.last3_days,
        &cache.this_week,
        &cache.this_month,
    ]
    .into_iter()
    .all(git_usage_report_has_commit_details)
        && cache
            .custom_days
            .iter()
            .all(git_usage_cached_day_has_commit_details)
}

fn git_usage_report_has_commit_details(report: &GitUsageReport) -> bool {
    let has_activity = report.totals.added_lines > 0
        || report.totals.deleted_lines > 0
        || report.totals.changed_files > 0;
    !has_activity
        || (!report.commits.is_empty()
            && report
                .commits
                .iter()
                .all(git_usage_commit_has_duplicate_metadata))
}

fn git_usage_cached_day_has_commit_details(day: &git_usage::GitUsageCachedDay) -> bool {
    let has_activity =
        day.totals.added_lines > 0 || day.totals.deleted_lines > 0 || day.totals.changed_files > 0;
    !has_activity
        || (!day.commits.is_empty()
            && day
                .commits
                .iter()
                .all(git_usage_commit_has_duplicate_metadata))
}

fn git_usage_commit_has_duplicate_metadata(commit: &crate::models::GitUsageCommit) -> bool {
    !commit.commit_hash.trim().is_empty()
        && !commit.committer_name.trim().is_empty()
        && !commit.committer_email.trim().is_empty()
        && !commit.patch_id.trim().is_empty()
        && !commit.commit_role.trim().is_empty()
        && (commit.duplicate_group_size == 0 || !commit.duplicate_group_id.trim().is_empty())
}

pub(crate) fn git_default_branch_overrides_fingerprint(settings: &AppSettings) -> String {
    git_default_branch_overrides_fingerprint_from_map(&settings.git_default_branch_overrides)
}

fn git_default_branch_overrides_fingerprint_from_map(
    overrides: &HashMap<String, String>,
) -> String {
    let mut items = overrides
        .iter()
        .map(|(path, reference)| (path.as_str(), reference.as_str()))
        .collect::<Vec<_>>();
    items.sort_by(|left, right| left.cmp(right));
    items
        .into_iter()
        .map(|(path, reference)| format!("{path}={reference}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn git_branch_management_cache_is_stale(
    cache: &GitBranchManagementCache,
    root_path: &str,
    branch_override_fingerprint: &str,
    max_age_minutes: i64,
) -> bool {
    if cache.root_path != root_path {
        return true;
    }
    if cache.default_branch_override_fingerprint != branch_override_fingerprint {
        return true;
    }
    if max_age_minutes <= 0 {
        return true;
    }
    (Utc::now() - cache.generated_at).num_minutes() >= max_age_minutes
}

fn pr_kpi_cache_is_stale(
    cache: &pr_kpi::PrKpiCache,
    max_age_minutes: i64,
    root_path: &str,
    branch_override_fingerprint: &str,
) -> bool {
    if cache.root_path != root_path {
        return true;
    }
    if cache.default_branch_override_fingerprint != branch_override_fingerprint {
        return true;
    }
    if max_age_minutes <= 0 {
        return true;
    }
    (Utc::now() - cache.generated_at).num_minutes() >= max_age_minutes
}

fn kpi_overview_from_cached_stats(
    app: &AppHandle,
    request: &UsageRangeRequest,
) -> Result<PrKpiOverview, String> {
    let token_report = get_local_token_usage(app.clone(), request.clone())
        .unwrap_or_else(|_| local_usage::pending_report(LocalTokenUsageRange::ThisMonth, None));
    let git_report = get_git_usage(app.clone(), request.clone())
        .unwrap_or_else(|_| git_usage::pending_report(LocalTokenUsageRange::ThisMonth, None));
    Ok(pr_kpi::build_overview(&token_report, &git_report))
}

async fn kpi_overview_after_refresh(
    app: &AppHandle,
    request: &UsageRangeRequest,
) -> Result<PrKpiOverview, String> {
    let token_report = refresh_local_token_usage(app.clone(), request.clone())
        .await
        .or_else(|_| get_local_token_usage(app.clone(), request.clone()))?;
    let git_report = refresh_git_usage(app.clone(), request.clone())
        .await
        .or_else(|_| get_git_usage(app.clone(), request.clone()))?;
    Ok(pr_kpi::build_overview(&token_report, &git_report))
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
    account_statuses_from_settings_snapshots_and_errors(settings, snapshots, &HashMap::new())
}

fn account_statuses_from_settings_snapshots_and_errors(
    settings: &AppSettings,
    snapshots: &HashMap<String, QuotaSnapshot>,
    errors: &HashMap<String, String>,
) -> Vec<AccountQuotaStatus> {
    let mut statuses = accounts_for_status(settings)
        .into_iter()
        .map(|account| {
            account_status_from_snapshot(
                &account,
                snapshots.get(&account.account_id),
                errors.get(&account.account_id),
            )
        })
        .collect::<Vec<_>>();

    if let Some(default_copilot_account_id) =
        settings.reverse_proxy.default_copilot_account_id.as_deref()
    {
        let managed_account_id = managed_copilot_snapshot_id(default_copilot_account_id);
        if let Some(snapshot) = snapshots.get(&managed_account_id) {
            statuses.push(AccountQuotaStatus {
                account_id: managed_account_id.clone(),
                account_name: snapshot.account_name.clone(),
                provider: PROVIDER_COPILOT.into(),
                five_hour: snapshot.five_hour.clone(),
                seven_day: snapshot.seven_day.clone(),
                fetched_at: Some(snapshot.fetched_at),
                source: Some(snapshot.source.clone()),
                last_error: errors.get(&managed_account_id).cloned(),
            });
        }
    }

    statuses
}

fn accounts_for_status(settings: &AppSettings) -> Vec<ConnectedAccount> {
    if !settings.accounts.is_empty() {
        return settings
            .accounts
            .iter()
            .filter(|account| account.provider != PROVIDER_COPILOT)
            .cloned()
            .collect();
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
    last_error: Option<&String>,
) -> AccountQuotaStatus {
    AccountQuotaStatus {
        account_id: account.account_id.clone(),
        account_name: display_account_name_for_status(account, snapshot),
        provider: account.provider.clone(),
        five_hour: snapshot.and_then(|snapshot| snapshot.five_hour.clone()),
        seven_day: snapshot.and_then(|snapshot| snapshot.seven_day.clone()),
        fetched_at: snapshot.map(|snapshot| snapshot.fetched_at),
        source: snapshot.map(|snapshot| snapshot.source.clone()),
        last_error: last_error.cloned(),
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

    #[test]
    fn managed_copilot_probe_credentials_use_github_token_semantics() {
        let credentials = managed_copilot_probe_credentials("ghu_test_token".into());

        assert_eq!(credentials.provider, PROVIDER_COPILOT);
        assert!(matches!(credentials.auth_mode, AuthMode::ApiKey));
        assert_eq!(credentials.secret, "ghu_test_token");
        assert_eq!(credentials.chatgpt_account_id, None);
    }

    #[test]
    fn reverse_proxy_settings_fall_back_to_first_openai_oauth_account() {
        let settings = AppSettings {
            accounts: vec![
                ConnectedAccount {
                    account_id: "openai-1".into(),
                    account_name: "OpenAI 1".into(),
                    provider: PROVIDER_OPENAI.into(),
                    auth_mode: AuthMode::OAuth,
                    chatgpt_account_id: Some("openai-1".into()),
                    secret_configured: true,
                },
                ConnectedAccount {
                    account_id: "openai-2".into(),
                    account_name: "OpenAI 2".into(),
                    provider: PROVIDER_OPENAI.into(),
                    auth_mode: AuthMode::OAuth,
                    chatgpt_account_id: Some("openai-2".into()),
                    secret_configured: true,
                },
            ],
            ..AppSettings::default()
        };

        let managed = openai_oauth_managed_accounts(&settings);
        assert_eq!(
            resolve_default_openai_reverse_account_id(&settings, &managed).as_deref(),
            Some("openai-1")
        );
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

    #[tokio::test]
    async fn failed_refresh_operations_do_not_leave_refreshing_state_stuck() {
        let store = StateStore::default();

        let error = run_refresh_operation(&store, async {
            Err::<(), _>("后台刷新失败".to_string())
        })
        .await
        .unwrap_err();

        let status = store.inner.read().await.clone();

        assert_eq!(error, "后台刷新失败");
        assert!(matches!(
            status.refresh_status,
            crate::models::RefreshStatus::Error
        ));
        assert_eq!(status.last_error.as_deref(), Some("后台刷新失败"));
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
                    label: None,
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
    fn account_statuses_attach_refresh_errors_to_matching_accounts() {
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
        let snapshots = HashMap::new();
        let mut errors = HashMap::new();
        errors.insert("second".to_string(), "认证失败，请重新授权".to_string());

        let statuses =
            account_statuses_from_settings_snapshots_and_errors(&settings, &snapshots, &errors);

        assert_eq!(statuses[0].last_error, None);
        assert_eq!(
            statuses[1].last_error.as_deref(),
            Some("认证失败，请重新授权")
        );
    }

    #[test]
    fn local_token_usage_refresh_guard_rejects_parallel_refreshes_and_releases() {
        let guard = claim_local_token_usage_refresh().unwrap();

        let error = claim_local_token_usage_refresh().unwrap_err();

        assert_eq!(error, "Token 用量正在刷新，请稍后再试");
        drop(guard);
        assert!(claim_local_token_usage_refresh().is_ok());
    }

    #[test]
    fn git_usage_refresh_guard_rejects_parallel_refreshes_and_releases() {
        let guard = claim_git_usage_refresh().unwrap();

        let error = claim_git_usage_refresh().unwrap_err();

        assert_eq!(error, "Git 统计正在刷新，请稍后再试");
        drop(guard);
        assert!(claim_git_usage_refresh().is_ok());
    }

    #[test]
    fn git_usage_cache_is_stale_when_root_path_changes() {
        let now = chrono::Utc::now();
        let cache = git_usage::GitUsageCache {
            root_path: "/tmp/old".into(),
            generated_at: now,
            default_branch_override_fingerprint: String::new(),
            today: git_usage::empty_report(LocalTokenUsageRange::Today, None),
            last3_days: git_usage::empty_report(LocalTokenUsageRange::Last3Days, None),
            this_week: git_usage::empty_report(LocalTokenUsageRange::ThisWeek, None),
            this_month: git_usage::empty_report(LocalTokenUsageRange::ThisMonth, None),
            custom_window_start: None,
            custom_window_end: None,
            custom_days: vec![],
        };

        assert!(git_usage_cache_is_stale(&cache, 15, "/tmp/new", ""));
        assert!(!git_usage_cache_is_stale(&cache, 15, "/tmp/old", ""));
    }

    #[test]
    fn git_usage_cache_is_stale_when_commit_details_are_missing_from_active_cache() {
        let now = chrono::Utc::now();
        let mut cache = git_usage::GitUsageCache {
            root_path: "/tmp/old".into(),
            generated_at: now,
            default_branch_override_fingerprint: String::new(),
            today: git_usage::empty_report(LocalTokenUsageRange::Today, None),
            last3_days: git_usage::empty_report(LocalTokenUsageRange::Last3Days, None),
            this_week: git_usage::empty_report(LocalTokenUsageRange::ThisWeek, None),
            this_month: git_usage::empty_report(LocalTokenUsageRange::ThisMonth, None),
            custom_window_start: None,
            custom_window_end: None,
            custom_days: vec![],
        };
        cache.today.totals.added_lines = 12;
        cache.today.totals.deleted_lines = 3;
        cache.today.commits.clear();

        assert!(git_usage_cache_is_stale(&cache, 15, "/tmp/old", ""));

        cache.today.commits.push(crate::models::GitUsageCommit {
            commit_hash: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            short_hash: "aaaaaaaaaa".into(),
            timestamp: now,
            author_name: "Test User".into(),
            author_email: "test@example.com".into(),
            committer_name: "Test User".into(),
            committer_email: "test@example.com".into(),
            subject: "test commit".into(),
            repository_name: "repo".into(),
            repository_path: "/tmp/repo".into(),
            parent_count: 1,
            patch_id: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            duplicate_group_id: "group-a".into(),
            duplicate_group_size: 1,
            is_group_representative: true,
            commit_role: "original".into(),
            added_lines: 12,
            deleted_lines: 3,
            changed_files: 1,
        });

        assert!(!git_usage_cache_is_stale(&cache, 15, "/tmp/old", ""));
    }

    fn git_usage_cache_is_stale_when_default_branch_overrides_change() {
        let now = chrono::Utc::now();
        let cache = git_usage::GitUsageCache {
            root_path: "/tmp/old".into(),
            generated_at: now,
            default_branch_override_fingerprint: "repo=refs/heads/main".into(),
            today: git_usage::empty_report(LocalTokenUsageRange::Today, None),
            last3_days: git_usage::empty_report(LocalTokenUsageRange::Last3Days, None),
            this_week: git_usage::empty_report(LocalTokenUsageRange::ThisWeek, None),
            this_month: git_usage::empty_report(LocalTokenUsageRange::ThisMonth, None),
            custom_window_start: None,
            custom_window_end: None,
            custom_days: vec![],
        };

        assert!(git_usage_cache_is_stale(
            &cache,
            15,
            "/tmp/old",
            "repo=refs/heads/dev",
        ));
        assert!(!git_usage_cache_is_stale(
            &cache,
            15,
            "/tmp/old",
            "repo=refs/heads/main",
        ));
    }

    #[test]
    fn pr_kpi_cache_is_stale_when_default_branch_overrides_change() {
        let now = chrono::Utc::now();
        let cache = pr_kpi::PrKpiCache {
            root_path: "/tmp/old".into(),
            generated_at: now,
            default_branch_override_fingerprint: "repo=refs/heads/main".into(),
            github_login: Some("octocat".into()),
            custom_window_start: None,
            custom_window_end: None,
            pull_requests: vec![],
            missing_sources: vec![],
            warnings: vec![],
        };

        assert!(pr_kpi_cache_is_stale(
            &cache,
            15,
            "/tmp/old",
            "repo=refs/heads/dev",
        ));
        assert!(!pr_kpi_cache_is_stale(
            &cache,
            15,
            "/tmp/old",
            "repo=refs/heads/main",
        ));
    }

    #[test]
    fn git_usage_cache_is_stale_when_duplicate_metadata_is_missing_from_cached_commits() {
        let now = chrono::Utc::now();
        let mut cache = git_usage::GitUsageCache {
            root_path: "/tmp/old".into(),
            generated_at: now,
            default_branch_override_fingerprint: String::new(),
            today: git_usage::empty_report(LocalTokenUsageRange::Today, None),
            last3_days: git_usage::empty_report(LocalTokenUsageRange::Last3Days, None),
            this_week: git_usage::empty_report(LocalTokenUsageRange::ThisWeek, None),
            this_month: git_usage::empty_report(LocalTokenUsageRange::ThisMonth, None),
            custom_window_start: None,
            custom_window_end: None,
            custom_days: vec![],
        };
        cache.today.totals.added_lines = 12;
        cache.today.totals.deleted_lines = 3;
        cache.today.commits.push(crate::models::GitUsageCommit {
            commit_hash: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            short_hash: "aaaaaaaaaa".into(),
            timestamp: now,
            author_name: "Test User".into(),
            author_email: "test@example.com".into(),
            committer_name: String::new(),
            committer_email: String::new(),
            subject: "test commit".into(),
            repository_name: "repo".into(),
            repository_path: "/tmp/repo".into(),
            parent_count: 0,
            patch_id: String::new(),
            duplicate_group_id: String::new(),
            duplicate_group_size: 0,
            is_group_representative: false,
            commit_role: String::new(),
            added_lines: 12,
            deleted_lines: 3,
            changed_files: 1,
        });

        assert!(git_usage_cache_is_stale(&cache, 15, "/tmp/old", ""));
    }

    #[test]
    fn custom_usage_range_request_rejects_invalid_dates() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 4, 27).unwrap();

        let bad_format = UsageRangeRequest::Custom {
            start_date: "2026/04/20".into(),
            end_date: "2026-04-27".into(),
        };
        assert_eq!(
            validate_usage_range_request_with_today(&bad_format, today).unwrap_err(),
            "自定义开始日期格式无效，请使用 YYYY-MM-DD"
        );

        let reversed = UsageRangeRequest::Custom {
            start_date: "2026-04-28".into(),
            end_date: "2026-04-27".into(),
        };
        assert_eq!(
            validate_usage_range_request_with_today(&reversed, today).unwrap_err(),
            "自定义开始日期不能晚于结束日期"
        );

        let future_end = UsageRangeRequest::Custom {
            start_date: "2026-04-20".into(),
            end_date: "2026-04-28".into(),
        };
        assert_eq!(
            validate_usage_range_request_with_today(&future_end, today).unwrap_err(),
            "自定义结束日期不能晚于今天"
        );

        let too_old = UsageRangeRequest::Custom {
            start_date: "2026-01-27".into(),
            end_date: "2026-04-27".into(),
        };
        assert_eq!(
            validate_usage_range_request_with_today(&too_old, today).unwrap_err(),
            "自定义开始日期不能早于 2026-01-28"
        );
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
    fn refreshable_accounts_include_minimax_accounts() {
        let settings = AppSettings {
            accounts: vec![crate::models::ConnectedAccount {
                account_id: "minimax-work".into(),
                account_name: "MiniMax Work".into(),
                provider: crate::models::PROVIDER_MINIMAX.into(),
                auth_mode: AuthMode::ApiKey,
                chatgpt_account_id: None,
                secret_configured: true,
            }],
            ..AppSettings::default()
        };

        let account_ids = refreshable_quota_accounts(&settings)
            .into_iter()
            .map(|account| account.account_id)
            .collect::<Vec<_>>();

        assert_eq!(account_ids, vec!["minimax-work"]);
    }

    #[test]
    fn refreshable_accounts_include_glm_accounts() {
        let settings = AppSettings {
            accounts: vec![crate::models::ConnectedAccount {
                account_id: "glm-work".into(),
                account_name: "GLM Work".into(),
                provider: crate::models::PROVIDER_GLM.into(),
                auth_mode: AuthMode::ApiKey,
                chatgpt_account_id: None,
                secret_configured: true,
            }],
            ..AppSettings::default()
        };

        let account_ids = refreshable_quota_accounts(&settings)
            .into_iter()
            .map(|account| account.account_id)
            .collect::<Vec<_>>();

        assert_eq!(account_ids, vec!["glm-work"]);
    }

    #[test]
    fn refreshable_accounts_exclude_legacy_copilot_accounts() {
        let settings = AppSettings {
            accounts: vec![crate::models::ConnectedAccount {
                account_id: "copilot-work".into(),
                account_name: "Copilot Work".into(),
                provider: crate::models::PROVIDER_COPILOT.into(),
                auth_mode: AuthMode::ApiKey,
                chatgpt_account_id: None,
                secret_configured: true,
            }],
            ..AppSettings::default()
        };

        let account_ids = refreshable_quota_accounts(&settings)
            .into_iter()
            .map(|account| account.account_id)
            .collect::<Vec<_>>();

        assert!(account_ids.is_empty());
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
