//! Planning phase (docs/06 §5): pure, deterministic decision function.
//!
//! Implements the two-way decision table (§5.1), conflict policy "newer
//! wins, loser kept" (§5.2), first-run matrix (§5.3), folder rules (§5.4)
//! and mirror modes (§5.5). The mass-deletion guard (§5.6) is enforced by
//! the caller using [`PlanSummary`]'s deletion counts.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use serde::{Deserialize, Serialize};

use super::checkpoint::{
    Checkpoint, CheckpointEntry, LocalCheck, NodeKind, RemoteCheck, FLAG_CONFLICT_COPY,
};
use super::snapshot::{
    ancestors, norm_key, parse_device_date, LocalFile, LocalView, RemoteFile, RemoteView,
};
use super::SyncMode;
use crate::Error;

/// Which side's content wins a conflict (docs/06 §5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Local,
    Remote,
}

/// One planned sync action. `relpath` is always the canonical (device-side,
/// NFC) relative path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    CreateLocalDir {
        relpath: String,
    },
    CreateRemoteDir {
        relpath: String,
    },
    Upload {
        relpath: String,
    },
    Download {
        relpath: String,
    },
    /// Both sides changed: the newer side's content becomes canonical, the
    /// older side's content is preserved as a local conflict copy (§5.2).
    ConflictResolve {
        relpath: String,
        winner: Side,
    },
    /// `keep_copy`: the local content was never synced in this state, so it
    /// is renamed to a conflict copy instead of being removed (safety rule:
    /// no version is discarded without a surviving copy).
    DeleteLocal {
        relpath: String,
        keep_copy: bool,
    },
    /// `fetch_copy`: the remote content was never synced in this state, so
    /// it is downloaded to a local conflict copy before deletion.
    DeleteRemote {
        relpath: String,
        fetch_copy: bool,
    },
    DeleteLocalDir {
        relpath: String,
    },
    DeleteRemoteDir {
        relpath: String,
    },
    /// Same content on both sides: record in the checkpoint, no transfer
    /// (§5.1 note ¹, §5.3).
    Adopt {
        relpath: String,
    },
    /// Gone from both sides: drop from the checkpoint.
    Forget {
        relpath: String,
    },
}

impl Action {
    pub fn relpath(&self) -> &str {
        match self {
            Action::CreateLocalDir { relpath }
            | Action::CreateRemoteDir { relpath }
            | Action::Upload { relpath }
            | Action::Download { relpath }
            | Action::ConflictResolve { relpath, .. }
            | Action::DeleteLocal { relpath, .. }
            | Action::DeleteRemote { relpath, .. }
            | Action::DeleteLocalDir { relpath }
            | Action::DeleteRemoteDir { relpath }
            | Action::Adopt { relpath }
            | Action::Forget { relpath } => relpath,
        }
    }

    /// True for actions that remove something on the local side.
    pub fn deletes_local(&self) -> bool {
        matches!(
            self,
            Action::DeleteLocal { .. } | Action::DeleteLocalDir { .. }
        )
    }

