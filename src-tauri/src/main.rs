mod app_time;
mod commands;
mod copilot_oauth;
mod errors;
mod git_usage;
mod local_proxy;
mod local_proxy_error;
mod local_proxy_sse;
mod local_proxy_streaming_responses;
mod local_proxy_transform_chat;
mod local_proxy_transform_responses;
mod local_usage;
mod models;
mod notifications;
mod oauth;
mod panel;
mod pr_kpi;
mod provider;
mod secrets;
mod settings;
mod state;
mod storage;

use std::time::Duration;
use tauri::{
    image::Image,
    menu::{Menu, MenuBuilder, MenuItem, MenuItemBuilder},
    tray::TrayIconBuilder,
    Emitter, Manager, Runtime, WindowEvent,
};

const TRAY_ID: &str = "ai-usage-tray";
const MENU_MAIN: &str = "main";
const MENU_REFRESH: &str = "refresh";
const MENU_QUIT: &str = "quit";
const TRAY_USAGE_LINE_WIDTH: usize = 52;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .manage(oauth::OAuthStore::default())
        .manage(panel::PanelAnchor::default())
        .manage(local_proxy::LocalProxyManager::default())
        .manage(state::StateStore::default())
        .setup(|app| {
            let handle = app.handle().clone();
            handle.plugin(tauri_plugin_autostart::init(
                tauri_plugin_autostart::MacosLauncher::LaunchAgent,
                None::<Vec<&str>>,
            ))?;
            let data_dir = storage::app_dir(&handle)?;
            app.manage(copilot_oauth::CopilotAuthState(std::sync::Arc::new(
                tokio::sync::RwLock::new(copilot_oauth::CopilotOAuthManager::new(data_dir)),
            )));
            set_main_window_dock_visible(&handle, false);
            let tray_menu = build_tray_menu(&handle, &models::AppStatus::default())?;

            TrayIconBuilder::with_id(TRAY_ID)
                .icon(tray_icon_image())
                .icon_as_template(true)
                .tooltip("AI Usage")
                .menu(&tray_menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    MENU_MAIN => open_main_window(app),
                    MENU_REFRESH => {
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            let state = app.state::<state::StateStore>();
                            let settings = settings::load_settings(&app).unwrap_or_default();
                            let status = state.inner.read().await.clone();
                            if !should_enable_refresh_menu(&settings, &status) {
                                refresh_tray_menu(&app, &status);
                                return;
                            }
                            let _ = commands::refresh_inner(&app, &state).await;
                            refresh_tray_menu_from_state(&app).await;
                        });
                    }
                    MENU_QUIT => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            tauri::async_runtime::spawn(async move {
                {
                    let state = handle.state::<state::StateStore>();
                    let _ = commands::hydrate_cached_snapshot(&handle, &state).await;
                    let initial_settings = settings::load_settings(&handle).unwrap_or_default();
                    let _ =
                        commands::sync_launch_at_login(&handle, initial_settings.launch_at_login);
                    let token_cache_max_age = i64::from(initial_settings.refresh_interval_minutes);
                    commands::ensure_local_token_usage_cache(&handle, token_cache_max_age);
                    commands::ensure_git_usage_cache(
                        &handle,
                        token_cache_max_age,
                        &initial_settings.git_usage_root,
                    );
                    commands::ensure_pr_kpi_cache(
                        &handle,
                        token_cache_max_age,
                        &initial_settings.git_usage_root,
                    );
                    refresh_tray_menu_from_state(&handle).await;
                }

                loop {
                    let state = handle.state::<state::StateStore>();
                    let maybe_settings = settings::load_settings(&handle);
                    if let Ok(settings) = maybe_settings {
                        commands::ensure_local_token_usage_cache(
                            &handle,
                            i64::from(settings.refresh_interval_minutes),
                        );
                        commands::ensure_git_usage_cache(
                            &handle,
                            i64::from(settings.refresh_interval_minutes),
                            &settings.git_usage_root,
                        );
                        commands::ensure_pr_kpi_cache(
                            &handle,
                            i64::from(settings.refresh_interval_minutes),
                            &settings.git_usage_root,
                        );

                        let should_refresh = {
                            let guard = state.inner.read().await;
                            if matches!(guard.refresh_status, models::RefreshStatus::Refreshing) {
                                false
                            } else if !should_enable_refresh_menu(&settings, &guard) {
                                false
                            } else {
                                match guard.last_refreshed_at {
                                    Some(last) => {
                                        UtcNow::minutes_since(last)
                                            >= i64::from(settings.refresh_interval_minutes)
                                    }
                                    None => true,
                                }
                            }
                        };

                        if should_refresh {
                            let _ = commands::refresh_inner(&handle, &state).await;
                            refresh_tray_menu_from_state(&handle).await;
                        }
                    }

                    tokio::time::sleep(Duration::from_secs(30)).await;
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_current_quota,
            commands::refresh_quota,
            commands::test_connection,
            commands::get_settings,
            commands::save_settings,
            commands::get_local_proxy_settings,
            commands::save_local_proxy_settings,
            commands::save_claude_proxy_profile,
            commands::get_reverse_proxy_settings,
            commands::save_reverse_proxy_settings,
            commands::get_reverse_proxy_status,
            commands::copilot_start_device_flow,
            commands::copilot_poll_for_account,
            commands::copilot_list_accounts,
            commands::copilot_set_default_account,
            commands::copilot_remove_account,
            commands::copilot_get_auth_status,
            commands::get_local_proxy_status,
            commands::start_local_proxy,
            commands::stop_local_proxy,
            commands::import_kimi_account,
            commands::import_glm_account,
            commands::import_minimax_account,
            commands::import_copilot_account,
            commands::start_openai_oauth,
            commands::start_anthropic_oauth,
            commands::get_oauth_status,
            commands::complete_openai_oauth,
            commands::complete_anthropic_oauth,
            commands::delete_openai_account,
            commands::delete_connected_account,
            commands::resize_main_panel,
            commands::get_local_token_usage,
            commands::refresh_local_token_usage,
            commands::get_git_usage,
            commands::refresh_git_usage,
            commands::get_pr_kpi,
            commands::refresh_pr_kpi,
            sync_tray_menu,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::WindowEvent { label, event, .. } = event {
                if let WindowEvent::CloseRequested { api, .. } = &event {
                    if label == "main" {
                        api.prevent_close();
                        if let Some(window) = app.get_webview_window("main") {
                            hide_main_window(app, &window);
                        }
                    }
                } else if should_hide_main_panel(&label, &event) {
                    if let Some(window) = app.get_webview_window("main") {
                        hide_main_window(app, &window);
                    }
                }
            }
        });
}

#[tauri::command]
async fn sync_tray_menu(app: tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<state::StateStore>();
    commands::hydrate_cached_snapshot(&app, &state).await?;
    refresh_tray_menu_from_state(&app).await;
    Ok(())
}

struct UtcNow;

impl UtcNow {
    fn minutes_since(time: chrono::DateTime<chrono::Utc>) -> i64 {
        (chrono::Utc::now() - time).num_minutes()
    }
}

fn open_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        set_main_window_dock_visible(app, true);
        let _ = window.show();
        let _ = window.set_focus();
        let _ = window.emit("show-main-panel", ());
    }
}

