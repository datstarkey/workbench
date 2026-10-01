//! Images attached to a chat message from a file (drag-and-drop), encoded the
//! way the Claude API takes them.

use std::path::Path;

use anyhow::{bail, Context, Result};
use base64::Engine;
use serde::Serialize;

/// The Claude API's per-image limit.
const MAX_IMAGE_BYTES: u64 = 5 * 1024 * 1024;

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChatImage {
    pub media_type: String,
    /// Base64 of the file's bytes.
    pub data: String,
    pub name: String,
}

/// The API media type for an image file, by extension.
pub fn media_type(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

/// Read an image file for a chat message. Only PNG/JPEG/GIF/WebP under 5 MB.
pub fn read(path: &Path) -> Result<ChatImage> {
    let Some(media_type) = media_type(path) else {
        bail!("only PNG, JPEG, GIF and WebP images can be attached");
    };
    let size = std::fs::metadata(path)
        .with_context(|| format!("can't read {}", path.display()))?
        .len();
    if size > MAX_IMAGE_BYTES {
        bail!("images must be under 5 MB");
    }
    let bytes = std::fs::read(path).with_context(|| format!("can't read {}", path.display()))?;
    Ok(ChatImage {
        media_type: media_type.to_string(),
        data: base64::engine::general_purpose::STANDARD.encode(bytes),
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_supported_images_and_refuses_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("shot.PNG");
        std::fs::write(&png, [0x89, b'P', b'N', b'G']).unwrap();
        let image = read(&png).unwrap();
        assert_eq!(image.media_type, "image/png");
        assert_eq!(image.data, "iVBORw==");
        assert_eq!(image.name, "shot.PNG");

        let txt = dir.path().join("notes.txt");
        std::fs::write(&txt, "hi").unwrap();
        assert!(read(&txt).is_err());
        assert!(read(&dir.path().join("missing.png")).is_err());
    }
}
