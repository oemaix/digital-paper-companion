//! IPC command handlers (contract: docs/04 §5.2).
//!
//! Skeleton: two smoke-test commands proving the frontend↔backend bridge.
//! The real command set (discovery, pairing, entries, transfers, sync, …)
//! lands with the corresponding features.

/// Returns the application version (used by the frontend footer/about box).
#[tauri::command]
pub fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Connection state placeholder until the connection supervisor exists
/// (FR-CONN-5). Values will follow docs/04 §5.1 `ConnectionState`.
#[tauri::command]
pub fn connection_state() -> String {
    "disconnected".to_string()
}
