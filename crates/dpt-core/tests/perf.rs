//! Performance guards for the sync engine against large libraries
//! (NFR-PRF-4: a no-change sync of 1 000 files completes in < 10 s, with
//! metadata-only change detection).
//!
//! The in-memory fake device isolates engine overhead from device I/O: the
//! budgets asserted here are deliberately generous so debug-mode CI stays
//! reliable — regressions of an order of magnitude (accidental content
//! hashing, quadratic planning) still fail loudly.
//!
//! The 10 000-entry soak variant is `#[ignore]`d (it writes 10 000 files);
//! run it with `cargo test -p dpt-core --release -- --ignored perf`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{TimeZone, Utc};

use dpt_core::model::{Entry, EntryType};
use dpt_core::sync::{self, ApplyHooks, Checkpoint, SyncDevice, SyncMode, SyncPairConfig};
use dpt_core::Error;

// ---- read-only fake device --------------------------------------------------

enum Node {
    Folder,
    Doc { content: Vec<u8> },
}

/// Immutable in-memory device: enough of [`SyncDevice`] for listing and
/// downloading. Mutating operations are never planned in these scenarios.
struct PerfDevice {
    nodes: BTreeMap<String, Node>,
}

impl PerfDevice {
    /// Seeds `folders × docs_per_folder` documents under `Document/`.
    fn seeded(folders: usize, docs_per_folder: usize) -> Self {
        let mut nodes = BTreeMap::new();
        nodes.insert("Document".to_string(), Node::Folder);
        for f in 0..folders {
            nodes.insert(format!("Document/Folder {f:03}"), Node::Folder);
            for d in 0..docs_per_folder {
                nodes.insert(
                    format!("Document/Folder {f:03}/Paper {d:04}.pdf"),
                    Node::Doc {
                        content: format!("%PDF-fake {f}/{d}").into_bytes(),
                    },
                );
            }
        }
        Self { nodes }
    }

    fn to_entry(path: &str, node: &Node) -> Entry {
        let modified = Utc
            .with_ymd_and_hms(2026, 1, 1, 12, 0, 0)
            .unwrap()
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();
        let (entry_type, modified, size, revision) = match node {
            Node::Folder => (EntryType::Folder, None, None, None),
            Node::Doc { content } => (
                EntryType::Document,
                Some(modified),
                Some(content.len() as u64),
                Some(format!("{path}.1.0")),
            ),
        };
        Entry {
            entry_id: format!("id:{path}"),
            entry_name: path.rsplit('/').next().unwrap_or(path).to_string(),
            entry_path: path.to_string(),
            entry_type,
            parent_folder_id: None,
            created_date: None,
            modified_date: modified,
            reading_date: None,
            file_size: size,
            file_revision: revision,
            mime_type: None,
            title: None,
            total_page: None,
            is_new: None,
        }
    }
}

#[async_trait]
impl SyncDevice for PerfDevice {
    async fn sync_list_entries(&self) -> Result<Vec<Entry>, Error> {
        Ok(self
            .nodes
            .iter()
            .map(|(p, n)| Self::to_entry(p, n))
            .collect())
    }

    async fn sync_resolve_path(&self, path: &str) -> Result<Entry, Error> {
        self.nodes
            .get(path)
            .map(|n| Self::to_entry(path, n))
            .ok_or_else(|| Error::Api {
                status: 404,
                message: "not found".into(),
            })
    }

    async fn sync_download_to(&self, entry_id: &str, dest: &Path) -> Result<(), Error> {
        let path = entry_id.strip_prefix("id:").unwrap_or(entry_id);
        let Some(Node::Doc { content }) = self.nodes.get(path) else {
            return Err(Error::Api {
                status: 404,
                message: "document not found".into(),
            });
        };
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(dest, content)?;
        Ok(())
    }

    async fn sync_upload_new(
        &self,
        _parent_folder_id: &str,
        _file_name: &str,
        _local_path: &Path,
    ) -> Result<String, Error> {
        Err(Error::Protocol("not used in perf tests".into()))
    }

    async fn sync_upload_replace(
        &self,
        _document_id: &str,
        _file_name: &str,
        _local_path: &Path,
    ) -> Result<(), Error> {
        Err(Error::Protocol("not used in perf tests".into()))
    }

