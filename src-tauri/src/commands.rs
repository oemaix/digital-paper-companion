//! IPC commands exposed to the frontend (docs/04 §5.2).
//!
//! Commands validate, orchestrate `dpt-core` calls and map errors to
//! [`AppError`]; no protocol logic lives here.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{Manager, State};
use tauri_plugin_opener::OpenerExt;

use dpt_core::client::DeviceClient;
use dpt_core::discovery;
use dpt_core::model::{BatteryStatus, DeviceAddr, DeviceInfo, Entry, StorageStatus};

use crate::error::{AppError, CmdResult};
use crate::state::{AppState, ConnectionPayload, DeviceContext, PendingPairing};
use crate::stores::{KnownDevice, Settings, SyncPair};
use crate::sync::{self, Decision, ExcludedAction, RunOptions, SyncStatusPayload, Trigger};
use crate::transfers::{self, JobKind, JobSnapshot};

type S<'a> = State<'a, Arc<AppState>>;

// ---- app ------------------------------------------------------------------

#[tauri::command]
pub fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[tauri::command]
pub fn get_settings(state: S<'_>) -> Settings {
    state.stores.load_settings()
}

#[tauri::command]
pub fn set_theme(state: S<'_>, theme: String) -> CmdResult<()> {
    let mut settings = state.stores.load_settings();
    settings.theme = theme;
    state.stores.save_settings(&settings)
}

/// Persists the UI language (`"system"` or a locale code; NFR-I18N-1).
#[tauri::command]
pub fn set_language(state: S<'_>, language: String) -> CmdResult<()> {
    let mut settings = state.stores.load_settings();
    settings.language = language;
    state.stores.save_settings(&settings)
}

// ---- update check (FR-APP-5) --------------------------------------------------

/// GitHub repository whose latest release the update check queries.
const UPDATE_REPO: &str = "oemaix/digital-paper-companion";

/// Result of an update check: notify-only, the user follows `url` to the
/// release page — no download, no auto-install (FR-APP-5).
#[derive(Serialize)]
pub struct UpdateCheck {
    pub current: String,
    pub latest: Option<String>,
    pub url: String,
    pub update_available: bool,
}

/// Enables/disables the automatic check on startup (NFR-SEC-4).
#[tauri::command]
pub fn set_update_check(state: S<'_>, enabled: bool) -> CmdResult<()> {
    let mut settings = state.stores.load_settings();
    settings.update_check = enabled;
    state.stores.save_settings(&settings)
}

/// Fetches the latest GitHub release (static JSON over HTTPS) and compares
/// it with the running version. This is the app's only non-device network
/// access and runs only when enabled or manually triggered (NFR-SEC-4).
#[tauri::command]
pub async fn check_for_update() -> CmdResult<UpdateCheck> {
    let current = env!("CARGO_PKG_VERSION").to_string();
    let url = format!("https://github.com/{UPDATE_REPO}/releases/latest");

    let client = dpt_core::reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| AppError::new("network", e.to_string()))?;
    let resp = client
        .get(format!(
            "https://api.github.com/repos/{UPDATE_REPO}/releases/latest"
        ))
        .header("User-Agent", format!("digital-paper-companion/{current}"))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| AppError::new("network", e.to_string()))?;
    if !resp.status().is_success() {
        return Err(AppError::new(
            "update_check",
            format!("release query failed (HTTP {})", resp.status().as_u16()),
        ));
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::new("network", e.to_string()))?;

    let latest = body["tag_name"]
        .as_str()
        .map(|t| t.trim_start_matches('v').to_string());
    let url = body["html_url"].as_str().map(str::to_string).unwrap_or(url);
    let update_available = latest
        .as_deref()
        .is_some_and(|l| version_is_newer(l, &current));
    Ok(UpdateCheck {
        current,
        latest,
        url,
        update_available,
    })
}