fn hide_main_window(app: &tauri::AppHandle, window: &tauri::WebviewWindow) {
    let _ = window.hide();
    set_main_window_dock_visible(app, false);
}

#[cfg(target_os = "macos")]
fn set_main_window_dock_visible(app: &tauri::AppHandle, visible: bool) {
    let _ = app.set_dock_visibility(visible);
}

#[cfg(not(target_os = "macos"))]
fn set_main_window_dock_visible(_app: &tauri::AppHandle, _visible: bool) {}

fn should_hide_main_panel(label: &str, event: &WindowEvent) -> bool {
    label == "main" && matches!(event, WindowEvent::CloseRequested { .. })
}

async fn refresh_tray_menu_from_state(app: &tauri::AppHandle) {
    let state = app.state::<state::StateStore>();
    let status = state.inner.read().await.clone();
    refresh_tray_menu(app, &status);
}

fn refresh_tray_menu(app: &tauri::AppHandle, status: &models::AppStatus) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let _ = tray.set_tooltip(Some(tray_tooltip(status)));
    if let Ok(menu) = build_tray_menu(app, status) {
        let _ = tray.set_menu(Some(menu));
    }
}

fn build_tray_menu(
    app: &tauri::AppHandle,
    status: &models::AppStatus,
) -> tauri::Result<Menu<tauri::Wry>> {
    let title = disabled_menu_item(app, "AI Usage")?;
    let settings = settings::load_settings(app).unwrap_or_default();
    let account_lines = tray_account_summary_lines(status);
    let has_account =
        settings.secret_configured || status.snapshot.is_some() || !account_lines.is_empty();

    let mut menu = MenuBuilder::new(app).item(&title).separator();

    if has_account {
        let lines = if account_lines.is_empty() {
            fallback_tray_account_summary_lines(&settings, status)
        } else {
            account_lines
        };
        for account_group in lines {
            for line in account_group {
                let item = account_usage_menu_item(app, &line)?;
                menu = menu.item(&item);
            }
            menu = menu.separator();
        }
    }

    let refresh = MenuItemBuilder::with_id(MENU_REFRESH, "刷新用量")
        .enabled(should_enable_refresh_menu(&settings, status))
        .build(app)?;

    menu.text(MENU_MAIN, "打开主界面")
        .item(&refresh)
        .separator()
        .text(MENU_QUIT, "退出 AI Usage")
        .build()
}

