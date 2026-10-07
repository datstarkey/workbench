//! Images and files attached to a chat prompt.

use anyhow::{bail, Context, Result};
use base64::Engine;
use workbench_core::chat_attachment::{MAX_PDF_BYTES, MAX_TEXT_BYTES, PDF_TYPE, TEXT_TYPE};

/// An image pasted into the chat, base64-encoded.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptImage {
    pub media_type: String,
    pub data: String,
}

/// An uploaded PDF (base64) or text file (the text itself).
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptFile {
    pub name: String,
    pub media_type: String,
    pub data: String,
}

/// Formats the Claude API accepts.
const IMAGE_TYPES: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];
pub const MAX_IMAGES: usize = 10;
pub use workbench_core::chat_attachment::MAX_FILES;
/// The API's per-image cap is 5 MB decoded; base64 is 4/3 of that.
const MAX_IMAGE_BASE64: usize = 5 * 1024 * 1024 * 4 / 3 + 4;
const MAX_PDF_BASE64: usize = MAX_PDF_BYTES as usize * 4 / 3 + 4;
const MAX_NAME_CHARS: usize = 255;

fn is_base64(data: &str) -> bool {
    data.bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b))
}

impl PromptImage {
    pub fn validate(&self) -> Result<()> {
        if !IMAGE_TYPES.contains(&self.media_type.as_str()) {
            bail!("unsupported image type: {}", self.media_type);
        }
        if self.data.len() > MAX_IMAGE_BASE64 {
            bail!("images must be under 5 MB");
        }
        if !is_base64(&self.data) {
            bail!("image data must be base64");
        }
        Ok(())
    }
}

impl PromptFile {
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() || self.name.chars().count() > MAX_NAME_CHARS {
            bail!("attached files need a name of at most {MAX_NAME_CHARS} characters");
        }
        match self.media_type.as_str() {
            PDF_TYPE if self.data.len() > MAX_PDF_BASE64 => bail!("PDFs must be under 10 MB"),
            PDF_TYPE if !is_base64(&self.data) => bail!("PDF data must be base64"),
            TEXT_TYPE if self.data.len() > MAX_TEXT_BYTES as usize => {
                bail!("text files must be under 256 KB")
            }
            PDF_TYPE | TEXT_TYPE => Ok(()),
            other => bail!("unsupported file type: {other}"),
        }
    }
}

/// Where a session's attachments are saved; removed when the session ends.
pub fn attachment_dir(session_id: &str) -> std::path::PathBuf {
    std::env::temp_dir().join("workbench-chat").join(session_id)
}

/// Save uploads and append `@path` references to the prompt. A terminal
/// plugin's prompt never has its mentions expanded, so the plugin lists the
/// files for Claude to Read (images included); Codex reads document uploads
/// with tools and keeps images as native input.
pub fn attachments_as_mentions(
    session_id: &str,
    text: &str,
    images: &[PromptImage],
    files: &[PromptFile],
) -> Result<String> {
    if images.is_empty() && files.is_empty() {
        return Ok(text.to_string());
    }
    let dir = attachment_dir(session_id).join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    // Other local users can read a world-readable temp dir.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for d in [dir.parent().unwrap_or(&dir), &dir] {
            std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    let b64 = base64::engine::general_purpose::STANDARD;
    let mut paths = Vec::new();
    for (i, image) in images.iter().enumerate() {
        let ext = image.media_type.rsplit('/').next().unwrap_or("png");
        let path = dir.join(format!("image-{}.{ext}", i + 1));
        std::fs::write(&path, b64.decode(&image.data).context("decode image")?)?;
        paths.push(path);
    }
    for (i, file) in files.iter().enumerate() {
        let name: String = std::path::Path::new(&file.name)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();
        let path = dir.join(format!("{}-{name}", i + 1));
        let bytes = if file.media_type == PDF_TYPE {
            b64.decode(&file.data).context("decode PDF")?
        } else {
            file.data.clone().into_bytes()
        };
        std::fs::write(&path, bytes)?;
        paths.push(path);
    }
    let mentions: Vec<String> = paths
        .iter()
        .map(|p| {
            let p = p.to_string_lossy();
            if p.contains(' ') {
                format!("@\"{p}\"")
            } else {
                format!("@{p}")
            }
        })
        .collect();
    Ok(format!("{text}\n\n{}", mentions.join(" "))
        .trim_start()
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachments_become_mentions_of_saved_files() {
        let id = uuid::Uuid::new_v4().to_string();
        let image = PromptImage {
            media_type: "image/png".into(),
            data: "aGk=".into(),
        };
        let file = PromptFile {
            name: "../notes.txt".into(),
            media_type: "text/plain".into(),
            data: "hello".into(),
        };
        let text = attachments_as_mentions(&id, "look", &[image], &[file]).unwrap();
        let (prompt, mentions) = text.split_once("\n\n").unwrap();
        assert_eq!(prompt, "look");
        let paths: Vec<&str> = mentions
            .split(' ')
            .map(|m| m.trim_start_matches('@'))
            .collect();
        assert_eq!(std::fs::read(paths[0]).unwrap(), b"hi");
        assert!(
            paths[1].ends_with("notes.txt"),
            "a name can't climb out of the folder"
        );
        assert_eq!(std::fs::read_to_string(paths[1]).unwrap(), "hello");
        assert_eq!(
            attachments_as_mentions(&id, "plain", &[], &[]).unwrap(),
            "plain"
        );
        std::fs::remove_dir_all(attachment_dir(&id)).unwrap();
    }

    fn image(media_type: &str, data: &str) -> PromptImage {
        PromptImage {
            media_type: media_type.into(),
            data: data.into(),
        }
    }

    fn file(name: &str, media_type: &str, data: &str) -> PromptFile {
        PromptFile {
            name: name.into(),
            media_type: media_type.into(),
            data: data.into(),
        }
    }

    #[test]
    fn prompt_images_must_be_api_formats_in_base64() {
        assert!(image("image/png", "iVBORw==").validate().is_ok());
        assert!(image("image/svg+xml", "PHN2Zz4=").validate().is_err());
        assert!(image("image/png", "not base64!").validate().is_err());
        assert!(image("image/png", &"A".repeat(MAX_IMAGE_BASE64 + 1))
            .validate()
            .is_err());
    }

    #[test]
    fn prompt_files_are_named_pdfs_or_text_under_their_limits() {
        assert!(file("a.pdf", PDF_TYPE, "JVBERg==").validate().is_ok());
        assert!(file("a.rs", TEXT_TYPE, "fn main() {} // ünïcode")
            .validate()
            .is_ok());
        assert!(file("a.pdf", PDF_TYPE, "not base64!").validate().is_err());
        assert!(file("a.pdf", PDF_TYPE, &"A".repeat(MAX_PDF_BASE64 + 1))
            .validate()
            .is_err());
        assert!(
            file("a.txt", TEXT_TYPE, &"a".repeat(MAX_TEXT_BYTES as usize + 1))
                .validate()
                .is_err()
        );
        assert!(file("a.zip", "application/zip", "UEs=").validate().is_err());
        assert!(file(" ", TEXT_TYPE, "hi").validate().is_err());
        assert!(file(&"n".repeat(MAX_NAME_CHARS + 1), TEXT_TYPE, "hi")
            .validate()
            .is_err());
    }
}
