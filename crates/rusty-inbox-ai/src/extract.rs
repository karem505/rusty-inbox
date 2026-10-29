//! Text out of attachments, for "summarize this attachment" (feature 10): PDF (pdf-extract), DOCX
//! (zip + word/document.xml), plain text / CSV / Markdown / JSON / HTML (tags stripped) and EML
//! (headline headers and body). Output is trimmed to `max_chars`. Never runs or opens anything.

use std::io::Read;
use std::path::Path;

/// Files larger than this are not read.
const MAX_FILE_BYTES: u64 = 40 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Pdf,
    Docx,
    Text,
    Html,
    Eml,
}

fn extension(name: &str) -> String {
    name.rsplit_once('.').map(|x| x.1.to_lowercase()).unwrap_or_default()
}

fn kind_of(mime: &str, name: &str) -> Option<Kind> {
    match extension(name).as_str() {
        "pdf" => return Some(Kind::Pdf),
        "docx" => return Some(Kind::Docx),
        "txt" | "text" | "csv" | "tsv" | "md" | "markdown" | "json" | "log" | "xml" | "yaml" | "yml" | "ics" => {
            return Some(Kind::Text)
        }
        "html" | "htm" => return Some(Kind::Html),
        "eml" => return Some(Kind::Eml),
        _ => {}
    }
    let mime = mime.trim().to_lowercase();
    let mime = mime.split(';').next().unwrap_or("").trim();
    match mime {
        "application/pdf" => Some(Kind::Pdf),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => Some(Kind::Docx),
        "text/html" | "application/xhtml+xml" => Some(Kind::Html),
        "message/rfc822" => Some(Kind::Eml),
        "application/json" | "application/xml" | "application/x-yaml" => Some(Kind::Text),
        m if m.starts_with("text/") => Some(Kind::Text),
        _ => None,
    }
}

/// File types [`text_of`] understands, by MIME type or file name.
pub fn supported(mime: &str, name: &str) -> bool {
    kind_of(mime, name).is_some()
}

/// The readable text of the file at `path`, at most `max_chars` characters. `None` when the type is
/// not supported or nothing readable is inside (for example a scanned PDF).
pub fn text_of(path: &Path, mime: &str, max_chars: usize) -> Option<String> {
    let name = path.file_name()?.to_string_lossy().to_string();
    let kind = kind_of(mime, &name)?;
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return None;
    }
    let text = match kind {
        Kind::Pdf => pdf_text(path)?,
        Kind::Docx => docx_text(path)?,
        Kind::Text => String::from_utf8_lossy(&std::fs::read(path).ok()?).replace("\r\n", "\n"),
        Kind::Html => html_to_text(&String::from_utf8_lossy(&std::fs::read(path).ok()?)),
        Kind::Eml => eml_text(&std::fs::read(path).ok()?),
    };
    let text = text.trim();
    (!text.is_empty()).then(|| text.chars().take(max_chars).collect())
}

/// PDF text. Some malformed PDFs make the parser panic: that is treated as "no text".
fn pdf_text(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let r = std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem(&bytes));
    match r {
        Ok(Ok(t)) => {
            // collapse the runs of blank lines the extractor leaves between positioned text
            let mut out = String::new();
            let mut blank = 0;
            for line in t.lines() {
                if line.trim().is_empty() {
                    blank += 1;
                    if blank > 1 {
                        continue;
                    }
                } else {
                    blank = 0;
                }
                out.push_str(line.trim_end());
                out.push('\n');
            }
            Some(out)
        }
        Ok(Err(e)) => {
            tracing::debug!("pdf text: {e}");
            None
        }
        Err(_) => {
            tracing::warn!("pdf text: the parser panicked");
            None
        }
    }
}

/// DOCX text: paragraphs of `word/document.xml`, one per line.
fn docx_text(path: &Path) -> Option<String> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).ok()?).ok()?;
    let mut xml = String::new();
    zip.by_name("word/document.xml").ok()?.take(MAX_FILE_BYTES).read_to_string(&mut xml).ok()?;
    Some(docx_xml_text(&xml))
}

fn docx_xml_text(xml: &str) -> String {
    let mut out = String::new();
    let mut rest = xml;
    while let Some(i) = rest.find('<') {
        rest = &rest[i + 1..];
        let Some(end) = rest.find('>') else { break };
        let tag = &rest[..end];
        rest = &rest[end + 1..];
        let closing = tag.starts_with('/');
        let name = tag.trim_start_matches('/').split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("");
        match (closing, name) {
            (false, "w:t") if !tag.ends_with('/') => {
                let close = rest.find("</w:t>").unwrap_or(rest.len());
                out.push_str(&decode_entities(&rest[..close]));
                rest = &rest[(close + 6).min(rest.len())..];
            }
            (false, "w:tab") => out.push('\t'),
            (false, "w:br" | "w:cr") => out.push('\n'),
            (true, "w:p") => out.push('\n'),
            (true, "w:tc") => out.push('\t'),
            _ => {}
        }
    }
    out.lines().map(str::trim_end).collect::<Vec<_>>().join("\n")
}

