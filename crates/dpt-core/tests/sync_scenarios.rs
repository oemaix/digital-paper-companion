//! Scenario test suite for the sync engine (docs/06 §10, NFR-QLT-3).
//!
//! Drives the full snapshot → plan → apply pipeline against an in-memory
//! fake device and a real temporary directory, covering the two-way
//! decision table (§5.1), conflict policy (§5.2), first-run matrix (§5.3),
//! folder rules (§5.4), mirror modes (§5.5), filters, interrupted-run
//! resume and precondition failures.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};

use dpt_core::model::{Entry, EntryType};
use dpt_core::sync::{
    self, Action, ApplyHooks, Checkpoint, Plan, RunReport, SyncDevice, SyncMode, SyncPairConfig,
};
use dpt_core::Error;

// ---- in-memory fake device ---------------------------------------------------

#[derive(Clone)]
enum Node {
    Folder {
        id: String,
    },
    Doc {
        id: String,
        content: Vec<u8>,
        modified: DateTime<Utc>,
        revision: u64,
    },
}

struct FakeState {
    nodes: BTreeMap<String, Node>,
    next_id: u64,
    clock: DateTime<Utc>,
    network_down: bool,
    /// Simulates connection loss mid-run: listing still works (snapshot
    /// already happened), but transfers/mutations fail with a network error.
    fail_transfers: bool,
}

/// Minimal in-memory device implementing [`SyncDevice`] (docs/04 §7).
struct FakeDevice {
    state: Mutex<FakeState>,
}

impl FakeDevice {
    fn new() -> Self {
        let mut nodes = BTreeMap::new();
        nodes.insert("Document".to_string(), Node::Folder { id: "root".into() });
        Self {
            state: Mutex::new(FakeState {
                nodes,
                next_id: 1,
                clock: Utc::now(),
                network_down: false,
                fail_transfers: false,
            }),
        }
    }

    fn fresh_id(state: &mut FakeState) -> String {
        state.next_id += 1;
        format!("fake-{}", state.next_id)
    }

    fn mkdir(&self, path: &str) {
        let mut st = self.state.lock().unwrap();
        let id = Self::fresh_id(&mut st);
        st.nodes.insert(path.to_string(), Node::Folder { id });
    }

    fn put_doc(&self, path: &str, content: &[u8]) {
        let mut st = self.state.lock().unwrap();
        let clock = st.clock;
        match st.nodes.get_mut(path) {
            Some(Node::Doc {
                content: c,
                modified,
                revision,
                ..
            }) => {
                *c = content.to_vec();
                *modified = clock;
                *revision += 1;
            }
            _ => {
                let id = Self::fresh_id(&mut st);
                st.nodes.insert(
                    path.to_string(),
                    Node::Doc {
                        id,
                        content: content.to_vec(),
                        modified: clock,
                        revision: 1,
                    },
                );
            }
        }
    }

    fn delete(&self, path: &str) {
        let mut st = self.state.lock().unwrap();
        let prefix = format!("{path}/");
        st.nodes.retain(|p, _| p != path && !p.starts_with(&prefix));
    }

    fn tick(&self, seconds: i64) {
        self.state.lock().unwrap().clock += Duration::seconds(seconds);
    }

    fn set_clock(&self, at: DateTime<Utc>) {
        self.state.lock().unwrap().clock = at;
    }

    fn set_fail_transfers(&self, fail: bool) {
        self.state.lock().unwrap().fail_transfers = fail;
    }

    fn check_transfer(&self) -> Result<(), Error> {
        let st = self.state.lock().unwrap();
        if st.network_down || st.fail_transfers {
            Err(Error::Network("connection lost".into()))
        } else {
            Ok(())
        }
    }

    fn content(&self, path: &str) -> Option<Vec<u8>> {
        match self.state.lock().unwrap().nodes.get(path) {
            Some(Node::Doc { content, .. }) => Some(content.clone()),
            _ => None,
        }
    }

    fn exists(&self, path: &str) -> bool {
        self.state.lock().unwrap().nodes.contains_key(path)
    }

