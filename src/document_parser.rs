use std::cmp::Ordering;
use std::io::{Cursor, Read};
use std::panic::{catch_unwind, AssertUnwindSafe};

const MAX_DOCUMENT_TEXT_CHARS: usize = 60_000;
const MAX_ZIP_XML_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct ExtractedDocument {
    pub text: String,
    pub truncated: bool,
}

pub fn extract_document_text(file_name: &str, mime_type: &str, bytes: &[u8]) -> Result<ExtractedDocument, String> {
    let raw = match document_kind(file_name, mime_type) {
        DocumentKind::Text => decode_text_lossy(bytes),
        DocumentKind::Pdf => extract_pdf_text(bytes)?,
        DocumentKind::Docx => extract_docx_text(bytes)?,
        DocumentKind::Pptx => extract_pptx_text(bytes)?,
        DocumentKind::LegacyOffice => {
            return Err("legacy .doc/.ppt files are not supported by the local parser; convert the file to .docx, .pptx, .pdf, or plain text".to_string());
        }
        DocumentKind::Unsupported => {
            return Err(format!(
                "unsupported document type for local parsing: {}",
                if mime_type.trim().is_empty() { "unknown" } else { mime_type.trim() }
            ));
        }
    };

    Ok(cap_document_text(clean_extracted_text(&raw)))
}

fn document_kind(file_name: &str, mime_type: &str) -> DocumentKind {
    let mime = mime_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let ext = file_name
        .rsplit('.')
        .next()
        .filter(|value| *value != file_name)
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();

    if matches!(ext.as_str(), "txt" | "md" | "markdown" | "csv" | "json" | "log")
        || mime.starts_with("text/")
        || matches!(
            mime.as_str(),
            "application/json"
                | "application/x-ndjson"
                | "application/csv"
                | "text/csv"
                | "text/markdown"
                | "application/markdown"
        )
    {
        return DocumentKind::Text;
    }

    if ext == "pdf" || mime == "application/pdf" {
        return DocumentKind::Pdf;
    }
    if ext == "docx" || mime == "application/vnd.openxmlformats-officedocument.wordprocessingml.document" {
        return DocumentKind::Docx;
    }
    if ext == "pptx" || mime == "application/vnd.openxmlformats-officedocument.presentationml.presentation" {
        return DocumentKind::Pptx;
    }
    if matches!(ext.as_str(), "doc" | "ppt") {
        return DocumentKind::LegacyOffice;
    }

    DocumentKind::Unsupported
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DocumentKind {
    Text,
    Pdf,
    Docx,
    Pptx,
    LegacyOffice,
    Unsupported,
}

fn decode_text_lossy(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&bytes[3..]).into_owned();
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return decode_utf16_lossy(&bytes[2..], true);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return decode_utf16_lossy(&bytes[2..], false);
    }
    String::from_utf8_lossy(bytes).into_owned()
}

fn decode_utf16_lossy(bytes: &[u8], little_endian: bool) -> String {
    let mut units = Vec::with_capacity(bytes.len() / 2);
    for chunk in bytes.chunks_exact(2) {
        let value = if little_endian {
            u16::from_le_bytes([chunk[0], chunk[1]])
        } else {
            u16::from_be_bytes([chunk[0], chunk[1]])
        };
        units.push(value);
    }
    String::from_utf16_lossy(&units)
}

fn extract_pdf_text(bytes: &[u8]) -> Result<String, String> {
    match catch_unwind(AssertUnwindSafe(|| pdf_extract::extract_text_from_mem(bytes))) {
        Ok(Ok(text)) => Ok(text),
        Ok(Err(error)) => Err(format!("PDF text extraction failed: {error}")),
        Err(_) => Err("PDF text extraction failed unexpectedly".to_string()),
    }
}

fn extract_docx_text(bytes: &[u8]) -> Result<String, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| format!("DOCX archive could not be opened: {error}"))?;
    let mut names = zip_entry_names(&mut archive)?;
    names.retain(|name| {
        name == "word/document.xml"
            || name.starts_with("word/header") && name.ends_with(".xml")
            || name.starts_with("word/footer") && name.ends_with(".xml")
            || name == "word/footnotes.xml"
            || name == "word/endnotes.xml"
            || name == "word/comments.xml"
    });
    names.sort_by(|left, right| office_part_order(left).cmp(&office_part_order(right)).then_with(|| left.cmp(right)));
    extract_ooxml_text_entries(&mut archive, &names)
}

fn extract_pptx_text(bytes: &[u8]) -> Result<String, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| format!("PPTX archive could not be opened: {error}"))?;
    let mut names = zip_entry_names(&mut archive)?;
    names.retain(|name| name.starts_with("ppt/slides/slide") && name.ends_with(".xml"));
    names.sort_by(|left, right| natural_slide_order(left, right));
    extract_ooxml_text_entries(&mut archive, &names)
}

fn zip_entry_names<R: Read + std::io::Seek>(archive: &mut zip::ZipArchive<R>) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .map_err(|error| format!("archive entry could not be read: {error}"))?;
        names.push(file.name().replace('\\', "/"));
    }
    Ok(names)
}

