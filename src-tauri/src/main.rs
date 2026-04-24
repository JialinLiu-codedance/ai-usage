mod commands;
mod errors;
mod models;
mod oauth;
mod provider;
mod secrets;
mod settings;
mod state;
mod storage;

use std::time::Duration;
use tauri::{
    image::Image,
    menu::{MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
    ActivationPolicy, Emitter, Manager, WindowEvent,
};

const TRAY_ID: &str = "ai-usage-tray";
const MENU_REFRESH: &str = "refresh";
const MENU_SETTINGS: &str = "settings";
const MENU_QUIT: &str = "quit";

fn main() {
    tauri::Builder::default()
        .manage(oauth::OAuthStore::default())
        .manage(state::StateStore::default())
        .setup(|app| {
            let handle = app.handle().clone();
            let _ = handle.set_activation_policy(ActivationPolicy::Accessory);
            refresh_tray_menu(&handle, &models::AppStatus::default());

            let tray_menu = build_tray_menu(&handle, &models::AppStatus::default())?;
            TrayIconBuilder::with_id(TRAY_ID)
                .menu(&tray_menu)
                .icon(tray_icon_image())
                .icon_as_template(true)
                .title("C")
                .tooltip("AI Usage")
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    MENU_REFRESH => {
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            let state = app.state::<state::StateStore>();
                            let _ = commands::refresh_inner(&app, &state).await;
                            refresh_tray_menu_from_state(&app).await;
                        });
                    }
                    MENU_SETTINGS => open_settings_window(app),
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

struct UtcNow;

impl UtcNow {
    fn minutes_since(time: chrono::DateTime<chrono::Utc>) -> i64 {
        (chrono::Utc::now() - time).num_minutes()
    }
}

fn open_settings_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
        let _ = window.emit("show-settings-window", ());
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
    if let Ok(menu) = build_tray_menu(app, status) {
        let _ = tray.set_menu(Some(menu));
    }
    let _ = tray.set_tooltip(Some(tray_tooltip(status)));
}

fn build_tray_menu(
    app: &tauri::AppHandle,
    status: &models::AppStatus,
) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    let app_title = disabled_menu_item(app, "app-title", "AI Usage")?;
    let account = disabled_menu_item(app, "account", account_line(status))?;
    let usage_title = disabled_menu_item(app, "usage-title", "Usage")?;
    let five_hour = disabled_menu_item(
        app,
        "usage-5h",
        usage_line(
            "5 小时",
            status
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.five_hour.as_ref()),
        ),
    )?;
    let seven_day = disabled_menu_item(
        app,
        "usage-7d",
        usage_line(
            "1 周",
            status
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.seven_day.as_ref()),
        ),
    )?;
    let status_item = disabled_menu_item(app, "usage-status", status_line(status))?;

    MenuBuilder::new(app)
        .item(&app_title)
        .item(&account)
        .separator()
        .item(&usage_title)
        .item(&five_hour)
        .item(&seven_day)
        .item(&status_item)
        .separator()
        .text(MENU_REFRESH, "刷新用量")
        .text(MENU_SETTINGS, "打开设置...")
        .separator()
        .text(MENU_QUIT, "退出 AI Usage")
        .build()
}

fn disabled_menu_item<S: AsRef<str>>(
    app: &tauri::AppHandle,
    id: &str,
    text: S,
) -> tauri::Result<tauri::menu::MenuItem<tauri::Wry>> {
    MenuItemBuilder::with_id(id, text).enabled(false).build(app)
}

fn account_line(status: &models::AppStatus) -> String {
    status
        .snapshot
        .as_ref()
        .map(|snapshot| snapshot.account_name.clone())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "未选择账号".to_string())
}

fn usage_line(label: &str, window: Option<&models::QuotaWindow>) -> String {
    let Some(window) = window else {
        return format!("{label} 暂无数据");
    };

    let percent = format!("{:.0}%", window.remaining_percent.round());
    let reset_at = window
        .reset_at
        .map(|time| {
            let local_time = time.with_timezone(&chrono::Local);
            if window.window_minutes.unwrap_or_default() <= 360 {
                local_time.format("%H:%M").to_string()
            } else {
                local_time.format("%-m月%-d日").to_string()
            }
        })
        .unwrap_or_else(|| "--".to_string());

    format!("{label} 剩余 {percent} · 重置 {reset_at}")
}

fn status_line(status: &models::AppStatus) -> String {
    if let Some(error) = status.last_error.as_deref() {
        return format!("状态 刷新失败 · {error}");
    }

    match status.refresh_status {
        models::RefreshStatus::Refreshing => "状态 正在刷新".to_string(),
        models::RefreshStatus::Ok => status
            .last_refreshed_at
            .map(|time| {
                format!(
                    "状态 已更新 · {}",
                    time.with_timezone(&chrono::Local).format("%H:%M")
                )
            })
            .unwrap_or_else(|| "状态 已更新".to_string()),
        models::RefreshStatus::Error => "状态 刷新失败".to_string(),
        models::RefreshStatus::Idle => "状态 等待刷新".to_string(),
    }
}

fn tray_tooltip(status: &models::AppStatus) -> String {
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
    let mut rgba = vec![0; SIZE * SIZE * 4];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f64 - 8.5;
            let dy = y as f64 - 8.5;
            let distance = (dx * dx + dy * dy).sqrt();
            let index = (y * SIZE + x) * 4;
            if distance <= 8.0 {
                rgba[index] = 9;
                rgba[index + 1] = 9;
                rgba[index + 2] = 11;
                rgba[index + 3] = 255;
            }
            if (5..=12).contains(&x) && (5..=12).contains(&y) {
                rgba[index] = 255;
                rgba[index + 1] = 255;
                rgba[index + 2] = 255;
                rgba[index + 3] = 255;
            }
            if (7..=13).contains(&x) && (7..=10).contains(&y) {
                rgba[index] = 9;
                rgba[index + 1] = 9;
                rgba[index + 2] = 11;
                rgba[index + 3] = 255;
            }
        }
    }
    Image::new_owned(rgba, SIZE as u32, SIZE as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_settings_window_visible_for_focus_events() {
        assert!(!should_hide_main_panel(
            "main",
            &WindowEvent::Focused(false)
        ));
        assert!(!should_hide_main_panel("main", &WindowEvent::Focused(true)));
    }
}