#[cfg(test)]
fn tray_menu_action_ids() -> [&'static str; 3] {
    [MENU_MAIN, MENU_REFRESH, MENU_QUIT]
}

fn disabled_menu_item(app: &tauri::AppHandle, text: &str) -> tauri::Result<MenuItem<tauri::Wry>> {
    MenuItemBuilder::new(text).enabled(false).build(app)
}

fn account_usage_menu_item<R: Runtime, M: Manager<R>>(
    manager: &M,
    text: &str,
) -> tauri::Result<MenuItem<R>> {
    MenuItemBuilder::new(text).enabled(true).build(manager)
}

fn should_enable_refresh_menu(settings: &models::AppSettings, _status: &models::AppStatus) -> bool {
    has_refreshable_quota_account(settings)
}

fn has_refreshable_quota_account(settings: &models::AppSettings) -> bool {
    if settings.accounts.is_empty() {
        return settings.secret_configured
            && provider_supports_quota_refresh(settings.active_provider());
    }

    settings.accounts.iter().any(|account| {
        account.secret_configured && provider_supports_quota_refresh(&account.provider)
    })
}

fn provider_supports_quota_refresh(provider: &str) -> bool {
    matches!(
        provider,
        models::PROVIDER_OPENAI
            | models::PROVIDER_ANTHROPIC
            | models::PROVIDER_KIMI
            | models::PROVIDER_GLM
            | models::PROVIDER_MINIMAX
            | models::PROVIDER_COPILOT
    )
}

fn fallback_tray_account_summary_lines(
    settings: &models::AppSettings,
    status: &models::AppStatus,
) -> Vec<Vec<String>> {
    let name = status
        .snapshot
        .as_ref()
        .map(|snapshot| snapshot.account_name.as_str())
        .unwrap_or(settings.account_name.as_str());
    vec![tray_account_summary_group(
        format!(
            "{}    {}",
            provider_display_label(settings.active_provider()),
            display_account_name(name)
        ),
        status.snapshot.as_ref().and_then(|s| s.five_hour.as_ref()),
        status.snapshot.as_ref().and_then(|s| s.seven_day.as_ref()),
    )]
}

