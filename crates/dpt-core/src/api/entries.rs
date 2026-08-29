//! Documents and folders (protocol §7.3; FR-BRW-1, FR-TRF-*).

use std::path::Path;

use reqwest::multipart::{Form, Part};
use reqwest::Body;
use tokio_util::io::ReaderStream;

use crate::client::DeviceClient;
use crate::model::{Entry, EntryListResponse};
use crate::Error;

/// The root folder name; always exists and cannot be deleted (protocol §10.11).
pub const ROOT: &str = "Document";

impl DeviceClient {
    /// Lists every document and folder on the device (protocol §7.3.2),
    /// transparently falling back to recursive per-folder listing when the
    /// single-call response is truncated at the ~1300-entry limit
    /// (protocol §7.3.3, §10.8; FR-BRW-1).
    pub async fn list_all_entries(&self) -> Result<Vec<Entry>, Error> {
        let resp: EntryListResponse = self.get_json("/documents2?entry_type=all").await?;
        let got = resp.entry_list.len() as u64;
        match resp.count {
            Some(count) if count > got => self.list_recursive().await,
            _ => Ok(resp.entry_list),
        }
    }

    /// Recursive per-folder traversal starting at the root folder.
    async fn list_recursive(&self) -> Result<Vec<Entry>, Error> {
        let root = self.resolve_path(ROOT).await?;
        let mut out = Vec::new();
        let mut stack = vec![root.entry_id.clone()];
        while let Some(folder_id) = stack.pop() {
            let resp: EntryListResponse = self
                .get_json(&format!("/folders/{folder_id}/entries2"))
                .await?;
            for entry in resp.entry_list {
                if entry.is_folder() {
                    stack.push(entry.entry_id.clone());
                }
                out.push(entry);
            }
        }
        Ok(out)
    }

    /// Lists the direct children of a folder (protocol §7.3.3).
    pub async fn list_folder(&self, folder_id: &str) -> Result<Vec<Entry>, Error> {
        let resp: EntryListResponse = self
            .get_json(&format!("/folders/{folder_id}/entries2"))
            .await?;
        Ok(resp.entry_list)
    }

    /// Resolves a human-readable path (e.g. `Document/Papers/a.pdf`) to its
    /// entry (protocol §7.3.1). Paths are form-encoded in the URL (§6.3).
    pub async fn resolve_path(&self, path: &str) -> Result<Entry, Error> {
        let encoded = form_encode(path);
        self.get_json(&format!("/resolve/entry/path/{encoded}"))
            .await
    }

    /// Opens a streaming download of a document's PDF bytes (protocol §7.3.4).
    /// The caller streams the response body to disk with progress.
    pub async fn download_response(&self, document_id: &str) -> Result<reqwest::Response, Error> {
        let id = document_id.to_string();
        let resp = self
            .send(move |http, base| http.get(format!("{base}/documents/{id}/file")))
            .await?;
        if !resp.status().is_success() {
            return Err(Error::Api {
                status: resp.status().as_u16(),
                message: "download failed".into(),
            });
        }
        Ok(resp)
    }

    /// Creates a folder under `parent_folder_id` (protocol §7.3.6). Parents
    /// must already exist; create one level at a time.
    pub async fn create_folder(&self, parent_folder_id: &str, name: &str) -> Result<(), Error> {
        self.put_or_post_folder(parent_folder_id, name).await
    }

    async fn put_or_post_folder(&self, parent_folder_id: &str, name: &str) -> Result<(), Error> {
        #[derive(serde::Deserialize)]
        struct Created {
            #[allow(dead_code)]
            folder_id: Option<String>,
        }
        let _: Created = self
            .post_json(
                "/folders2",
                &serde_json::json!({
                    "folder_name": name,
                    "parent_folder_id": parent_folder_id,
                }),
            )
            .await?;
        Ok(())
    }

    /// Creates a document entry and returns its new id (protocol §7.3.5 step 1).
    pub async fn create_document(
        &self,
        parent_folder_id: &str,
        file_name: &str,
    ) -> Result<String, Error> {
        #[derive(serde::Deserialize)]
        struct Created {
            document_id: String,
        }
        let created: Created = self
            .post_json(
                "/documents2",
                &serde_json::json!({
                    "file_name": file_name,
                    "parent_folder_id": parent_folder_id,
                    "document_source": "",
                }),
            )
            .await?;
        Ok(created.document_id)
    }

