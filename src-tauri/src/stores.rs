//! Local, non-secret state persisted as versioned JSON files with atomic
//! writes (docs/07 §1; NFR-REL-1). Secrets live in [`crate::credentials`].

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::AppError;

/// Application-level settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default = "default_theme")]
    pub theme: String,
    /// UI language: `"system"` or a locale code like `"en"`, `"de"`
    /// (FR-APP-4, NFR-I18N-1).
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default)]
    pub last_active_serial: Option<String>,
}

fn default_version() -> u32 {
    1
}
fn default_theme() -> String {
    "system".into()
}
fn default_language() -> String {
    "system".into()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            theme: default_theme(),
            language: default_language(),
            last_active_serial: None,
        }
    }
}

/// Configuration of one sync pair (docs/06 §1; FR-SYN-1/2/4/8). Stored in
/// `sync-pairs.json`; engine-facing fields are converted in
/// [`crate::sync`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPair {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub local_root: String,
    #[serde(default = "default_remote_root")]
    pub remote_root: String,
    #[serde(default)]
    pub mode: dpt_core::sync::SyncMode,
    /// Run automatically when the device connects (FR-SYN-4).
    #[serde(default)]
    pub on_connect: bool,
    /// Run every N minutes while connected (FR-SYN-4).
    #[serde(default)]
    pub interval_minutes: Option<u32>,
    /// Mass-deletion confirmation threshold (FR-SYN-5).
    #[serde(default = "default_deletion_threshold")]
    pub deletion_threshold: u32,
    /// Exclude glob patterns on relpaths (FR-SYN-8).
    #[serde(default)]
    pub filters: Vec<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_remote_root() -> String {
    "Document".into()
}
fn default_deletion_threshold() -> u32 {
    10
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SyncPairsFile {
    #[serde(default = "default_version")]
    version: u32,
    #[serde(default)]
    pairs: Vec<SyncPair>,
}

impl Default for SyncPairsFile {
    fn default() -> Self {
        Self {
            version: 1,
            pairs: Vec::new(),
        }
    }
}

/// A device this app has paired with or connected to before (FR-CONN-7).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnownDevice {
    pub serial: String,
    pub name: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub last_address: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct DevicesFile {
    #[serde(default)]
    devices: Vec<KnownDevice>,
}

/// Filesystem-backed store rooted at the app config directory; sync
/// checkpoints and run history live in the data directory (docs/07 §1).
#[derive(Clone)]
pub struct Stores {
    config_dir: PathBuf,
    data_dir: PathBuf,
}

impl Stores {
    pub fn new(config_dir: PathBuf, data_dir: PathBuf) -> Self {
        Self {
            config_dir,
            data_dir,
        }
    }

    fn settings_path(&self) -> PathBuf {
        self.config_dir.join("settings.json")
    }
    fn devices_path(&self) -> PathBuf {
        self.config_dir.join("devices.json")
    }
    fn certs_dir(&self) -> PathBuf {
        self.config_dir.join("certs")
    }
    pub fn fallback_credentials_dir(&self) -> PathBuf {
        self.config_dir.join("credentials")
    }

    pub fn load_settings(&self) -> Settings {
        read_json(&self.settings_path()).unwrap_or_default()
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<(), AppError> {
        write_json_atomic(&self.settings_path(), settings)
    }

    pub fn load_devices(&self) -> Vec<KnownDevice> {
        read_json::<DevicesFile>(&self.devices_path())
            .unwrap_or_default()
            .devices
    }

    pub fn save_devices(&self, devices: &[KnownDevice]) -> Result<(), AppError> {
        write_json_atomic(
            &self.devices_path(),
            &DevicesFile {
                devices: devices.to_vec(),
            },
        )
    }

    /// Records/updates a known device (keyed by serial).
    pub fn upsert_device(&self, device: KnownDevice) -> Result<(), AppError> {
        let mut devices = self.load_devices();
        if let Some(existing) = devices.iter_mut().find(|d| d.serial == device.serial) {
            *existing = device;
        } else {
            devices.push(device);
        }
        self.save_devices(&devices)
    }

    pub fn remove_device(&self, serial: &str) -> Result<(), AppError> {
        let mut devices = self.load_devices();
        devices.retain(|d| d.serial != serial);
        self.save_devices(&devices)?;
        let _ = std::fs::remove_file(self.cert_path(serial));
        Ok(())
    }

    fn cert_path(&self, serial: &str) -> PathBuf {
        self.certs_dir().join(format!("{serial}.pem"))
    }

    /// Stores the pinned device certificate (public data; docs/07 §3).
    pub fn save_cert(&self, serial: &str, pem: &str) -> Result<(), AppError> {
        std::fs::create_dir_all(self.certs_dir())?;
        write_atomic(&self.cert_path(serial), pem.as_bytes())
    }

    pub fn load_cert(&self, serial: &str) -> Result<String, AppError> {
        std::fs::read_to_string(self.cert_path(serial))
            .map_err(|_| AppError::new("no_cert", "no pinned certificate for this device"))
    }

    // ---- sync pairs, checkpoints, history (docs/07 §1) ----------------------

    fn sync_pairs_path(&self) -> PathBuf {
        self.config_dir.join("sync-pairs.json")
    }

    /// Checkpoint per (pair, device): a pair is device-agnostic — each device
    /// tracks its own sync state against the shared local folder, like one
    /// cloud account synced by several computers (docs/06 §7).
    pub fn checkpoint_path(&self, pair_id: &str, device_serial: &str) -> PathBuf {
        let serial: String = device_serial
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect();
        self.data_dir
            .join("checkpoints")
            .join(format!("{pair_id}@{serial}.json"))
    }

    fn history_path(&self, pair_id: &str) -> PathBuf {
        self.data_dir
            .join("sync-history")
            .join(format!("{pair_id}.jsonl"))
    }

    pub fn load_sync_pairs(&self) -> Vec<SyncPair> {
        read_json::<SyncPairsFile>(&self.sync_pairs_path())
            .unwrap_or_default()
            .pairs
    }

    pub fn save_sync_pairs(&self, pairs: &[SyncPair]) -> Result<(), AppError> {
        write_json_atomic(
            &self.sync_pairs_path(),
            &SyncPairsFile {
                version: 1,
                pairs: pairs.to_vec(),
            },
        )
    }

    pub fn upsert_sync_pair(&self, pair: SyncPair) -> Result<(), AppError> {
        let mut pairs = self.load_sync_pairs();
        if let Some(existing) = pairs.iter_mut().find(|p| p.id == pair.id) {
            *existing = pair;
        } else {
            pairs.push(pair);
        }
        self.save_sync_pairs(&pairs)
    }

    /// Removes a pair together with its per-device checkpoints and history.
    pub fn remove_sync_pair(&self, pair_id: &str) -> Result<(), AppError> {
        let mut pairs = self.load_sync_pairs();
        pairs.retain(|p| p.id != pair_id);
        self.save_sync_pairs(&pairs)?;
        if let Ok(dir) = std::fs::read_dir(self.data_dir.join("checkpoints")) {
            let prefix = format!("{pair_id}@");
            for entry in dir.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with(&prefix) || name.as_ref() == format!("{pair_id}.json") {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        let _ = std::fs::remove_file(self.history_path(pair_id));
        Ok(())
    }

    /// Appends a run record to the pair's history, capped at the last 100
    /// runs (FR-SYN-7; docs/06 §9).
    pub fn append_sync_history(
        &self,
        pair_id: &str,
        record: &serde_json::Value,
    ) -> Result<(), AppError> {
        let path = self.history_path(pair_id);
        let mut lines: Vec<String> = std::fs::read_to_string(&path)
            .map(|s| s.lines().map(str::to_string).collect())
            .unwrap_or_default();
        lines.push(serde_json::to_string(record)?);
        if lines.len() > 100 {
            let skip = lines.len() - 100;
            lines.drain(..skip);
        }
        write_atomic(&path, (lines.join("\n") + "\n").as_bytes())
    }

    /// Loads the run history, most recent first.
    pub fn load_sync_history(&self, pair_id: &str) -> Vec<serde_json::Value> {
        let Ok(text) = std::fs::read_to_string(self.history_path(pair_id)) else {
            return Vec::new();
        };
        let mut out: Vec<serde_json::Value> = text
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        out.reverse();
        out
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let bytes = std::fs::read(path).ok()?;
    match serde_json::from_slice(&bytes) {
        Ok(v) => Some(v),
        Err(e) => {
            tracing::warn!(?path, %e, "corrupt state file; backing up and ignoring");
            let _ = std::fs::rename(path, path.with_extension("bad"));
            None
        }
    }
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), AppError> {
    let bytes = serde_json::to_vec_pretty(value)?;
    write_atomic(path, &bytes)
}

/// Writes `bytes` to `path` via a temp file + rename (NFR-REL-1).
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}
