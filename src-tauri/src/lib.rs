//! Digital Paper Companion — application layer (docs/04 §5).
//!
//! Owns the Tauri runtime: IPC commands, event emission, task orchestration
//! (connection supervisor, transfer queue), settings and credential stores.
//! All protocol logic lives in `dpt-core`.

mod commands;
mod credentials;
mod error;
mod state;
mod stores;
mod transfers;

use std::sync::Arc;

use tauri::Manager;

use credentials::CredentialStore;
use state::{AppState, DeviceContext};
use stores::Stores;

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            let stores = Stores::new(config_dir);
            let creds = CredentialStore::new(stores.fallback_credentials_dir());
            let app_state = Arc::new(AppState::new(app.handle().clone(), stores, creds));
            app.manage(app_state.clone());

            // Auto-connect to the last active device, best effort (FR-CONN-8).
            tauri::async_runtime::spawn(async move {
                autoconnect(app_state).await;
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_version,
            commands::get_settings,
            commands::set_theme,
            commands::connection_state,
            commands::discover_devices,
            commands::probe_device,
            commands::known_devices,
            commands::connect_known_device,
            commands::disconnect_device,
            commands::forget_device,
            commands::start_pairing,
            commands::submit_pairing_pin,
            commands::cancel_pairing,
            commands::list_entries,
            commands::resolve_folder,
            commands::create_remote_folder,
            commands::delete_entries,
            commands::rename_entry,
            commands::move_entries,
            commands::open_entry,
            commands::open_on_device,
            commands::device_status,
            commands::set_device_clock,
            commands::upload_files,
            commands::upload_folder,
            commands::download_entries,
            commands::classify_paths,
            commands::transfer_list,
            commands::transfer_cancel,
            commands::transfers_clear_finished,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Tries to reconnect to the last used device at startup (FR-CONN-8,
/// FR-APP-1). Silent on failure; the UI just stays disconnected.
async fn autoconnect(state: Arc<AppState>) {
    let settings = state.stores.load_settings();
    let Some(serial) = settings.last_active_serial else {
        return;
    };
    let Some(device) = state
        .stores
        .load_devices()
        .into_iter()
        .find(|d| d.serial == serial)
    else {
        return;
    };
    let Some(address) = device.last_address.clone() else {
        return;
    };
    let Ok(Some(credentials)) = state.creds.load(&serial) else {
        return;
    };
    let Ok(cert_pem) = state.stores.load_cert(&serial) else {
        return;
    };
    let ctx = DeviceContext {
        serial: device.serial,
        name: device.name,
        addr: dpt_core::model::DeviceAddr::new(address),
        cert_pem,
        credentials,
    };
    if let Err(e) = state.connect(ctx).await {
        tracing::info!(error = %e, "autoconnect failed; staying disconnected");
    }
}
