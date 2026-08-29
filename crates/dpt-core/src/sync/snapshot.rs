//! Snapshot phase (docs/06 §3): build the local and remote views as maps
//! `normalized relpath → state`, with NFC path normalization and the
//! platform rules of docs/06 §3.1.
//!
//! Map keys are *normalized* (NFC, and case-folded on Windows); map values
//! carry the canonical relpath (device-side form) plus, for local nodes,
//! the actual on-disk relative path (which may be escaped).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use unicode_normalization::UnicodeNormalization;

use crate::model::Entry;

/// A PDF file found under the pair's local root.
#[derive(Debug, Clone)]
pub struct LocalFile {
    /// Canonical relpath (NFC, device-side form after unescaping).
    pub relpath: String,
    /// Actual relative path on disk (`/`-separated), possibly escaped.
    pub disk_relpath: String,
    pub mtime: DateTime<Utc>,
    pub size: u64,
}

#[derive(Debug, Clone)]
pub struct LocalFolder {
    pub relpath: String,
    pub disk_relpath: String,
}

#[derive(Debug, Default)]
pub struct LocalView {
    pub files: BTreeMap<String, LocalFile>,
    pub folders: BTreeMap<String, LocalFolder>,
    /// Keys of non-participating local files (non-PDF). They are never
    /// synced, but their presence blocks folder deletion (docs/06 §5.4).
    pub others: std::collections::BTreeSet<String>,
}

/// A document on the device under the pair's remote root.
#[derive(Debug, Clone)]
pub struct RemoteFile {
    pub relpath: String,
    pub entry_id: String,
    /// Raw device string, kept for the checkpoint.
    pub modified_date: Option<String>,
    pub modified: Option<DateTime<Utc>>,
    pub size: Option<u64>,
    pub revision: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RemoteFolder {
    pub relpath: String,
    pub entry_id: String,
}

#[derive(Debug, Default)]
pub struct RemoteView {
    pub files: BTreeMap<String, RemoteFile>,
    pub folders: BTreeMap<String, RemoteFolder>,
    /// Keys of non-participating remote documents (non-PDF). Their
    /// presence blocks remote folder deletion, because the device deletes
    /// folders recursively (docs/06 §5.4).
    pub others: std::collections::BTreeSet<String>,
}

/// NFC-normalizes a relpath (macOS filesystems return NFD; the device uses
/// NFC — NFR-I18N-3).
pub fn nfc(relpath: &str) -> String {
    relpath.nfc().collect()
}

/// Map key for a relpath: NFC everywhere; additionally case-folded on
/// Windows, where paths match case-insensitively (docs/06 §3.1).
pub fn norm_key(relpath: &str) -> String {
    let normalized = nfc(relpath);
    #[cfg(windows)]
    {
        normalized.to_lowercase()
    }
    #[cfg(not(windows))]
    {
        normalized
    }
}

/// Parses a device `modified_date` (e.g. `2026-08-28T20:11:00Z`).
pub fn parse_device_date(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .ok()
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%SZ")
                .ok()
                .map(|ndt| ndt.and_utc())
        })
}

/// Walks `root` and collects PDF files and folders (docs/06 §1: only PDFs
/// and folders participate; other local files are ignored). `unescape`
/// maps normalized on-disk relpaths back to canonical relpaths for files
/// recorded with escaped names (docs/06 §3.1).
pub fn walk_local(root: &Path, unescape: &HashMap<String, String>) -> std::io::Result<LocalView> {
    let mut view = LocalView::default();
    let mut stack: Vec<(PathBuf, String)> = vec![(root.to_path_buf(), String::new())];

    while let Some((dir, rel)) = stack.pop() {
        for item in std::fs::read_dir(&dir)? {
            let item = item?;
            let name = match item.file_name().into_string() {
                Ok(n) => n,
                Err(_) => continue, // non-Unicode name: cannot sync, skip
            };
            let disk_relpath = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            let canonical = unescape
                .get(&norm_key(&disk_relpath))
                .cloned()
                .unwrap_or_else(|| nfc(&disk_relpath));

            let file_type = item.file_type()?;
            if file_type.is_dir() {
                view.folders.insert(
                    norm_key(&canonical),
                    LocalFolder {
                        relpath: canonical.clone(),
                        disk_relpath: disk_relpath.clone(),
                    },
                );
                stack.push((item.path(), disk_relpath));
            } else if file_type.is_file() {
                if name.to_ascii_lowercase().ends_with(".pdf") && !name.ends_with(".part") {
                    let meta = item.metadata()?;
                    let mtime: DateTime<Utc> = meta.modified()?.into();
                    view.files.insert(
                        norm_key(&canonical),
                        LocalFile {
                            relpath: canonical,
                            disk_relpath,
                            mtime,
                            size: meta.len(),
                        },
                    );
                } else {
                    view.others.insert(norm_key(&canonical));
                }
            }
        }
    }
    Ok(view)
}

