//! Sync orchestration: run queue, progress events, mass-deletion gate and
//! run history (docs/04 §5, docs/06 §2/§5.6/§8/§9; FR-SYN-3/5/7/9).
//!
//! All engine logic lives in `dpt_core::sync`; this module owns the
//! application concerns: one global runner (at most one run at a time
//! against the device), the confirmation flow, checkpoint/history
//! persistence and Tauri events.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::Emitter;
use tokio::sync::{oneshot, Notify};

use dpt_core::sync::{self as engine, Action, ApplyHooks, Checkpoint, PlanSummary, SyncPairConfig};

use crate::error::AppError;
use crate::state::{events, AppState};
use crate::stores::SyncPair;

/// How long a paused run waits for the mass-deletion decision before it is
/// cancelled (a scheduled run must not block the queue forever).
const CONFIRMATION_TIMEOUT: Duration = Duration::from_secs(15 * 60);

// ---- payloads ----------------------------------------------------------------

/// What started a run (docs/06 §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Trigger {
    Manual,
    OnConnect,
    Interval,
}

impl Trigger {
    fn label(self) -> &'static str {
        match self {
            Trigger::Manual => "manual",
            Trigger::OnConnect => "on-connect",
            Trigger::Interval => "interval",
        }
    }
}

/// An action the user deselected in the preview dialog (FR-SYN-5).
#[derive(Debug, Clone, Deserialize)]
pub struct ExcludedAction {
    pub kind: String,
    pub relpath: String,
}

/// Options travelling with a queued run.
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    /// True when the user already saw and approved the plan (preview flow):
    /// the mass-deletion gate is skipped.
    pub confirmed: bool,
    pub excluded: Vec<ExcludedAction>,
}

pub struct QueuedRun {
    pub pair_id: String,
    pub trigger: Trigger,
    pub options: RunOptions,
}

/// Live status snapshot pushed on [`events::SYNC_UPDATED`].
#[derive(Debug, Clone, Serialize, Default)]
pub struct SyncStatusPayload {
    pub running: Option<RunningStatus>,
    pub queued: Vec<String>,
    pub pending_confirmation: Option<ConfirmationRequest>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunningStatus {
    pub pair_id: String,
    /// `waiting` | `snapshot` | `apply` | `confirmation`
    pub phase: String,
    pub done: usize,
    pub total: usize,
    pub current: Option<String>,
}

/// Payload of [`events::SYNC_CONFIRMATION_REQUIRED`] (FR-SYN-5).
#[derive(Debug, Clone, Serialize)]
pub struct ConfirmationRequest {
    pub pair_id: String,
    pub pair_name: String,
    pub threshold: u32,
    pub local_deletions: Vec<String>,
    pub remote_deletions: Vec<String>,
}

/// One line of the persistent run history (FR-SYN-7).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRunRecord {
    pub pair_id: String,
    /// Serial of the device this run talked to (multi-device hub model).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub device_serial: String,
    pub trigger: String,
    pub started_at: String,
    pub finished_at: String,
    /// `ok` | `partial` | `cancelled` | `failed`
    pub result: String,
    #[serde(default)]
    pub summary: Option<PlanSummary>,
    pub done: u32,
    pub failed: u32,
    pub skipped: u32,
    #[serde(default)]
    pub conflicts: Vec<String>,
    #[serde(default)]
    pub errors: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Apply,
    SkipDeletions,
    Cancel,
}

struct PendingConfirmation {
    request: ConfirmationRequest,
    tx: oneshot::Sender<Decision>,
}

/// Shared sync state, owned by [`AppState`].
#[derive(Default)]
pub struct SyncRuntime {
    queue: StdMutex<VecDeque<QueuedRun>>,
    running: StdMutex<Option<RunningStatus>>,
    cancel: StdMutex<Option<Arc<AtomicBool>>>,
    pending: StdMutex<Option<PendingConfirmation>>,
    pub notify: Notify,
}