    /// Uploads (or replaces) a document's content by streaming a local file
    /// (protocol §7.3.5 step 2, §8). Used both for new documents and to
    /// overwrite an existing `document_id`.
    pub async fn upload_file_content(
        &self,
        document_id: &str,
        file_name: &str,
        local_path: &Path,
    ) -> Result<(), Error> {
        // The device's embedded HTTP server rejects chunked uploads, so we
        // must send a Content-Length. Providing the part length lets reqwest
        // compute the full multipart length and emit Content-Length instead
        // of Transfer-Encoding: chunked.
        let len = tokio::fs::metadata(local_path).await?.len();
        let file = tokio::fs::File::open(local_path).await?;
        let stream = ReaderStream::new(file);
        let part = Part::stream_with_length(Body::wrap_stream(stream), len)
            .file_name(form_encode(file_name))
            .mime_str("application/octet-stream")
            .map_err(|e| Error::Protocol(e.to_string()))?;
        let form = Form::new().part("file", part);

        let cookie = self.cookie_header().await?;
        let resp = self
            .http()
            .put(format!(
                "{}/documents/{}/file",
                self.api_base(),
                document_id
            ))
            .header(reqwest::header::COOKIE, cookie)
            .multipart(form)
            .send()
            .await?;
        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            Err(Error::Api {
                status,
                message: if text.is_empty() {
                    "upload failed".into()
                } else {
                    text
                },
            })
        }
    }

    /// Full upload: create the entry then stream the content, deleting the
    /// created entry if the content upload fails so no zero-byte ghost is
    /// left behind (FR-TRF-10). Returns the new document id.
    pub async fn upload_document(
        &self,
        parent_folder_id: &str,
        file_name: &str,
        local_path: &Path,
    ) -> Result<String, Error> {
        let document_id = self.create_document(parent_folder_id, file_name).await?;
        if let Err(e) = self
            .upload_file_content(&document_id, file_name, local_path)
            .await
        {
            let _ = self.delete_document(&document_id).await;
            return Err(e);
        }
        Ok(document_id)
    }

    /// Deletes a document (protocol §7.3.7). Irreversible on the device.
    pub async fn delete_document(&self, document_id: &str) -> Result<(), Error> {
        self.delete_ok(&format!("/documents/{document_id}")).await
    }

    /// Deletes a folder and its contents (protocol §7.3.7). Irreversible.
    pub async fn delete_folder(&self, folder_id: &str) -> Result<(), Error> {
        self.delete_ok(&format!("/folders/{folder_id}")).await
    }

    /// Moves and/or renames a document (protocol §7.3.8).
    pub async fn move_document(
        &self,
        document_id: &str,
        target_folder_id: &str,
        new_name: Option<&str>,
    ) -> Result<(), Error> {
        let mut body = serde_json::json!({ "parent_folder_id": target_folder_id });
        if let Some(name) = new_name {
            body["file_name"] = serde_json::Value::String(name.to_string());
        }
        self.put_ok(&format!("/documents/{document_id}"), &body)
            .await
    }

    /// Copies a document to another folder, optionally renaming (protocol §7.3.9).
    pub async fn copy_document(
        &self,
        document_id: &str,
        target_folder_id: &str,
        new_name: Option<&str>,
    ) -> Result<(), Error> {
        let mut body = serde_json::json!({ "parent_folder_id": target_folder_id });
        if let Some(name) = new_name {
            body["file_name"] = serde_json::Value::String(name.to_string());
        }
        let _: serde_json::Value = self
            .post_json(&format!("/documents/{document_id}/copy"), &body)
            .await?;
        Ok(())
    }

    /// Opens a document on the device screen at a 1-based page (protocol §7.5;
    /// FR-BRW-6).
    pub async fn open_on_device(&self, document_id: &str, page: u32) -> Result<(), Error> {
        self.put_ok(
            "/viewer/controls/open2",
            &serde_json::json!({ "document_id": document_id, "page": page }),
        )
        .await
    }
}

/// Form-encodes a string like Python's `quote_plus` (protocol §6.3, §8):
/// everything except `A-Za-z0-9-_.~` is percent-encoded and spaces become
/// `+`. Applied to whole paths including their `/` separators.
pub fn form_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_encode_matches_quote_plus() {
        assert_eq!(
            form_encode("Document/My Folder/a b.pdf"),
            "Document%2FMy+Folder%2Fa+b.pdf"
        );
    }
}
