//! Client-side sync engine (authoritative spec: docs/06-sync-specification.md).
//!
//! Pipeline: [`snapshot`] (three views: checkpoint / local / remote)
//! → [`plan`] (pure decision function → `Vec<Action>`)
//! → [`apply`] (execution with incremental [`checkpoint`] advancement).
//!
//! `plan` being pure is the testing linchpin (NFR-QLT-3): scenario tests
//! feed synthetic trees and assert on the produced actions.
//!
//! The engine talks to the device through the [`device::SyncDevice`] trait,
//! implemented for [`crate::client::DeviceClient`] and — in tests — by an
//! in-memory fake device.

pub mod apply;
pub mod checkpoint;
pub mod device;
pub mod plan;
pub mod snapshot;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::Error;

pub use apply::{ActionResult, ApplyHooks, ProgressEvent, RunReport};
pub use checkpoint::Checkpoint;
pub use device::SyncDevice;
pub use plan::{Action, Filters, Plan, PlanSummary, Side};
pub use snapshot::{LocalView, RemoteView};

/// Sync direction of a pair (docs/06 §1, FR-SYN-2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SyncMode {
    /// Both sides exchange changes; conflicts resolved newer-wins-loser-kept.
    #[default]
    TwoWay,
    /// The device is the source of truth; local extras are removed.
    MirrorToLocal,
    /// The computer is the source of truth; remote extras are removed.
    MirrorToRemote,
}

/// Engine-facing configuration of one sync pair (docs/06 §1). Scheduling
/// lives in the application layer; the engine only needs these fields.
#[derive(Debug, Clone)]
pub struct SyncPairConfig {
    pub id: String,
    pub local_root: PathBuf,
    /// Device-side root path, e.g. `Document` or `Document/Papers`.
    pub remote_root: String,
    pub mode: SyncMode,
    /// Glob patterns excluding relpaths from all three views (FR-SYN-8).
    pub filters: Vec<String>,
}

/// The local and remote views of one snapshot phase (docs/06 §3).
pub struct Snapshot {
    pub local: LocalView,
    pub remote: RemoteView,
    /// Entry id of `remote_root`, if it exists on the device.
    pub remote_root_id: Option<String>,
}

/// Snapshot phase: builds both views and enforces the run preconditions
/// that belong to the engine (docs/06 §2.3: a missing `local_root` aborts —
/// it is never treated as "user deleted everything").
pub async fn take_snapshot<D: SyncDevice + ?Sized>(
    device: &D,
    cfg: &SyncPairConfig,
    check: &Checkpoint,
) -> Result<Snapshot, Error> {
    if !cfg.local_root.is_dir() {
        return Err(Error::Sync(format!(
            "local folder '{}' does not exist or is not a directory (unmounted drive?)",
            cfg.local_root.display()
        )));
    }

    let unescape = check.local_name_map();
    let local = snapshot::walk_local(&cfg.local_root, &unescape)?;

    let entries = device.sync_list_entries().await?;
    let remote = snapshot::remote_view(&entries, &cfg.remote_root);
    let remote_root_id = match device.sync_resolve_path(&cfg.remote_root).await {
        Ok(e) if e.is_folder() => Some(e.entry_id),
        Ok(_) => {
            return Err(Error::Sync(format!(
                "device path '{}' is a document, not a folder",
                cfg.remote_root
            )))
        }
        Err(_) => None, // missing remote root: empty view; created on demand
    };

    Ok(Snapshot {
        local,
        remote,
        remote_root_id,
    })
}

/// Planning phase: compiles the pair's filters and produces the action plan
/// (pure, docs/06 §5).
pub fn make_plan(cfg: &SyncPairConfig, check: &Checkpoint, snap: &Snapshot) -> Result<Plan, Error> {
    let filters = Filters::new(&cfg.filters)?;
    Ok(plan::plan(
        check,
        &snap.local,
        &snap.remote,
        cfg.mode,
        &filters,
    ))
}

/// Apply phase plus the closing consistency pass (docs/06 §6): executes
/// `actions`, then refreshes the checkpoint's remote fields from a fresh
/// listing and stamps `completed_at`. Returns the advanced checkpoint and
/// the run report. The checkpoint is valid (and persisted via
/// `hooks.persist`) even when the run was interrupted (FR-SYN-9).
pub async fn execute<D: SyncDevice + ?Sized>(
    device: &D,
    cfg: &SyncPairConfig,
    snap: &Snapshot,
    actions: Vec<Action>,
    checkpoint: Checkpoint,
    hooks: ApplyHooks<'_>,
) -> (Checkpoint, RunReport) {
    let (mut checkpoint, mut report) =
        apply::apply(device, cfg, snap, &actions, checkpoint, &hooks).await;

    // Cheap consistency pass: refresh remote fields from a fresh listing
    // (catches entries the device modified during the run).
    if !report.aborted {
        match device.sync_list_entries().await {
            Ok(entries) => {
                let fresh = snapshot::remote_view(&entries, &cfg.remote_root);
                checkpoint.refresh_remote(&fresh);
            }
            Err(e) => report
                .warnings
                .push(format!("post-run remote refresh failed: {e}")),
        }
    }
    checkpoint.completed_at = Some(chrono::Utc::now().to_rfc3339());
    if let Some(persist) = hooks.persist {
        persist(&checkpoint);
    }
    (checkpoint, report)
}