/// `&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;`, `&nbsp;` and numeric references.
fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let decoded = rest.find(';').filter(|&e| e <= 10).and_then(|e| {
            let ent = &rest[1..e];
            let ch = match ent {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some(' '),
                _ => {
                    let num = ent.strip_prefix('#')?;
                    let n = match num.strip_prefix(['x', 'X']) {
                        Some(h) => u32::from_str_radix(h, 16).ok()?,
                        None => num.parse().ok()?,
                    };
                    char::from_u32(n)
                }
            }?;
            Some((ch, e + 1))
        });
        match decoded {
            Some((ch, len)) => {
                out.push(ch);
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Visible text of an HTML document: scripts, styles, the head and comments dropped, block tags
/// become line breaks, common entities decoded.
pub fn html_to_text(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len() / 2);
    let mut i = 0;
    let b = html.as_bytes();
    while i < b.len() {
        if b[i] != b'<' {
            let next = html[i..].find('<').map(|n| i + n).unwrap_or(b.len());
            out.push_str(&html[i..next]);
            i = next;
            continue;
        }
        if html[i..].starts_with("<!--") {
            i = html[i..].find("-->").map(|n| i + n + 3).unwrap_or(b.len());
            continue;
        }
        let Some(end) = html[i..].find('>').map(|n| i + n) else { break };
        let tag = lower[i + 1..end].trim_start_matches('/');
        let name = tag.split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("");
        let closing = lower[i + 1..].starts_with('/');
        i = end + 1;
        match name {
            "script" | "style" | "head" | "title" if !closing => {
                let close = format!("</{name}");
                i = lower[i..].find(&close).map(|n| i + n).unwrap_or(b.len());
            }
            "li" | "tr" if !closing => {}
            "br" | "p" | "div" | "li" | "tr" | "table" | "ul" | "ol" | "section" | "article" | "blockquote" | "hr"
            | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "pre" => out.push('\n'),
            "td" | "th" if closing => out.push('\t'),
            _ => {}
        }
    }
    let text = decode_entities(&out);
    let mut res = String::new();
    let mut blank = 0;
    for line in text.lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() {
            blank += 1;
            if blank > 1 || res.is_empty() {
                continue;
            }
        } else {
            blank = 0;
        }
        res.push_str(&line);
        res.push('\n');
    }
    res.trim().to_string()
}

// ---- EML ---------------------------------------------------------------------------------------

fn split_headers(raw: &str) -> (Vec<(String, String)>, &str) {
    let (head, body) = match raw.find("\r\n\r\n") {
        Some(i) => (&raw[..i], &raw[i + 4..]),
        None => match raw.find("\n\n") {
            Some(i) => (&raw[..i], &raw[i + 2..]),
            None => (raw, ""),
        },
    };
    let mut headers: Vec<(String, String)> = vec![];
    for line in head.lines() {
        if line.starts_with([' ', '\t']) {
            if let Some(last) = headers.last_mut() {
                last.1.push(' ');
                last.1.push_str(line.trim());
            }
        } else if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_lowercase(), v.trim().to_string()));
        }
    }
    (headers, body)
}

fn header<'a>(h: &'a [(String, String)], name: &str) -> &'a str {
    h.iter().find(|x| x.0 == name).map(|x| x.1.as_str()).unwrap_or("")
}

fn param(value: &str, key: &str) -> Option<String> {
    let lower = value.to_lowercase();
    let at = lower.find(&format!("{key}="))?;
    let rest = value[at + key.len() + 1..].trim_start();
    let v = if let Some(q) = rest.strip_prefix('"') { q.split('"').next()? } else { rest.split(';').next()?.trim() };
    Some(v.to_string())
}