fn tray_account_summary_lines(status: &models::AppStatus) -> Vec<Vec<String>> {
    status
        .accounts
        .iter()
        .map(|account| {
            tray_account_summary_group(
                format!(
                    "{}    {}",
                    provider_display_label(&account.provider),
                    display_account_name(&account.account_name)
                ),
                account.five_hour.as_ref(),
                account.seven_day.as_ref(),
            )
        })
        .collect()
}

fn tray_account_summary_group(
    account_line: String,
    five_hour: Option<&models::QuotaWindow>,
    seven_day: Option<&models::QuotaWindow>,
) -> Vec<String> {
    let mut lines = vec![account_line];
    if let Some(window) = five_hour {
        lines.push(usage_line(quota_window_label(window, "5H"), Some(window)));
    }
    if let Some(window) = seven_day {
        lines.push(usage_line(quota_window_label(window, "7D"), Some(window)));
    }
    lines
}

fn provider_display_label(provider: &str) -> &'static str {
    match provider {
        models::PROVIDER_ANTHROPIC => "Anthropic",
        models::PROVIDER_COPILOT => "Copilot",
        models::PROVIDER_GLM => "GLM",
        models::PROVIDER_KIMI => "Kimi",
        models::PROVIDER_MINIMAX => "MiniMax",
        _ => "OpenAI",
    }
}

fn display_account_name(name: &str) -> &str {
    if name.trim().is_empty() {
        "OpenAI"
    } else {
        name
    }
}

fn usage_line(label: &str, window: Option<&models::QuotaWindow>) -> String {
    let percent = compact_percent(window);
    let label_width = label.chars().count();
    let value_width = TRAY_USAGE_LINE_WIDTH.saturating_sub(label_width);
    format!("{label}{percent:>width$}", width = value_width)
}

fn tray_tooltip(status: &models::AppStatus) -> String {
    if !status.accounts.is_empty() {
        let account_lines = status
            .accounts
            .iter()
            .map(|account| {
                let mut lines = vec![display_account_name(&account.account_name).to_string()];
                lines.extend(tooltip_quota_lines(
                    account.five_hour.as_ref(),
                    account.seven_day.as_ref(),
                ));
                lines.join("\n")
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        return format!("AI Usage\n{account_lines}");
    }

    let Some(snapshot) = status.snapshot.as_ref() else {
        return "AI Usage".to_string();
    };

    let lines = tooltip_quota_lines(snapshot.five_hour.as_ref(), snapshot.seven_day.as_ref());
    if lines.is_empty() {
        "AI Usage".to_string()
    } else {
        format!("AI Usage\n{}", lines.join("\n"))
    }
}

fn tooltip_quota_lines(
    five_hour: Option<&models::QuotaWindow>,
    seven_day: Option<&models::QuotaWindow>,
) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(window) = five_hour {
        lines.push(format!(
            "{} {}",
            quota_window_label(window, "5 小时"),
            compact_percent(Some(window))
        ));
    }
    if let Some(window) = seven_day {
        lines.push(format!(
            "{} {}",
            quota_window_label(window, "1 周"),
            compact_percent(Some(window))
        ));
    }
    lines
}

fn quota_window_label<'a>(window: &'a models::QuotaWindow, fallback: &'a str) -> &'a str {
    window.label.as_deref().unwrap_or(fallback)
}

fn compact_percent(window: Option<&models::QuotaWindow>) -> String {
    window
        .map(|window| format!("{:.0}%", window.remaining_percent.round()))
        .unwrap_or_else(|| "--".to_string())
}

fn tray_icon_image() -> Image<'static> {
    const SIZE: usize = 18;
    const SAMPLES: usize = 4;
    let mut rgba = vec![0; SIZE * SIZE * 4];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let mut covered = 0;
            for sample_y in 0..SAMPLES {
                for sample_x in 0..SAMPLES {
                    let px = x as f64 + (sample_x as f64 + 0.5) / SAMPLES as f64;
                    let py = y as f64 + (sample_y as f64 + 0.5) / SAMPLES as f64;
                    if logo_sample_is_filled(px, py) {
                        covered += 1;
                    }
                }
            }
            let alpha = ((covered as f64 / (SAMPLES * SAMPLES) as f64) * 255.0).round() as u8;
            let index = (y * SIZE + x) * 4;
            rgba[index] = 0;
            rgba[index + 1] = 0;
            rgba[index + 2] = 0;
            rgba[index + 3] = alpha;
        }
    }
    Image::new_owned(rgba, SIZE as u32, SIZE as u32)
}

