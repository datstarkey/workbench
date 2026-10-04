//! Files attached to a chat message from disk (drag-and-drop), encoded the way
//! the Claude API takes them: images as image blocks, PDFs and text files as
//! document blocks.

use std::io::Read;
use std::path::Path;

use anyhow::{bail, Context, Result};
use base64::Engine;
use serde::Serialize;

/// The Claude API's per-image limit.
pub const MAX_IMAGE_BYTES: u64 = 5 * 1024 * 1024;
/// Well under the API's 32 MB request cap, so a few fit in one message.
pub const MAX_PDF_BYTES: u64 = 10 * 1024 * 1024;
/// Roughly 64k tokens: a text file goes into the context whole.
pub const MAX_TEXT_BYTES: u64 = 256 * 1024;
/// PDFs and text files per message (images have their own cap).
pub const MAX_FILES: usize = 5;
pub const PDF_TYPE: &str = "application/pdf";
pub const TEXT_TYPE: &str = "text/plain";

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChatImage {
    pub media_type: String,
    /// Base64 of the file's bytes.
    pub data: String,
    pub name: String,
}

/// A PDF or text file, sent as a document block.
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChatFile {
    /// [`PDF_TYPE`] or [`TEXT_TYPE`].
    pub media_type: String,
    /// Base64 for a PDF, the text itself for a text file (the API's two source kinds).
    pub data: String,
    pub name: String,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ChatAttachment {
    Image(ChatImage),
    File(ChatFile),
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

fn is_pdf(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
}

/// Read a file for a chat message: a PNG/JPEG/GIF/WebP image, a PDF, or any
/// UTF-8 text file, each under its size limit.
pub fn read(path: &Path) -> Result<ChatAttachment> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let size = std::fs::metadata(path)
        .with_context(|| format!("can't read {}", path.display()))?
        .len();
    if let Some(media_type) = media_type(path) {
        if size > MAX_IMAGE_BYTES {
            bail!("images must be under 5 MB");
        }
        return Ok(ChatAttachment::Image(ChatImage {
            media_type: media_type.to_string(),
            data: base64_file(path)?,
            name,
        }));
    }
    if is_pdf(path) {
        if size > MAX_PDF_BYTES {
            bail!("PDFs must be under 10 MB");
        }
        return Ok(ChatAttachment::File(ChatFile {
            media_type: PDF_TYPE.to_string(),
            data: base64_file(path)?,
            name,
        }));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(MAX_TEXT_BYTES + 1).read_to_end(&mut bytes))
        .with_context(|| format!("can't read {}", path.display()))?;
    if bytes.contains(&0) {
        bail!("only images, PDFs and text files can be attached");
    }
    if bytes.len() as u64 > MAX_TEXT_BYTES {
        bail!("text files must be under 256 KB");
    }
    let Ok(text) = String::from_utf8(bytes) else {
        bail!("only images, PDFs and text files can be attached");
    };
    Ok(ChatAttachment::File(ChatFile {
        media_type: TEXT_TYPE.to_string(),
        data: text,
        name,
    }))
}

fn base64_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).with_context(|| format!("can't read {}", path.display()))?;
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_images_pdfs_and_text_and_refuses_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("shot.PNG");
        std::fs::write(&png, [0x89, b'P', b'N', b'G']).unwrap();
        assert_eq!(
            read(&png).unwrap(),
            ChatAttachment::Image(ChatImage {
                media_type: "image/png".into(),
                data: "iVBORw==".into(),
                name: "shot.PNG".into(),
            })
        );

        let pdf = dir.path().join("report.pdf");
        std::fs::write(&pdf, b"%PDF").unwrap();
        assert_eq!(
            read(&pdf).unwrap(),
            ChatAttachment::File(ChatFile {
                media_type: PDF_TYPE.into(),
                data: "JVBERg==".into(),
                name: "report.pdf".into(),
            })
        );

        let code = dir.path().join("main.rs");
        std::fs::write(&code, "fn main() {}\n").unwrap();
        assert_eq!(
            read(&code).unwrap(),
            ChatAttachment::File(ChatFile {
                media_type: TEXT_TYPE.into(),
                data: "fn main() {}\n".into(),
                name: "main.rs".into(),
            })
        );

        let binary = dir.path().join("app.bin");
        std::fs::write(&binary, [0x7f, b'E', b'L', b'F', 0, 1]).unwrap();
        assert!(read(&binary).is_err());
        let latin1 = dir.path().join("old.txt");
        std::fs::write(&latin1, [b'c', b'a', b'f', 0xe9]).unwrap();
        assert!(read(&latin1).is_err());
        assert!(read(&dir.path().join("missing.png")).is_err());
    }

    #[test]
    fn refuses_oversized_files() {
        let dir = tempfile::tempdir().unwrap();
        let text = dir.path().join("big.log");
        std::fs::write(&text, "a".repeat(MAX_TEXT_BYTES as usize + 1)).unwrap();
        assert!(read(&text).unwrap_err().to_string().contains("256 KB"));
        let pdf = dir.path().join("big.pdf");
        std::fs::File::create(&pdf)
            .unwrap()
            .set_len(MAX_PDF_BYTES + 1)
            .unwrap();
        assert!(read(&pdf).unwrap_err().to_string().contains("10 MB"));
    }
}
