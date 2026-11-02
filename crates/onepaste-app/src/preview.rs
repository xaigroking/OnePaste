//! Read-only document text. No external relationships, executable content or plaintext files.
use crate::AppError;
use serde::Serialize;
use std::io::{Cursor, Read};

#[path = "preview_doc.rs"]
mod doc;
pub const MAX_PREVIEW_TEXT: usize = 2 * 1024 * 1024;
const MAX_XML: usize = 8 * 1024 * 1024;
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentPreview {
    pub format: String,
    pub text: String,
}
pub(super) fn error(code: &'static str) -> AppError {
    AppError::Attachment(code)
}
pub(super) fn bounded(text: String) -> Result<String, AppError> {
    if text.len() > MAX_PREVIEW_TEXT {
        Err(error("preview_limit"))
    } else {
        Ok(text)
    }
}
pub fn preview_document(name: &str, bytes: &[u8]) -> Result<DocumentPreview, AppError> {
    if bytes.len() > onepaste_core::attachment::MAX_FILE_BYTES as usize {
        return Err(error("preview_limit"));
    }
    let format = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    let text = match format.as_str() {
        "txt" => text(bytes)?,
        "docx" => docx(bytes)?,
        "doc" => doc::text(bytes)?,
        _ => return Err(error("preview_unsupported")),
    };
    Ok(DocumentPreview {
        format,
        text: bounded(text)?,
    })
}
fn text(bytes: &[u8]) -> Result<String, AppError> {
    if bytes.len() > MAX_XML {
        return Err(error("preview_limit"));
    }
    let value = if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        if bytes.len() % 2 != 0 {
            return Err(error("preview_encoding"));
        }
        let little = bytes[0] == 0xff;
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|p| {
                if little {
                    u16::from_le_bytes([p[0], p[1]])
                } else {
                    u16::from_be_bytes([p[0], p[1]])
                }
            })
            .collect();
        String::from_utf16(&units).map_err(|_| error("preview_encoding"))?
    } else {
        std::str::from_utf8(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes))
            .map_err(|_| error("preview_encoding"))?
            .to_owned()
    };
    if value
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(error("preview_encoding"));
    }
    bounded(value)
}
fn docx(bytes: &[u8]) -> Result<String, AppError> {
    // Encrypted OOXML uses a compound file rather than an ordinary ZIP package.
    if bytes.starts_with(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]) {
        return Err(error("preview_encrypted"));
    }
    // Reject excessive entry counts before the ZIP reader allocates its directory index.
    let start = bytes.len().saturating_sub(65_557);
    let end = (start..bytes.len().saturating_sub(21))
        .rev()
        .find(|at| {
            bytes.get(*at..*at + 4) == Some(b"PK\x05\x06")
                && bytes.get(*at + 20..*at + 22).is_some_and(|p| {
                    *at + 22 + u16::from_le_bytes([p[0], p[1]]) as usize == bytes.len()
                })
        })
        .ok_or(error("preview_corrupt"))?;
    let count = u16::from_le_bytes([bytes[end + 10], bytes[end + 11]]);
    if count > 2048 {
        return Err(error("preview_limit"));
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| error("preview_corrupt"))?;
    if zip.len() > 2048 {
        return Err(error("preview_limit"));
    }
    let mut file = zip
        .by_name("word/document.xml")
        .map_err(|_| error("preview_corrupt"))?;
    if file.encrypted() {
        return Err(error("preview_encrypted"));
    }
    if file.size() > MAX_XML as u64 {
        return Err(error("preview_limit"));
    }
    let mut xml = zeroize::Zeroizing::new(Vec::new());
    (&mut file)
        .take(MAX_XML as u64 + 1)
        .read_to_end(&mut xml)
        .map_err(|_| error("preview_corrupt"))?;
    if xml.len() > MAX_XML {
        return Err(error("preview_limit"));
    }
    let mut reader = quick_xml::Reader::from_reader(xml.as_slice());
    let mut output = String::new();
    let (mut depth, mut body, mut in_text, mut seen_body) = (0usize, false, false, false);
    use quick_xml::events::Event;
    loop {
        match reader.read_event().map_err(|_| error("preview_corrupt"))? {
            Event::Start(e) => {
                depth += 1;
                if depth > 128 {
                    return Err(error("preview_limit"));
                }
                match e.local_name().as_ref() {
                    "body" => {
                        body = true;
                        seen_body = true;
                    }
                    "t" if body => in_text = true,
                    _ => {}
                }
            }
            Event::Empty(e) if e.local_name().as_ref() == "body" => seen_body = true,
            Event::Empty(e) if body => match e.local_name().as_ref() {
                "tab" => output.push('\t'),
                "br" | "cr" => output.push('\n'),
                _ => {}
            },
            Event::End(e) => {
                depth = depth.checked_sub(1).ok_or(error("preview_corrupt"))?;
                match e.local_name().as_ref() {
                    "t" => in_text = false,
                    "p" if body => output.push('\n'),
                    "tc" if body => output.push('\t'),
                    "body" => body = false,
                    _ => {}
                }
            }
            Event::Text(e) if in_text => output.push_str(&e.xml10_content()),
            Event::CData(e) if in_text => output.push_str(&e.xml10_content()),
            Event::GeneralRef(e) if in_text => {
                let reference: &str = e.as_ref();
                let escaped = format!("&{reference};");
                output.push_str(
                    &quick_xml::escape::unescape(&escaped).map_err(|_| error("preview_corrupt"))?,
                );
            }
            Event::DocType(_) => return Err(error("preview_unsupported")),
            Event::Eof => break,
            _ => {}
        }
        if output.len() > MAX_PREVIEW_TEXT {
            return Err(error("preview_limit"));
        }
    }
    if depth != 0 || !seen_body {
        return Err(error("preview_corrupt"));
    }
    Ok(output)
}

impl crate::App {
    pub async fn document_preview(&self, id: &str) -> Result<DocumentPreview, AppError> {
        let epoch = self.session_epoch();
        let info = self.attachment_info(id).await?;
        let bytes = zeroize::Zeroizing::new(self.attachment_bytes(id).await?);
        let preview = preview_document(&info.file_name, &bytes)?;
        // A lock or a replacement session must not publish a completed old preview.
        let _guard = self.session_access().await?;
        if epoch != self.session_epoch() {
            return Err(AppError::Locked);
        }
        Ok(preview)
    }
}

impl crate::App {
    pub async fn pdf_preview_bytes(&self, id: &str) -> Result<Vec<u8>, AppError> {
        let epoch = self.session_epoch();
        let info = self.attachment_info(id).await?;
        if !info.file_name.to_ascii_lowercase().ends_with(".pdf") {
            return Err(error("preview_unsupported"));
        }
        let bytes = zeroize::Zeroizing::new(self.attachment_bytes(id).await?);
        if !bytes.starts_with(b"%PDF-") {
            return Err(error("preview_corrupt"));
        }
        let _guard = self.session_access().await?;
        if epoch != self.session_epoch() {
            return Err(AppError::Locked);
        }
        Ok(bytes.to_vec())
    }
}