/// Builds the remote view from a full device listing, restricted to
/// `remote_root` (docs/06 §3). Non-PDF documents are excluded like local
/// non-PDF files (only PDFs participate).
pub fn remote_view(entries: &[Entry], remote_root: &str) -> RemoteView {
    let mut view = RemoteView::default();
    let prefix = format!("{remote_root}/");
    for entry in entries {
        let Some(rel) = entry.entry_path.strip_prefix(&prefix) else {
            continue;
        };
        if rel.is_empty() {
            continue;
        }
        let relpath = nfc(rel);
        let key = norm_key(&relpath);
        if entry.is_folder() {
            view.folders.insert(
                key,
                RemoteFolder {
                    relpath,
                    entry_id: entry.entry_id.clone(),
                },
            );
        } else {
            if !relpath.to_ascii_lowercase().ends_with(".pdf") {
                view.others.insert(key);
                continue;
            }
            view.files.insert(
                key,
                RemoteFile {
                    relpath,
                    entry_id: entry.entry_id.clone(),
                    modified_date: entry.modified_date.clone(),
                    modified: entry.modified_date.as_deref().and_then(parse_device_date),
                    size: entry.file_size,
                    revision: entry.file_revision.clone(),
                },
            );
        }
    }
    view
}

// ---- illegal-character escaping (docs/06 §3.1) ------------------------------

#[cfg(windows)]
const ILLEGAL_CHARS: &[char] = &['<', '>', ':', '"', '\\', '|', '?', '*'];
#[cfg(not(windows))]
const ILLEGAL_CHARS: &[char] = &[];

/// Escapes characters in a single name that are illegal on the local OS
/// with a reversible `%3A`-style encoding. The mapping is recorded in the
/// checkpoint so future runs recognize the file.
pub fn escape_local_name(name: &str) -> String {
    escape_name_with(name, ILLEGAL_CHARS)
}

pub(crate) fn escape_name_with(name: &str, illegal: &[char]) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if illegal.contains(&c) || (c as u32) < 0x20 {
            for b in c.to_string().as_bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Escapes every segment of a relpath for local use. Returns `None` when no
/// escaping was needed (the common case).
pub fn escape_local_relpath(relpath: &str) -> Option<String> {
    let escaped: Vec<String> = relpath.split('/').map(escape_local_name).collect();
    let joined = escaped.join("/");
    (joined != relpath).then_some(joined)
}

/// Ancestor folder relpaths of `relpath`, shallowest first
/// (`a/b/c.pdf` → `["a", "a/b"]`).
pub fn ancestors(relpath: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut acc = String::new();
    let segments: Vec<&str> = relpath.split('/').collect();
    for seg in &segments[..segments.len().saturating_sub(1)] {
        if !acc.is_empty() {
            acc.push('/');
        }
        acc.push_str(seg);
        out.push(acc.clone());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EntryType;

    fn entry(path: &str, folder: bool) -> Entry {
        Entry {
            entry_id: format!("id-{path}"),
            entry_name: path.rsplit('/').next().unwrap().to_string(),
            entry_path: path.to_string(),
            entry_type: if folder {
                EntryType::Folder
            } else {
                EntryType::Document
            },
            parent_folder_id: None,
            created_date: None,
            modified_date: Some("2026-08-28T20:11:00Z".into()),
            reading_date: None,
            file_size: Some(100),
            file_revision: Some("rev.1".into()),
            mime_type: None,
            title: None,
            total_page: None,
            is_new: None,
        }
    }

    #[test]
    fn remote_view_restricts_to_root() {
        let entries = vec![
            entry("Document/a.pdf", false),
            entry("Document/Papers", true),
            entry("Document/Papers/b.pdf", false),
            entry("Document/Note/n.pdf", false),
        ];
        let view = remote_view(&entries, "Document/Papers");
        assert_eq!(view.files.len(), 1);
        assert!(view.files.contains_key("b.pdf"));
        assert!(view.folders.is_empty());
    }

    #[test]
    fn nfd_local_names_match_nfc_keys() {
        // "ü" as NFD (u + combining diaeresis) must normalize to the same
        // map key as NFC (and, on Windows, the case-folded form).
        let nfd = "u\u{0308}ber.pdf";
        assert_eq!(norm_key(nfd), norm_key("über.pdf"));
    }

    #[test]
    fn escaping_is_reversible_via_recorded_mapping() {
        let escaped = escape_name_with("a:b?.pdf", &[':', '?']);
        assert_eq!(escaped, "a%3Ab%3F.pdf");
    }

    #[test]
    fn ancestors_are_shallowest_first() {
        assert_eq!(
            ancestors("a/b/c.pdf"),
            vec!["a".to_string(), "a/b".to_string()]
        );
        assert!(ancestors("c.pdf").is_empty());
    }

    #[test]
    fn walk_local_collects_pdfs_and_folders() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Papers/Deep")).unwrap();
        std::fs::write(dir.path().join("a.pdf"), b"x").unwrap();
        std::fs::write(dir.path().join("Papers/b.PDF"), b"xy").unwrap();
        std::fs::write(dir.path().join("Papers/notes.txt"), b"ignored").unwrap();
        std::fs::write(dir.path().join("c.pdf.part"), b"ignored").unwrap();

        let view = walk_local(dir.path(), &HashMap::new()).unwrap();
        assert_eq!(view.files.len(), 2);
        assert!(view.files.contains_key(&norm_key("a.pdf")));
        // Map keys are `norm_key` (case-folded on Windows); the stored
        // relpath keeps the on-disk spelling.
        let mixed = &view.files[&norm_key("Papers/b.PDF")];
        assert_eq!(mixed.size, 2);
        assert_eq!(mixed.relpath.to_ascii_lowercase(), "papers/b.pdf");
        assert_eq!(
            view.folders.keys().cloned().collect::<Vec<_>>(),
            vec![norm_key("Papers"), norm_key("Papers/Deep")]
        );
    }
}
