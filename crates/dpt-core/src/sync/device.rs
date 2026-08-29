//! Device abstraction for the sync engine.
//!
//! The engine talks to the device exclusively through [`SyncDevice`], so
//! the scenario test suite can drive it against an in-memory fake device
//! (NFR-QLT-3) while production uses [`DeviceClient`].

use std::path::Path;

use async_trait::async_trait;
use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;

use crate::client::DeviceClient;
use crate::model::Entry;
use crate::{Error, Result};

/// The device operations the sync engine needs (docs/06 §6).
#[async_trait]
pub trait SyncDevice: Send + Sync {
    /// Complete entry listing (with the 1300-entry fallback, FR-BRW-1).
    async fn sync_list_entries(&self) -> Result<Vec<Entry>>;

    /// Resolves a device path (e.g. `Document/Papers/a.pdf`) to its entry.
    async fn sync_resolve_path(&self, path: &str) -> Result<Entry>;

    /// Streams a document's bytes into `dest` (the caller supplies a
    /// `*.part` path and renames afterwards, NFR-REL-2).
    async fn sync_download_to(&self, entry_id: &str, dest: &Path) -> Result<()>;

    /// Creates a document and uploads its content; returns the new entry id.
    /// Cleans up the half-created entry on failure (FR-TRF-10).
    async fn sync_upload_new(
        &self,
        parent_folder_id: &str,
        file_name: &str,
        local_path: &Path,
    ) -> Result<String>;

    /// Replaces an existing document's content.
    async fn sync_upload_replace(
        &self,
        document_id: &str,
        file_name: &str,
        local_path: &Path,
    ) -> Result<()>;

    /// Creates one folder level and returns its entry id (protocol §7.3.6).
    async fn sync_create_folder(&self, parent_folder_id: &str, name: &str) -> Result<String>;

    async fn sync_delete_document(&self, document_id: &str) -> Result<()>;

    /// Deletes a folder. NOTE: the device deletes recursively; the planner
    /// only schedules this when everything beneath is already gone.
    async fn sync_delete_folder(&self, folder_id: &str) -> Result<()>;
}

#[async_trait]
impl SyncDevice for DeviceClient {
    async fn sync_list_entries(&self) -> Result<Vec<Entry>> {
        self.list_all_entries().await
    }

    async fn sync_resolve_path(&self, path: &str) -> Result<Entry> {
        self.resolve_path(path).await
    }

    async fn sync_download_to(&self, entry_id: &str, dest: &Path) -> Result<()> {
        let resp = self.download_response(entry_id).await?;
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let mut file = tokio::fs::File::create(dest).await?;
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| Error::Network(e.to_string()))?;
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        Ok(())
    }

    async fn sync_upload_new(
        &self,
        parent_folder_id: &str,
        file_name: &str,
        local_path: &Path,
    ) -> Result<String> {
        self.upload_document(parent_folder_id, file_name, local_path)
            .await
    }

    async fn sync_upload_replace(
        &self,
        document_id: &str,
        file_name: &str,
        local_path: &Path,
    ) -> Result<()> {
        self.upload_file_content(document_id, file_name, local_path)
            .await
    }

    async fn sync_create_folder(&self, parent_folder_id: &str, name: &str) -> Result<String> {
        self.create_folder(parent_folder_id, name).await?;
        // The create endpoint does not reliably return the id; find the new
        // folder among the parent's children.
        let children = self.list_folder(parent_folder_id).await?;
        children
            .into_iter()
            .find(|e| e.is_folder() && e.entry_name == name)
            .map(|e| e.entry_id)
            .ok_or_else(|| Error::Protocol(format!("created folder '{name}' not found")))
    }

    async fn sync_delete_document(&self, document_id: &str) -> Result<()> {
        self.delete_document(document_id).await
    }

    async fn sync_delete_folder(&self, folder_id: &str) -> Result<()> {
        self.delete_folder(folder_id).await
    }
}