/// `true` if `candidate` is newer than `current`. Versions are
/// `major.minor.patch` with an optional numeric pre-release suffix
/// (`0.3.0-1`, the WiX-compatible scheme used by this project); a
/// pre-release precedes its release.
fn version_is_newer(candidate: &str, current: &str) -> bool {
    fn parse(v: &str) -> (Vec<u64>, Option<u64>) {
        let (main, pre) = match v.split_once('-') {
            Some((m, p)) => (m, Some(p.parse().unwrap_or(0))),
            None => (v, None),
        };
        let parts = main
            .split('.')
            .map(|p| p.trim().parse().unwrap_or(0))
            .collect();
        (parts, pre)
    }
    let (main_a, pre_a) = parse(candidate);
    let (main_b, pre_b) = parse(current);
    for i in 0..main_a.len().max(main_b.len()) {
        let a = main_a.get(i).copied().unwrap_or(0);
        let b = main_b.get(i).copied().unwrap_or(0);
        if a != b {
            return a > b;
        }
    }
    match (pre_a, pre_b) {
        // Same main version: the release is newer than any pre-release.
        (None, Some(_)) => true,
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

// ---- discovery & connection -------------------------------------------------

#[tauri::command]
pub async fn connection_state(state: S<'_>) -> CmdResult<ConnectionPayload> {
    Ok(state.connection_payload().await)
}

/// A device found on the network, enriched with its identity so the UI can
/// deduplicate by serial and hide/label already-paired/connected devices
/// (FR-CONN-1/7). The serial is read from the unauthenticated
/// `GET /register/information` endpoint, so identity is known *before*
/// pairing.
#[derive(Serialize)]
pub struct ScannedDevice {
    pub address: String,
    pub name: String,
    pub port: u16,
    pub serial: Option<String>,
    pub model: Option<String>,
    pub paired: bool,
    pub connected: bool,
}

/// Browses mDNS for up to `seconds` (default 5), probes each result for its
/// serial, deduplicates by serial and flags paired/connected devices
/// (FR-CONN-1/7).
#[tauri::command]
pub async fn discover_devices(state: S<'_>, seconds: Option<u64>) -> CmdResult<Vec<ScannedDevice>> {
    let timeout = Duration::from_secs(seconds.unwrap_or(5).clamp(1, 60));
    let found = discovery::discover(timeout).await?;

    let known: std::collections::HashSet<String> = state
        .stores
        .load_devices()
        .into_iter()
        .map(|d| d.serial)
        .collect();
    let connected_serial = state.connection_payload().await.serial;

    // Probe each address (short timeout) to learn its serial. Probes run
    // concurrently; unreachable devices simply keep an unknown serial.
    let probes = found.into_iter().map(|d| async move {
        let info = tokio::time::timeout(
            Duration::from_secs(4),
            DeviceClient::probe(&DeviceAddr::new(d.address.clone())),
        )
        .await
        .ok()
        .and_then(Result::ok);
        (d, info)
    });
    let results = futures_util::future::join_all(probes).await;

    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (d, info) in results {
        let serial = info.as_ref().map(|i| i.serial_number.clone());
        // Dedupe by serial when known, otherwise by address.
        let key = serial.clone().unwrap_or_else(|| d.address.clone());
        if !seen.insert(key) {
            continue;
        }
        let paired = serial.as_ref().is_some_and(|s| known.contains(s));
        let connected = serial.is_some() && serial == connected_serial;
        out.push(ScannedDevice {
            address: d.address,
            name: info
                .as_ref()
                .and_then(|i| i.model_name.clone())
                .unwrap_or(d.name),
            port: d.port,
            model: info.and_then(|i| i.model_name),
            serial,
            paired,
            connected,
        });
    }
    Ok(out)
}

/// Probes a manual address for a compatible device (FR-CONN-2).
#[tauri::command]
pub async fn probe_device(address: String) -> CmdResult<DeviceInfo> {
    Ok(DeviceClient::probe(&DeviceAddr::new(address)).await?)
}

#[tauri::command]
pub fn known_devices(state: S<'_>) -> Vec<KnownDevice> {
    state.stores.load_devices()
}

/// Connects to a previously paired device (FR-CONN-3/4).
#[tauri::command]
pub async fn connect_known_device(
    state: S<'_>,
    serial: String,
    address: Option<String>,
) -> CmdResult<ConnectionPayload> {
    let device = state
        .stores
        .load_devices()
        .into_iter()
        .find(|d| d.serial == serial)
        .ok_or_else(|| AppError::new("unknown_device", "device is not paired"))?;
    let addr_str = address
        .or(device.last_address.clone())
        .ok_or_else(|| AppError::new("no_address", "no known address; enter one manually"))?;
    let credentials = state
        .creds
        .load(&serial)?
        .ok_or_else(|| AppError::new("no_credentials", "no stored credentials; pair again"))?;
    let cert_pem = state.stores.load_cert(&serial)?;

    let ctx = DeviceContext {
        serial: serial.clone(),
        name: device.name.clone(),
        addr: DeviceAddr::new(addr_str.clone()),
        cert_pem,
        credentials,
    };
    let app_state = state.inner().clone();
    app_state.connect(ctx).await?;

    state.stores.upsert_device(KnownDevice {
        last_address: Some(addr_str),
        ..device
    })?;
    Ok(app_state.connection_payload().await)
}

#[tauri::command]
pub async fn disconnect_device(state: S<'_>) -> CmdResult<()> {
    state.disconnect().await;
    Ok(())
}

/// Removes a paired device: credentials, pinned cert and registry entry
/// (docs/07 §2 "Forget this device").
#[tauri::command]
pub async fn forget_device(state: S<'_>, serial: String) -> CmdResult<()> {
    let payload = state.connection_payload().await;
    if payload.serial.as_deref() == Some(serial.as_str()) {
        state.disconnect().await;
    }
    state.creds.delete(&serial)?;
    state.stores.remove_device(&serial)?;
    Ok(())
}

// ---- pairing (FR-REG-1/2/3) -------------------------------------------------

/// Starts the pairing handshake; on success the device shows a PIN.
#[tauri::command]
pub async fn start_pairing(state: S<'_>, address: String) -> CmdResult<DeviceInfo> {
    let addr = DeviceAddr::new(address);
    let info = DeviceClient::probe(&addr).await?;
    let pending = DeviceClient::register(&addr)?.begin().await?;
    *state.pending.lock().await = Some(PendingPairing {
        pin: pending,
        addr,
        info: info.clone(),
    });
    Ok(info)
}

/// Completes pairing with the on-device PIN, persists credentials and the
/// pinned certificate, then connects.
#[tauri::command]
pub async fn submit_pairing_pin(state: S<'_>, pin: String) -> CmdResult<ConnectionPayload> {
    let pending = state
        .pending
        .lock()
        .await
        .take()
        .ok_or_else(|| AppError::new("no_pairing", "no pairing in progress"))?;

    let registration = pending.pin.submit_pin(pin.trim()).await?;

    let serial = pending.info.serial_number.clone();
    let name = pending
        .info
        .model_name
        .clone()
        .unwrap_or_else(|| "Digital Paper".to_string());

    state.creds.save(&serial, &registration.credentials)?;
    state
        .stores
        .save_cert(&serial, &registration.device_cert_pem)?;
    state.stores.upsert_device(KnownDevice {
        serial: serial.clone(),
        name: name.clone(),
        model: pending.info.model_name.clone(),
        last_address: Some(pending.addr.0.clone()),
    })?;

    let ctx = DeviceContext {
        serial,
        name,
        addr: pending.addr,
        cert_pem: registration.device_cert_pem,
        credentials: registration.credentials,
    };
    let app_state = state.inner().clone();
    app_state.connect(ctx).await?;
    Ok(app_state.connection_payload().await)
}

#[tauri::command]
pub async fn cancel_pairing(state: S<'_>) -> CmdResult<()> {
    state.pending.lock().await.take();
    Ok(())
}

// ---- entries (FR-BRW-*) -----------------------------------------------------

/// Returns the full entry list, from cache unless `refresh` is set.
#[tauri::command]
pub async fn list_entries(state: S<'_>, refresh: Option<bool>) -> CmdResult<Vec<Entry>> {
    if !refresh.unwrap_or(false) {
        if let Some(cached) = state.entries().await {
            return Ok(cached);
        }
    }
    let client = state.require_client().await?;
    let entries = client.list_all_entries().await?;
    state.set_entries(entries.clone()).await;
    Ok(entries)
}

async fn cached_entries(state: &Arc<AppState>) -> CmdResult<Vec<Entry>> {
    if let Some(cached) = state.entries().await {
        return Ok(cached);
    }
    let client = state.require_client().await?;
    let entries = client.list_all_entries().await?;
    state.set_entries(entries.clone()).await;
    Ok(entries)
}

fn find_entry<'e>(entries: &'e [Entry], id: &str) -> CmdResult<&'e Entry> {
    entries
        .iter()
        .find(|e| e.entry_id == id)
        .ok_or_else(|| AppError::new("not_found", "entry not found; refresh the library"))
}

