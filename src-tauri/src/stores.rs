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
    #[serde(default)]
    pub last_active_serial: Option<String>,
}

fn default_version() -> u32 {
    1
}
fn default_theme() -> String {
    "system".into()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            theme: default_theme(),
            last_active_serial: None,
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

/// Filesystem-backed store rooted at the app config directory.
#[derive(Clone)]
pub struct Stores {
    config_dir: PathBuf,
}

impl Stores {
    pub fn new(config_dir: PathBuf) -> Self {
        Self { config_dir }
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
