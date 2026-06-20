use std::io::Cursor;

use base64::{Engine as _, engine::general_purpose};
use calamine::Reader;
use tracing::warn;

const MAX_TEXT_LEN: usize = 50_000;

/// Extracts text content from a document attachment.
/// Accepts a data URL (base64 encoded) and mime type, returns extracted text.
pub fn parse_document(mime_type: &str, data_url: &str) -> Result<String, String> {
    let bytes = decode_data_url(data_url)?;

    let text = match mime_type {
        "application/pdf" => extract_pdf(&bytes)?,
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => {
            extract_docx(&bytes)?
        }
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => {
            extract_pptx(&bytes)?
        }
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        | "application/vnd.ms-excel" => extract_xlsx(&bytes)?,
        m if m.starts_with("text/")
            || m == "application/json"
            || m == "application/xml"
            || m == "application/toml" =>
        {
            extract_text_utf8(&bytes)
        }
        _ => {
            return Err(format!("Unsupported document type: {mime_type}"));
        }
    };

    Ok(truncate_text(&text, MAX_TEXT_LEN))
}

fn decode_data_url(data_url: &str) -> Result<Vec<u8>, String> {
    let base64_part = if let Some(pos) = data_url.find(";base64,") {
        &data_url[pos + 8..]
    } else if let Some(pos) = data_url.find(',') {
        &data_url[pos + 1..]
    } else {
        data_url
    };

    general_purpose::STANDARD
        .decode(base64_part)
        .map_err(|e| format!("Base64 decode error: {e}"))
}

fn extract_pdf(bytes: &[u8]) -> Result<String, String> {
    pdf_extract::extract_text_from_mem(bytes).map_err(|e| format!("PDF extraction error: {e}"))
}

fn extract_docx(bytes: &[u8]) -> Result<String, String> {
    let cursor = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| format!("DOCX zip error: {e}"))?;

    let mut text = String::new();
    let doc_xml = archive
        .by_name("word/document.xml")
        .map_err(|e| format!("DOCX missing document.xml: {e}"))?;

    let mut reader = quick_xml::Reader::from_reader(std::io::BufReader::new(doc_xml));
    let mut in_text = false;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(ref e)) if e.name().as_ref() == b"w:t" => {
                in_text = true;
            }
            Ok(quick_xml::events::Event::End(ref e)) if e.name().as_ref() == b"w:t" => {
                in_text = false;
            }
            Ok(quick_xml::events::Event::Text(e)) if in_text => {
                if let Ok(t) = e.unescape() {
                    text.push_str(&t);
                }
            }
            Ok(quick_xml::events::Event::Start(ref e)) if e.name().as_ref() == b"w:p" => {}
            Ok(quick_xml::events::Event::End(ref e)) if e.name().as_ref() == b"w:p" => {
                text.push('\n');
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(e) => {
                warn!("DOCX XML parse error: {e}");
                break;
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(text)
}

fn extract_pptx(bytes: &[u8]) -> Result<String, String> {
    let cursor = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| format!("PPTX zip error: {e}"))?;

    let mut all_text = String::new();
    let mut slide_names: Vec<String> = Vec::new();

    for i in 0..archive.len() {
        if let Ok(file) = archive.by_index(i) {
            let name = file.name().to_string();
            if name.starts_with("ppt/slides/slide") && name.ends_with(".xml") {
                slide_names.push(name);
            }
        }
    }
    slide_names.sort();

    for (idx, name) in slide_names.iter().enumerate() {
        if let Ok(file) = archive.by_name(name) {
            all_text.push_str(&format!("--- Slide {} ---\n", idx + 1));
            let slide_text = extract_xml_text_content(file);
            all_text.push_str(&slide_text);
            all_text.push('\n');
        }
    }

    Ok(all_text)
}

fn extract_xlsx(bytes: &[u8]) -> Result<String, String> {
    let cursor = Cursor::new(bytes);
    let mut workbook = calamine::open_workbook_auto_from_rs(cursor)
        .map_err(|e| format!("Excel open error: {e}"))?;

    let mut result = String::new();
    let sheet_names = workbook.sheet_names().to_vec();

    for name in &sheet_names {
        if let Ok(range) = workbook.worksheet_range(name) {
            result.push_str(&format!("--- Sheet: {name} ---\n"));
            for row in range.rows() {
                let cells: Vec<String> = row
                    .iter()
                    .map(|cell| match cell {
                        calamine::Data::Empty => String::new(),
                        calamine::Data::String(s) => s.clone(),
                        calamine::Data::Float(f) => f.to_string(),
                        calamine::Data::Int(i) => i.to_string(),
                        calamine::Data::Bool(b) => b.to_string(),
                        calamine::Data::DateTime(dt) => dt.to_string(),
                        calamine::Data::Error(e) => format!("#ERR:{e:?}"),
                        _ => String::new(),
                    })
                    .collect();
                result.push_str(&cells.join("\t"));
                result.push('\n');
            }
        }
    }

    Ok(result)
}

fn extract_text_utf8(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn extract_xml_text_content(reader: impl std::io::Read) -> String {
    let mut xml_reader = quick_xml::Reader::from_reader(std::io::BufReader::new(reader));
    let mut text = String::new();
    let mut in_text = false;
    let mut buf = Vec::new();

    loop {
        match xml_reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(ref e)) if e.name().as_ref() == b"a:t" => {
                in_text = true;
            }
            Ok(quick_xml::events::Event::End(ref e)) if e.name().as_ref() == b"a:t" => {
                in_text = false;
                text.push(' ');
            }
            Ok(quick_xml::events::Event::Text(e)) if in_text => {
                if let Ok(t) = e.unescape() {
                    text.push_str(&t);
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    text.trim().to_string()
}

fn truncate_text(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        text.to_string()
    } else {
        let truncated = &text[..text.floor_char_boundary(max_len)];
        format!("{truncated}\n\n[... content truncated at {max_len} characters]")
    }
}