fn logo_sample_is_filled(x: f64, y: f64) -> bool {
    const SIGNAL_WIDTH: f64 = 2.25;

    stroke_segment_distance(x, y, 3.0, 11.6, 6.2, 11.6) <= SIGNAL_WIDTH / 2.0
        || stroke_segment_distance(x, y, 6.2, 11.6, 7.9, 4.2) <= SIGNAL_WIDTH / 2.0
        || stroke_segment_distance(x, y, 7.9, 4.2, 10.7, 15.7) <= SIGNAL_WIDTH / 2.0
        || stroke_segment_distance(x, y, 10.7, 15.7, 13.1, 9.0) <= SIGNAL_WIDTH / 2.0
        || stroke_segment_distance(x, y, 13.1, 9.0, 15.4, 9.0) <= SIGNAL_WIDTH / 2.0
}

fn stroke_segment_distance(px: f64, py: f64, ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    let dx = bx - ax;
    let dy = by - ay;
    let length_squared = dx * dx + dy * dy;
    if length_squared == 0.0 {
        return ((px - ax).powi(2) + (py - ay).powi(2)).sqrt();
    }

    let t = (((px - ax) * dx + (py - ay) * dy) / length_squared).clamp(0.0, 1.0);
    let closest_x = ax + t * dx;
    let closest_y = ay + t * dy;
    ((px - closest_x).powi(2) + (py - closest_y).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_menu_has_no_settings_entry() {
        assert_eq!(tray_menu_action_ids(), [MENU_MAIN, MENU_REFRESH, MENU_QUIT]);
    }

    #[test]
    fn account_usage_menu_items_are_enabled_for_normal_text_color() {
        let app = tauri::test::mock_app();
        let item = account_usage_menu_item(app.handle(), "5H    90%").unwrap();

        assert!(item.is_enabled().unwrap());
    }

    #[test]
    fn refresh_menu_is_disabled_without_bound_account() {
        let mut settings = models::AppSettings::default();
        let mut status = models::AppStatus::default();

        settings.secret_configured = false;
        assert!(!should_enable_refresh_menu(&settings, &status));

        status.snapshot = Some(models::QuotaSnapshot {
            account_id: settings.account_id.clone(),
            account_name: settings.account_name.clone(),
            five_hour: None,
            seven_day: None,
            fetched_at: chrono::Utc::now(),
            source: "test".to_string(),
        });
        assert!(!should_enable_refresh_menu(&settings, &status));

        settings.secret_configured = true;
        assert!(should_enable_refresh_menu(&settings, &status));

        settings.accounts = vec![models::ConnectedAccount {
            account_id: settings.account_id.clone(),
            account_name: "claude@example.com".into(),
            provider: models::PROVIDER_ANTHROPIC.into(),
            auth_mode: models::AuthMode::OAuth,
            chatgpt_account_id: None,
            secret_configured: true,
        }];
        assert!(should_enable_refresh_menu(&settings, &status));

        settings.accounts.push(models::ConnectedAccount {
            account_id: "openai".into(),
            account_name: "openai@example.com".into(),
            provider: models::PROVIDER_OPENAI.into(),
            auth_mode: models::AuthMode::OAuth,
            chatgpt_account_id: None,
            secret_configured: true,
        });
        assert!(should_enable_refresh_menu(&settings, &status));

        settings.accounts = vec![models::ConnectedAccount {
            account_id: "glm".into(),
            account_name: "GLM Work".into(),
            provider: models::PROVIDER_GLM.into(),
            auth_mode: models::AuthMode::ApiKey,
            chatgpt_account_id: None,
            secret_configured: true,
        }];
        assert!(should_enable_refresh_menu(&settings, &status));
    }

    #[test]
    fn tray_summary_lines_include_multiple_accounts() {
        let status = models::AppStatus {
            accounts: vec![
                models::AccountQuotaStatus {
                    account_id: "first".into(),
                    account_name: "first@example.com".into(),
                    provider: models::PROVIDER_OPENAI.into(),
                    five_hour: Some(models::QuotaWindow {
                        label: None,
                        used_percent: 4.0,
                        remaining_percent: 96.0,
                        reset_at: None,
                        window_minutes: Some(300),
                    }),
                    seven_day: None,
                    fetched_at: Some(chrono::Utc::now()),
                    source: Some("probe_headers".into()),
                    last_error: None,
                },
                models::AccountQuotaStatus {
                    account_id: "second".into(),
                    account_name: "second@example.com".into(),
                    provider: models::PROVIDER_ANTHROPIC.into(),
                    five_hour: None,
                    seven_day: Some(models::QuotaWindow {
                        label: None,
                        used_percent: 10.0,
                        remaining_percent: 90.0,
                        reset_at: None,
                        window_minutes: Some(10080),
                    }),
                    fetched_at: Some(chrono::Utc::now()),
                    source: Some("probe_headers".into()),
                    last_error: None,
                },
            ],
            ..models::AppStatus::default()
        };

        let lines = tray_account_summary_lines(&status);

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].len(), 2);
        assert_eq!(lines[0][0], "OpenAI    first@example.com");
        assert_eq!(
            lines[0][1],
            format!(
                "{:<2}{:>width$}",
                "5H",
                "96%",
                width = TRAY_USAGE_LINE_WIDTH - 2
            )
        );
        assert_eq!(lines[1].len(), 2);
        assert_eq!(lines[1][0], "Anthropic    second@example.com");
        assert_eq!(
            lines[1][1],
            format!(
                "{:<2}{:>width$}",
                "7D",
                "90%",
                width = TRAY_USAGE_LINE_WIDTH - 2
            )
        );
    }

    #[test]
    fn usage_line_right_aligns_percent_value() {
        let line = usage_line(
            "5H",
            Some(&models::QuotaWindow {
                label: None,
                used_percent: 12.0,
                remaining_percent: 88.0,
                reset_at: None,
                window_minutes: Some(300),
            }),
        );

        assert_eq!(line.chars().count(), 52);
        assert!(line.starts_with("5H"));
        assert!(line.ends_with("88%"));
    }

    #[test]
    fn tray_summary_lines_hide_missing_quota_windows() {
        let status = models::AppStatus {
            accounts: vec![models::AccountQuotaStatus {
                account_id: "minimax".into(),
                account_name: "MiniMax Account".into(),
                provider: models::PROVIDER_MINIMAX.into(),
                five_hour: Some(models::QuotaWindow {
                    label: None,
                    used_percent: 0.0,
                    remaining_percent: 100.0,
                    reset_at: None,
                    window_minutes: Some(300),
                }),
                seven_day: None,
                fetched_at: Some(chrono::Utc::now()),
                source: Some("minimax_coding_plan".into()),
                last_error: None,
            }],
            ..models::AppStatus::default()
        };

        let lines = tray_account_summary_lines(&status);

        assert!(!lines[0].iter().any(|line| line.contains("--")));
    }

    #[test]
    fn keeps_settings_window_visible_for_focus_events() {
        assert!(!should_hide_main_panel(
            "main",
            &WindowEvent::Focused(false)
        ));
        assert!(!should_hide_main_panel("main", &WindowEvent::Focused(true)));
    }
}