/// Resolves a device path to its entry (used for folder ids not present in
/// the flat listing, e.g. the `Document` root).
#[tauri::command]
pub async fn resolve_folder(state: S<'_>, path: String) -> CmdResult<Entry> {
    let client = state.require_client().await?;
    Ok(client.resolve_path(&path).await?)
}

#[tauri::command]
pub async fn create_remote_folder(
    state: S<'_>,
    parent_folder_id: String,
    name: String,
) -> CmdResult<()> {
    let client = state.require_client().await?;
    client.create_folder(&parent_folder_id, &name).await?;
    state.invalidate_entries().await;
    Ok(())
}

/// Deletes documents/folders by id (irreversible on the device; the UI
/// confirms first, docs/05 §4.3).
#[tauri::command]
pub async fn delete_entries(state: S<'_>, ids: Vec<String>) -> CmdResult<()> {
    let entries = cached_entries(state.inner()).await?;
    let client = state.require_client().await?;
    for id in &ids {
        let entry = find_entry(&entries, id)?;
        if entry.is_folder() {
            client.delete_folder(id).await?;
        } else {
            client.delete_document(id).await?;
        }
    }
    state.invalidate_entries().await;
    Ok(())
}

/// Renames a document (folder rename is not supported by the protocol).
#[tauri::command]
pub async fn rename_entry(state: S<'_>, id: String, new_name: String) -> CmdResult<()> {
    let entries = cached_entries(state.inner()).await?;
    let entry = find_entry(&entries, &id)?;
    if entry.is_folder() {
        return Err(AppError::new(
            "unsupported",
            "folders cannot be renamed by the device API",
        ));
    }
    let parent = entry
        .parent_folder_id
        .clone()
        .ok_or_else(|| AppError::new("protocol", "entry has no parent folder"))?;
    let client = state.require_client().await?;
    client.move_document(&id, &parent, Some(&new_name)).await?;
    state.invalidate_entries().await;
    Ok(())
}

/// Moves documents into another folder.
#[tauri::command]
pub async fn move_entries(
    state: S<'_>,
    ids: Vec<String>,
    target_folder_id: String,
) -> CmdResult<()> {
    let entries = cached_entries(state.inner()).await?;
    let client = state.require_client().await?;
    for id in &ids {
        let entry = find_entry(&entries, id)?;
        if entry.is_folder() {
            return Err(AppError::new(
                "unsupported",
                "moving folders is not supported by the device API",
            ));
        }
        client.move_document(id, &target_folder_id, None).await?;
    }
    state.invalidate_entries().await;
    Ok(())
}

/// Downloads a document into the app cache and opens it with the OS PDF
/// viewer (FR-BRW-5).
#[tauri::command]
pub async fn open_entry(state: S<'_>, id: String) -> CmdResult<()> {
    let entries = cached_entries(state.inner()).await?;
    let entry = find_entry(&entries, &id)?.clone();
    if entry.is_folder() {
        return Err(AppError::new("unsupported", "cannot preview a folder"));
    }
    let client = state.require_client().await?;

    let cache_dir = state
        .app
        .path()
        .app_cache_dir()
        .map_err(|e| AppError::new("io", e.to_string()))?
        .join("previews")
        .join(&entry.entry_id);
    tokio::fs::create_dir_all(&cache_dir).await?;
    let target = cache_dir.join(&entry.entry_name);

    let resp = client.download_response(&id).await?;
    stream_to_file(resp, &target).await?;

    state
        .app
        .opener()
        .open_path(target.to_string_lossy(), None::<&str>)
        .map_err(|e| AppError::new("opener", e.to_string()))?;
    Ok(())
}

