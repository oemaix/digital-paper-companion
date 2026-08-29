//! Apply phase (docs/06 §6): execute planned actions.
//!
//! Order: create dirs → transfers (interleaved, concurrency 2) → file
//! deletions → folder deletions (deepest first) → checkpoint-only actions.
//! Downloads go via `*.part` + atomic rename (NFR-REL-2); the checkpoint is
//! advanced per completed action and flushed periodically so an interrupted
//! run resumes cleanly (FR-SYN-9). Per-action failures don't abort the run
//! (except connection loss); they are collected and reported.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

use futures_util::StreamExt;
use serde::Serialize;

use super::checkpoint::{
    Checkpoint, CheckpointEntry, LocalCheck, RemoteCheck, FLAG_CONFLICT_COPY, FLAG_NAME_ESCAPED,
};
use super::device::SyncDevice;
use super::plan::{Action, Side};
use super::snapshot::{escape_local_relpath, norm_key};
use super::{Snapshot, SyncPairConfig};
use crate::Error;

/// Transfers run interleaved with this concurrency (device-friendly, §6).
const TRANSFER_CONCURRENCY: usize = 2;
/// The checkpoint file is flushed after this many completed actions.
const PERSIST_EVERY: usize = 10;

/// Live progress of a run, emitted before each action starts.
#[derive(Debug, Clone, Serialize)]
pub struct ProgressEvent {
    pub done: usize,
    pub total: usize,
    /// Human-readable description of the action now starting.
    pub current: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ActionStatus {
    Done,
    Failed,
    /// Not executed (cancelled run, aborted connection, or a folder that
    /// could not be removed because it still has non-synced content).
    Skipped,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActionResult {
    pub action: Action,
    pub status: ActionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Outcome of the apply phase (docs/06 §9 feeds from this).
#[derive(Debug, Clone, Default, Serialize)]
pub struct RunReport {
    pub results: Vec<ActionResult>,
    /// Relpaths of conflict copies created during this run (§5.2).
    pub conflicts: Vec<String>,
    /// True when the run stopped early (connection loss or cancellation).
    pub aborted: bool,
    pub cancelled: bool,
    pub warnings: Vec<String>,
}

impl RunReport {
    pub fn counts(&self) -> (usize, usize, usize) {
        let mut done = 0;
        let mut failed = 0;
        let mut skipped = 0;
        for r in &self.results {
            match r.status {
                ActionStatus::Done => done += 1,
                ActionStatus::Failed => failed += 1,
                ActionStatus::Skipped => skipped += 1,
            }
        }
        (done, failed, skipped)
    }
}

/// Caller-provided hooks for persistence, progress and cancellation.
#[derive(Default)]
pub struct ApplyHooks<'a> {
    /// Called with the current checkpoint every few completed actions and
    /// at the end; the caller writes it to disk (FR-SYN-9).
    pub persist: Option<&'a (dyn Fn(&Checkpoint) + Send + Sync)>,
    pub progress: Option<&'a (dyn Fn(ProgressEvent) + Send + Sync)>,
    /// Cooperative cancellation: set to `true` to stop after the actions
    /// currently in flight.
    pub cancel: Option<&'a AtomicBool>,
}

/// Checkpoint plus an index from normalized key to stored relpath, so
/// entries can be updated regardless of case/NFC differences.
struct CpState {
    cp: Checkpoint,
    index: HashMap<String, String>,
}

impl CpState {
    fn new(cp: Checkpoint) -> Self {
        let index = cp.key_index();
        Self { cp, index }
    }

    fn insert(&mut self, relpath: &str, entry: CheckpointEntry) {
        let key = norm_key(relpath);
        if let Some(old) = self.index.get(&key) {
            if old != relpath {
                self.cp.entries.remove(old);
            }
        }
        self.index.insert(key, relpath.to_string());
        self.cp.entries.insert(relpath.to_string(), entry);
    }

    fn remove(&mut self, relpath: &str) {
        let key = norm_key(relpath);
        if let Some(stored) = self.index.remove(&key) {
            self.cp.entries.remove(&stored);
        } else {
            self.cp.entries.remove(relpath);
        }
    }

    fn get(&self, relpath: &str) -> Option<&CheckpointEntry> {
        let key = norm_key(relpath);
        self.index.get(&key).and_then(|p| self.cp.entries.get(p))
    }
}

struct Ctx<'a, D: SyncDevice + ?Sized> {
    device: &'a D,
    cfg: &'a SyncPairConfig,
    snap: &'a Snapshot,
    cp: Mutex<CpState>,
    /// norm_key(folder relpath) → device folder id; grows as dirs are created.
    folder_ids: Mutex<HashMap<String, String>>,
    remote_root_id: Mutex<Option<String>>,
    completed: AtomicUsize,
    /// Set on connection loss; remaining actions are skipped (§6).
    abort: AtomicBool,
    /// Normalized keys of failed actions (blocks dependent folder deletes).
    failed_keys: Mutex<Vec<String>>,
    hooks: &'a ApplyHooks<'a>,
    total: usize,
}

impl<'a, D: SyncDevice + ?Sized> Ctx<'a, D> {
    fn cancelled(&self) -> bool {
        self.hooks
            .cancel
            .map(|c| c.load(Ordering::Relaxed))
            .unwrap_or(false)
    }

