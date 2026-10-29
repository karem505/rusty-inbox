//! Text out of attachments, for "summarize this attachment" (feature 10). WORKER C1 implements:
//! PDF (pdf-extract), DOCX (zip + word/document.xml), plain text / CSV / Markdown / JSON / HTML (tags
//! stripped), EML (body text). Output is trimmed to `max_chars`.

use std::path::Path;

/// File types [`text_of`] understands, by MIME type or file name.
pub fn supported(mime: &str, name: &str) -> bool {
    let _ = (mime, name);
    false
}

/// The readable text of the file at `path`, at most `max_chars` characters. `None` when the type is
/// not supported or nothing readable is inside (for example a scanned PDF).
pub fn text_of(path: &Path, mime: &str, max_chars: usize) -> Option<String> {
    let _ = (path, mime, max_chars);
    None
}