/// Opens a document on the device screen (FR-BRW-6).
#[tauri::command]
pub async fn open_on_device(state: S<'_>, id: String, page: Option<u32>) -> CmdResult<()> {
    let client = state.require_client().await?;
    client.open_on_device(&id, page.unwrap_or(1)).await?;
    Ok(())
}

async fn stream_to_file(resp: dpt_core::reqwest::Response, target: &Path) -> CmdResult<()> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;
    let tmp = target.with_extension("part");
    let mut file = tokio::fs::File::create(&tmp).await?;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| AppError::new("network", e.to_string()))?;
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    drop(file);
    tokio::fs::rename(&tmp, target).await?;
    Ok(())
}

// ---- device status & settings (FR-SET-1/3) ----------------------------------

#[derive(Serialize)]
pub struct DeviceStatus {
    pub serial: String,
    pub model: Option<String>,
    pub firmware: Option<String>,
    pub mac_address: Option<String>,
    pub battery: BatteryStatus,
    pub storage: StorageStatus,
}

#[tauri::command]
pub async fn device_status(state: S<'_>) -> CmdResult<DeviceStatus> {
    let client = state.require_client().await?;
    let info = client.device_info().await?;
    let battery = client.battery().await?;
    let storage = client.storage().await?;
    let firmware = client.firmware_version().await.ok();
    let mac_address = client.mac_address().await.ok();
    Ok(DeviceStatus {
        serial: info.serial_number,
        model: info.model_name,
        firmware,
        mac_address,
        battery,
        storage,
    })
}

/// Sets the device clock to the host's current time (FR-SET-3).
#[tauri::command]
pub async fn set_device_clock(state: S<'_>) -> CmdResult<()> {
    let client = state.require_client().await?;
    client.set_clock_now().await?;
    Ok(())
}

// ---- device configuration (FR-SET-2) ----------------------------------------

/// One key/value from `GET /system/configs/` — values are flattened to
/// strings so the UI can render known keys as form fields and the rest as a
/// generic table (FR-SET-2).
#[derive(Serialize)]
pub struct ConfigEntry {
    pub key: String,
    pub value: String,
}

fn config_value_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        // Some firmwares nest each setting as `{ "value": ... }`.
        serde_json::Value::Object(m) if m.contains_key("value") => {
            config_value_to_string(&m["value"])
        }
        other => other.to_string(),
    }
}

/// All device configuration values, sorted by key (FR-SET-2).
#[tauri::command]
pub async fn device_configs(state: S<'_>) -> CmdResult<Vec<ConfigEntry>> {
    let client = state.require_client().await?;
    let raw = client.configs().await?;
    let obj = raw
        .as_object()
        .ok_or_else(|| AppError::new("protocol", "unexpected /system/configs/ shape"))?;
    let mut out: Vec<ConfigEntry> = obj
        .iter()
        .map(|(key, value)| ConfigEntry {
            key: key.clone(),
            value: config_value_to_string(value),
        })
        .collect();
    out.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(out)
}

/// Writes one configuration value (FR-SET-2).
#[tauri::command]
pub async fn set_device_config(state: S<'_>, key: String, value: String) -> CmdResult<()> {
    let key = key.trim();
    if key.is_empty() || key.contains(['/', '?', '#']) || key.contains(char::is_whitespace) {
        return Err(AppError::new("invalid", "invalid configuration key"));
    }
    let client = state.require_client().await?;
    client.set_config(key, &value).await?;
    Ok(())
}

// ---- screenshots (FR-SET-5) ---------------------------------------------------

/// Captures the device screen and puts it on the OS clipboard (FR-SET-5).
/// The device's PNG is decoded to raw RGBA because clipboards want bitmaps;
/// the clipboard-manager plugin handles per-platform quirks and lifetime.
#[tauri::command]
pub async fn copy_screenshot_to_clipboard(app: tauri::AppHandle, state: S<'_>) -> CmdResult<()> {
    use tauri_plugin_clipboard_manager::ClipboardExt;

    let client = state.require_client().await?;
    let png = client.screenshot_png().await?;
    let rgba = tokio::task::spawn_blocking(move || {
        image::load_from_memory_with_format(&png, image::ImageFormat::Png)
            .map(|img| img.into_rgba8())
    })
    .await
    .map_err(|e| AppError::new("io", e.to_string()))?
    .map_err(|e| AppError::new("screenshot", format!("cannot decode screenshot: {e}")))?;

    let (width, height) = rgba.dimensions();
    app.clipboard()
        .write_image(&tauri::image::Image::new_owned(
            rgba.into_raw(),
            width,
            height,
        ))
        .map_err(|e| AppError::new("clipboard", e.to_string()))?;
    Ok(())
}

// ---- Wi-Fi management (FR-SET-4) ----------------------------------------------

#[tauri::command]
pub async fn wifi_enabled(state: S<'_>) -> CmdResult<bool> {
    let client = state.require_client().await?;
    Ok(client.wifi_enabled().await?)
}