    /// True for actions that remove something on the device side.
    pub fn deletes_remote(&self) -> bool {
        matches!(
            self,
            Action::DeleteRemote { .. } | Action::DeleteRemoteDir { .. }
        )
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanSummary {
    pub uploads: u32,
    pub downloads: u32,
    pub conflicts: u32,
    pub delete_local: u32,
    pub delete_remote: u32,
    pub create_local_dirs: u32,
    pub create_remote_dirs: u32,
    pub adopts: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub actions: Vec<Action>,
    pub summary: PlanSummary,
    pub warnings: Vec<String>,
}

/// Compiled exclude filters (FR-SYN-8). A relpath is excluded when it or
/// any of its ancestors matches a pattern.
pub struct Filters {
    set: globset::GlobSet,
    empty: bool,
}

impl Filters {
    pub fn new(patterns: &[String]) -> Result<Self, Error> {
        let mut builder = globset::GlobSetBuilder::new();
        let mut any = false;
        for pattern in patterns {
            let trimmed = pattern.trim().trim_end_matches('/');
            if trimmed.is_empty() {
                continue;
            }
            let glob = globset::Glob::new(trimmed)
                .map_err(|e| Error::Sync(format!("invalid filter pattern '{trimmed}': {e}")))?;
            builder.add(glob);
            any = true;
        }
        let set = builder
            .build()
            .map_err(|e| Error::Sync(format!("invalid filter set: {e}")))?;
        Ok(Self { set, empty: !any })
    }

    pub fn excluded(&self, relpath: &str) -> bool {
        if self.empty {
            return false;
        }
        if self.set.is_match(relpath) {
            return true;
        }
        ancestors(relpath).iter().any(|a| self.set.is_match(a))
    }
}

/// Per-side classification against the checkpoint (docs/06 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    /// Not present and no checkpoint record on this side.
    Absent,
    /// Present, no checkpoint record.
    New,
    Unchanged,
    Changed,
    /// Absent, but a checkpoint record exists.
    Deleted,
}

/// Local `mtime` comparison tolerance (filesystem granularity, docs/06 §4).
const MTIME_TOLERANCE_SECS: i64 = 2;

fn classify_local(file: Option<&LocalFile>, check: Option<&LocalCheck>) -> Class {
    match (file, check) {
        (None, None) => Class::Absent,
        (Some(_), None) => Class::New,
        (None, Some(_)) => Class::Deleted,
        (Some(f), Some(c)) => {
            if f.size != c.size {
                return Class::Changed;
            }
            match chrono::DateTime::parse_from_rfc3339(&c.mtime) {
                Ok(cm) => {
                    let delta = (f.mtime - cm.with_timezone(&chrono::Utc))
                        .num_seconds()
                        .abs();
                    if delta <= MTIME_TOLERANCE_SECS {
                        Class::Unchanged
                    } else {
                        Class::Changed
                    }
                }
                Err(_) => Class::Changed,
            }
        }
    }
}

fn classify_remote(file: Option<&RemoteFile>, check: Option<&RemoteCheck>) -> Class {
    match (file, check) {
        (None, None) => Class::Absent,
        (Some(_), None) => Class::New,
        (None, Some(_)) => Class::Deleted,
        (Some(f), Some(c)) => {
            // `file_revision` is the preferred change signal: it changes on
            // any modification, including annotation strokes that may not
            // change the size (docs/06 §4).
            if let (Some(fr), Some(cr)) = (&f.revision, &c.file_revision) {
                return if fr == cr {
                    Class::Unchanged
                } else {
                    Class::Changed
                };
            }
            let same_date = match (&f.modified_date, &c.modified_date) {
                (Some(a), Some(b)) => {
                    a == b
                        || matches!(
                            (parse_device_date(a), parse_device_date(b)),
                            (Some(x), Some(y)) if x == y
                        )
                }
                (None, None) => true,
                _ => false,
            };
            if same_date && f.size == c.size {
                Class::Unchanged
            } else {
                Class::Changed
            }
        }
    }
}

/// Per-file decision for a single relpath.
enum Decision {
    Nothing,
    Upload,
    Download,
    Conflict(Side),
    DeleteLocal { keep_copy: bool },
    DeleteRemote { fetch_copy: bool },
    Adopt,
    Forget,
}

fn newer_side(lf: &LocalFile, rf: &RemoteFile) -> Side {
    match rf.modified {
        Some(rm) if rm > lf.mtime => Side::Remote,
        _ => Side::Local,
    }
}

fn sizes_equal(lf: &LocalFile, rf: &RemoteFile) -> bool {
    rf.size == Some(lf.size)
}

/// Two-way decision table (docs/06 §5.1) plus the first-run matrix (§5.3),
/// which falls out naturally: with no checkpoint, both sides classify New.
fn decide_two_way(l: Class, r: Class, lf: Option<&LocalFile>, rf: Option<&RemoteFile>) -> Decision {
    match (lf, rf) {
        (Some(lf), Some(rf)) => match (l, r) {
            (Class::Unchanged, Class::Unchanged) => Decision::Nothing,
            (Class::Unchanged, Class::Changed) => Decision::Download,
            (Class::Changed, Class::Unchanged) => Decision::Upload,
            (Class::Changed, Class::Changed) => Decision::Conflict(newer_side(lf, rf)),
            // At least one side is New (present without a checkpoint
            // record): adopt silently when the sizes match, otherwise
            // treat as a conflict (§5.1 note ¹, §5.3).
            _ => {
                if sizes_equal(lf, rf) && l != Class::Changed && r != Class::Changed {
                    Decision::Adopt
                } else {
                    Decision::Conflict(newer_side(lf, rf))
                }
            }
        },
        // Remote side absent.
        (Some(_), None) => match r {
            // Row "Unchanged/Changed/New" × column "Deleted": a locally
            // changed or new file survives a remote deletion (upload);
            // an unchanged one follows the deletion (§5.1).
            Class::Deleted if l == Class::Unchanged => Decision::DeleteLocal { keep_copy: false },
            _ => Decision::Upload,
        },
        // Local side absent.
        (None, Some(_)) => match l {
            Class::Deleted if r == Class::Unchanged => Decision::DeleteRemote { fetch_copy: false },
            // Remote changed after local delete → download, no delete (§5.1).
            _ => Decision::Download,
        },
        (None, None) => Decision::Forget,
    }
}

/// Mirror-to-local: the device is the source of truth (docs/06 §5.5).
/// Changed/new local content is never discarded silently: it either becomes
/// a conflict copy (overwrite) or is renamed instead of deleted.
fn decide_mirror_to_local(
    l: Class,
    r: Class,
    lf: Option<&LocalFile>,
    rf: Option<&RemoteFile>,
) -> Decision {
    match (lf, rf) {
        (Some(lf), Some(rf)) => {
            if l == Class::Unchanged && r == Class::Unchanged {
                Decision::Nothing
            } else if sizes_equal(lf, rf) && l != Class::Changed && r != Class::Changed {
                Decision::Adopt
            } else if l == Class::Unchanged {
                Decision::Download
            } else {
                Decision::Conflict(Side::Remote)
            }
        }
        (Some(_), None) => Decision::DeleteLocal {
            keep_copy: l != Class::Unchanged,
        },
        (None, Some(_)) => Decision::Download,
        (None, None) => Decision::Forget,
    }
}

/// Mirror-to-remote: the computer is the source of truth (docs/06 §5.5).
fn decide_mirror_to_remote(
    l: Class,
    r: Class,
    lf: Option<&LocalFile>,
    rf: Option<&RemoteFile>,
) -> Decision {
    match (lf, rf) {
        (Some(lf), Some(rf)) => {
            if l == Class::Unchanged && r == Class::Unchanged {
                Decision::Nothing
            } else if sizes_equal(lf, rf) && l != Class::Changed && r != Class::Changed {
                Decision::Adopt
            } else if r == Class::Unchanged {
                Decision::Upload
            } else {
                // Changed remote content is downloaded to a conflict copy
                // before being overwritten (§5.5).
                Decision::Conflict(Side::Local)
            }
        }
        (Some(_), None) => Decision::Upload,
        (None, Some(_)) => Decision::DeleteRemote {
            fetch_copy: r != Class::Unchanged,
        },
        (None, None) => Decision::Forget,
    }
}

/// The pure planning function (docs/04 §4.3): three views + mode + filters
/// → deterministic action list, ordered for application (§6): create dirs
/// (shallowest first) → transfers → file deletions → folder deletions
/// (deepest first) → checkpoint-only actions.
pub fn plan(
    check: &Checkpoint,
    local: &LocalView,
    remote: &RemoteView,
    mode: SyncMode,
    filters: &Filters,
) -> Plan {
    let mut warnings = Vec::new();

    // Checkpoint lookup by normalized key.
    let check_by_key: BTreeMap<String, (&String, &CheckpointEntry)> = check
        .entries
        .iter()
        .map(|(relpath, entry)| (norm_key(relpath), (relpath, entry)))
        .collect();

    // Conflict copies are local-only artifacts excluded from planning
    // (docs/06 §5.2.4). Blocked keys also prevent local folder deletion.
    let mut skip: HashSet<String> = HashSet::new();
    let mut blocked_local: HashSet<String> = HashSet::new();
    let mut forgets: Vec<Action> = Vec::new();
    for (key, (relpath, entry)) in &check_by_key {
        if entry.has_flag(FLAG_CONFLICT_COPY) {
            skip.insert(key.clone());
            if local.files.contains_key(key) {
                blocked_local.insert(key.clone());
            } else {
                forgets.push(Action::Forget {
                    relpath: (*relpath).clone(),
                });
            }
        }
    }

    // Type mismatches (file on one side, folder on the other) are not
    // resolvable automatically; skip with a warning.
    let mut mismatched: BTreeSet<String> = BTreeSet::new();
    for key in local.files.keys() {
        if remote.folders.contains_key(key) {
            mismatched.insert(key.clone());
        }
    }
    for key in local.folders.keys() {
        if remote.files.contains_key(key) {
            mismatched.insert(key.clone());
        }
    }
    for key in &mismatched {
        let display = local
            .files
            .get(key)
            .map(|f| f.relpath.clone())
            .or_else(|| local.folders.get(key).map(|f| f.relpath.clone()))
            .unwrap_or_else(|| key.clone());
        warnings.push(format!(
            "'{display}' is a file on one side and a folder on the other; skipped"
        ));
        skip.insert(key.clone());
    }

    // ---- file decisions ------------------------------------------------
    let mut file_keys: BTreeSet<&String> = BTreeSet::new();
    file_keys.extend(local.files.keys());
    file_keys.extend(remote.files.keys());
    for (key, (_, entry)) in &check_by_key {
        if entry.kind == NodeKind::File {
            file_keys.insert(key);
        }
    }

    let mut transfers: Vec<Action> = Vec::new();
    let mut file_deletes: Vec<Action> = Vec::new();
    let mut deleted_local_files: HashSet<String> = HashSet::new();
    let mut deleted_remote_files: HashSet<String> = HashSet::new();
    let mut filtered_local: HashSet<String> = HashSet::new();
    let mut filtered_remote: HashSet<String> = HashSet::new();

    for key in file_keys {
        if skip.contains(key) {
            continue;
        }
        let lf = local.files.get(key);
        let rf = remote.files.get(key);
        let centry = check_by_key.get(key).map(|(_, e)| *e);
        let display = lf
            .map(|f| f.relpath.clone())
            .or_else(|| rf.map(|f| f.relpath.clone()))
            .or_else(|| check_by_key.get(key).map(|(p, _)| (*p).clone()))
            .unwrap_or_else(|| key.clone());

        // Filters remove matching relpaths from all three views (§1).
        if filters.excluded(&display) {
            if lf.is_some() {
                filtered_local.insert(key.clone());
            }
            if rf.is_some() {
                filtered_remote.insert(key.clone());
            }
            continue;
        }

        let l = classify_local(lf, centry.and_then(|e| e.local.as_ref()));
        let r = classify_remote(rf, centry.and_then(|e| e.remote.as_ref()));

        let decision = match mode {
            SyncMode::TwoWay => decide_two_way(l, r, lf, rf),
            SyncMode::MirrorToLocal => decide_mirror_to_local(l, r, lf, rf),
            SyncMode::MirrorToRemote => decide_mirror_to_remote(l, r, lf, rf),
        };

        match decision {
            Decision::Nothing => {}
            Decision::Upload => transfers.push(Action::Upload { relpath: display }),
            Decision::Download => transfers.push(Action::Download { relpath: display }),
            Decision::Conflict(winner) => transfers.push(Action::ConflictResolve {
                relpath: display,
                winner,
            }),
            Decision::Adopt => transfers.push(Action::Adopt { relpath: display }),
            Decision::DeleteLocal { keep_copy } => {
                deleted_local_files.insert(key.clone());
                file_deletes.push(Action::DeleteLocal {
                    relpath: display,
                    keep_copy,
                });
            }
            Decision::DeleteRemote { fetch_copy } => {
                deleted_remote_files.insert(key.clone());
                file_deletes.push(Action::DeleteRemote {
                    relpath: display,
                    fetch_copy,
                });
            }
            Decision::Forget => {
                if centry.is_some() {
                    forgets.push(Action::Forget { relpath: display });
                }
            }
        }
    }

    // ---- folder decisions (§5.4) ----------------------------------------
    let mut folder_keys: BTreeSet<String> = BTreeSet::new();
    folder_keys.extend(local.folders.keys().cloned());
    folder_keys.extend(remote.folders.keys().cloned());
    for (key, (_, entry)) in &check_by_key {
        if entry.kind == NodeKind::Folder {
            folder_keys.insert(key.clone());
        }
    }

    // Directories required by planned transfers.
    let mut need_local_dirs: BTreeMap<String, String> = BTreeMap::new(); // key → display
    let mut need_remote_dirs: BTreeMap<String, String> = BTreeMap::new();
    for action in &transfers {
        let (to_local, to_remote) = match action {
            Action::Download { .. } => (true, false),
            Action::Upload { .. } => (false, true),
            Action::ConflictResolve { .. } => (false, false), // both files exist
            _ => (false, false),
        };
        for anc in ancestors(action.relpath()) {
            let key = norm_key(&anc);
            if to_local && !local.folders.contains_key(&key) {
                need_local_dirs.insert(key.clone(), anc.clone());
            }
            if to_remote && !remote.folders.contains_key(&key) {
                need_remote_dirs.insert(key, anc.clone());
            }
        }
    }

    let mut adopt_folders: Vec<Action> = Vec::new();
    let mut candidate_local_dir_deletes: BTreeMap<String, String> = BTreeMap::new();
    let mut candidate_remote_dir_deletes: BTreeMap<String, String> = BTreeMap::new();

    for key in &folder_keys {
        if skip.contains(key) {
            continue;
        }
        let in_local = local.folders.get(key);
        let in_remote = remote.folders.get(key);
        let in_check = check_by_key
            .get(key)
            .filter(|(_, e)| e.kind == NodeKind::Folder);
        let display = in_local
            .map(|f| f.relpath.clone())
            .or_else(|| in_remote.map(|f| f.relpath.clone()))
            .or_else(|| in_check.map(|(p, _)| (*p).clone()))
            .unwrap_or_else(|| key.clone());

        if filters.excluded(&display) {
            if in_local.is_some() {
                filtered_local.insert(key.clone());
            }
            if in_remote.is_some() {
                filtered_remote.insert(key.clone());
            }
            continue;
        }

        match (in_local.is_some(), in_remote.is_some()) {
            (true, true) => {
                if in_check.is_none() {
                    adopt_folders.push(Action::Adopt { relpath: display });
                }
            }
            (true, false) => match mode {
                SyncMode::TwoWay => {
                    if in_check.is_none() {
                        // New local folder → mirror it (empty folders sync).
                        need_remote_dirs.insert(key.clone(), display);
                    } else {
                        // Deleted remotely → delete locally if fully drained.
                        candidate_local_dir_deletes.insert(key.clone(), display);
                    }
                }
                SyncMode::MirrorToRemote => {
                    need_remote_dirs.insert(key.clone(), display);
                }
                SyncMode::MirrorToLocal => {
                    candidate_local_dir_deletes.insert(key.clone(), display);
                }
            },
            (false, true) => match mode {
                SyncMode::TwoWay => {
                    if in_check.is_none() {
                        need_local_dirs.insert(key.clone(), display);
                    } else {
                        candidate_remote_dir_deletes.insert(key.clone(), display);
                    }
                }
                SyncMode::MirrorToLocal => {
                    need_local_dirs.insert(key.clone(), display);
                }
                SyncMode::MirrorToRemote => {
                    candidate_remote_dir_deletes.insert(key.clone(), display);
                }
            },
            (false, false) => {
                if in_check.is_some() {
                    forgets.push(Action::Forget { relpath: display });
                }
            }
        }
    }

    // A folder is deleted only if everything visible under it on that side
    // is also being deleted, and nothing filtered/ignored/blocked lives
    // beneath it (§5.4). Evaluated deepest-first so nested folders resolve
    // before their parents.
    let local_dir_deletes = resolve_dir_deletes(
        &candidate_local_dir_deletes,
        local.files.keys(),
        local.folders.keys(),
        &local.others,
        &deleted_local_files,
        &blocked_local,
        &filtered_local,
    );
    let remote_dir_deletes = resolve_dir_deletes(
        &candidate_remote_dir_deletes,
        remote.files.keys(),
        remote.folders.keys(),
        &remote.others,
        &deleted_remote_files,
        &HashSet::new(),
        &filtered_remote,
    );

    // ---- assemble in application order (§6) ------------------------------
    let mut actions: Vec<Action> = Vec::new();

    let mut local_dirs: Vec<&String> = need_local_dirs.values().collect();
    local_dirs.sort_by_key(|d| (d.matches('/').count(), (*d).clone()));
    for d in local_dirs {
        actions.push(Action::CreateLocalDir { relpath: d.clone() });
    }
    let mut remote_dirs: Vec<&String> = need_remote_dirs.values().collect();
    remote_dirs.sort_by_key(|d| (d.matches('/').count(), (*d).clone()));
    for d in remote_dirs {
        actions.push(Action::CreateRemoteDir { relpath: d.clone() });
    }

    transfers.sort_by(|a, b| a.relpath().cmp(b.relpath()));
    actions.extend(transfers);
    adopt_folders.sort_by(|a, b| a.relpath().cmp(b.relpath()));
    actions.extend(adopt_folders);

    file_deletes.sort_by(|a, b| a.relpath().cmp(b.relpath()));
    actions.extend(file_deletes);

    for d in local_dir_deletes {
        actions.push(Action::DeleteLocalDir { relpath: d });
    }
    for d in remote_dir_deletes {
        actions.push(Action::DeleteRemoteDir { relpath: d });
    }

    forgets.sort_by(|a, b| a.relpath().cmp(b.relpath()));
    actions.extend(forgets);

    let mut summary = PlanSummary::default();
    for action in &actions {
        match action {
            Action::Upload { .. } => summary.uploads += 1,
            Action::Download { .. } => summary.downloads += 1,
            Action::ConflictResolve { .. } => summary.conflicts += 1,
            Action::DeleteLocal { .. } | Action::DeleteLocalDir { .. } => summary.delete_local += 1,
            Action::DeleteRemote { .. } | Action::DeleteRemoteDir { .. } => {
                summary.delete_remote += 1
            }
            Action::CreateLocalDir { .. } => summary.create_local_dirs += 1,
            Action::CreateRemoteDir { .. } => summary.create_remote_dirs += 1,
            Action::Adopt { .. } => summary.adopts += 1,
            Action::Forget { .. } => {}
        }
    }

    Plan {
        actions,
        summary,
        warnings,
    }
}

/// Resolves which candidate folders may actually be deleted, returning
/// their display paths deepest-first.
#[allow(clippy::too_many_arguments)]
fn resolve_dir_deletes<'a>(
    candidates: &BTreeMap<String, String>,
    view_files: impl Iterator<Item = &'a String>,
    view_folders: impl Iterator<Item = &'a String>,
    view_others: &BTreeSet<String>,
    deleted_files: &HashSet<String>,
    blocked: &HashSet<String>,
    filtered: &HashSet<String>,
) -> Vec<String> {
    let files: Vec<&String> = view_files.collect();
    let folders: Vec<&String> = view_folders.collect();

    // Deepest-first so children are approved before their parents.
    let mut order: Vec<(&String, &String)> = candidates.iter().collect();
    order.sort_by_key(|(k, _)| std::cmp::Reverse(k.matches('/').count()));

    let mut approved: HashSet<&String> = HashSet::new();
    let mut result: Vec<String> = Vec::new();

    'candidate: for (key, display) in order {
        let prefix = format!("{key}/");
        for f in &files {
            if f.starts_with(&prefix)
                && (!deleted_files.contains(*f) || blocked.contains(*f) || filtered.contains(*f))
            {
                continue 'candidate;
            }
        }
        for o in view_others {
            if o.starts_with(&prefix) {
                continue 'candidate;
            }
        }
        for f in filtered {
            if f.starts_with(&prefix) {
                continue 'candidate;
            }
        }
        for sub in &folders {
            if sub.starts_with(&prefix) && !approved.contains(*sub) {
                continue 'candidate;
            }
        }
        approved.insert(key);
        result.push(display.clone());
    }
    result
}