    fn to_entry(path: &str, node: &Node) -> Entry {
        let name = path.rsplit('/').next().unwrap_or(path).to_string();
        let (entry_type, id, modified, size, revision) = match node {
            Node::Folder { id } => (EntryType::Folder, id.clone(), None, None, None),
            Node::Doc {
                id,
                content,
                modified,
                revision,
            } => (
                EntryType::Document,
                id.clone(),
                Some(modified.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
                Some(content.len() as u64),
                Some(format!("{id}.{revision}.0")),
            ),
        };
        Entry {
            entry_id: id,
            entry_name: name,
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

    fn check_network(&self) -> Result<(), Error> {
        if self.state.lock().unwrap().network_down {
            Err(Error::Network("connection lost".into()))
        } else {
            Ok(())
        }
    }

    fn path_of_id(&self, id: &str) -> Option<String> {
        self.state
            .lock()
            .unwrap()
            .nodes
            .iter()
            .find(|(_, n)| match n {
                Node::Folder { id: i } | Node::Doc { id: i, .. } => i == id,
            })
            .map(|(p, _)| p.clone())
    }
}

#[async_trait]
impl SyncDevice for FakeDevice {
    async fn sync_list_entries(&self) -> Result<Vec<Entry>, Error> {
        self.check_network()?;
        let st = self.state.lock().unwrap();
        Ok(st.nodes.iter().map(|(p, n)| Self::to_entry(p, n)).collect())
    }

    async fn sync_resolve_path(&self, path: &str) -> Result<Entry, Error> {
        self.check_network()?;
        let st = self.state.lock().unwrap();
        st.nodes
            .get(path)
            .map(|n| Self::to_entry(path, n))
            .ok_or_else(|| Error::Api {
                status: 404,
                message: "not found".into(),
            })
    }

    async fn sync_download_to(&self, entry_id: &str, dest: &Path) -> Result<(), Error> {
        self.check_transfer()?;
        let content = {
            let st = self.state.lock().unwrap();
            st.nodes
                .values()
                .find_map(|n| match n {
                    Node::Doc { id, content, .. } if id == entry_id => Some(content.clone()),
                    _ => None,
                })
                .ok_or_else(|| Error::Api {
                    status: 404,
                    message: "document not found".into(),
                })?
        };
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(dest, content)?;
        Ok(())
    }

    async fn sync_upload_new(
        &self,
        parent_folder_id: &str,
        file_name: &str,
        local_path: &Path,
    ) -> Result<String, Error> {
        self.check_transfer()?;
        let content = std::fs::read(local_path)?;
        let parent = self
            .path_of_id(parent_folder_id)
            .ok_or_else(|| Error::Api {
                status: 404,
                message: "parent folder not found".into(),
            })?;
        let mut st = self.state.lock().unwrap();
        let id = Self::fresh_id(&mut st);
        let clock = st.clock;
        st.nodes.insert(
            format!("{parent}/{file_name}"),
            Node::Doc {
                id: id.clone(),
                content,
                modified: clock,
                revision: 1,
            },
        );
        Ok(id)
    }

    async fn sync_upload_replace(
        &self,
        document_id: &str,
        _file_name: &str,
        local_path: &Path,
    ) -> Result<(), Error> {
        self.check_transfer()?;
        let new_content = std::fs::read(local_path)?;
        let mut st = self.state.lock().unwrap();
        let clock = st.clock;
        for node in st.nodes.values_mut() {
            if let Node::Doc {
                id,
                content,
                modified,
                revision,
            } = node
            {
                if id == document_id {
                    *content = new_content;
                    *modified = clock;
                    *revision += 1;
                    return Ok(());
                }
            }
        }
        Err(Error::Api {
            status: 404,
            message: "document not found".into(),
        })
    }

    async fn sync_create_folder(
        &self,
        parent_folder_id: &str,
        name: &str,
    ) -> Result<String, Error> {
        self.check_network()?;
        let parent = self
            .path_of_id(parent_folder_id)
            .ok_or_else(|| Error::Api {
                status: 404,
                message: "parent folder not found".into(),
            })?;
        let mut st = self.state.lock().unwrap();
        let id = Self::fresh_id(&mut st);
        st.nodes
            .insert(format!("{parent}/{name}"), Node::Folder { id: id.clone() });
        Ok(id)
    }

    async fn sync_delete_document(&self, document_id: &str) -> Result<(), Error> {
        self.check_network()?;
        let path = self.path_of_id(document_id).ok_or_else(|| Error::Api {
            status: 404,
            message: "document not found".into(),
        })?;
        self.state.lock().unwrap().nodes.remove(&path);
        Ok(())
    }

    async fn sync_delete_folder(&self, folder_id: &str) -> Result<(), Error> {
        self.check_network()?;
        let path = self.path_of_id(folder_id).ok_or_else(|| Error::Api {
            status: 404,
            message: "folder not found".into(),
        })?;
        self.delete(&path);
        Ok(())
    }
}

// ---- harness -----------------------------------------------------------------

struct Harness {
    device: FakeDevice,
    local: tempfile::TempDir,
    /// Kept for its `Drop` (holds the checkpoint file at `cp_path`).
    _state_dir: tempfile::TempDir,
    cfg: SyncPairConfig,
    cp_path: PathBuf,
}

impl Harness {
    fn new(mode: SyncMode) -> Self {
        let local = tempfile::tempdir().unwrap();
        let state_dir = tempfile::tempdir().unwrap();
        let cfg = SyncPairConfig {
            id: "pair-1".into(),
            local_root: local.path().to_path_buf(),
            remote_root: "Document".into(),
            mode,
            filters: Vec::new(),
        };
        let cp_path = state_dir.path().join("checkpoint.json");
        Self {
            device: FakeDevice::new(),
            local,
            _state_dir: state_dir,
            cfg,
            cp_path,
        }
    }

    fn checkpoint(&self) -> Checkpoint {
        Checkpoint::load(&self.cp_path)
            .unwrap_or_else(|| Checkpoint::new(&self.cfg.id, "serial", &self.cfg.remote_root))
    }

    fn local_path(&self, relpath: &str) -> PathBuf {
        let mut p = self.local.path().to_path_buf();
        for seg in relpath.split('/') {
            p.push(seg);
        }
        p
    }

    fn write_local(&self, relpath: &str, content: &[u8]) {
        let p = self.local_path(relpath);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    fn mkdir_local(&self, relpath: &str) {
        std::fs::create_dir_all(self.local_path(relpath)).unwrap();
    }

    async fn plan(&self) -> Plan {
        let cp = self.checkpoint();
        let snap = sync::take_snapshot(&self.device, &self.cfg, &cp)
            .await
            .unwrap();
        sync::make_plan(&self.cfg, &cp, &snap).unwrap()
    }

    async fn run(&self) -> (Plan, RunReport) {
        self.run_with_cancel(None).await
    }

    async fn run_with_cancel(&self, cancel_after: Option<usize>) -> (Plan, RunReport) {
        let cp = self.checkpoint();
        let snap = sync::take_snapshot(&self.device, &self.cfg, &cp)
            .await
            .unwrap();
        let plan = sync::make_plan(&self.cfg, &cp, &snap).unwrap();

        let cp_path = self.cp_path.clone();
        let persist = move |c: &Checkpoint| {
            c.save(&cp_path).unwrap();
        };
        let cancel = AtomicBool::new(false);
        let started = AtomicUsize::new(0);
        let progress = |_e: sync::ProgressEvent| {
            if let Some(n) = cancel_after {
                if started.fetch_add(1, Ordering::SeqCst) + 1 >= n {
                    cancel.store(true, Ordering::SeqCst);
                }
            }
        };
        let hooks = ApplyHooks {
            persist: Some(&persist),
            progress: Some(&progress),
            cancel: Some(&cancel),
        };
        let (_cp, report) = sync::execute(
            &self.device,
            &self.cfg,
            &snap,
            plan.actions.clone(),
            cp,
            hooks,
        )
        .await;
        (plan, report)
    }
}

fn kinds(plan: &Plan) -> Vec<&'static str> {
    plan.actions
        .iter()
        .map(|a| match a {
            Action::CreateLocalDir { .. } => "create_local_dir",
            Action::CreateRemoteDir { .. } => "create_remote_dir",
            Action::Upload { .. } => "upload",
            Action::Download { .. } => "download",
            Action::ConflictResolve { .. } => "conflict",
            Action::DeleteLocal { .. } => "delete_local",
            Action::DeleteRemote { .. } => "delete_remote",
            Action::DeleteLocalDir { .. } => "delete_local_dir",
            Action::DeleteRemoteDir { .. } => "delete_remote_dir",
            Action::Adopt { .. } => "adopt",
            Action::Forget { .. } => "forget",
        })
        .collect()
}

fn assert_all_done(report: &RunReport) {
    for r in &report.results {
        assert!(
            r.status == sync::apply::ActionStatus::Done,
            "action {:?} not done: {:?}",
            r.action,
            r.message
        );
    }
}

// ---- first run (§5.3) ---------------------------------------------------------

#[tokio::test]
async fn first_run_upload_download_adopt() {
    let h = Harness::new(SyncMode::TwoWay);
    h.write_local("only-local.pdf", b"local");
    h.device.put_doc("Document/only-remote.pdf", b"remote");
    h.write_local("both.pdf", b"same!");
    h.device.put_doc("Document/both.pdf", b"12345"); // equal size

    let (plan, report) = h.run().await;
    assert_all_done(&report);
    let mut k = kinds(&plan);
    k.sort();
    assert_eq!(k, vec!["adopt", "download", "upload"]);

    assert!(h.device.exists("Document/only-local.pdf"));
    assert_eq!(
        std::fs::read(h.local_path("only-remote.pdf")).unwrap(),
        b"remote"
    );
    // Adopt: no transfer — both keep their (same-sized) content.
    assert_eq!(h.device.content("Document/both.pdf").unwrap(), b"12345");
    assert_eq!(std::fs::read(h.local_path("both.pdf")).unwrap(), b"same!");

    // Idempotent: an immediate second run plans nothing.
    let plan2 = h.plan().await;
    assert!(
        plan2.actions.is_empty(),
        "expected empty plan, got {:?}",
        plan2.actions
    );
}

#[tokio::test]
async fn first_run_size_mismatch_is_conflict_with_surviving_copy() {
    let h = Harness::new(SyncMode::TwoWay);
    h.write_local("d.pdf", b"local content");
    h.device.set_clock(Utc::now() + Duration::hours(1)); // remote newer
    h.device.put_doc("Document/d.pdf", b"remote!");

    let (plan, report) = h.run().await;
    assert_all_done(&report);
    assert_eq!(kinds(&plan), vec!["conflict"]);
    assert_eq!(report.conflicts.len(), 1);

    // Remote won: canonical file has remote content; the local version
    // survives as a conflict copy.
    assert_eq!(std::fs::read(h.local_path("d.pdf")).unwrap(), b"remote!");
    let copy = &report.conflicts[0];
    assert!(copy.contains("(conflict"), "copy name: {copy}");
    assert_eq!(std::fs::read(h.local_path(copy)).unwrap(), b"local content");

    // The conflict copy is excluded from future runs (§5.2.4).
    let plan2 = h.plan().await;
    assert!(plan2.actions.is_empty(), "got {:?}", plan2.actions);
}

// ---- two-way decision table (§5.1) --------------------------------------------

#[tokio::test]
async fn remote_change_downloads() {
    let h = Harness::new(SyncMode::TwoWay);
    h.device.put_doc("Document/a.pdf", b"v1");
    let (_, r) = h.run().await;
    assert_all_done(&r);

    h.device.tick(60);
    h.device.put_doc("Document/a.pdf", b"v2"); // same size, new revision

    let (plan, report) = h.run().await;
    assert_all_done(&report);
    assert_eq!(kinds(&plan), vec!["download"]);
    assert_eq!(std::fs::read(h.local_path("a.pdf")).unwrap(), b"v2");
}

/// Downloads carry the device's modification time as the local mtime, so
/// conflict arbitration compares edit times, not sync times — important when
/// several devices share one local folder (multi-device hub model).
#[tokio::test]
async fn download_preserves_device_edit_time() {
    let h = Harness::new(SyncMode::TwoWay);
    let edit_time = Utc::now() - Duration::hours(6);
    h.device.set_clock(edit_time);
    h.device.put_doc("Document/a.pdf", b"v1");

    let (_, report) = h.run().await;
    assert_all_done(&report);

    let mtime: DateTime<Utc> = std::fs::metadata(h.local_path("a.pdf"))
        .unwrap()
        .modified()
        .unwrap()
        .into();
    assert!(
        (mtime - edit_time).num_seconds().abs() <= 2,
        "local mtime {mtime} should match the device edit time {edit_time}"
    );
}

#[tokio::test]
async fn local_change_uploads() {
    let h = Harness::new(SyncMode::TwoWay);
    h.device.put_doc("Document/a.pdf", b"v1");
    let (_, r) = h.run().await;
    assert_all_done(&r);

    h.write_local("a.pdf", b"local v2 longer");

    let (plan, report) = h.run().await;
    assert_all_done(&report);
    assert_eq!(kinds(&plan), vec!["upload"]);
    assert_eq!(
        h.device.content("Document/a.pdf").unwrap(),
        b"local v2 longer"
    );
    // Uploaded metadata is fresh → next plan is empty (no false "changed").
    let plan2 = h.plan().await;
    assert!(plan2.actions.is_empty(), "got {:?}", plan2.actions);
}

#[tokio::test]
async fn remote_delete_removes_local() {
    let h = Harness::new(SyncMode::TwoWay);
    h.device.put_doc("Document/a.pdf", b"v1");
    let (_, r) = h.run().await;
    assert_all_done(&r);

    h.device.delete("Document/a.pdf");
    let (plan, report) = h.run().await;
    assert_all_done(&report);
    assert_eq!(kinds(&plan), vec!["delete_local"]);
    assert!(!h.local_path("a.pdf").exists());
}

#[tokio::test]
async fn local_delete_removes_remote() {
    let h = Harness::new(SyncMode::TwoWay);
    h.write_local("a.pdf", b"v1");
    let (_, r) = h.run().await;
    assert_all_done(&r);

    std::fs::remove_file(h.local_path("a.pdf")).unwrap();
    let (plan, report) = h.run().await;
    assert_all_done(&report);
    assert_eq!(kinds(&plan), vec!["delete_remote"]);
    assert!(!h.device.exists("Document/a.pdf"));
}

#[tokio::test]
async fn local_change_survives_remote_delete() {
    // L=Changed, R=Deleted → upload, keep local (§5.1).
    let h = Harness::new(SyncMode::TwoWay);
    h.write_local("a.pdf", b"v1");
    let (_, r) = h.run().await;
    assert_all_done(&r);

    h.device.delete("Document/a.pdf");
    h.write_local("a.pdf", b"v2 with edits");

    let (plan, report) = h.run().await;
    assert_all_done(&report);
    assert_eq!(kinds(&plan), vec!["upload"]);
    assert_eq!(
        h.device.content("Document/a.pdf").unwrap(),
        b"v2 with edits"
    );
}

#[tokio::test]
async fn remote_change_survives_local_delete() {
    // L=Deleted, R=Changed → download, no delete (§5.1).
    let h = Harness::new(SyncMode::TwoWay);
    h.device.put_doc("Document/a.pdf", b"v1");
    let (_, r) = h.run().await;
    assert_all_done(&r);

    std::fs::remove_file(h.local_path("a.pdf")).unwrap();
    h.device.tick(60);
    h.device.put_doc("Document/a.pdf", b"v2 annotated");

    let (plan, report) = h.run().await;
    assert_all_done(&report);
    assert_eq!(kinds(&plan), vec!["download"]);
    assert_eq!(
        std::fs::read(h.local_path("a.pdf")).unwrap(),
        b"v2 annotated"
    );
}

#[tokio::test]
async fn both_deleted_drops_from_checkpoint() {
    let h = Harness::new(SyncMode::TwoWay);
    h.write_local("a.pdf", b"v1");
    let (_, r) = h.run().await;
    assert_all_done(&r);

    std::fs::remove_file(h.local_path("a.pdf")).unwrap();
    h.device.delete("Document/a.pdf");

    let (plan, report) = h.run().await;
    assert_all_done(&report);
    assert_eq!(kinds(&plan), vec!["forget"]);
    assert!(h.checkpoint().entries.is_empty());
}

// ---- conflicts (§5.2) ----------------------------------------------------------

#[tokio::test]
async fn both_changed_remote_newer_wins_local_kept() {
    let h = Harness::new(SyncMode::TwoWay);
    h.device.put_doc("Document/a.pdf", b"base");
    let (_, r) = h.run().await;
    assert_all_done(&r);

    h.write_local("a.pdf", b"local edit");
    h.device.set_clock(Utc::now() + Duration::hours(2));
    h.device.put_doc("Document/a.pdf", b"remote edit, newer");

    let (plan, report) = h.run().await;
    assert_all_done(&report);
    assert_eq!(kinds(&plan), vec!["conflict"]);
    assert_eq!(
        std::fs::read(h.local_path("a.pdf")).unwrap(),
        b"remote edit, newer"
    );
    let copy = &report.conflicts[0];
    assert_eq!(std::fs::read(h.local_path(copy)).unwrap(), b"local edit");
    // No version was discarded.
}

#[tokio::test]
async fn both_changed_local_newer_wins_remote_kept() {
    let h = Harness::new(SyncMode::TwoWay);
    h.device.put_doc("Document/a.pdf", b"base");
    let (_, r) = h.run().await;
    assert_all_done(&r);

    h.device.set_clock(Utc::now() - Duration::hours(2));
    h.device.put_doc("Document/a.pdf", b"remote edit, older");
    h.write_local("a.pdf", b"local edit, newer");

    let (plan, report) = h.run().await;
    assert_all_done(&report);
    assert_eq!(kinds(&plan), vec!["conflict"]);
    // Local content became canonical on both sides.
    assert_eq!(
        h.device.content("Document/a.pdf").unwrap(),
        b"local edit, newer"
    );
    assert_eq!(
        std::fs::read(h.local_path("a.pdf")).unwrap(),
        b"local edit, newer"
    );
    // The remote version survives as a local conflict copy.
    let copy = &report.conflicts[0];
    assert_eq!(
        std::fs::read(h.local_path(copy)).unwrap(),
        b"remote edit, older"
    );
}

// ---- folders (§5.4) ------------------------------------------------------------

#[tokio::test]
async fn empty_folders_sync_both_ways() {
    let h = Harness::new(SyncMode::TwoWay);
    h.mkdir_local("LocalDir");
    h.device.mkdir("Document/RemoteDir");

    let (plan, report) = h.run().await;
    assert_all_done(&report);
    let mut k = kinds(&plan);
    k.sort();
    assert_eq!(k, vec!["create_local_dir", "create_remote_dir"]);
    assert!(h.device.exists("Document/LocalDir"));
    assert!(h.local_path("RemoteDir").is_dir());
}

#[tokio::test]
async fn folder_deleted_remotely_is_drained_and_removed_locally() {
    let h = Harness::new(SyncMode::TwoWay);
    h.device.mkdir("Document/Papers");
    h.device.put_doc("Document/Papers/a.pdf", b"a");
    h.device.put_doc("Document/Papers/b.pdf", b"b");
    let (_, r) = h.run().await;
    assert_all_done(&r);

    h.device.delete("Document/Papers");
    let (plan, report) = h.run().await;
    assert_all_done(&report);
    let mut k = kinds(&plan);
    k.sort();
    assert_eq!(k, vec!["delete_local", "delete_local", "delete_local_dir"]);
    assert!(!h.local_path("Papers").exists());
}

#[tokio::test]
async fn local_folder_with_non_pdf_content_is_left_in_place() {
    let h = Harness::new(SyncMode::TwoWay);
    h.device.mkdir("Document/Papers");
    h.device.put_doc("Document/Papers/a.pdf", b"a");
    let (_, r) = h.run().await;
    assert_all_done(&r);

    // A non-PDF file lives in the synced folder.
    h.write_local("Papers/notes.txt", b"my notes");
    h.device.delete("Document/Papers");

    let (plan, report) = h.run().await;
    assert_all_done(&report);
    // The PDF is deleted, the folder is NOT (non-PDF content).
    assert_eq!(kinds(&plan), vec!["delete_local"]);
    assert!(h.local_path("Papers/notes.txt").exists());
}

#[tokio::test]
async fn remote_folder_with_non_pdf_document_is_left_in_place() {
    let h = Harness::new(SyncMode::TwoWay);
    h.device.mkdir("Document/Papers");
    h.device.put_doc("Document/Papers/a.pdf", b"a");
    let (_, r) = h.run().await;
    assert_all_done(&r);

    // The device holds a non-PDF document the engine must not destroy.
    h.device.put_doc("Document/Papers/scan.png", b"img");
    std::fs::remove_file(h.local_path("Papers/a.pdf")).unwrap();
    std::fs::remove_dir(h.local_path("Papers")).unwrap();

    let (plan, report) = h.run().await;
    assert_all_done(&report);
    assert_eq!(kinds(&plan), vec!["delete_remote"]);
    assert!(h.device.exists("Document/Papers/scan.png"));
    assert!(h.device.exists("Document/Papers"));
}

#[tokio::test]
async fn folder_with_conflict_is_not_deleted() {
    let h = Harness::new(SyncMode::TwoWay);
    h.device.mkdir("Document/Papers");
    h.device.put_doc("Document/Papers/a.pdf", b"base");
    h.device.put_doc("Document/Papers/b.pdf", b"b");
    let (_, r) = h.run().await;
    assert_all_done(&r);

    // Local edit → conflict copy artifact lives under Papers/.
    h.write_local("Papers/a.pdf", b"local edit");
    h.device.set_clock(Utc::now() + Duration::hours(1));
    h.device.put_doc("Document/Papers/a.pdf", b"remote edit");
    let (_, r2) = h.run().await;
    assert_all_done(&r2);
    assert_eq!(r2.conflicts.len(), 1);

    // Now the folder is deleted remotely; the conflict copy must survive
    // and thus the local folder must stay.
    h.device.delete("Document/Papers");
    let (plan3, r3) = h.run().await;
    assert_all_done(&r3);
    assert!(
        !plan3
            .actions
            .iter()
            .any(|a| a.deletes_local() && matches!(a, Action::DeleteLocalDir { .. })),
        "folder with conflict copy must not be deleted: {:?}",
        plan3.actions
    );
    assert!(h.local_path("Papers").is_dir());
}

// ---- filters (FR-SYN-8) --------------------------------------------------------

#[tokio::test]
async fn filters_exclude_subtree_from_all_views() {
    let mut h = Harness::new(SyncMode::TwoWay);
    h.cfg.filters = vec!["Note".into()];
    h.device.mkdir("Document/Note");
    h.device.put_doc("Document/Note/n.pdf", b"note");
    h.device.put_doc("Document/a.pdf", b"a");

    let (plan, report) = h.run().await;
    assert_all_done(&report);
    assert_eq!(kinds(&plan), vec!["download"]);
    assert!(!h.local_path("Note").exists());
    assert!(h.local_path("a.pdf").exists());
}

// ---- mirror modes (§5.5) -------------------------------------------------------

#[tokio::test]
async fn mirror_to_local_restores_device_state_and_keeps_copies() {
    let h = Harness::new(SyncMode::MirrorToLocal);
    h.device.put_doc("Document/a.pdf", b"device a");
    let (_, r) = h.run().await;
    assert_all_done(&r);

    // Local mutations: a changed file, a new extra file, plus an extra
    // unchanged-from-device file is impossible — extras are always "new".
    h.write_local("a.pdf", b"local edit of a");
    h.write_local("extra.pdf", b"never synced");

    let (plan, report) = h.run().await;
    assert_all_done(&report);
    let mut k = kinds(&plan);
    k.sort();
    assert_eq!(k, vec!["conflict", "delete_local"]);

    // Device state restored; both local versions survive as copies.
    assert_eq!(std::fs::read(h.local_path("a.pdf")).unwrap(), b"device a");
    assert_eq!(report.conflicts.len(), 2);
    for copy in &report.conflicts {
        assert!(h.local_path(copy).exists(), "missing copy {copy}");
    }
    assert!(h.device.exists("Document/a.pdf"));
    assert!(!h.device.exists("Document/extra.pdf"));
}

#[tokio::test]
async fn mirror_to_remote_overwrites_device_and_keeps_copies() {
    let h = Harness::new(SyncMode::MirrorToRemote);
    h.write_local("a.pdf", b"pc a");
    let (_, r) = h.run().await;
    assert_all_done(&r);

    // Device mutations: annotation on a.pdf, extra document.
    h.device.tick(60);
    h.device.put_doc("Document/a.pdf", b"device edit");
    h.device.put_doc("Document/extra.pdf", b"device only");

    let (plan, report) = h.run().await;
    assert_all_done(&report);
    let mut k = kinds(&plan);
    k.sort();
    assert_eq!(k, vec!["conflict", "delete_remote"]);

    // Device mirrors the computer again.
    assert_eq!(h.device.content("Document/a.pdf").unwrap(), b"pc a");
    assert!(!h.device.exists("Document/extra.pdf"));
    // The device-side versions survive as local conflict copies.
    assert_eq!(report.conflicts.len(), 2);
    for copy in &report.conflicts {
        assert!(h.local_path(copy).exists(), "missing copy {copy}");
    }
}

// ---- resilience (§6, FR-SYN-9) -------------------------------------------------

#[tokio::test]
async fn interrupted_run_resumes_remaining_work() {
    let h = Harness::new(SyncMode::TwoWay);
    for i in 0..10 {
        h.device.put_doc(&format!("Document/f{i}.pdf"), b"content");
    }

    // Cancel after a few actions have started.
    let (plan, report) = h.run_with_cancel(Some(3)).await;
    assert_eq!(plan.actions.len(), 10);
    assert!(report.cancelled);
    let (done, _, skipped) = report.counts();
    assert!(done > 0 && skipped > 0, "done={done} skipped={skipped}");

    // Second run: only the remaining files are planned; afterwards clean.
    let (plan2, report2) = h.run().await;
    assert_all_done(&report2);
    assert_eq!(plan2.actions.len(), 10 - done);
    let plan3 = h.plan().await;
    assert!(plan3.actions.is_empty());
}

#[tokio::test]
async fn connection_loss_aborts_without_corrupting_checkpoint() {
    let h = Harness::new(SyncMode::TwoWay);
    h.device.put_doc("Document/a.pdf", b"a");
    let (_, r) = h.run().await;
    assert_all_done(&r);

    h.write_local("b.pdf", b"b");
    h.device.set_fail_transfers(true);
    let (_, report) = h.run_with_cancel(None).await;
    assert!(report.aborted);

    h.device.set_fail_transfers(false);
    let (plan2, report2) = h.run().await;
    assert_all_done(&report2);
    assert_eq!(kinds(&plan2), vec!["upload"]);
    assert!(h.device.exists("Document/b.pdf"));
}

#[tokio::test]
async fn missing_local_root_aborts_the_run() {
    let h = Harness::new(SyncMode::TwoWay);
    let cfg = SyncPairConfig {
        local_root: PathBuf::from("/nonexistent/sync/root"),
        ..h.cfg.clone()
    };
    let cp = h.checkpoint();
    let Err(err) = sync::take_snapshot(&h.device, &cfg, &cp).await else {
        panic!("expected the run to abort on a missing local root");
    };
    assert!(matches!(err, Error::Sync(_)), "got {err:?}");
}

// ---- unicode & subtree roots ---------------------------------------------------

#[tokio::test]
async fn nfd_local_name_matches_nfc_remote_name() {
    let h = Harness::new(SyncMode::TwoWay);
    // Local file written with an NFD name (as a macOS filesystem would
    // return); the device stores NFC. Same size → adopt, no duplicate.
    h.write_local("u\u{0308}ber.pdf", b"12345");
    h.device.put_doc("Document/\u{00FC}ber.pdf", b"same!");

    let (plan, report) = h.run().await;
    assert_all_done(&report);
    assert_eq!(kinds(&plan), vec!["adopt"]);
}

#[tokio::test]
async fn remote_subtree_root_is_created_on_demand() {
    let h = Harness::new(SyncMode::TwoWay);
    let cfg = SyncPairConfig {
        remote_root: "Document/Sync/Inbox".into(),
        ..h.cfg.clone()
    };
    h.write_local("a.pdf", b"a");

    let cp = Checkpoint::new(&cfg.id, "serial", &cfg.remote_root);
    let snap = sync::take_snapshot(&h.device, &cfg, &cp).await.unwrap();
    let plan = sync::make_plan(&cfg, &cp, &snap).unwrap();
    let (_cp2, report) = sync::execute(
        &h.device,
        &cfg,
        &snap,
        plan.actions.clone(),
        cp,
        ApplyHooks::default(),
    )
    .await;
    assert_all_done(&report);
    assert!(h.device.exists("Document/Sync/Inbox/a.pdf"));
}

// ---- checkpoint corruption (§7) ------------------------------------------------

#[tokio::test]
async fn corrupt_checkpoint_falls_back_to_first_run_semantics() {
    let h = Harness::new(SyncMode::TwoWay);
    h.write_local("a.pdf", b"12345");
    h.device.put_doc("Document/a.pdf", b"same!");
    let (_, r) = h.run().await;
    assert_all_done(&r);

    // Corrupt the checkpoint file.
    std::fs::write(&h.cp_path, b"garbage{{{").unwrap();

    // First-run semantics: equal sizes adopt, nothing is deleted.
    let (plan, report) = h.run().await;
    assert_all_done(&report);
    assert_eq!(kinds(&plan), vec!["adopt"]);
    assert!(h.cp_path.with_extension("corrupt").exists());
}

// ---- mass-deletion guard input (§5.6) -------------------------------------------

#[tokio::test]
async fn plan_summary_counts_deletions_per_side() {
    let h = Harness::new(SyncMode::TwoWay);
    for i in 0..12 {
        h.device.put_doc(&format!("Document/f{i}.pdf"), b"x");
    }
    let (_, r) = h.run().await;
    assert_all_done(&r);

    for i in 0..12 {
        h.device.delete(&format!("Document/f{i}.pdf"));
    }
    let plan = h.plan().await;
    assert_eq!(plan.summary.delete_local, 12);
    assert_eq!(plan.summary.delete_remote, 0);
}