    async fn sync_create_folder(
        &self,
        _parent_folder_id: &str,
        _name: &str,
    ) -> Result<String, Error> {
        Err(Error::Protocol("not used in perf tests".into()))
    }

    async fn sync_delete_document(&self, _document_id: &str) -> Result<(), Error> {
        Err(Error::Protocol("not used in perf tests".into()))
    }

    async fn sync_delete_folder(&self, _folder_id: &str) -> Result<(), Error> {
        Err(Error::Protocol("not used in perf tests".into()))
    }
}

// ---- harness ----------------------------------------------------------------

struct Perf {
    device: PerfDevice,
    cfg: SyncPairConfig,
    cp_path: PathBuf,
    _local: tempfile::TempDir,
    _state: tempfile::TempDir,
}

fn setup(folders: usize, docs_per_folder: usize) -> Perf {
    let local = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    Perf {
        device: PerfDevice::seeded(folders, docs_per_folder),
        cfg: SyncPairConfig {
            id: "perf".into(),
            local_root: local.path().to_path_buf(),
            remote_root: "Document".into(),
            mode: SyncMode::TwoWay,
            filters: Vec::new(),
        },
        cp_path: state.path().join("checkpoint.json"),
        _local: local,
        _state: state,
    }
}

/// First run: mirrors everything down and establishes the checkpoint.
async fn establish_checkpoint(p: &Perf) {
    let cp = Checkpoint::new(&p.cfg.id, "serial", &p.cfg.remote_root);
    let snap = sync::take_snapshot(&p.device, &p.cfg, &cp).await.unwrap();
    let plan = sync::make_plan(&p.cfg, &cp, &snap).unwrap();
    let hooks = ApplyHooks {
        persist: None,
        progress: None,
        cancel: None,
    };
    let (cp, report) = sync::execute(&p.device, &p.cfg, &snap, plan.actions, cp, hooks).await;
    let (_done, failed, _skipped) = report.counts();
    assert_eq!(failed, 0, "seed run must succeed: {:?}", report.warnings);
    assert!(!report.aborted, "seed run must not abort");
    cp.save(&p.cp_path).unwrap();
}

/// Measures the no-change detection path (snapshot + plan) and asserts the
/// plan is empty and the elapsed time stays within `budget`.
async fn assert_no_change_run(p: &Perf, budget: Duration, label: &str) {
    let cp = Checkpoint::load(&p.cp_path).unwrap();
    let started = Instant::now();
    let snap = sync::take_snapshot(&p.device, &p.cfg, &cp).await.unwrap();
    let plan = sync::make_plan(&p.cfg, &cp, &snap).unwrap();
    let elapsed = started.elapsed();
    println!("{label}: no-change snapshot+plan took {elapsed:?}");
    assert!(
        plan.actions.is_empty(),
        "{label}: no-change plan must be empty, got {:?}",
        plan.summary
    );
    assert!(
        elapsed < budget,
        "{label}: no-change detection took {elapsed:?}, budget {budget:?} (NFR-PRF-4)"
    );
}

// ---- tests --------------------------------------------------------------------

/// NFR-PRF-4: a no-change sync of 1 000 files completes in < 10 s. The
/// in-memory engine part must stay far below that; device listing I/O
/// consumes the rest of the real-world budget.
#[tokio::test]
async fn no_change_sync_of_1000_files_stays_within_budget() {
    let p = setup(10, 100); // 1 000 documents in 10 folders
    establish_checkpoint(&p).await;
    assert_no_change_run(&p, Duration::from_secs(10), "1k").await;
}

/// Scaling guard for NFR-PRF-2/4 with a 10 000-entry library. Writes
/// 10 000 files; run explicitly:
/// `cargo test -p dpt-core --release -- --ignored perf`.
#[tokio::test]
#[ignore = "large-library soak; run with --ignored"]
async fn no_change_sync_of_10000_files_stays_within_budget() {
    let p = setup(100, 100); // 10 000 documents in 100 folders
    establish_checkpoint(&p).await;
    assert_no_change_run(&p, Duration::from_secs(30), "10k").await;
}
