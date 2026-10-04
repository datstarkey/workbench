//! Images and files attached to a chat prompt.

use anyhow::{bail, Result};
use workbench_core::chat_attachment::{MAX_PDF_BYTES, MAX_TEXT_BYTES, PDF_TYPE, TEXT_TYPE};

/// An image pasted into the chat, base64-encoded.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptImage {
    pub media_type: String,
    pub data: String,
}

/// A PDF (base64) or text file (the text itself), sent as a document block.
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

#[cfg(test)]
mod tests {
    use super::*;

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