/// Switches the device's Wi-Fi radio. Turning it off while connected over
/// Wi-Fi drops the connection — the UI warns first.
#[tauri::command]
pub async fn set_wifi_enabled(state: S<'_>, on: bool) -> CmdResult<()> {
    let client = state.require_client().await?;
    client.set_wifi_enabled(on).await?;
    Ok(())
}

#[tauri::command]
pub async fn wifi_stored_networks(
    state: S<'_>,
) -> CmdResult<Vec<dpt_core::api::wifi::AccessPoint>> {
    let client = state.require_client().await?;
    Ok(client.stored_access_points().await?)
}

#[tauri::command]
pub async fn wifi_scan(state: S<'_>) -> CmdResult<Vec<dpt_core::api::wifi::AccessPoint>> {
    let client = state.require_client().await?;
    Ok(client.scan_access_points().await?)
}

/// Adds/configures a network. The passphrase goes straight to the device
/// and is never stored by the app (NFR-SEC-5).
#[tauri::command]
pub async fn wifi_add_network(
    state: S<'_>,
    config: dpt_core::api::wifi::WifiNetworkConfig,
) -> CmdResult<()> {
    let client = state.require_client().await?;
    client.register_access_point(&config).await?;
    Ok(())
}

#[tauri::command]
pub async fn wifi_remove_network(state: S<'_>, ssid: String, security: String) -> CmdResult<()> {
    let client = state.require_client().await?;
    client.delete_access_point(&ssid, &security).await?;
    Ok(())
}

// ---- note templates (FR-BRW-7, FR-TRF-6) ---------------------------------------

#[tauri::command]
pub async fn list_templates(state: S<'_>) -> CmdResult<Vec<dpt_core::model::NoteTemplate>> {
    let client = state.require_client().await?;
    Ok(client.list_templates().await?)
}

/// Enqueues template uploads (one per PDF path); the template name is the
/// file name without extension. Runs through the normal transfer queue
/// (FR-TRF-6, docs/05 §3.4).
#[tauri::command]
pub async fn upload_templates(state: S<'_>, paths: Vec<String>) -> CmdResult<Vec<u64>> {
    let mut kinds = Vec::new();
    for p in paths {
        let path = PathBuf::from(&p);
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| AppError::new("io", "invalid file path"))?
            .to_string();
        if !file_name.to_lowercase().ends_with(".pdf") {
            return Err(AppError::new(
                "invalid",
                format!("'{file_name}' is not a PDF — templates must be PDF files"),
            ));
        }
        let template_name = path
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or(&file_name)
            .to_string();
        kinds.push((
            file_name.clone(),
            JobKind::UploadTemplate {
                local_path: path,
                template_name,
                file_name,
            },
        ));
    }
    Ok(transfers::enqueue(state.inner(), kinds).await)
}

/// Deletes a template (irreversible on the device; the UI confirms first).
#[tauri::command]
pub async fn delete_template(state: S<'_>, id: String) -> CmdResult<()> {
    let client = state.require_client().await?;
    client.delete_template(&id).await?;
    Ok(())
}

// ---- USB connection (FR-CONN-4) -------------------------------------------------

/// Serial ports that may be a Digital Paper in CDC ACM mode.
#[tauri::command]
pub async fn usb_ports() -> CmdResult<Vec<dpt_core::usb::UsbCandidate>> {
    tokio::task::spawn_blocking(dpt_core::usb::list_candidate_ports)
        .await
        .map_err(|e| AppError::new("io", e.to_string()))?
        .map_err(AppError::from)
}

/// Writes the Ethernet-over-USB mode switch to a serial port. `mode` is
/// `"rndis"`, `"cdc-ecm"` or empty for the host-OS default (protocol §2.1).
/// Returns the mode that was applied.
#[tauri::command]
pub async fn usb_switch_mode(port: String, mode: Option<String>) -> CmdResult<String> {
    use dpt_core::usb::UsbNetMode;
    let mode = match mode.as_deref() {
        Some("rndis") => UsbNetMode::Rndis,
        Some("cdc-ecm") => UsbNetMode::CdcEcm,
        _ => UsbNetMode::for_host_os(),
    };
    tokio::task::spawn_blocking(move || dpt_core::usb::switch_mode(&port, mode))
        .await
        .map_err(|e| AppError::new("io", e.to_string()))?
        .map_err(AppError::from)?;
    Ok(match mode {
        UsbNetMode::Rndis => "rndis".into(),
        UsbNetMode::CdcEcm => "cdc-ecm".into(),
    })
}

// ---- credential import (FR-REG-6) -------------------------------------------------

/// Credential pairs found in the default Sony / dptrp1 locations
/// (docs/07 §2).
#[tauri::command]
pub async fn import_candidates() -> CmdResult<Vec<crate::import::ImportCandidate>> {
    tokio::task::spawn_blocking(crate::import::find_candidates)
        .await
        .map_err(|e| AppError::new("io", e.to_string()))
}

