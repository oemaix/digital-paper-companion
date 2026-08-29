//! Checkpoint persistence (docs/06 §7).
//!
//! One schema-versioned JSON file per sync pair, written atomically
//! (write-temp-then-rename, NFR-REL-1). A corrupt checkpoint is backed up
//! and treated as "no checkpoint" (safe first-run semantics). Unknown
//! fields are preserved on rewrite via `serde(flatten)` maps.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::snapshot::{self, RemoteView};

pub const CHECKPOINT_VERSION: u32 = 1;

/// Local-only conflict copy, excluded from future upload (docs/06 §5.2.4).
pub const FLAG_CONFLICT_COPY: &str = "conflict_copy";
/// The local file name was escaped because the device name contains
/// characters illegal on the local OS (docs/06 §3.1).
pub const FLAG_NAME_ESCAPED: &str = "name_escaped";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    File,
    Folder,
}

/// Remote-side state as of the last completed action for a relpath.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RemoteCheck {
    pub entry_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Local-side state as of the last completed action for a relpath.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LocalCheck {
    /// RFC 3339 UTC.
    pub mtime: String,
    pub size: u64,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointEntry {
    #[serde(rename = "type")]
    pub kind: NodeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<RemoteCheck>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<LocalCheck>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,
    /// On-disk relative path when it differs from the canonical relpath
    /// (illegal-character escaping, docs/06 §3.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_relpath: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl CheckpointEntry {
    pub fn folder() -> Self {
        Self {
            kind: NodeKind::Folder,
            remote: None,
            local: None,
            flags: Vec::new(),
            local_relpath: None,
            extra: serde_json::Map::new(),
        }
    }

    pub fn file() -> Self {
        Self {
            kind: NodeKind::File,
            ..Self::folder()
        }
    }

    pub fn has_flag(&self, flag: &str) -> bool {
        self.flags.iter().any(|f| f == flag)
    }

    pub fn set_flag(&mut self, flag: &str) {
        if !self.has_flag(flag) {
            self.flags.push(flag.to_string());
        }
    }
}

/// Persisted snapshot of the tree state as of the end of the last
/// successful application of actions (docs/06 §1, §7). Keyed by canonical
/// relpath (device-side form, NFC).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub version: u32,
    pub pair_id: String,
    #[serde(default)]
    pub device_serial: String,
    pub remote_root: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(default)]
    pub entries: BTreeMap<String, CheckpointEntry>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Checkpoint {
    pub fn new(pair_id: &str, device_serial: &str, remote_root: &str) -> Self {
        Self {
            version: CHECKPOINT_VERSION,
            pair_id: pair_id.to_string(),
            device_serial: device_serial.to_string(),
            remote_root: remote_root.to_string(),
            completed_at: None,
            entries: BTreeMap::new(),
            extra: serde_json::Map::new(),
        }
    }

    /// Loads a checkpoint. A missing file yields `None`; an unreadable or
    /// corrupt file is backed up to `<name>.corrupt` and also yields `None`
    /// (first-run semantics never delete anything, docs/06 §7).
    pub fn load(path: &Path) -> Option<Self> {
        let bytes = std::fs::read(path).ok()?;
        match serde_json::from_slice::<Self>(&bytes) {
            Ok(cp) => Some(cp),
            Err(e) => {
                tracing::warn!(?path, %e, "corrupt sync checkpoint; backing up");
                let _ = std::fs::rename(path, path.with_extension("corrupt"));
                None
            }
        }
    }

    /// Atomic write (temp file + rename, NFR-REL-1).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, path)
    }

    /// Case/NFC-normalized lookup index: normalized key → canonical relpath.
    pub fn key_index(&self) -> HashMap<String, String> {
        self.entries
            .keys()
            .map(|relpath| (snapshot::norm_key(relpath), relpath.clone()))
            .collect()
    }

    /// Map from the normalized *on-disk* relpath to the canonical relpath,
    /// for entries whose local name was escaped. Used by the local walk to
    /// recognize escaped files (docs/06 §3.1).
    pub fn local_name_map(&self) -> HashMap<String, String> {
        self.entries
            .iter()
            .filter_map(|(relpath, entry)| {
                entry
                    .local_relpath
                    .as_ref()
                    .map(|disk| (snapshot::norm_key(disk), relpath.clone()))
            })
            .collect()
    }

    /// Refreshes the remote fields of existing entries from a fresh remote
    /// listing (docs/06 §6, closing consistency pass).
    pub fn refresh_remote(&mut self, fresh: &RemoteView) {
        for (relpath, entry) in self.entries.iter_mut() {
            let key = snapshot::norm_key(relpath);
            if entry.remote.is_none() {
                continue;
            }
            if let Some(file) = fresh.files.get(&key) {
                entry.remote = Some(RemoteCheck {
                    entry_id: file.entry_id.clone(),
                    modified_date: file.modified_date.clone(),
                    file_revision: file.revision.clone(),
                    size: file.size,
                    extra: serde_json::Map::new(),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_preserves_unknown_fields() {
        let json = serde_json::json!({
            "version": 1,
            "pair_id": "p1",
            "device_serial": "s1",
            "remote_root": "Document",
            "future_field": {"a": 1},
            "entries": {
                "Papers/a.pdf": {
                    "type": "file",
                    "remote": {"entry_id": "e1", "size": 10, "custom": true},
                    "local": {"mtime": "2026-01-01T00:00:00Z", "size": 10},
                    "flags": ["conflict_copy"]
                }
            }
        });
        let cp: Checkpoint = serde_json::from_value(json).unwrap();
        assert!(cp.entries["Papers/a.pdf"].has_flag(FLAG_CONFLICT_COPY));
        let out = serde_json::to_value(&cp).unwrap();
        assert_eq!(out["future_field"]["a"], 1);
        assert_eq!(out["entries"]["Papers/a.pdf"]["remote"]["custom"], true);
    }

    #[test]
    fn corrupt_checkpoint_is_backed_up() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cp.json");
        std::fs::write(&path, b"{ not json").unwrap();
        assert!(Checkpoint::load(&path).is_none());
        assert!(!path.exists());
        assert!(path.with_extension("corrupt").exists());
    }

    #[test]
    fn missing_checkpoint_loads_none() {
        assert!(Checkpoint::load(Path::new("/nonexistent/cp.json")).is_none());
    }
}
