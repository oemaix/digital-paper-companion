//! Transfer queue: sequential worker with progress events and cancellation
//! (FR-TRF-1…5, 7, 9, 10; docs/05 §4.4).
//!
//! Jobs are processed one at a time (the device handles concurrent
//! transfers poorly). Downloads stream to a `.part` file and are renamed on
//! completion; uploads go through `dpt-core`'s create-then-stream flow which
//! cleans up half-created entries. Every state change is broadcast as a full
//! queue snapshot on [`crate::state::events::TRANSFER_UPDATED`].

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use futures_util::StreamExt;
use serde::Serialize;
use tauri::Emitter;
use tokio::io::AsyncWriteExt;

use crate::error::AppError;
use crate::state::{events, AppState};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

/// What a job does; not serialized directly (paths/ids stay backend-side).
#[derive(Debug, Clone)]
pub enum JobKind {
    Upload {
        local_path: PathBuf,
        file_name: String,
        dest_folder_id: String,
        /// Set when the user chose "overwrite": replace this document's
        /// content instead of creating a new entry.
        existing_doc_id: Option<String>,
    },
    Download {
        entry_id: String,
        target_path: PathBuf,
    },
    /// Note-template upload (FR-TRF-6): create-then-stream like documents,
    /// shown in the same queue (docs/05 §3.4).
    UploadTemplate {
        local_path: PathBuf,
        template_name: String,
        file_name: String,
    },
}

/// Frontend-visible snapshot of one job.
#[derive(Debug, Clone, Serialize)]
pub struct JobSnapshot {
    pub id: u64,
    pub kind: String,
    pub name: String,
    pub status: JobStatus,
    /// 0.0–1.0, or `null` for indeterminate (uploads).
    pub progress: Option<f64>,
    pub error: Option<String>,
}

pub struct TransferJob {
    pub snapshot: JobSnapshot,
    pub kind: JobKind,
    pub cancel: Arc<AtomicBool>,
}

#[derive(Default)]
pub struct TransferState {
    pub jobs: Vec<TransferJob>,
    pub worker_running: bool,
    next_id: u64,
}

impl TransferState {
    fn next_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }
}

/// Adds jobs to the queue and makes sure the worker runs.
pub async fn enqueue(state: &Arc<AppState>, kinds: Vec<(String, JobKind)>) -> Vec<u64> {
    let mut ids = Vec::new();
    {
        let mut ts = state.transfers.lock().await;
        for (name, kind) in kinds {
            let id = ts.next_id();
            ids.push(id);
            let kind_label = match kind {
                JobKind::Upload { .. } => "upload",
                JobKind::Download { .. } => "download",
                JobKind::UploadTemplate { .. } => "upload-template",
            };
            ts.jobs.push(TransferJob {
                snapshot: JobSnapshot {
                    id,
                    kind: kind_label.into(),
                    name,
                    status: JobStatus::Queued,
                    progress: None,
                    error: None,
                },
                kind,
                cancel: Arc::new(AtomicBool::new(false)),
            });
        }
        if !ts.worker_running {
            ts.worker_running = true;
            let state = state.clone();
            tokio::spawn(async move { worker_loop(state).await });
        }
    }
    emit_snapshot(state).await;
    ids
}

/// Requests cancellation of a queued or running job.
pub async fn cancel(state: &Arc<AppState>, id: u64) {
    {
        let mut ts = state.transfers.lock().await;
        if let Some(job) = ts.jobs.iter_mut().find(|j| j.snapshot.id == id) {
            job.cancel.store(true, Ordering::Relaxed);
            if matches!(job.snapshot.status, JobStatus::Queued) {
                job.snapshot.status = JobStatus::Cancelled;
            }
        }
    }
    emit_snapshot(state).await;
}

/// Removes finished jobs from the list.
pub async fn clear_finished(state: &Arc<AppState>) {
    {
        let mut ts = state.transfers.lock().await;
        ts.jobs
            .retain(|j| matches!(j.snapshot.status, JobStatus::Queued | JobStatus::Running));
    }
    emit_snapshot(state).await;
}

pub async fn snapshot(state: &Arc<AppState>) -> Vec<JobSnapshot> {
    state
        .transfers
        .lock()
        .await
        .jobs
        .iter()
        .map(|j| j.snapshot.clone())
        .collect()
}

pub async fn emit_snapshot(state: &Arc<AppState>) {
    let snap = snapshot(state).await;
    let _ = state.app.emit(events::TRANSFER_UPDATED, snap);
}