    fn stopping(&self) -> bool {
        self.cancelled() || self.abort.load(Ordering::Relaxed)
    }

    fn emit_progress(&self, current: Option<String>) {
        if let Some(cb) = self.hooks.progress {
            cb(ProgressEvent {
                done: self.completed.load(Ordering::Relaxed),
                total: self.total,
                current,
            });
        }
    }

    fn after_action(&self) {
        let done = self.completed.fetch_add(1, Ordering::Relaxed) + 1;
        if done.is_multiple_of(PERSIST_EVERY) {
            self.persist();
        }
    }

    fn persist(&self) {
        if let Some(persist) = self.hooks.persist {
            let cp = self.cp.lock().unwrap();
            persist(&cp.cp);
        }
    }

    /// Absolute local path + on-disk relpath for a canonical relpath,
    /// consulting the local view, then the checkpoint's recorded escape
    /// mapping, then computing a fresh escape (docs/06 §3.1).
    fn local_location(&self, relpath: &str) -> (PathBuf, String) {
        let key = norm_key(relpath);
        let disk = if let Some(f) = self.snap.local.files.get(&key) {
            f.disk_relpath.clone()
        } else if let Some(f) = self.snap.local.folders.get(&key) {
            f.disk_relpath.clone()
        } else if let Some(disk) = self
            .cp
            .lock()
            .unwrap()
            .get(relpath)
            .and_then(|e| e.local_relpath.clone())
        {
            disk
        } else {
            escape_local_relpath(relpath).unwrap_or_else(|| relpath.to_string())
        };
        let mut path = self.cfg.local_root.to_path_buf();
        for seg in disk.split('/') {
            path.push(seg);
        }
        (path, disk)
    }

    fn full_remote_path(&self, relpath: &str) -> String {
        format!("{}/{}", self.cfg.remote_root, relpath)
    }

    /// Entry id of the pair's remote root, creating missing levels on
    /// demand (one level at a time, protocol §7.3.6).
    async fn remote_root_id(&self) -> Result<String, Error> {
        if let Some(id) = self.remote_root_id.lock().unwrap().clone() {
            return Ok(id);
        }
        let segments: Vec<&str> = self.cfg.remote_root.split('/').collect();
        let mut path = segments[0].to_string();
        let root = self
            .device
            .sync_resolve_path(&path)
            .await
            .map_err(|_| Error::Sync(format!("device root folder '{path}' not found")))?;
        let mut id = root.entry_id;
        for seg in &segments[1..] {
            path = format!("{path}/{seg}");
            id = match self.device.sync_resolve_path(&path).await {
                Ok(e) if e.is_folder() => e.entry_id,
                Ok(_) => {
                    return Err(Error::Sync(format!(
                        "device path '{path}' is a document, not a folder"
                    )))
                }
                Err(_) => self.device.sync_create_folder(&id, seg).await?,
            };
        }
        *self.remote_root_id.lock().unwrap() = Some(id.clone());
        Ok(id)
    }