impl SyncRuntime {
    pub fn status(&self) -> SyncStatusPayload {
        SyncStatusPayload {
            running: self.running.lock().unwrap().clone(),
            queued: self
                .queue
                .lock()
                .unwrap()
                .iter()
                .map(|q| q.pair_id.clone())
                .collect(),
            pending_confirmation: self
                .pending
                .lock()
                .unwrap()
                .as_ref()
                .map(|p| p.request.clone()),
        }
    }
}

fn emit_status(state: &AppState) {
    let _ = state.app.emit(events::SYNC_UPDATED, state.sync.status());
}

// ---- queueing ------------------------------------------------------------------

/// Enqueues a run unless the pair is already queued or running. Runs are
/// strictly serialized (docs/06 §2.4, §8). Returns whether a run was
/// actually enqueued.
pub fn enqueue(
    state: &Arc<AppState>,
    pair_id: &str,
    trigger: Trigger,
    options: RunOptions,
) -> bool {
    {
        let running = state.sync.running.lock().unwrap();
        let mut queue = state.sync.queue.lock().unwrap();
        let already = queue.iter().any(|q| q.pair_id == pair_id)
            || running.as_ref().is_some_and(|r| r.pair_id == pair_id);
        if already {
            tracing::info!(
                pair_id,
                trigger = trigger.label(),
                "sync run already queued/running; skipping"
            );
            return false;
        }
        let item = QueuedRun {
            pair_id: pair_id.to_string(),
            trigger,
            options,
        };
        // A manual "Sync now" jumps the queue (docs/06 §8).
        if trigger == Trigger::Manual {
            queue.push_front(item);
        } else {
            queue.push_back(item);
        }
    }
    tracing::info!(pair_id, trigger = trigger.label(), "sync run enqueued");
    state.sync.notify.notify_one();
    emit_status(state);
    true
}

/// Cancels a queued or running run of `pair_id`.
pub fn cancel(state: &Arc<AppState>, pair_id: &str) {
    state
        .sync
        .queue
        .lock()
        .unwrap()
        .retain(|q| q.pair_id != pair_id);
    let is_running = state
        .sync
        .running
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|r| r.pair_id == pair_id);
    if is_running {
        if let Some(flag) = state.sync.cancel.lock().unwrap().as_ref() {
            flag.store(true, Ordering::Relaxed);
        }
        // A run paused at the confirmation gate is cancelled immediately.
        if let Some(pending) = state.sync.pending.lock().unwrap().take() {
            let _ = pending.tx.send(Decision::Cancel);
        }
    }
    emit_status(state);
}

/// Resolves the pending mass-deletion confirmation (FR-SYN-5).
pub fn confirm(state: &Arc<AppState>, pair_id: &str, decision: Decision) -> Result<(), AppError> {
    let pending = {
        let mut slot = state.sync.pending.lock().unwrap();
        match slot.as_ref() {
            Some(p) if p.request.pair_id == pair_id => slot.take(),
            _ => None,
        }
    };
    match pending {
        Some(p) => {
            let _ = p.tx.send(decision);
            emit_status(state);
            Ok(())
        }
        None => Err(AppError::new(
            "no_confirmation",
            "no confirmation is pending for this sync pair",
        )),
    }
}

// ---- the runner ---------------------------------------------------------------

/// Background task processing the queue; spawned once at startup.
pub async fn runner_loop(state: Arc<AppState>) {
    loop {
        let next = state.sync.queue.lock().unwrap().pop_front();
        let Some(run) = next else {
            state.sync.notify.notified().await;
            continue;
        };
        run_pair(&state, run).await;
    }
}

fn set_running(state: &AppState, status: Option<RunningStatus>) {
    *state.sync.running.lock().unwrap() = status;
    emit_status(state);
}

fn engine_config(pair: &SyncPair) -> SyncPairConfig {
    SyncPairConfig {
        id: pair.id.clone(),
        local_root: PathBuf::from(&pair.local_root),
        remote_root: pair.remote_root.clone(),
        mode: pair.mode,
        filters: pair.filters.clone(),
    }
}

