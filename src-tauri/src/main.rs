mod commands;
mod errors;
mod models;
mod oauth;
mod panel;
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
    ActivationPolicy, Emitter, Manager, WindowEvent,
};

const TRAY_ID: &str = "ai-usage-tray";
const MENU_MAIN: &str = "main";
const MENU_REFRESH: &str = "refresh";
const MENU_QUIT: &str = "quit";

fn main() {
    tauri::Builder::default()
        .manage(oauth::OAuthStore::default())
        .manage(panel::PanelAnchor::default())
        .manage(state::StateStore::default())
        .setup(|app| {
            let handle = app.handle().clone();
            let _ = handle.set_activation_policy(ActivationPolicy::Accessory);
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
                    refresh_tray_menu_from_state(&handle).await;
                }

                loop {
                    let state = handle.state::<state::StateStore>();
                    let maybe_settings = settings::load_settings(&handle);
                    if let Ok(settings) = maybe_settings {
                        let should_refresh = {
                            let guard = state.inner.read().await;
                            if matches!(guard.refresh_status, models::RefreshStatus::Refreshing) {
                                false
                            } else {
                                match guard.last_refreshed_at {
                                    Some(last) => {
                                        UtcNow::minutes_since(last)
                                            >= i64::from(settings.refresh_interval_minutes)
                                    }
                                    None => settings.secret_configured,
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
            commands::start_openai_oauth,
            commands::get_oauth_status,
            commands::complete_openai_oauth,
            commands::delete_openai_account,
            commands::resize_main_panel,
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
                            let _ = window.hide();
                        }
                    }
                }

                if should_hide_main_panel(&label, &event) {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.hide();
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
        let _ = window.show();
        let _ = window.set_focus();
        let _ = window.emit("show-main-panel", ());
    }
}

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
            let account = disabled_menu_item(app, &account_group[0])?;
            let five_hour = disabled_menu_item(app, &account_group[1])?;
            let seven_day = disabled_menu_item(app, &account_group[2])?;
            menu = menu
                .item(&account)
                .item(&five_hour)
                .item(&seven_day)
                .separator();
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

fn should_enable_refresh_menu(settings: &models::AppSettings, _status: &models::AppStatus) -> bool {
    settings.secret_configured
}

fn fallback_tray_account_summary_lines(
    settings: &models::AppSettings,
    status: &models::AppStatus,
) -> Vec<[String; 3]> {
    let name = status
        .snapshot
        .as_ref()
        .map(|snapshot| snapshot.account_name.as_str())
        .unwrap_or(settings.account_name.as_str());
    vec![[
        format!("OpenAI    {}", display_account_name(name)),
        usage_line(
            "5H",
            status.snapshot.as_ref().and_then(|s| s.five_hour.as_ref()),
        ),
        usage_line(
            "7D",
            status.snapshot.as_ref().and_then(|s| s.seven_day.as_ref()),
        ),
    ]]
}

fn tray_account_summary_lines(status: &models::AppStatus) -> Vec<[String; 3]> {
    status
        .accounts
        .iter()
        .map(|account| {
            [
                format!("OpenAI    {}", display_account_name(&account.account_name)),
                usage_line("5H", account.five_hour.as_ref()),
                usage_line("7D", account.seven_day.as_ref()),
            ]
        })
        .collect()
}

fn display_account_name(name: &str) -> &str {
    if name.trim().is_empty() {
        "OpenAI"
    } else {
        name
    }
}

fn usage_line(label: &str, window: Option<&models::QuotaWindow>) -> String {
    format!("{label}    {}", compact_percent(window))
}

fn tray_tooltip(status: &models::AppStatus) -> String {
    if !status.accounts.is_empty() {
        let account_lines = status
            .accounts
            .iter()
            .map(|account| {
                format!(
                    "{}\n5 小时 {}\n1 周 {}",
                    display_account_name(&account.account_name),
                    compact_percent(account.five_hour.as_ref()),
                    compact_percent(account.seven_day.as_ref())
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        return format!("AI Usage\n{account_lines}");
    }

    let Some(snapshot) = status.snapshot.as_ref() else {
        return "AI Usage".to_string();
    };

    let five = compact_percent(snapshot.five_hour.as_ref());
    let seven = compact_percent(snapshot.seven_day.as_ref());
    format!("AI Usage\n5 小时 {five}\n1 周 {seven}")
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
    const FRAME_WIDTH: f64 = 2.2;
    const SIGNAL_WIDTH: f64 = 2.15;

    stroke_segment_distance(x, y, 4.6, 5.0, 4.6, 12.4) <= FRAME_WIDTH / 2.0
        || stroke_segment_distance(x, y, 4.6, 12.4, 8.2, 14.2) <= FRAME_WIDTH / 2.0
        || stroke_segment_distance(x, y, 8.2, 14.2, 13.4, 14.2) <= FRAME_WIDTH / 2.0
        || stroke_segment_distance(x, y, 13.4, 14.2, 13.4, 8.9) <= FRAME_WIDTH / 2.0
        || stroke_segment_distance(x, y, 7.0, 11.2, 9.0, 9.2) <= SIGNAL_WIDTH / 2.0
        || stroke_segment_distance(x, y, 9.0, 9.2, 11.0, 10.9) <= SIGNAL_WIDTH / 2.0
        || stroke_segment_distance(x, y, 11.0, 10.9, 14.2, 6.8) <= SIGNAL_WIDTH / 2.0
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
    }

    #[test]
    fn tray_summary_lines_include_multiple_accounts() {
        let status = models::AppStatus {
            accounts: vec![
                models::AccountQuotaStatus {
                    account_id: "first".into(),
                    account_name: "first@example.com".into(),
                    five_hour: Some(models::QuotaWindow {
                        used_percent: 4.0,
                        remaining_percent: 96.0,
                        reset_at: None,
                        window_minutes: Some(300),
                    }),
                    seven_day: None,
                    fetched_at: Some(chrono::Utc::now()),
                    source: Some("probe_headers".into()),
                },
                models::AccountQuotaStatus {
                    account_id: "second".into(),
                    account_name: "second@example.com".into(),
                    five_hour: None,
                    seven_day: Some(models::QuotaWindow {
                        used_percent: 10.0,
                        remaining_percent: 90.0,
                        reset_at: None,
                        window_minutes: Some(10080),
                    }),
                    fetched_at: Some(chrono::Utc::now()),
                    source: Some("probe_headers".into()),
                },
            ],
            ..models::AppStatus::default()
        };

        let lines = tray_account_summary_lines(&status);

        assert_eq!(lines.len(), 2);
        assert_eq!(
            lines[0],
            ["OpenAI    first@example.com", "5H    96%", "7D    --"]
        );
        assert_eq!(
            lines[1],
            ["OpenAI    second@example.com", "5H    --", "7D    90%"]
        );
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