    /// Device folder id for the parent of `relpath`.
    async fn parent_folder_id(&self, relpath: &str) -> Result<String, Error> {
        match relpath.rsplit_once('/') {
            None => self.remote_root_id().await,
            Some((parent, _)) => self
                .folder_ids
                .lock()
                .unwrap()
                .get(&norm_key(parent))
                .cloned()
                .ok_or_else(|| {
                    Error::Sync(format!("device folder '{parent}' unexpectedly missing"))
                }),
        }
    }

    /// Fetches fresh remote metadata after an upload so the checkpoint
    /// records the device's own revision/date (otherwise the next run would
    /// misclassify the file as remotely changed).
    async fn record_uploaded(
        &self,
        relpath: &str,
        entry_id: String,
        local_path: &Path,
        disk_relpath: &str,
    ) -> Result<(), Error> {
        let remote = match self
            .device
            .sync_resolve_path(&self.full_remote_path(relpath))
            .await
        {
            Ok(e) => RemoteCheck {
                entry_id: e.entry_id,
                modified_date: e.modified_date,
                file_revision: e.file_revision,
                size: e.file_size,
                extra: Default::default(),
            },
            Err(_) => RemoteCheck {
                entry_id,
                ..Default::default()
            },
        };
        let mut entry = CheckpointEntry::file();
        entry.remote = Some(remote);
        entry.local = Some(local_check(local_path)?);
        set_disk_relpath(&mut entry, relpath, disk_relpath);
        self.cp.lock().unwrap().insert(relpath, entry);
        Ok(())
    }

    /// Renames an existing local file to its conflict-copy name and records
    /// the copy in the checkpoint as a local-only artifact (§5.2).
    fn preserve_local_as_conflict_copy(&self, relpath: &str) -> Result<String, Error> {
        let (path, _disk) = self.local_location(relpath);
        let copy_rel = unique_conflict_relpath(relpath, &self.cfg.local_root);
        let copy_disk = escape_local_relpath(&copy_rel).unwrap_or_else(|| copy_rel.clone());
        let mut copy_path = self.cfg.local_root.to_path_buf();
        for seg in copy_disk.split('/') {
            copy_path.push(seg);
        }
        std::fs::rename(&path, &copy_path)?;
        self.record_conflict_copy(&copy_rel, &copy_disk, &copy_path)?;
        Ok(copy_rel)
    }

