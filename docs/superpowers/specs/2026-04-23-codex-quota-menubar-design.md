# Codex Quota Menubar App Design

## Overview

This project is a macOS menu bar app built with Tauri and Rust. Its first milestone is intentionally narrow: monitor the remaining quota for Codex, keep the app resident in the macOS status bar, and provide lightweight settings and notifications for a single personal account.

The product is local-first and does not depend on any cloud backend. It should prefer official interfaces for quota retrieval, while allowing compatible non-official authentication inputs when needed to make Codex quota fetching work in practice.

## Product Goals

The first version should solve three concrete problems:

1. Let the user see current Codex quota without opening a browser or client.
2. Alert the user when remaining quota drops below a chosen threshold.
3. Keep the setup and maintenance burden low enough for a personal utility app.

## Out Of Scope

The first version does not include:

1. Multi-platform support for Claude, GLM, MiniMax, or Kimi.
2. Multi-account support.
3. Historical charts and long-term analytics.
4. Refresh logs and confidence labels.
5. Import/export, cloud sync, or team features.

## Functional Scope

### 1. Menu Bar Residency

The app lives in the macOS status bar and is designed for quick-glance usage.

Required behavior:

1. Show a persistent menu bar icon.
2. Reflect one of four states in the icon or title area:
   - normal
   - low quota
   - refresh in progress
   - refresh failed
3. Open a lightweight panel when clicked.

### 2. Quota Overview Panel

The menu bar panel is the primary interaction surface. It should show only high-signal information.

Required fields:

1. Account name
2. Remaining quota
3. Used quota
4. Total quota
5. Reset time, if available
6. Last refresh time

Required actions:

1. Manual refresh
2. Open settings

### 3. Automatic Refresh

The app should refresh Codex quota without user intervention.

Required behavior:

1. Perform an initial refresh on app startup after loading settings and secrets.
2. Support fixed refresh intervals such as 5, 15, and 30 minutes.
3. Prevent overlapping refresh tasks.
4. Update UI state immediately when refresh starts, succeeds, or fails.

### 4. Connection Setup

The first version supports a single Codex account only.

The settings UI should allow configuration of the authentication method needed to retrieve Codex quota. The exact fields can be finalized during implementation once the working Codex access path is verified, but the design should reserve support for:

1. API key
2. Session token
3. Cookie
4. Base URL override

### 5. Connection Test

The app should provide a way to validate setup before relying on background refresh.

Required results:

1. Connection successful
2. Authentication failed
3. Network failed
4. Endpoint unavailable or unsupported

The result should be readable in the settings UI without requiring a separate diagnostics page.

### 6. Low Quota Notification

The user can define a low-quota threshold. When the remaining quota falls below that threshold, the app should send a native macOS notification.

Required behavior:

1. Allow enabling or disabling low-quota notification.
2. Allow setting a threshold value.
3. Deduplicate repeated notifications for the same low-quota condition so the app does not spam the user.

### 7. Reset Reminder

If the Codex quota source exposes a reliable reset time, the app should support a reset reminder.

Required behavior:

1. Allow enabling or disabling reset reminder.
2. Notify the user before reset based on a simple lead time.
3. Omit the feature in the UI when reset time is unavailable from the source.

## Architecture

The app should be split into a thin Tauri UI shell and a Rust service core. Business logic should stay on the Rust side so future platform integrations can be added without reworking the front end.

### Frontend Responsibilities

The Tauri frontend handles:

1. Menu bar panel rendering
2. Settings page
3. Connection test trigger
4. Manual refresh trigger
5. Display of current quota state

The frontend should not contain provider-specific fetch logic.

### Rust Responsibilities

The Rust backend handles:

1. Quota refresh orchestration
2. Codex-specific request building and response parsing
3. Scheduler and refresh timing
4. Notification triggering
5. Keychain access
6. Local state persistence

## Module Breakdown

### `tray`

Responsible for:

1. macOS status bar icon and menu
2. Display state updates
3. Click handling and panel open behavior

### `quota_service`

Central application service for:

1. `refresh_quota`
2. `get_current_quota`
3. `test_connection`

This service owns refresh coordination and is the only path that should initiate provider fetches.

### `codex_provider`

Responsible for:

1. Codex authentication injection
2. Endpoint request construction
3. Response parsing into a normalized quota snapshot

### `scheduler`

Responsible for:

1. Startup refresh
2. Interval-based refresh
3. Ensuring only one refresh runs at a time

### `settings`

Responsible for:

1. Refresh interval
2. Threshold settings
3. Notification toggles
4. Non-secret configuration persistence

### `secrets`

Responsible for storing and retrieving credentials from macOS Keychain.

### `notifications`

Responsible for:

1. Low-quota notifications
2. Reset reminder notifications
3. Notification deduplication

### `state_store`

Responsible for current in-memory application state, including:

1. latest quota snapshot
2. refresh status
3. last refresh time
4. last error state

UI and tray rendering should subscribe to this state instead of issuing network requests directly.

## Data Model

Even for a single-provider first release, a normalized snapshot model should be defined up front.

### `QuotaSnapshot`

Fields:

1. `account_name`
2. `remaining`
3. `used`
4. `total`
5. `unit`
6. `reset_at`
7. `fetched_at`
8. `status`

### `RefreshStatus`

Values:

1. `idle`
2. `refreshing`
3. `ok`
4. `error`

### `AppSettings`

Fields:

1. `refresh_interval_minutes`
2. `low_quota_threshold`
3. `notify_on_low_quota`
4. `notify_on_reset`

## State Flow

The app should follow a single refresh path:

1. App launches.
2. Settings load from local storage.
3. Secrets load from Keychain.
4. `quota_service` performs initial refresh.
5. `state_store` updates with the latest result.
6. Tray and panel UI re-render from state.
7. Scheduler triggers future refreshes.
8. Notification rules evaluate against the updated snapshot.

This flow avoids duplicated request logic and reduces inconsistent UI state.

## Storage Strategy

Sensitive values must be stored in macOS Keychain. Non-sensitive settings and the latest snapshot can be stored locally, using SQLite or a lightweight local persistence mechanism depending on implementation complexity.

For the MVP, the storage decision should optimize for simplicity:

1. Keychain for credentials
2. Lightweight local persistence for settings
3. Cached latest snapshot for fast startup rendering

## Error Handling

The first version should not expose detailed logs, but it still needs clear behavior when refresh fails.

Required behavior:

1. Preserve the last successful snapshot when a refresh fails.
2. Mark the app state as failed.
3. Surface a simple readable failure state in the UI.
4. Allow manual retry from the menu bar panel.

## Testing Strategy

The MVP should be validated with practical tests at three levels:

1. Unit tests for quota normalization, threshold checks, and notification deduplication.
2. Integration tests for `codex_provider` parsing using recorded sample responses.
3. Manual macOS validation for tray behavior, Keychain storage, notifications, and startup refresh.

## MVP Success Criteria

The first release is successful if:

1. A user can configure one Codex account locally.
2. The app can fetch and display the current Codex quota reliably enough for daily use.
3. The menu bar always reflects the current app state.
4. Low-quota notification works without repeated spam.
5. Connection test clearly identifies setup failures.

## Iteration Plan After MVP

After the Codex-first release is stable, expansion should proceed in this order:

1. Hardening the Codex integration
2. Adding one more provider through the same provider abstraction
3. Introducing multi-provider overview
4. Adding optional history and analytics
5. Adding import/export or sync only if clearly needed
