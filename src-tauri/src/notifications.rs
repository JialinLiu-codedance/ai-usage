use crate::{
    models::{AccountQuotaStatus, AppSettings, QuotaWindow},
    storage,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tauri::AppHandle;

pub(crate) const LOW_QUOTA_NOTIFICATION_TITLE: &str = "AI Usage 额度提醒";

const LOW_QUOTA_NOTIFICATIONS_FILE: &str = "low-quota-notifications.json";
const FIVE_HOUR_WINDOW: &str = "five_hour";
const SEVEN_DAY_WINDOW: &str = "seven_day";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct LowQuotaNotificationState {
    #[serde(default)]
    accounts: HashMap<String, HashSet<String>>,
}

impl LowQuotaNotificationState {
    pub(crate) fn is_low(&self, account_id: &str, window_key: &str) -> bool {
        self.accounts
            .get(account_id)
            .map(|windows| windows.contains(window_key))
            .unwrap_or(false)
    }

    fn set_low(&mut self, account_id: &str, window_key: &str) {
        self.accounts
            .entry(account_id.to_string())
            .or_default()
            .insert(window_key.to_string());
    }

    fn clear_low(&mut self, account_id: &str, window_key: &str) {
        let Some(windows) = self.accounts.get_mut(account_id) else {
            return;
        };
        windows.remove(window_key);
        if windows.is_empty() {
            self.accounts.remove(account_id);
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.accounts.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LowQuotaNotification {
    pub(crate) account_id: String,
    pub(crate) title: String,
    pub(crate) body: String,
}

pub(crate) fn notify_low_quota_after_refresh(
    app: &AppHandle,
    settings: &AppSettings,
    account_statuses: &[AccountQuotaStatus],
    updated_account_ids: &HashSet<String>,
) -> Result<(), String> {
    let previous_state = read_low_quota_notification_state(app)?;
    let (notifications, next_state) = evaluate_low_quota_notifications(
        settings,
        account_statuses,
        updated_account_ids,
        &previous_state,
    );
    write_low_quota_notification_state(app, &next_state)?;

    for notification in notifications {
        let _ = send_low_quota_notification(app, &notification);
    }

    Ok(())
}

pub(crate) fn clear_low_quota_notification_state(app: &AppHandle) -> Result<(), String> {
    write_low_quota_notification_state(app, &LowQuotaNotificationState::default())
}

pub(crate) fn evaluate_low_quota_notifications(
    settings: &AppSettings,
    account_statuses: &[AccountQuotaStatus],
    updated_account_ids: &HashSet<String>,
    previous_state: &LowQuotaNotificationState,
) -> (Vec<LowQuotaNotification>, LowQuotaNotificationState) {
    if !settings.notify_on_low_quota {
        return (Vec::new(), LowQuotaNotificationState::default());
    }

    let known_account_ids = account_statuses
        .iter()
        .map(|account| account.account_id.as_str())
        .collect::<HashSet<_>>();
    let mut next_state = previous_state.clone();
    next_state
        .accounts
        .retain(|account_id, _| known_account_ids.contains(account_id.as_str()));

    let mut notifications = Vec::new();
    for account in account_statuses {
        if !updated_account_ids.contains(&account.account_id) {
            continue;
        }

        let mut triggered_segments = Vec::new();
        collect_low_quota_segment(
            &mut next_state,
            &mut triggered_segments,
            &account.account_id,
            FIVE_HOUR_WINDOW,
            "5H",
            account.five_hour.as_ref(),
            settings.low_quota_threshold_percent,
        );
        collect_low_quota_segment(
            &mut next_state,
            &mut triggered_segments,
            &account.account_id,
            SEVEN_DAY_WINDOW,
            "7D",
            account.seven_day.as_ref(),
            settings.low_quota_threshold_percent,
        );

        if !triggered_segments.is_empty() {
            notifications.push(LowQuotaNotification {
                account_id: account.account_id.clone(),
                title: LOW_QUOTA_NOTIFICATION_TITLE.into(),
                body: format!(
                    "{}：{}",
                    account.account_name.trim(),
                    triggered_segments.join("，")
                ),
            });
        }
    }

    (notifications, next_state)
}

fn collect_low_quota_segment(
    state: &mut LowQuotaNotificationState,
    triggered_segments: &mut Vec<String>,
    account_id: &str,
    window_key: &str,
    window_label: &str,
    window: Option<&QuotaWindow>,
    threshold: f64,
) {
    let Some(window) = window else {
        state.clear_low(account_id, window_key);
        return;
    };

    let remaining = window.remaining_percent.clamp(0.0, 100.0);
    if remaining <= threshold {
        if !state.is_low(account_id, window_key) {
            triggered_segments.push(format!("{window_label} 剩余 {:.0}%", remaining.round()));
        }
        state.set_low(account_id, window_key);
    } else {
        state.clear_low(account_id, window_key);
    }
}

fn read_low_quota_notification_state(app: &AppHandle) -> Result<LowQuotaNotificationState, String> {
    Ok(
        storage::read_json::<LowQuotaNotificationState>(app, LOW_QUOTA_NOTIFICATIONS_FILE)?
            .unwrap_or_default(),
    )
}

fn write_low_quota_notification_state(
    app: &AppHandle,
    state: &LowQuotaNotificationState,
) -> Result<(), String> {
    storage::write_json(app, LOW_QUOTA_NOTIFICATIONS_FILE, state)
}

fn send_low_quota_notification(
    app: &AppHandle,
    notification: &LowQuotaNotification,
) -> Result<(), String> {
    use tauri_plugin_notification::NotificationExt;

    app.notification()
        .builder()
        .title(&notification.title)
        .body(&notification.body)
        .show()
        .map_err(|error| format!("发送低额度通知失败: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{
        evaluate_low_quota_notifications, LowQuotaNotificationState, LOW_QUOTA_NOTIFICATION_TITLE,
    };
    use crate::models::{AccountQuotaStatus, AppSettings, QuotaWindow};
    use std::collections::HashSet;

    fn quota_window(remaining_percent: f64) -> QuotaWindow {
        QuotaWindow {
            used_percent: 100.0 - remaining_percent,
            remaining_percent,
            reset_at: None,
            window_minutes: None,
        }
    }

    fn account(
        account_id: &str,
        account_name: &str,
        five: Option<f64>,
        seven: Option<f64>,
    ) -> AccountQuotaStatus {
        AccountQuotaStatus {
            account_id: account_id.into(),
            account_name: account_name.into(),
            provider: "openai".into(),
            five_hour: five.map(quota_window),
            seven_day: seven.map(quota_window),
            fetched_at: Some(chrono::Utc::now()),
            source: Some("test".into()),
            last_error: None,
        }
    }

    fn updated_account_ids(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|id| (*id).to_string()).collect()
    }

    fn enabled_settings() -> AppSettings {
        AppSettings {
            notify_on_low_quota: true,
            low_quota_threshold_percent: 10.0,
            ..AppSettings::default()
        }
    }

    #[test]
    fn first_refresh_below_threshold_generates_notification() {
        let (notifications, state) = evaluate_low_quota_notifications(
            &enabled_settings(),
            &[account("first", "first@example.com", Some(9.0), None)],
            &updated_account_ids(&["first"]),
            &LowQuotaNotificationState::default(),
        );

        assert_eq!(notifications.len(), 1);
        assert_eq!(notifications[0].title, LOW_QUOTA_NOTIFICATION_TITLE);
        assert_eq!(notifications[0].body, "first@example.com：5H 剩余 9%");
        assert!(state.is_low("first", "five_hour"));
    }

    #[test]
    fn consecutive_refresh_below_threshold_does_not_repeat_notification() {
        let (first_notifications, first_state) = evaluate_low_quota_notifications(
            &enabled_settings(),
            &[account("first", "first@example.com", Some(9.0), None)],
            &updated_account_ids(&["first"]),
            &LowQuotaNotificationState::default(),
        );

        let (second_notifications, second_state) = evaluate_low_quota_notifications(
            &enabled_settings(),
            &[account("first", "first@example.com", Some(8.0), None)],
            &updated_account_ids(&["first"]),
            &first_state,
        );

        assert_eq!(first_notifications.len(), 1);
        assert!(second_notifications.is_empty());
        assert!(second_state.is_low("first", "five_hour"));
    }

    #[test]
    fn quota_recovery_allows_next_crossing_to_notify_again() {
        let (_, low_state) = evaluate_low_quota_notifications(
            &enabled_settings(),
            &[account("first", "first@example.com", Some(9.0), None)],
            &updated_account_ids(&["first"]),
            &LowQuotaNotificationState::default(),
        );
        let (_, recovered_state) = evaluate_low_quota_notifications(
            &enabled_settings(),
            &[account("first", "first@example.com", Some(11.0), None)],
            &updated_account_ids(&["first"]),
            &low_state,
        );

        let (notifications, next_state) = evaluate_low_quota_notifications(
            &enabled_settings(),
            &[account("first", "first@example.com", Some(10.0), None)],
            &updated_account_ids(&["first"]),
            &recovered_state,
        );

        assert_eq!(notifications.len(), 1);
        assert_eq!(notifications[0].body, "first@example.com：5H 剩余 10%");
        assert!(next_state.is_low("first", "five_hour"));
    }

    #[test]
    fn multiple_windows_for_one_account_are_combined_into_one_notification() {
        let (notifications, state) = evaluate_low_quota_notifications(
            &enabled_settings(),
            &[account("first", "first@example.com", Some(9.0), Some(8.0))],
            &updated_account_ids(&["first"]),
            &LowQuotaNotificationState::default(),
        );

        assert_eq!(notifications.len(), 1);
        assert_eq!(
            notifications[0].body,
            "first@example.com：5H 剩余 9%，7D 剩余 8%"
        );
        assert!(state.is_low("first", "five_hour"));
        assert!(state.is_low("first", "seven_day"));
    }

    #[test]
    fn multiple_accounts_generate_one_notification_per_account() {
        let (notifications, state) = evaluate_low_quota_notifications(
            &enabled_settings(),
            &[
                account("first", "first@example.com", Some(9.0), None),
                account("second", "second@example.com", None, Some(7.0)),
            ],
            &updated_account_ids(&["first", "second"]),
            &LowQuotaNotificationState::default(),
        );

        assert_eq!(notifications.len(), 2);
        assert_eq!(notifications[0].body, "first@example.com：5H 剩余 9%");
        assert_eq!(notifications[1].body, "second@example.com：7D 剩余 7%");
        assert!(state.is_low("first", "five_hour"));
        assert!(state.is_low("second", "seven_day"));
    }

    #[test]
    fn disabled_low_quota_notifications_clear_state_without_notifying() {
        let (_, low_state) = evaluate_low_quota_notifications(
            &enabled_settings(),
            &[account("first", "first@example.com", Some(9.0), None)],
            &updated_account_ids(&["first"]),
            &LowQuotaNotificationState::default(),
        );
        let settings = AppSettings {
            notify_on_low_quota: false,
            ..enabled_settings()
        };

        let (notifications, next_state) = evaluate_low_quota_notifications(
            &settings,
            &[account("first", "first@example.com", Some(9.0), None)],
            &updated_account_ids(&["first"]),
            &low_state,
        );

        assert!(notifications.is_empty());
        assert!(next_state.is_empty());
    }
}