fn extract_ooxml_text_entries<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    names: &[String],
) -> Result<String, String> {
    let mut out = String::new();
    for name in names {
        let xml = read_zip_text_entry(archive, name)?;
        let text = ooxml_text_from_xml(&xml);
        if text.trim().is_empty() {
            continue;
        }
        if !out.trim().is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(text.trim());
    }
    if out.trim().is_empty() {
        return Err("no extractable text was found in the document".to_string());
    }
    Ok(out)
}

fn read_zip_text_entry<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Result<String, String> {
    let file = archive
        .by_name(name)
        .map_err(|error| format!("archive entry {name} could not be read: {error}"))?;
    let mut limited = file.take(MAX_ZIP_XML_BYTES as u64 + 1);
    let mut data = Vec::new();
    limited
        .read_to_end(&mut data)
        .map_err(|error| format!("archive entry {name} could not be decoded: {error}"))?;
    if data.len() > MAX_ZIP_XML_BYTES {
        data.truncate(MAX_ZIP_XML_BYTES);
    }
    Ok(String::from_utf8_lossy(&data).into_owned())
}

fn ooxml_text_from_xml(xml: &str) -> String {
    let mut out = String::new();
    let mut text = String::new();
    let mut tag = String::new();
    let mut in_tag = false;

    for ch in xml.chars() {
        if in_tag {
            if ch == '>' {
                handle_xml_tag(&tag, &mut out);
                tag.clear();
                in_tag = false;
            } else {
                tag.push(ch);
            }
            continue;
        }

        if ch == '<' {
            flush_xml_text(&mut text, &mut out);
            in_tag = true;
        } else {
            text.push(ch);
        }
    }
    flush_xml_text(&mut text, &mut out);
    clean_extracted_text(&out)
}

fn flush_xml_text(text: &mut String, out: &mut String) {
    if text.is_empty() {
        return;
    }
    out.push_str(&decode_xml_entities(text));
    text.clear();
}

fn handle_xml_tag(raw: &str, out: &mut String) {
    let tag = raw.trim();
    let is_end = tag.starts_with('/');
    let name = tag
        .trim_start_matches('/')
        .trim_end_matches('/')
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();

    match name.as_str() {
        "w:p" | "a:p" if is_end => push_newline(out),
        "w:br" | "a:br" | "w:cr" => push_newline(out),
        "w:tab" => out.push(' '),
        _ => {}
    }
}

fn decode_xml_entities(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '&' {
            out.push(ch);
            continue;
        }

        let mut entity = String::new();
        while let Some(next) = chars.peek().copied() {
            chars.next();
            if next == ';' {
                break;
            }
            if entity.len() > 24 {
                out.push('&');
                out.push_str(&entity);
                out.push(next);
                entity.clear();
                break;
            }
            entity.push(next);
        }

        if entity.is_empty() {
            continue;
        }
        if let Some(decoded) = decode_xml_entity(&entity) {
            out.push(decoded);
        } else {
            out.push('&');
            out.push_str(&entity);
            out.push(';');
        }
    }
    out
}

fn decode_xml_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        value if value.starts_with("#x") || value.starts_with("#X") => {
            u32::from_str_radix(&value[2..], 16).ok().and_then(char::from_u32)
        }
        value if value.starts_with('#') => value[1..].parse::<u32>().ok().and_then(char::from_u32),
        _ => None,
    }
}

fn clean_extracted_text(raw: &str) -> String {
    let mut lines = Vec::new();
    for line in raw.replace('\r', "\n").lines() {
        let collapsed = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if !collapsed.is_empty() {
            lines.push(collapsed);
        }
    }
    lines.join("\n")
}

fn cap_document_text(text: String) -> ExtractedDocument {
    if text.chars().count() <= MAX_DOCUMENT_TEXT_CHARS {
        return ExtractedDocument {
            text,
            truncated: false,
        };
    }

    let mut capped = text.chars().take(MAX_DOCUMENT_TEXT_CHARS).collect::<String>();
    capped.push_str("\n\n[Document text truncated after 60000 characters.]");
    ExtractedDocument {
        text: capped,
        truncated: true,
    }
}

fn push_newline(out: &mut String) {
    if !out.ends_with('\n') {
        out.push('\n');
    }
}

fn office_part_order(name: &str) -> (u8, u32) {
    if name == "word/document.xml" {
        return (0, 0);
    }
    if name.starts_with("word/header") {
        return (1, number_in_name(name));
    }
    if name.starts_with("word/footer") {
        return (2, number_in_name(name));
    }
    if name == "word/footnotes.xml" {
        return (3, 0);
    }
    if name == "word/endnotes.xml" {
        return (4, 0);
    }
    if name == "word/comments.xml" {
        return (5, 0);
    }
    (9, number_in_name(name))
}

fn natural_slide_order(left: &str, right: &str) -> Ordering {
    number_in_name(left)
        .cmp(&number_in_name(right))
        .then_with(|| left.cmp(right))
}

fn number_in_name(name: &str) -> u32 {
    let digits = name
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .collect::<String>();
    digits.parse::<u32>().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_basic_xml_entities() {
        assert_eq!(decode_xml_entities("a &amp; b &#x4E2D;&#25991;"), "a & b 中文");
    }

    #[test]
    fn caps_long_text() {
        let text = "a".repeat(MAX_DOCUMENT_TEXT_CHARS + 1);
        let parsed = cap_document_text(text);
        assert!(parsed.truncated);
        assert!(parsed.text.contains("Document text truncated"));
    }
}
