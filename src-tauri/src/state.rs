//! Application state, connection lifecycle and the connection supervisor
//! (docs/04 §5.1; FR-CONN-5/6/8).

use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, RwLock};

use dpt_core::client::DeviceClient;
use dpt_core::model::{Credentials, DeviceAddr, DeviceInfo, Entry};
use dpt_core::register::PendingPin;

use crate::credentials::CredentialStore;
use crate::stores::Stores;
use crate::transfers::TransferState;

/// Event channel names (Rust → frontend). See docs/04 §5.2.
pub mod events {
    pub const CONNECTION_CHANGED: &str = "connection:changed";
    pub const ENTRIES_INVALIDATED: &str = "entries:invalidated";
    pub const TRANSFER_UPDATED: &str = "transfer:updated";
}

/// High-level connection state (mirrors docs/04 §5.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Reauthenticating,
}

/// The active device's connection context, kept so the supervisor can
/// rebuild the client after a drop.
#[derive(Clone)]
pub struct DeviceContext {
    pub serial: String,
    pub name: String,
    pub addr: DeviceAddr,
    pub cert_pem: String,
    pub credentials: Credentials,
}

/// A pairing handshake waiting for the user to type the on-device PIN.
pub struct PendingPairing {
    pub pin: PendingPin,
    pub addr: DeviceAddr,
    pub info: DeviceInfo,
}

/// Payload of [`events::CONNECTION_CHANGED`].
#[derive(Debug, Clone, Serialize)]
pub struct ConnectionPayload {
    pub state: ConnectionState,
    pub serial: Option<String>,
    pub name: Option<String>,
}

struct Inner {
    connection: ConnectionState,
    client: Option<Arc<DeviceClient>>,
    context: Option<DeviceContext>,
    entries: Option<Vec<Entry>>,
    supervisor: Option<tokio::task::JoinHandle<()>>,
}

/// Shared application state, managed by Tauri as `Arc<AppState>`.
pub struct AppState {
    pub app: AppHandle,
    pub stores: Stores,
    pub creds: CredentialStore,
    pub pending: Mutex<Option<PendingPairing>>,
    pub transfers: Mutex<TransferState>,
    inner: RwLock<Inner>,
}

impl AppState {
    pub fn new(app: AppHandle, stores: Stores, creds: CredentialStore) -> Self {
        Self {
            app,
            stores,
            creds,
            pending: Mutex::new(None),
            transfers: Mutex::new(TransferState::default()),
            inner: RwLock::new(Inner {
                connection: ConnectionState::Disconnected,
                client: None,
                context: None,
                entries: None,
                supervisor: None,
            }),
        }
    }

    pub async fn connection_state(&self) -> ConnectionState {
        self.inner.read().await.connection
    }

    pub async fn connection_payload(&self) -> ConnectionPayload {
        let inner = self.inner.read().await;
        ConnectionPayload {
            state: inner.connection,
            serial: inner.context.as_ref().map(|c| c.serial.clone()),
            name: inner.context.as_ref().map(|c| c.name.clone()),
        }
    }

    pub async fn client(&self) -> Option<Arc<DeviceClient>> {
        self.inner.read().await.client.clone()
    }

    pub async fn require_client(&self) -> Result<Arc<DeviceClient>, crate::error::AppError> {
        self.client()
            .await
            .ok_or_else(|| crate::error::AppError::new("not_connected", "no device connected"))
    }

    async fn set_connection(&self, state: ConnectionState) {
        {
            let mut inner = self.inner.write().await;
            inner.connection = state;
        }
        let payload = self.connection_payload().await;
        let _ = self.app.emit(events::CONNECTION_CHANGED, payload);
    }

    /// Cached entry list, if loaded.
    pub async fn entries(&self) -> Option<Vec<Entry>> {
        self.inner.read().await.entries.clone()
    }

    pub async fn set_entries(&self, entries: Vec<Entry>) {
        self.inner.write().await.entries = Some(entries);
    }

    pub async fn invalidate_entries(&self) {
        self.inner.write().await.entries = None;
        let _ = self.app.emit(events::ENTRIES_INVALIDATED, ());
    }

    /// Establishes a connection to a known device and starts the supervisor.
    pub async fn connect(self: &Arc<Self>, context: DeviceContext) -> Result<(), crate::error::AppError> {
        self.set_connection(ConnectionState::Connecting).await;

        let client = DeviceClient::connect(
            &context.addr,
            context.credentials.clone(),
            &context.cert_pem,
        )
        .await
        .map_err(crate::error::AppError::from)?;

        {
            let mut inner = self.inner.write().await;
            if let Some(handle) = inner.supervisor.take() {
                handle.abort();
            }
            inner.client = Some(Arc::new(client));
            inner.context = Some(context.clone());
            inner.entries = None;
        }
        self.set_connection(ConnectionState::Connected).await;

        // Remember as last active device (FR-APP-1).
        let mut settings = self.stores.load_settings();
        settings.last_active_serial = Some(context.serial.clone());
        let _ = self.stores.save_settings(&settings);

        // Start the health/reconnect supervisor (FR-CONN-5/6).
        let weak = Arc::downgrade(self);
        let handle = tokio::spawn(async move {
            supervisor_loop(weak).await;
        });
        self.inner.write().await.supervisor = Some(handle);
        Ok(())
    }

    pub async fn disconnect(&self) {
        let mut inner = self.inner.write().await;
        if let Some(handle) = inner.supervisor.take() {
            handle.abort();
        }
        inner.client = None;
        inner.context = None;
        inner.entries = None;
        inner.connection = ConnectionState::Disconnected;
        drop(inner);
        let payload = self.connection_payload().await;
        let _ = self.app.emit(events::CONNECTION_CHANGED, payload);
    }

    async fn context(&self) -> Option<DeviceContext> {
        self.inner.read().await.context.clone()
    }

    async fn replace_client(&self, client: DeviceClient) {
        self.inner.write().await.client = Some(Arc::new(client));
    }
}

/// Pings periodically; on failure re-authenticates and, if needed, rebuilds
/// the connection from the stored context with the session cookie refreshed
/// (FR-CONN-5/6). Exits when the state is dropped or the device disconnected.
async fn supervisor_loop(weak: std::sync::Weak<AppState>) {
    loop {
        tokio::time::sleep(Duration::from_secs(15)).await;
        let Some(state) = weak.upgrade() else { break };
        let Some(client) = state.client().await else { break };

        if client.ping().await.is_ok() {
            continue;
        }

        // Session may have lapsed; try to refresh it in place first.
        state.set_connection(ConnectionState::Reauthenticating).await;
        if client.authenticate().await.is_ok() && client.ping().await.is_ok() {
            state.set_connection(ConnectionState::Connected).await;
            continue;
        }

        // Full reconnect with backoff using the stored context.
        let Some(ctx) = state.context().await else { break };
        let mut delay = Duration::from_secs(2);
        for _ in 0..5 {
            if weak.upgrade().is_none() {
                return;
            }
            match DeviceClient::connect(&ctx.addr, ctx.credentials.clone(), &ctx.cert_pem).await {
                Ok(c) => {
                    state.replace_client(c).await;
                    state.set_connection(ConnectionState::Connected).await;
                    break;
                }
                Err(_) => {
                    tokio::time::sleep(delay).await;
                    delay = (delay * 2).min(Duration::from_secs(30));
                }
            }
        }
        if state.connection_state().await != ConnectionState::Connected {
            state.set_connection(ConnectionState::Disconnected).await;
            break;
        }
    }
}