async fn worker_loop(state: Arc<AppState>) {
    loop {
        // Pick the next queued job.
        let next = {
            let mut ts = state.transfers.lock().await;
            let job = ts
                .jobs
                .iter_mut()
                .find(|j| matches!(j.snapshot.status, JobStatus::Queued));
            match job {
                Some(job) => {
                    job.snapshot.status = JobStatus::Running;
                    Some((job.snapshot.id, job.kind.clone(), job.cancel.clone()))
                }
                None => {
                    ts.worker_running = false;
                    None
                }
            }
        };
        let Some((id, kind, cancel)) = next else {
            break;
        };
        emit_snapshot(&state).await;

        let uploaded = matches!(kind, JobKind::Upload { .. });
        let template = matches!(kind, JobKind::UploadTemplate { .. });
        let result = run_job(&state, id, kind, &cancel).await;

        {
            let mut ts = state.transfers.lock().await;
            if let Some(job) = ts.jobs.iter_mut().find(|j| j.snapshot.id == id) {
                match result {
                    Ok(()) => {
                        job.snapshot.status = JobStatus::Done;
                        job.snapshot.progress = Some(1.0);
                    }
                    Err(e) if cancel.load(Ordering::Relaxed) => {
                        job.snapshot.status = JobStatus::Cancelled;
                        job.snapshot.error = Some(e.message);
                    }
                    Err(e) => {
                        job.snapshot.status = JobStatus::Failed;
                        job.snapshot.error = Some(e.message);
                    }
                }
            }
        }
        emit_snapshot(&state).await;
        if uploaded {
            // Remote tree changed; browsing views must refetch (FR-BRW-1).
            state.invalidate_entries().await;
        }
        if template {
            // Template list changed; the templates view refetches (FR-BRW-7).
            let _ = state.app.emit(events::TEMPLATES_INVALIDATED, ());
        }
    }
}

async fn run_job(
    state: &Arc<AppState>,
    id: u64,
    kind: JobKind,
    cancel: &Arc<AtomicBool>,
) -> Result<(), AppError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(AppError::new("cancelled", "cancelled before start"));
    }
    let client = state.require_client().await?;
    match kind {
        JobKind::Upload {
            local_path,
            file_name,
            dest_folder_id,
            existing_doc_id,
        } => {
            match existing_doc_id {
                Some(doc_id) => {
                    client
                        .upload_file_content(&doc_id, &file_name, &local_path)
                        .await?
                }
                None => client
                    .upload_document(&dest_folder_id, &file_name, &local_path)
                    .await
                    .map(|_| ())?,
            }
            Ok(())
        }
        JobKind::Download {
            entry_id,
            target_path,
        } => {
            let resp = client.download_response(&entry_id).await?;
            let total = resp.content_length();
            if let Some(parent) = target_path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            let tmp = target_path.with_extension("part");
            let mut file = tokio::fs::File::create(&tmp).await?;
            let mut stream = resp.bytes_stream();
            let mut done: u64 = 0;
            let mut last_emitted = std::time::Instant::now();
            while let Some(chunk) = stream.next().await {
                if cancel.load(Ordering::Relaxed) {
                    drop(file);
                    let _ = tokio::fs::remove_file(&tmp).await;
                    return Err(AppError::new("cancelled", "download cancelled"));
                }
                let chunk = chunk.map_err(|e| AppError::new("network", e.to_string()))?;
                file.write_all(&chunk).await?;
                done += chunk.len() as u64;
                if let Some(total) = total {
                    if total > 0 && last_emitted.elapsed().as_millis() > 200 {
                        last_emitted = std::time::Instant::now();
                        set_progress(state, id, done as f64 / total as f64).await;
                    }
                }
            }
            file.flush().await?;
            drop(file);
            tokio::fs::rename(&tmp, &target_path).await?;
            Ok(())
        }
        JobKind::UploadTemplate {
            local_path,
            template_name,
            file_name,
        } => {
            client
                .upload_template(&template_name, &file_name, &local_path)
                .await?;
            Ok(())
        }
    }
}

async fn set_progress(state: &Arc<AppState>, id: u64, progress: f64) {
    {
        let mut ts = state.transfers.lock().await;
        if let Some(job) = ts.jobs.iter_mut().find(|j| j.snapshot.id == id) {
            job.snapshot.progress = Some(progress.clamp(0.0, 1.0));
        }
    }
    emit_snapshot(state).await;
}