    /// Downloads remote content to a conflict-copy file (§5.2 step 3).
    async fn fetch_remote_as_conflict_copy(
        &self,
        relpath: &str,
        entry_id: &str,
        remote_modified: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<String, Error> {
        let copy_rel = unique_conflict_relpath(relpath, &self.cfg.local_root);
        let copy_disk = escape_local_relpath(&copy_rel).unwrap_or_else(|| copy_rel.clone());
        let mut copy_path = self.cfg.local_root.to_path_buf();
        for seg in copy_disk.split('/') {
            copy_path.push(seg);
        }
        download_via_part(self.device, entry_id, &copy_path, remote_modified).await?;
        self.record_conflict_copy(&copy_rel, &copy_disk, &copy_path)?;
        Ok(copy_rel)
    }

    fn record_conflict_copy(
        &self,
        copy_rel: &str,
        copy_disk: &str,
        copy_path: &Path,
    ) -> Result<(), Error> {
        let mut entry = CheckpointEntry::file();
        entry.local = Some(local_check(copy_path)?);
        entry.set_flag(FLAG_CONFLICT_COPY);
        set_disk_relpath(&mut entry, copy_rel, copy_disk);
        self.cp.lock().unwrap().insert(copy_rel, entry);
        Ok(())
    }
}

fn set_disk_relpath(entry: &mut CheckpointEntry, relpath: &str, disk: &str) {
    if disk != relpath {
        entry.local_relpath = Some(disk.to_string());
        entry.set_flag(FLAG_NAME_ESCAPED);
    }
}

fn local_check(path: &Path) -> Result<LocalCheck, Error> {
    let meta = std::fs::metadata(path)?;
    let mtime: chrono::DateTime<chrono::Utc> = meta.modified()?.into();
    Ok(LocalCheck {
        mtime: mtime.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        size: meta.len(),
        extra: Default::default(),
    })
}

async fn download_via_part<D: SyncDevice + ?Sized>(
    device: &D,
    entry_id: &str,
    dest: &Path,
    remote_modified: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<(), Error> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file_name = dest
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let part = dest.with_file_name(format!("{file_name}.part"));
    match device.sync_download_to(entry_id, &part).await {
        Ok(()) => {
            std::fs::rename(&part, dest)?;
            // Preserve the device's modification time as the local mtime, so
            // conflict arbitration ("newer wins", §5.2) compares *edit* times
            // rather than sync times — with several devices sharing one local
            // folder, a file synced later must not beat a later edit.
            if let Some(modified) = remote_modified {
                if let Ok(f) = std::fs::OpenOptions::new().write(true).open(dest) {
                    let _ = f.set_modified(modified.into());
                }
            }
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            Err(e)
        }
    }
}

/// `Papers/a.pdf` → `Papers/a (conflict 2026-08-29 1712).pdf`, adding a
/// ` (n)` counter when the name is taken.
fn unique_conflict_relpath(relpath: &str, local_root: &Path) -> String {
    let (dir, name) = match relpath.rsplit_once('/') {
        Some((d, n)) => (format!("{d}/"), n),
        None => (String::new(), relpath),
    };
    let stem = name
        .strip_suffix(".pdf")
        .or_else(|| name.strip_suffix(".PDF"))
        .unwrap_or(name);
    let stamp = chrono::Local::now().format("%Y-%m-%d %H%M");
    let base = format!("{dir}{stem} (conflict {stamp})");
    for n in 0..1000 {
        let candidate = if n == 0 {
            format!("{base}.pdf")
        } else {
            format!("{base} ({n}).pdf")
        };
        let disk = escape_local_relpath(&candidate).unwrap_or_else(|| candidate.clone());
        let mut path = local_root.to_path_buf();
        for seg in disk.split('/') {
            path.push(seg);
        }
        if !path.exists() {
            return candidate;
        }
    }
    format!("{base}.pdf")
}

/// Executes one action. Returns the relpath of a conflict copy if one was
/// created.
async fn run_action<D: SyncDevice + ?Sized>(
    ctx: &Ctx<'_, D>,
    action: &Action,
) -> Result<Option<String>, Error> {
    match action {
        Action::CreateLocalDir { relpath } => {
            let (path, disk) = ctx.local_location(relpath);
            std::fs::create_dir_all(&path)?;
            let mut entry = CheckpointEntry::folder();
            set_disk_relpath(&mut entry, relpath, &disk);
            ctx.cp.lock().unwrap().insert(relpath, entry);
            Ok(None)
        }

        Action::CreateRemoteDir { relpath } => {
            let key = norm_key(relpath);
            let already = ctx.folder_ids.lock().unwrap().contains_key(&key);
            if !already {
                let parent_id = ctx.parent_folder_id(relpath).await?;
                let name = relpath.rsplit('/').next().unwrap_or(relpath);
                let id = ctx.device.sync_create_folder(&parent_id, name).await?;
                ctx.folder_ids.lock().unwrap().insert(key, id);
            }
            ctx.cp
                .lock()
                .unwrap()
                .insert(relpath, CheckpointEntry::folder());
            Ok(None)
        }

        Action::Upload { relpath } => {
            let key = norm_key(relpath);
            let (path, disk) = ctx.local_location(relpath);
            let name = relpath.rsplit('/').next().unwrap_or(relpath).to_string();
            let entry_id = match ctx.snap.remote.files.get(&key) {
                Some(rf) => {
                    ctx.device
                        .sync_upload_replace(&rf.entry_id, &name, &path)
                        .await?;
                    rf.entry_id.clone()
                }
                None => {
                    let parent_id = ctx.parent_folder_id(relpath).await?;
                    ctx.device.sync_upload_new(&parent_id, &name, &path).await?
                }
            };
            ctx.record_uploaded(relpath, entry_id, &path, &disk).await?;
            Ok(None)
        }

        Action::Download { relpath } => {
            let key = norm_key(relpath);
            let rf = ctx
                .snap
                .remote
                .files
                .get(&key)
                .ok_or_else(|| Error::Sync(format!("remote file '{relpath}' vanished")))?;
            let (path, disk) = ctx.local_location(relpath);
            download_via_part(ctx.device, &rf.entry_id, &path, rf.modified).await?;
            let mut entry = CheckpointEntry::file();
            entry.remote = Some(RemoteCheck {
                entry_id: rf.entry_id.clone(),
                modified_date: rf.modified_date.clone(),
                file_revision: rf.revision.clone(),
                size: rf.size,
                extra: Default::default(),
            });
            entry.local = Some(local_check(&path)?);
            set_disk_relpath(&mut entry, relpath, &disk);
            ctx.cp.lock().unwrap().insert(relpath, entry);
            Ok(None)
        }

        Action::ConflictResolve { relpath, winner } => {
            let key = norm_key(relpath);
            let rf = ctx
                .snap
                .remote
                .files
                .get(&key)
                .ok_or_else(|| Error::Sync(format!("remote file '{relpath}' vanished")))?
                .clone();
            match winner {
                Side::Remote => {
                    // Local loses: rename local to the conflict copy, then
                    // download the remote content to the canonical path.
                    let copy = ctx.preserve_local_as_conflict_copy(relpath)?;
                    let (path, disk) = ctx.local_location(relpath);
                    download_via_part(ctx.device, &rf.entry_id, &path, rf.modified).await?;
                    let mut entry = CheckpointEntry::file();
                    entry.remote = Some(RemoteCheck {
                        entry_id: rf.entry_id.clone(),
                        modified_date: rf.modified_date.clone(),
                        file_revision: rf.revision.clone(),
                        size: rf.size,
                        extra: Default::default(),
                    });
                    entry.local = Some(local_check(&path)?);
                    set_disk_relpath(&mut entry, relpath, &disk);
                    ctx.cp.lock().unwrap().insert(relpath, entry);
                    Ok(Some(copy))
                }
                Side::Local => {
                    // Remote loses: download its content to the conflict
                    // copy first, then overwrite the remote document.
                    let copy = ctx
                        .fetch_remote_as_conflict_copy(relpath, &rf.entry_id, rf.modified)
                        .await?;
                    let (path, disk) = ctx.local_location(relpath);
                    let name = relpath.rsplit('/').next().unwrap_or(relpath).to_string();
                    ctx.device
                        .sync_upload_replace(&rf.entry_id, &name, &path)
                        .await?;
                    ctx.record_uploaded(relpath, rf.entry_id.clone(), &path, &disk)
                        .await?;
                    Ok(Some(copy))
                }
            }
        }

        Action::DeleteLocal { relpath, keep_copy } => {
            if *keep_copy {
                let copy = ctx.preserve_local_as_conflict_copy(relpath)?;
                ctx.cp.lock().unwrap().remove(relpath);
                Ok(Some(copy))
            } else {
                let (path, _) = ctx.local_location(relpath);
                match std::fs::remove_file(&path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
                ctx.cp.lock().unwrap().remove(relpath);
                Ok(None)
            }
        }

        Action::DeleteRemote {
            relpath,
            fetch_copy,
        } => {
            let key = norm_key(relpath);
            let rf = ctx
                .snap
                .remote
                .files
                .get(&key)
                .ok_or_else(|| Error::Sync(format!("remote file '{relpath}' vanished")))?;
            let copy = if *fetch_copy {
                Some(
                    ctx.fetch_remote_as_conflict_copy(relpath, &rf.entry_id, rf.modified)
                        .await?,
                )
            } else {
                None
            };
            ctx.device.sync_delete_document(&rf.entry_id).await?;
            ctx.cp.lock().unwrap().remove(relpath);
            Ok(copy)
        }

        Action::DeleteLocalDir { relpath } => {
            let (path, _) = ctx.local_location(relpath);
            match std::fs::remove_dir(&path) {
                Ok(()) => {
                    ctx.cp.lock().unwrap().remove(relpath);
                    Ok(None)
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    ctx.cp.lock().unwrap().remove(relpath);
                    Ok(None)
                }
                // Non-empty: contains non-synced content → left in place
                // (§5.4). Reported as skipped, not failed.
                Err(e) => Err(Error::Sync(format!(
                    "folder left in place (not empty): {e}"
                ))),
            }
        }

        Action::DeleteRemoteDir { relpath } => {
            let key = norm_key(relpath);
            let prefix = format!("{key}/");
            // The device deletes recursively; refuse when an action under
            // this folder failed earlier in the run.
            {
                let failed = ctx.failed_keys.lock().unwrap();
                if failed.iter().any(|k| k.starts_with(&prefix)) {
                    return Err(Error::Sync(
                        "folder kept: an action beneath it failed".into(),
                    ));
                }
            }
            let folder = ctx
                .snap
                .remote
                .folders
                .get(&key)
                .ok_or_else(|| Error::Sync(format!("remote folder '{relpath}' vanished")))?;
            ctx.device.sync_delete_folder(&folder.entry_id).await?;
            ctx.cp.lock().unwrap().remove(relpath);
            Ok(None)
        }

        Action::Adopt { relpath } => {
            let key = norm_key(relpath);
            if let (Some(lf), Some(rf)) = (
                ctx.snap.local.files.get(&key),
                ctx.snap.remote.files.get(&key),
            ) {
                let mut entry = CheckpointEntry::file();
                entry.remote = Some(RemoteCheck {
                    entry_id: rf.entry_id.clone(),
                    modified_date: rf.modified_date.clone(),
                    file_revision: rf.revision.clone(),
                    size: rf.size,
                    extra: Default::default(),
                });
                entry.local = Some(LocalCheck {
                    mtime: lf.mtime.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                    size: lf.size,
                    extra: Default::default(),
                });
                set_disk_relpath(&mut entry, relpath, &lf.disk_relpath);
                ctx.cp.lock().unwrap().insert(relpath, entry);
            } else {
                // Folder present on both sides: record it.
                let mut entry = CheckpointEntry::folder();
                if let Some(f) = ctx.snap.local.folders.get(&key) {
                    set_disk_relpath(&mut entry, relpath, &f.disk_relpath);
                }
                ctx.cp.lock().unwrap().insert(relpath, entry);
            }
            Ok(None)
        }

        Action::Forget { relpath } => {
            ctx.cp.lock().unwrap().remove(relpath);
            Ok(None)
        }
    }
}

fn action_label(action: &Action) -> String {
    let (verb, relpath) = match action {
        Action::CreateLocalDir { relpath } => ("Create folder", relpath),
        Action::CreateRemoteDir { relpath } => ("Create folder on device", relpath),
        Action::Upload { relpath } => ("Upload", relpath),
        Action::Download { relpath } => ("Download", relpath),
        Action::ConflictResolve { relpath, .. } => ("Resolve conflict", relpath),
        Action::DeleteLocal { relpath, .. } => ("Delete locally", relpath),
        Action::DeleteRemote { relpath, .. } => ("Delete on device", relpath),
        Action::DeleteLocalDir { relpath } => ("Remove folder locally", relpath),
        Action::DeleteRemoteDir { relpath } => ("Remove folder on device", relpath),
        Action::Adopt { relpath } => ("Adopt", relpath),
        Action::Forget { relpath } => ("Forget", relpath),
    };
    format!("{verb} {relpath}")
}

/// Executes the plan. Returns the advanced checkpoint and the report; the
/// checkpoint is always usable, even after interruption (FR-SYN-9).
pub async fn apply<D: SyncDevice + ?Sized>(
    device: &D,
    cfg: &SyncPairConfig,
    snap: &Snapshot,
    actions: &[Action],
    checkpoint: Checkpoint,
    hooks: &ApplyHooks<'_>,
) -> (Checkpoint, RunReport) {
    let mut folder_ids: HashMap<String, String> = HashMap::new();
    for (key, folder) in &snap.remote.folders {
        folder_ids.insert(key.clone(), folder.entry_id.clone());
    }

    let ctx = Ctx {
        device,
        cfg,
        snap,
        cp: Mutex::new(CpState::new(checkpoint)),
        folder_ids: Mutex::new(folder_ids),
        remote_root_id: Mutex::new(snap.remote_root_id.clone()),
        completed: AtomicUsize::new(0),
        abort: AtomicBool::new(false),
        failed_keys: Mutex::new(Vec::new()),
        hooks,
        total: actions.len(),
    };

    // Split into phases while remembering the overall order for reporting.
    let mut sequential_pre: Vec<(usize, &Action)> = Vec::new(); // dirs
    let mut transfers: Vec<(usize, &Action)> = Vec::new();
    let mut sequential_post: Vec<(usize, &Action)> = Vec::new(); // deletes, forgets
    for (i, action) in actions.iter().enumerate() {
        match action {
            Action::CreateLocalDir { .. } | Action::CreateRemoteDir { .. } => {
                sequential_pre.push((i, action))
            }
            Action::Upload { .. } | Action::Download { .. } | Action::ConflictResolve { .. } => {
                transfers.push((i, action))
            }
            _ => sequential_post.push((i, action)),
        }
    }

    let results: Mutex<Vec<(usize, ActionResult)>> = Mutex::new(Vec::new());
    let conflicts: Mutex<Vec<String>> = Mutex::new(Vec::new());

    // Wrapper collecting conflict copies from run_action's Ok(Some(_)).
    let run_one = |order: usize, action: Action| {
        let ctx = &ctx;
        let results = &results;
        let conflicts = &conflicts;
        async move {
            if ctx.stopping() {
                results.lock().unwrap().push((
                    order,
                    ActionResult {
                        action,
                        status: ActionStatus::Skipped,
                        message: Some("run stopped".into()),
                    },
                ));
                return;
            }
            ctx.emit_progress(Some(action_label(&action)));
            match run_action(ctx, &action).await {
                Ok(copy) => {
                    if let Some(copy) = copy {
                        conflicts.lock().unwrap().push(copy);
                    }
                    ctx.after_action();
                    results.lock().unwrap().push((
                        order,
                        ActionResult {
                            action,
                            status: ActionStatus::Done,
                            message: None,
                        },
                    ));
                }
                Err(e) => {
                    if matches!(e, Error::Network(_)) {
                        ctx.abort.store(true, Ordering::Relaxed);
                    }
                    ctx.failed_keys
                        .lock()
                        .unwrap()
                        .push(norm_key(action.relpath()));
                    let skipped = matches!(&e, Error::Sync(msg) if msg.starts_with("folder"));
                    results.lock().unwrap().push((
                        order,
                        ActionResult {
                            action,
                            status: if skipped {
                                ActionStatus::Skipped
                            } else {
                                ActionStatus::Failed
                            },
                            message: Some(e.to_string()),
                        },
                    ));
                }
            }
        }
    };

    for (order, action) in sequential_pre {
        run_one(order, action.clone()).await;
    }

    let transfer_futures: Vec<_> = transfers
        .into_iter()
        .map(|(order, action)| run_one(order, action.clone()))
        .collect();
    futures_util::stream::iter(transfer_futures)
        .buffer_unordered(TRANSFER_CONCURRENCY)
        .collect::<Vec<()>>()
        .await;

    for (order, action) in sequential_post {
        run_one(order, action.clone()).await;
    }

    ctx.persist();
    ctx.emit_progress(None);

    let mut collected = results.into_inner().unwrap();
    collected.sort_by_key(|(order, _)| *order);
    let cancelled = ctx.cancelled();
    let aborted = ctx.abort.load(Ordering::Relaxed) || cancelled;
    let report = RunReport {
        results: collected.into_iter().map(|(_, r)| r).collect(),
        conflicts: conflicts.into_inner().unwrap(),
        aborted,
        cancelled,
        warnings: Vec::new(),
    };
    let checkpoint = ctx.cp.into_inner().unwrap().cp;
    (checkpoint, report)
}
