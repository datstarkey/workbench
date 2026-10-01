//! Images attached to a chat prompt.

use anyhow::{bail, Result};

/// An image pasted into the chat, base64-encoded.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptImage {
    pub media_type: String,
    pub data: String,
}

/// Formats the Claude API accepts.
const IMAGE_TYPES: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];
pub const MAX_IMAGES: usize = 10;
/// The API's per-image cap is 5 MB decoded; base64 is 4/3 of that.
const MAX_IMAGE_BASE64: usize = 5 * 1024 * 1024 * 4 / 3 + 4;

impl PromptImage {
    pub fn validate(&self) -> Result<()> {
        if !IMAGE_TYPES.contains(&self.media_type.as_str()) {
            bail!("unsupported image type: {}", self.media_type);
        }
        if self.data.len() > MAX_IMAGE_BASE64 {
            bail!("images must be under 5 MB");
        }
        if !self
            .data
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b))
        {
            bail!("image data must be base64");
        }
        Ok(())
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

    #[test]
    fn prompt_images_must_be_api_formats_in_base64() {
        assert!(image("image/png", "iVBORw==").validate().is_ok());
        assert!(image("image/svg+xml", "PHN2Zz4=").validate().is_err());
        assert!(image("image/png", "not base64!").validate().is_err());
        assert!(image("image/png", &"A".repeat(MAX_IMAGE_BASE64 + 1))
            .validate()
            .is_err());
    }
}
