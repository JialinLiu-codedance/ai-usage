mod commands;
mod errors;
mod models;
mod provider;
mod secrets;
mod settings;
mod state;
mod storage;

use std::time::Duration;
use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .manage(state::StateStore::default())
        .setup(|app| {
            let handle = app.handle().clone();

            tauri::async_runtime::spawn(async move {
                {
                    let state = handle.state::<state::StateStore>();
                    let _ = commands::hydrate_cached_snapshot(&handle, &state).await;
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

struct UtcNow;

impl UtcNow {
    fn minutes_since(time: chrono::DateTime<chrono::Utc>) -> i64 {
        (chrono::Utc::now() - time).num_minutes()
    }
}