/// Imports credentials from `deviceid.dat`/`privatekey.dat`, validates them
/// by authenticating against the device at `address`, then stores them
/// through the normal path (keychain + pinned cert + registry) and connects.
/// The certificate is obtained trust-on-first-use since Sony's app never
/// stored one (docs/07 §2). Source files are left untouched.
#[tauri::command]
pub async fn import_credentials(
    state: S<'_>,
    deviceid_path: String,
    privatekey_path: String,
    address: String,
) -> CmdResult<ConnectionPayload> {
    let credentials =
        crate::import::read_credentials(Path::new(&deviceid_path), Path::new(&privatekey_path))?;

    let addr = DeviceAddr::new(address.clone());
    let info = DeviceClient::probe(&addr).await?;
    let serial = info.serial_number.clone();
    let name = info
        .model_name
        .clone()
        .unwrap_or_else(|| "Digital Paper".to_string());

    let cert_pem = dpt_core::client::fetch_server_certificate(&addr).await?;

    // Validate before storing anything: connecting authenticates with the
    // imported key against the TOFU-pinned certificate.
    let ctx = DeviceContext {
        serial: serial.clone(),
        name: name.clone(),
        addr,
        cert_pem: cert_pem.clone(),
        credentials: credentials.clone(),
    };
    let app_state = state.inner().clone();
    if let Err(e) = app_state.connect(ctx).await {
        app_state.disconnect().await;
        return Err(AppError::new(
            "import_auth_failed",
            format!(
                "the device rejected the imported credentials: {}",
                e.message
            ),
        ));
    }

    state.creds.save(&serial, &credentials)?;
    state.stores.save_cert(&serial, &cert_pem)?;
    state.stores.upsert_device(KnownDevice {
        serial,
        name,
        model: info.model_name,
        last_address: Some(address),
    })?;
    Ok(app_state.connection_payload().await)
}

// ---- transfers (FR-TRF-*) ----------------------------------------------------

#[derive(Deserialize)]
pub struct UploadItem {
    pub local_path: String,
    pub file_name: String,
    /// Set when the user chose to overwrite an existing document.
    pub existing_doc_id: Option<String>,
}

/// Enqueues file uploads into a device folder. Conflict decisions
/// (overwrite/keep-both/skip) are made by the frontend beforehand
/// (FR-TRF-9); "overwrite" arrives as `existing_doc_id`.
#[tauri::command]
pub async fn upload_files(
    state: S<'_>,
    dest_folder_id: String,
    items: Vec<UploadItem>,
) -> CmdResult<Vec<u64>> {
    let kinds = items
        .into_iter()
        .map(|item| {
            (
                item.file_name.clone(),
                JobKind::Upload {
                    local_path: PathBuf::from(item.local_path),
                    file_name: item.file_name,
                    dest_folder_id: dest_folder_id.clone(),
                    existing_doc_id: item.existing_doc_id,
                },
            )
        })
        .collect();
    Ok(transfers::enqueue(state.inner(), kinds).await)
}

/// Uploads a local folder recursively: mirrors the directory structure on
/// the device and enqueues every PDF (FR-TRF-4). Existing documents with
/// the same path are overwritten.
#[tauri::command]
pub async fn upload_folder(
    state: S<'_>,
    dest_folder_id: String,
    dest_folder_path: String,
    local_dir: String,
) -> CmdResult<Vec<u64>> {
    let client = state.require_client().await?;
    let local_root = PathBuf::from(&local_dir);
    let root_name = local_root
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| AppError::new("io", "invalid folder path"))?
        .to_string();

    // Collect relative dirs and PDF files, breadth-first for parent-first
    // folder creation (protocol §7.3.6).
    let mut rel_dirs: Vec<PathBuf> = vec![PathBuf::new()];
    let mut files: Vec<(PathBuf, PathBuf)> = Vec::new(); // (abs, rel)
    let mut queue = vec![local_root.clone()];
    while let Some(dir) = queue.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            let rel = path.strip_prefix(&local_root).unwrap().to_path_buf();
            if path.is_dir() {
                rel_dirs.push(rel);
                queue.push(path);
            } else if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
            {
                files.push((path, rel));
            }
        }
    }
    rel_dirs.sort_by_key(|d| d.components().count());

    // Ensure remote folders exist; map rel dir -> folder id.
    let mut folder_ids: std::collections::HashMap<PathBuf, String> =
        std::collections::HashMap::new();
    for rel in &rel_dirs {
        let remote_path = join_remote(&dest_folder_path, &root_name, rel);
        let id = match client.resolve_path(&remote_path).await {
            Ok(entry) if entry.is_folder() => entry.entry_id,
            Ok(_) => {
                return Err(AppError::new(
                    "conflict",
                    format!("'{remote_path}' exists on the device as a document"),
                ))
            }
            Err(_) => {
                let (parent_id, name) = match rel.parent() {
                    Some(p) if p.as_os_str().is_empty() && rel.as_os_str().is_empty() => {
                        unreachable!()
                    }
                    _ if rel.as_os_str().is_empty() => (dest_folder_id.clone(), root_name.clone()),
                    Some(parent) => {
                        let pid = folder_ids
                            .get(parent)
                            .cloned()
                            .ok_or_else(|| AppError::new("io", "folder order error"))?;
                        let name = rel
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or_default()
                            .to_string();
                        (pid, name)
                    }
                    None => (dest_folder_id.clone(), root_name.clone()),
                };
                client.create_folder(&parent_id, &name).await?;
                client.resolve_path(&remote_path).await?.entry_id
            }
        };
        folder_ids.insert(rel.clone(), id);
    }
    state.invalidate_entries().await;

    // Enqueue file uploads (overwrite existing documents in place).
    let mut kinds = Vec::new();
    for (abs, rel) in files {
        let parent_rel = rel.parent().map(Path::to_path_buf).unwrap_or_default();
        let folder_id = folder_ids
            .get(&parent_rel)
            .cloned()
            .unwrap_or_else(|| dest_folder_id.clone());
        let file_name = rel
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        let remote_file_path = join_remote(&dest_folder_path, &root_name, &rel);
        let existing = match client.resolve_path(&remote_file_path).await {
            Ok(e) if !e.is_folder() => Some(e.entry_id),
            _ => None,
        };
        kinds.push((
            file_name.clone(),
            JobKind::Upload {
                local_path: abs,
                file_name,
                dest_folder_id: folder_id,
                existing_doc_id: existing,
            },
        ));
    }
    Ok(transfers::enqueue(state.inner(), kinds).await)
}