async fn run_pair(state: &Arc<AppState>, run: QueuedRun) {
    let started_at = chrono::Utc::now();
    let Some(pair) = state
        .stores
        .load_sync_pairs()
        .into_iter()
        .find(|p| p.id == run.pair_id)
    else {
        return; // deleted while queued
    };
    tracing::info!(
        pair_id = %pair.id,
        trigger = run.trigger.label(),
        "sync run starting"
    );

    let cancel_flag = Arc::new(AtomicBool::new(false));
    *state.sync.cancel.lock().unwrap() = Some(cancel_flag.clone());
    set_running(
        state,
        Some(RunningStatus {
            pair_id: pair.id.clone(),
            phase: "waiting".into(),
            done: 0,
            total: 0,
            current: None,
        }),
    );

    let outcome = execute_run(state, &pair, &run, &cancel_flag, started_at).await;

    let record = match outcome {
        Ok(record) => record,
        Err(e) => SyncRunRecord {
            pair_id: pair.id.clone(),
            device_serial: state.connected_serial().await.unwrap_or_default(),
            trigger: run.trigger.label().into(),
            started_at: started_at.to_rfc3339(),
            finished_at: chrono::Utc::now().to_rfc3339(),
            result: if cancel_flag.load(Ordering::Relaxed) {
                "cancelled".into()
            } else {
                "failed".into()
            },
            summary: None,
            done: 0,
            failed: 0,
            skipped: 0,
            conflicts: Vec::new(),
            errors: vec![e.message],
            warnings: Vec::new(),
        },
    };

    tracing::info!(
        pair_id = %pair.id,
        result = %record.result,
        done = record.done,
        failed = record.failed,
        skipped = record.skipped,
        errors = ?record.errors,
        "sync run finished"
    );
    if let Ok(value) = serde_json::to_value(&record) {
        if let Err(e) = state.stores.append_sync_history(&pair.id, &value) {
            tracing::warn!(error = %e, "failed to append sync history");
        }
    }
    let _ = state.app.emit(events::SYNC_FINISHED, &record);

    *state.sync.cancel.lock().unwrap() = None;
    set_running(state, None);
}

