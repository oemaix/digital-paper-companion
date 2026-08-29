//! Digital Paper Companion — application layer (docs/04 §5).
//!
//! Owns the Tauri runtime: IPC commands, event emission, task orchestration
//! (connection supervisor, transfer queue, sync scheduler), settings and
//! credential stores. All protocol logic lives in `dpt-core`.

mod commands;

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::app_version,
            commands::connection_state,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