fn join_remote(base: &str, root: &str, rel: &Path) -> String {
    let mut s = format!("{base}/{root}");
    for comp in rel.components() {
        s.push('/');
        s.push_str(&comp.as_os_str().to_string_lossy());
    }
    s
}

/// Enqueues downloads for entries; folders are mirrored recursively into
/// `target_dir` (FR-TRF-2/5). Local collisions get a ` (n)` suffix.
#[tauri::command]
pub async fn download_entries(
    state: S<'_>,
    ids: Vec<String>,
    target_dir: String,
) -> CmdResult<Vec<u64>> {
    let entries = cached_entries(state.inner()).await?;
    let target_dir = PathBuf::from(target_dir);
    let mut kinds = Vec::new();

    for id in &ids {
        let entry = find_entry(&entries, id)?;
        if entry.is_folder() {
            let base_path = format!("{}/", entry.entry_path);
            let local_base = unique_path(&target_dir.join(&entry.entry_name));
            for sub in entries
                .iter()
                .filter(|e| e.entry_path.starts_with(&base_path))
            {
                let rel = &sub.entry_path[base_path.len()..];
                let local = local_base.join(rel);
                if sub.is_folder() {
                    std::fs::create_dir_all(&local)?;
                } else {
                    kinds.push((
                        sub.entry_name.clone(),
                        JobKind::Download {
                            entry_id: sub.entry_id.clone(),
                            target_path: local,
                        },
                    ));
                }
            }
        } else {
            let target = unique_path(&target_dir.join(&entry.entry_name));
            kinds.push((
                entry.entry_name.clone(),
                JobKind::Download {
                    entry_id: entry.entry_id.clone(),
                    target_path: target,
                },
            ));
        }
    }
    Ok(transfers::enqueue(state.inner(), kinds).await)
}

/// Appends ` (n)` before the extension until the path does not exist.
fn unique_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file")
        .to_string();
    let ext = path.extension().and_then(|e| e.to_str());
    for n in 2..1000 {
        let name = match ext {
            Some(ext) => format!("{stem} ({n}).{ext}"),
            None => format!("{stem} ({n})"),
        };
        let candidate = path.with_file_name(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    path.to_path_buf()
}

#[derive(Serialize)]
pub struct PathInfo {
    pub path: String,
    pub file_name: String,
    pub is_dir: bool,
}

/// Classifies dropped/picked paths so the frontend can route folders to
/// `upload_folder` and files to `upload_files`.
#[tauri::command]
pub fn classify_paths(paths: Vec<String>) -> Vec<PathInfo> {
    paths
        .into_iter()
        .map(|p| {
            let path = PathBuf::from(&p);
            PathInfo {
                file_name: path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default()
                    .to_string(),
                is_dir: path.is_dir(),
                path: p,
            }
        })
        .collect()
}

#[tauri::command]
pub async fn transfer_list(state: S<'_>) -> CmdResult<Vec<JobSnapshot>> {
    Ok(transfers::snapshot(state.inner()).await)
}

#[tauri::command]
pub async fn transfer_cancel(state: S<'_>, id: u64) -> CmdResult<()> {
    transfers::cancel(state.inner(), id).await;
    Ok(())
}

#[tauri::command]
pub async fn transfers_clear_finished(state: S<'_>) -> CmdResult<()> {
    transfers::clear_finished(state.inner()).await;
    Ok(())
}

// ---- sync (FR-SYN-*) ----------------------------------------------------------

/// A sync pair together with its most recent run record, for the settings
/// list (docs/05 §3.6).
#[derive(Serialize)]
pub struct SyncPairInfo {
    #[serde(flatten)]
    pub pair: SyncPair,
    pub last_run: Option<serde_json::Value>,
}

#[tauri::command]
pub fn sync_pairs(state: S<'_>) -> Vec<SyncPairInfo> {
    state
        .stores
        .load_sync_pairs()
        .into_iter()
        .map(|pair| {
            let last_run = state.stores.load_sync_history(&pair.id).into_iter().next();
            SyncPairInfo { pair, last_run }
        })
        .collect()
}

/// Creates or updates a sync pair (FR-SYN-1). An empty id creates a new
/// pair.
#[tauri::command]
pub fn sync_pair_upsert(state: S<'_>, mut pair: SyncPair) -> CmdResult<SyncPair> {
    if pair.local_root.trim().is_empty() {
        return Err(AppError::new("invalid", "choose a local folder"));
    }
    if pair.remote_root.trim().is_empty() {
        pair.remote_root = "Document".into();
    }
    pair.remote_root = pair.remote_root.trim_matches('/').to_string();
    if !pair.remote_root.starts_with("Document") {
        return Err(AppError::new(
            "invalid",
            "the device folder must be inside 'Document'",
        ));
    }
    if pair.id.trim().is_empty() {
        pair.id = uuid::Uuid::new_v4().to_string();
    }
    // Validate the filter patterns early so the editor can show the error.
    dpt_core::sync::Filters::new(&pair.filters).map_err(AppError::from)?;
    state.stores.upsert_sync_pair(pair.clone())?;
    Ok(pair)
}

#[tauri::command]
pub fn sync_pair_delete(state: S<'_>, id: String) -> CmdResult<()> {
    sync::cancel(state.inner(), &id);
    state.stores.remove_sync_pair(&id)
}

/// Dry run (FR-SYN-5): plans without applying and returns every action.
#[tauri::command]
pub async fn sync_preview(state: S<'_>, id: String) -> CmdResult<dpt_core::sync::Plan> {
    let pair = state
        .stores
        .load_sync_pairs()
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| AppError::new("not_found", "sync pair not found"))?;
    let client = state.require_client().await?;

    let cfg = dpt_core::sync::SyncPairConfig {
        id: pair.id.clone(),
        local_root: std::path::PathBuf::from(&pair.local_root),
        remote_root: pair.remote_root.clone(),
        mode: pair.mode,
        filters: pair.filters.clone(),
    };
    // Per-device checkpoint: the plan is relative to the connected device's
    // last-known state (multi-device hub model).
    let serial = state.connected_serial().await.unwrap_or_default();
    let checkpoint =
        dpt_core::sync::Checkpoint::load(&state.stores.checkpoint_path(&pair.id, &serial))
            .unwrap_or_else(|| {
                dpt_core::sync::Checkpoint::new(&pair.id, &serial, &pair.remote_root)
            });
    let snap = dpt_core::sync::take_snapshot(client.as_ref(), &cfg, &checkpoint).await?;
    Ok(dpt_core::sync::make_plan(&cfg, &checkpoint, &snap)?)
}