async fn execute_run(
    state: &Arc<AppState>,
    pair: &SyncPair,
    run: &QueuedRun,
    cancel_flag: &Arc<AtomicBool>,
    started_at: chrono::DateTime<chrono::Utc>,
) -> Result<SyncRunRecord, AppError> {
    let mut warnings: Vec<String> = Vec::new();

    // Runs never start while user transfers are active (docs/06 §8).
    loop {
        if cancel_flag.load(Ordering::Relaxed) {
            return Err(AppError::new("cancelled", "sync cancelled"));
        }
        let busy = {
            let ts = state.transfers.lock().await;
            ts.jobs.iter().any(|j| {
                matches!(
                    j.snapshot.status,
                    crate::transfers::JobStatus::Queued | crate::transfers::JobStatus::Running
                )
            })
        };
        if !busy {
            break;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    let client = state
        .client()
        .await
        .ok_or_else(|| AppError::new("not_connected", "device unreachable"))?;
    let serial = state
        .connected_serial()
        .await
        .ok_or_else(|| AppError::new("not_connected", "device unreachable"))?;

    // Precondition: device clock set from the computer (docs/06 §2.2).
    if let Err(e) = client.set_clock_now().await {
        warnings.push(format!("could not set the device clock: {e}"));
    }

    // Per-device checkpoint: the same pair syncs any connected device, each
    // against its own last-known state (multi-device hub model, docs/06 §7).
    let cfg = engine_config(pair);
    let cp_path = state.stores.checkpoint_path(&pair.id, &serial);
    let checkpoint = Checkpoint::load(&cp_path)
        .unwrap_or_else(|| Checkpoint::new(&pair.id, &serial, &pair.remote_root));

    update_phase(state, &pair.id, "snapshot");
    let snap = engine::take_snapshot(client.as_ref(), &cfg, &checkpoint)
        .await
        .map_err(AppError::from)?;
    let plan = engine::make_plan(&cfg, &checkpoint, &snap).map_err(AppError::from)?;
    tracing::info!(
        pair_id = %pair.id,
        actions = plan.actions.len(),
        summary = ?plan.summary,
        "sync plan ready"
    );
    warnings.extend(plan.warnings.iter().cloned());

    // Apply preview deselections, then drop folder deletions whose subtree
    // is no longer fully drained (a remote folder delete is recursive).
    let mut actions = filter_excluded(plan.actions.clone(), &run.options.excluded);

    // Mass-deletion guard (docs/06 §5.6, FR-SYN-5).
    if !run.options.confirmed {
        let local: Vec<String> = actions
            .iter()
            .filter(|a| a.deletes_local())
            .map(|a| a.relpath().to_string())
            .collect();
        let remote: Vec<String> = actions
            .iter()
            .filter(|a| a.deletes_remote())
            .map(|a| a.relpath().to_string())
            .collect();
        let threshold = pair.deletion_threshold as usize;
        if local.len() > threshold || remote.len() > threshold {
            update_phase(state, &pair.id, "confirmation");
            let decision = await_confirmation(state, pair, local, remote).await;
            match decision {
                Decision::Apply => {}
                Decision::SkipDeletions => {
                    actions.retain(|a| !a.deletes_local() && !a.deletes_remote());
                    warnings.push("deletions skipped on user request".into());
                }
                Decision::Cancel => {
                    return Ok(SyncRunRecord {
                        pair_id: pair.id.clone(),
                        device_serial: serial.clone(),
                        trigger: run.trigger.label().into(),
                        started_at: started_at.to_rfc3339(),
                        finished_at: chrono::Utc::now().to_rfc3339(),
                        result: "cancelled".into(),
                        summary: Some(plan.summary),
                        done: 0,
                        failed: 0,
                        skipped: actions.len() as u32,
                        conflicts: Vec::new(),
                        errors: Vec::new(),
                        warnings,
                    });
                }
            }
        }
    }

    update_phase(state, &pair.id, "apply");

    let cp_path_for_persist = cp_path.clone();
    let persist = move |cp: &Checkpoint| {
        if let Err(e) = cp.save(&cp_path_for_persist) {
            tracing::error!(error = %e, "failed to persist sync checkpoint");
        }
    };

    let state_for_progress = state.clone();
    let last_emit = StdMutex::new(std::time::Instant::now() - Duration::from_secs(1));
    let progress = move |e: engine::ProgressEvent| {
        {
            let mut running = state_for_progress.sync.running.lock().unwrap();
            if let Some(r) = running.as_mut() {
                r.phase = "apply".into();
                r.done = e.done;
                r.total = e.total;
                r.current = e.current.clone();
            }
        }
        // Throttle event emission; always emit the final (current: None).
        let mut last = last_emit.lock().unwrap();
        if e.current.is_none() || last.elapsed() >= Duration::from_millis(150) {
            *last = std::time::Instant::now();
            let _ = state_for_progress
                .app
                .emit(events::SYNC_UPDATED, state_for_progress.sync.status());
        }
    };

    let hooks = ApplyHooks {
        persist: Some(&persist),
        progress: Some(&progress),
        cancel: Some(cancel_flag.as_ref()),
    };

    let (_checkpoint, report) =
        engine::execute(client.as_ref(), &cfg, &snap, actions, checkpoint, hooks).await;
    warnings.extend(report.warnings.iter().cloned());

    let (done, failed, skipped) = report.counts();
    let errors: Vec<String> = report
        .results
        .iter()
        .filter(|r| r.status == engine::apply::ActionStatus::Failed)
        .filter_map(|r| {
            r.message
                .as_ref()
                .map(|m| format!("{}: {m}", r.action.relpath()))
        })
        .collect();

    // Local changes may have been produced (downloads/conflict copies) —
    // no library invalidation needed; remote changes require one.
    if done > 0 {
        state.invalidate_entries().await;
    }

    let result = if report.cancelled {
        "cancelled"
    } else if failed > 0 || report.aborted {
        "partial"
    } else {
        "ok"
    };

    Ok(SyncRunRecord {
        pair_id: pair.id.clone(),
        device_serial: serial,
        trigger: run.trigger.label().into(),
        started_at: started_at.to_rfc3339(),
        finished_at: chrono::Utc::now().to_rfc3339(),
        result: result.into(),
        summary: Some(plan.summary),
        done: done as u32,
        failed: failed as u32,
        skipped: skipped as u32,
        conflicts: report.conflicts,
        errors,
        warnings,
    })
}

fn update_phase(state: &AppState, pair_id: &str, phase: &str) {
    let mut running = state.sync.running.lock().unwrap();
    if let Some(r) = running.as_mut() {
        if r.pair_id == pair_id {
            r.phase = phase.into();
        }
    }
    drop(running);
    emit_status(state);
}

async fn await_confirmation(
    state: &Arc<AppState>,
    pair: &SyncPair,
    local_deletions: Vec<String>,
    remote_deletions: Vec<String>,
) -> Decision {
    let (tx, rx) = oneshot::channel();
    let request = ConfirmationRequest {
        pair_id: pair.id.clone(),
        pair_name: if pair.name.is_empty() {
            pair.local_root.clone()
        } else {
            pair.name.clone()
        },
        threshold: pair.deletion_threshold,
        local_deletions,
        remote_deletions,
    };
    *state.sync.pending.lock().unwrap() = Some(PendingConfirmation {
        request: request.clone(),
        tx,
    });
    let _ = state.app.emit(events::SYNC_CONFIRMATION_REQUIRED, &request);
    emit_status(state);

    let decision = match tokio::time::timeout(CONFIRMATION_TIMEOUT, rx).await {
        Ok(Ok(d)) => d,
        // Timeout or dropped sender → safest choice: cancel the run.
        _ => Decision::Cancel,
    };
    *state.sync.pending.lock().unwrap() = None;
    emit_status(state);
    decision
}

/// Removes deselected actions and any folder deletion whose subtree would
/// no longer be fully drained (remote folder deletes are recursive on the
/// device, so applying them with surviving children would destroy data).
fn filter_excluded(actions: Vec<Action>, excluded: &[ExcludedAction]) -> Vec<Action> {
    if excluded.is_empty() {
        return actions;
    }
    let is_excluded = |action: &Action| {
        let kind = match action {
            Action::Upload { .. } => "upload",
            Action::Download { .. } => "download",
            Action::ConflictResolve { .. } => "conflict_resolve",
            Action::DeleteLocal { .. } => "delete_local",
            Action::DeleteRemote { .. } => "delete_remote",
            Action::DeleteLocalDir { .. } => "delete_local_dir",
            Action::DeleteRemoteDir { .. } => "delete_remote_dir",
            _ => return false, // dirs/adopts/forgets are never deselectable
        };
        excluded
            .iter()
            .any(|e| e.kind == kind && e.relpath == action.relpath())
    };

    let kept: Vec<Action> = actions.into_iter().filter(|a| !is_excluded(a)).collect();

    // Excluded deletions beneath a folder block that folder's deletion.
    let excluded_deletions: Vec<&ExcludedAction> = excluded
        .iter()
        .filter(|e| e.kind.starts_with("delete_"))
        .collect();
    kept.into_iter()
        .filter(|a| match a {
            Action::DeleteLocalDir { relpath } | Action::DeleteRemoteDir { relpath } => {
                let prefix = format!("{relpath}/");
                !excluded_deletions
                    .iter()
                    .any(|e| e.relpath.starts_with(&prefix))
            }
            _ => true,
        })
        .collect()
}
