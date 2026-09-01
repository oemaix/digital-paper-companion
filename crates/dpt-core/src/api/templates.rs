//! Note templates (protocol §7.4; FR-BRW-7, FR-TRF-6).
//!
//! Quirk: creation uses the camelCase key `templateName`, unlike the
//! snake_case used everywhere else.

use std::path::Path;

use crate::client::DeviceClient;
use crate::model::NoteTemplate;
use crate::Error;

/// Body of `POST /viewer/configs/note_templates`. Kept as a function so the
/// camelCase quirk is pinned by a unit test.
fn create_body(name: &str) -> serde_json::Value {
    serde_json::json!({ "templateName": name, "document_source": "" })
}

impl DeviceClient {
    /// Lists all note templates (protocol §7.4; FR-BRW-7).
    pub async fn list_templates(&self) -> Result<Vec<NoteTemplate>, Error> {
        #[derive(serde::Deserialize)]
        struct TemplateList {
            #[serde(default)]
            template_list: Vec<NoteTemplate>,
        }
        let resp: TemplateList = self.get_json("/viewer/configs/note_templates").await?;
        Ok(resp.template_list)
    }

    /// Creates a template entry and returns its id (protocol §7.4 step 1).
    pub async fn create_template(&self, name: &str) -> Result<String, Error> {
        #[derive(serde::Deserialize)]
        struct Created {
            note_template_id: String,
        }
        let created: Created = self
            .post_json("/viewer/configs/note_templates", &create_body(name))
            .await?;
        Ok(created.note_template_id)
    }

    /// Uploads (or replaces) a template's PDF content (protocol §7.4, §8).
    pub async fn upload_template_content(
        &self,
        template_id: &str,
        file_name: &str,
        local_path: &Path,
    ) -> Result<(), Error> {
        self.put_file_multipart(
            &format!("/viewer/configs/note_templates/{template_id}/file"),
            file_name,
            local_path,
        )
        .await
    }

    /// Full template upload: create the entry then stream the PDF, deleting
    /// the created entry if the content upload fails so no broken template
    /// is left behind (FR-TRF-6, same ghost-entry rule as FR-TRF-10).
    /// Returns the new template id.
    pub async fn upload_template(
        &self,
        name: &str,
        file_name: &str,
        local_path: &Path,
    ) -> Result<String, Error> {
        let template_id = self.create_template(name).await?;
        if let Err(e) = self
            .upload_template_content(&template_id, file_name, local_path)
            .await
        {
            let _ = self.delete_template(&template_id).await;
            return Err(e);
        }
        Ok(template_id)
    }

    /// Deletes a template (protocol §7.4). Irreversible on the device.
    pub async fn delete_template(&self, template_id: &str) -> Result<(), Error> {
        self.delete_ok(&format!("/viewer/configs/note_templates/{template_id}"))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The device expects camelCase `templateName` here — everywhere else
    /// the API uses snake_case (protocol §7.4).
    #[test]
    fn create_body_uses_camel_case_quirk() {
        let body = create_body("Daily Planner");
        assert_eq!(body["templateName"], "Daily Planner");
        assert_eq!(body["document_source"], "");
        assert!(body.get("template_name").is_none());
    }
}