fn quoted_printable(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'=' {
            if i + 1 < b.len() && (b[i + 1] == b'\n' || b[i + 1] == b'\r') {
                i += if b[i + 1] == b'\r' && b.get(i + 2) == Some(&b'\n') { 3 } else { 2 };
                continue;
            }
            if let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

fn decode_part(h: &[(String, String)], body: &str) -> String {
    use base64::Engine as _;
    let bytes = match header(h, "content-transfer-encoding").trim().to_lowercase().as_str() {
        "quoted-printable" => quoted_printable(body),
        "base64" => {
            let clean: String = body.chars().filter(|c| !c.is_whitespace()).collect();
            base64::engine::general_purpose::STANDARD.decode(clean).unwrap_or_default()
        }
        _ => body.as_bytes().to_vec(),
    };
    let charset = param(header(h, "content-type"), "charset").unwrap_or_default().to_lowercase();
    if charset.contains("8859-1") || charset.contains("1252") {
        bytes.iter().map(|&b| b as char).collect()
    } else {
        String::from_utf8_lossy(&bytes).to_string()
    }
}

/// (plain parts, html parts) of a MIME entity, attachments skipped.
fn collect_parts(raw: &str, depth: u8, plain: &mut Vec<String>, html: &mut Vec<String>) {
    let (h, body) = split_headers(raw);
    let ctype = header(&h, "content-type").to_lowercase();
    if header(&h, "content-disposition").to_lowercase().starts_with("attachment") {
        return;
    }
    if ctype.starts_with("multipart/") && depth < 6 {
        if let Some(boundary) = param(header(&h, "content-type"), "boundary") {
            let delim = format!("--{boundary}");
            for part in body.split(&delim).skip(1) {
                if part.starts_with("--") {
                    break;
                }
                collect_parts(part.trim_start_matches(['\r', '\n']), depth + 1, plain, html);
            }
            return;
        }
    }
    if ctype.is_empty() || ctype.starts_with("text/plain") {
        plain.push(decode_part(&h, body));
    } else if ctype.starts_with("text/html") {
        html.push(decode_part(&h, body));
    }
}

/// The readable part of an `.eml`: sender, recipients, subject and the text (plain part, else the HTML part stripped).
fn eml_text(raw: &[u8]) -> String {
    let raw = String::from_utf8_lossy(raw);
    let (h, _) = split_headers(&raw);
    let mut head = String::new();
    for (key, label) in [("from", "From"), ("to", "To"), ("date", "Date"), ("subject", "Subject")] {
        let v = header(&h, key);
        if !v.is_empty() {
            head += &format!("{label}: {v}\n");
        }
    }
    let (mut plain, mut html) = (vec![], vec![]);
    collect_parts(&raw, 0, &mut plain, &mut html);
    let body = if plain.iter().any(|p| !p.trim().is_empty()) {
        plain.join("\n").replace("\r\n", "\n")
    } else {
        html.iter().map(|x| html_to_text(x)).collect::<Vec<_>>().join("\n")
    };
    format!("{head}\n{}", body.trim())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write(dir: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    fn tmp() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ri-extract-{}-{}", std::process::id(), rand_suffix()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn rand_suffix() -> u128 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    }

    #[test]
    fn supported_types() {
        for (m, n) in [
            ("application/pdf", "a.bin"),
            ("", "Report.PDF"),
            ("", "notes.docx"),
            ("text/plain", "x"),
            ("text/csv", "data"),
            ("", "readme.md"),
            ("application/json", "x"),
            ("text/html; charset=utf-8", "x"),
            ("message/rfc822", "x"),
            ("", "mail.eml"),
        ] {
            assert!(supported(m, n), "{m} {n}");
        }
        for (m, n) in [
            ("image/png", "a.png"),
            ("application/zip", "a.zip"),
            ("", "a.exe"),
            ("", "noext"),
            ("application/msword", "a.doc"),
        ] {
            assert!(!supported(m, n), "{m} {n}");
        }
    }

    #[test]
    fn plain_text_formats_and_limits() {
        let d = tmp();
        let csv = write(&d, "t.csv", b"name,qty\r\nbolt,4\r\n");
        assert_eq!(text_of(&csv, "text/csv", 100).unwrap(), "name,qty\nbolt,4");
        let md = write(&d, "r.md", "# عنوان\nنص".as_bytes());
        assert_eq!(text_of(&md, "", 100).unwrap(), "# عنوان\nنص");
        let json = write(&d, "j.json", b"{\"a\": 1}");
        assert_eq!(text_of(&json, "application/json", 3).unwrap(), "{\"a", "limited to max_chars");
        let empty = write(&d, "e.txt", b"  \n ");
        assert!(text_of(&empty, "text/plain", 100).is_none());
        let bin = write(&d, "b.png", b"\x89PNG");
        assert!(text_of(&bin, "image/png", 100).is_none());
        assert!(text_of(&d.join("missing.txt"), "text/plain", 100).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn html_is_stripped() {
        let h = "<html><head><title>T</title><style>p{color:red}</style></head><body><!-- hidden --><h1>Invoice &amp; terms</h1>\
                 <p>Total: 5&nbsp;&lt;EUR&gt; &#8364; &#x41;</p><script>alert('x')</script><ul><li>one</li><li>two</li></ul>\
                 <table><tr><td>a</td><td>b</td></tr></table></body></html>";
        let t = html_to_text(h);
        assert!(t.contains("Invoice & terms"), "{t}");
        assert!(t.contains("Total: 5 <EUR> € A"), "{t}");
        assert!(t.contains("one\ntwo"), "{t}");
        assert!(!t.contains("alert") && !t.contains("color:red") && !t.contains("hidden"), "{t}");
        assert!(!t.starts_with('T'), "head dropped: {t}");
        assert_eq!(html_to_text("a &unknown; b &"), "a &unknown; b &");
        assert_eq!(html_to_text("<p>cut <b"), "cut");
    }

    #[test]
    fn docx_paragraphs() {
        let d = tmp();
        let p = d.join("memo.docx");
        let f = std::fs::File::create(&p).unwrap();
        let mut z = zip::ZipWriter::new(f);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        z.start_file("[Content_Types].xml", opts).unwrap();
        z.write_all(b"<Types/>").unwrap();
        z.start_file("word/document.xml", opts).unwrap();
        z.write_all(
            "<?xml version=\"1.0\"?><w:document><w:body>\
             <w:p><w:r><w:t>Project memo</w:t></w:r></w:p>\
             <w:p><w:r><w:t xml:space=\"preserve\">Budget: </w:t></w:r><w:r><w:t>5 &amp; 6</w:t></w:r><w:r><w:tab/><w:t>due</w:t></w:r></w:p>\
             <w:p><w:r><w:t>مرحبا</w:t></w:r><w:r><w:br/><w:t>line two</w:t></w:r></w:p>\
             <w:p/></w:body></w:document>"
                .as_bytes(),
        )
        .unwrap();
        z.finish().unwrap();
        let t = text_of(&p, "application/vnd.openxmlformats-officedocument.wordprocessingml.document", 1000).unwrap();
        assert_eq!(t, "Project memo\nBudget: 5 & 6\tdue\nمرحبا\nline two");
        // not a zip
        let bad = write(&d, "bad.docx", b"not a zip");
        assert!(text_of(&bad, "", 100).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A one-page PDF with a single text object, with a correct cross-reference table.
    fn minimal_pdf(text: &str) -> Vec<u8> {
        let content = format!("BT /F1 18 Tf 72 700 Td ({text}) Tj ET");
        let objs = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
            format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        ];
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = vec![];
        for (i, o) in objs.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
        }
        let xref = pdf.len();
        pdf.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).bytes());
        for o in offsets {
            pdf.extend(format!("{o:010} 00000 n \n").bytes());
        }
        pdf.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).bytes());
        pdf
    }

    #[test]
    fn pdf_text_is_read_and_garbage_is_survived() {
        let d = tmp();
        let p = write(&d, "invoice.pdf", &minimal_pdf("Hello Invoice 4711"));
        let t = text_of(&p, "application/pdf", 1000).expect("text from the generated pdf");
        assert!(t.contains("Hello Invoice 4711"), "{t:?}");
        let limited = text_of(&p, "application/pdf", 5).unwrap();
        assert_eq!(limited.chars().count(), 5);
        let junk = write(&d, "junk.pdf", b"%PDF-1.4\n garbage \x00\x01 not a pdf");
        assert!(text_of(&junk, "application/pdf", 100).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn eml_body_plain_and_html() {
        let d = tmp();
        let multi = "From: Dana <dana@acme.com>\r\nTo: me@x.org\r\nSubject: Plan =?UTF-8?Q?x?=\r\nContent-Type: multipart/mixed; boundary=\"b1\"\r\n\r\n\
            --b1\r\nContent-Type: multipart/alternative; boundary=b2\r\n\r\n\
            --b2\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\nCaf=C3=A9 at noon=\r\n!\r\n\
            --b2\r\nContent-Type: text/html\r\n\r\n<p>Café html</p>\r\n--b2--\r\n\
            --b1\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment; filename=a.pdf\r\n\r\nJVBERi0=\r\n--b1--\r\n";
        let p = write(&d, "m.eml", multi.as_bytes());
        let t = text_of(&p, "message/rfc822", 1000).unwrap();
        assert!(t.starts_with("From: Dana <dana@acme.com>\nTo: me@x.org\nSubject: Plan"), "{t}");
        assert!(t.contains("Café at noon!"), "{t}");
        assert!(!t.contains("JVBERi") && !t.contains("html"), "{t}");

        let html_only = "Subject: Hi\nContent-Type: text/html; charset=\"iso-8859-1\"\nContent-Transfer-Encoding: base64\n\nPGI+Q2Fm6TwvYj4g";
        let p = write(&d, "h.eml", html_only.as_bytes());
        let t = text_of(&p, "", 1000).unwrap();
        assert!(t.ends_with("Café"), "{t}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn entities() {
        assert_eq!(decode_entities("a &amp;&lt;&#65;&#x42;&quot;&apos; &nbsp;z"), "a &<AB\"'  z");
        assert_eq!(decode_entities("&#99999999;"), "&#99999999;");
    }
}