/// Starts a run of one pair (FR-SYN-3). `confirmed` marks a run applied
/// from the preview dialog (skips the mass-deletion gate); `excluded`
/// carries the deselected actions.
#[tauri::command]
pub fn sync_run(
    state: S<'_>,
    id: String,
    confirmed: Option<bool>,
    excluded: Option<Vec<ExcludedAction>>,
) -> CmdResult<()> {
    let options = RunOptions {
        confirmed: confirmed.unwrap_or(false),
        excluded: excluded.unwrap_or_default(),
    };
    sync::enqueue(state.inner(), &id, Trigger::Manual, options);
    Ok(())
}

/// Toolbar/tray "Sync now": queues every enabled pair (FR-SYN-3). Returns
/// the number of runs actually enqueued (already queued/running pairs are
/// not double-queued).
#[tauri::command]
pub fn sync_run_all(state: S<'_>) -> CmdResult<u32> {
    let pairs: Vec<_> = state
        .stores
        .load_sync_pairs()
        .into_iter()
        .filter(|p| p.enabled)
        .collect();
    let mut count = 0u32;
    for pair in pairs {
        if sync::enqueue(
            state.inner(),
            &pair.id,
            Trigger::Manual,
            RunOptions::default(),
        ) {
            count += 1;
        }
    }
    Ok(count)
}

#[tauri::command]
pub fn sync_cancel(state: S<'_>, id: String) -> CmdResult<()> {
    sync::cancel(state.inner(), &id);
    Ok(())
}

/// Resolves the mass-deletion confirmation (FR-SYN-5).
/// `decision`: `apply` | `skip-deletions` | `cancel`.
#[tauri::command]
pub fn sync_confirm(state: S<'_>, id: String, decision: String) -> CmdResult<()> {
    let decision = match decision.as_str() {
        "apply" => Decision::Apply,
        "skip-deletions" => Decision::SkipDeletions,
        "cancel" => Decision::Cancel,
        other => {
            return Err(AppError::new(
                "invalid",
                format!("unknown decision '{other}'"),
            ))
        }
    };
    sync::confirm(state.inner(), &id, decision)
}

#[tauri::command]
pub fn sync_history(state: S<'_>, id: String) -> Vec<serde_json::Value> {
    state.stores.load_sync_history(&id)
}

#[tauri::command]
pub fn sync_status(state: S<'_>) -> SyncStatusPayload {
    state.sync.status()
}

#[cfg(test)]
mod tests {
    use super::version_is_newer;

    #[test]
    fn version_comparison() {
        assert!(version_is_newer("0.4.0", "0.3.0-1"));
        assert!(version_is_newer("0.3.1", "0.3.0"));
        assert!(version_is_newer("1.0.0", "0.9.9"));
        // A release is newer than its own pre-release …
        assert!(version_is_newer("0.3.0", "0.3.0-1"));
        // … and pre-releases order numerically.
        assert!(version_is_newer("0.3.0-2", "0.3.0-1"));

        assert!(!version_is_newer("0.3.0", "0.3.0"));
        assert!(!version_is_newer("0.3.0-1", "0.3.0-1"));
        assert!(!version_is_newer("0.3.0-1", "0.3.0"));
        assert!(!version_is_newer("0.2.9", "0.3.0"));
        // Padding: "0.4" == "0.4.0".
        assert!(!version_is_newer("0.4", "0.4.0"));
    }
}
