//! PDF parsing: text via `pdf-extract`, structure/embedded images via
//! `lopdf`, tables via a whitespace-column heuristic on the extracted text
//! (RFC 0023 — approximate, documented in the RFC's Design section).

use crate::{
    DocumentParser, EmbeddedImage, ExtractedTable, ImageFormat, ParseError, ParsedDocument,
};
use lopdf::Document as LoDocument;
use lopdf::Object;

pub struct PdfParser;

impl DocumentParser for PdfParser {
    fn supported_extension(&self) -> &str {
        "pdf"
    }

    fn parse(&self, bytes: &[u8]) -> Result<ParsedDocument, ParseError> {
        let text = pdf_extract::extract_text_from_mem(bytes)
            .map_err(|e| ParseError::Malformed(format!("text extraction failed: {e}")))?;

        let doc = LoDocument::load_mem(bytes)
            .map_err(|e| ParseError::Malformed(format!("structure parse failed: {e}")))?;
        let pages = doc.get_pages();
        let page_count = Some(pages.len() as u32);

        let mut images = Vec::new();
        for (page_num, page_id) in &pages {
            let Ok((Some(resources), _)) = doc.get_page_resources(*page_id) else {
                continue;
            };
            let Ok(xobjects) = resources.get(b"XObject").and_then(Object::as_dict) else {
                continue;
            };
            for (_, xobj_ref) in xobjects.iter() {
                let Ok(xobj_id) = xobj_ref.as_reference() else {
                    continue;
                };
                let Ok(stream) = doc.get_object(xobj_id).and_then(Object::as_stream) else {
                    continue;
                };
                let is_image = stream
                    .dict
                    .get(b"Subtype")
                    .and_then(Object::as_name)
                    .map(|n| n == b"Image")
                    .unwrap_or(false);
                if !is_image {
                    continue;
                }
                // v1 only decodes JPEG (DCTDecode) streams — the common
                // case for scanned pages — and skips other encodings
                // rather than implementing a full PDF image decoder.
                let is_jpeg = stream
                    .dict
                    .get(b"Filter")
                    .and_then(Object::as_name)
                    .map(|n| n == b"DCTDecode")
                    .unwrap_or(false);
                if !is_jpeg {
                    continue;
                }
                images.push(EmbeddedImage {
                    page: Some(*page_num),
                    bytes: stream.content.clone(),
                    format: ImageFormat::Jpeg,
                });
            }
        }

        let tables = extract_tables(&text);

        Ok(ParsedDocument {
            page_count,
            text,
            tables,
            images,
        })
    }
}

/// Whitespace-column table heuristic: contiguous lines that each split into
/// ≥2 fields on a run of ≥2 whitespace characters are grouped into one
/// table. Approximate — operates on the flat text stream, not per-page
/// glyph coordinates — documented as such in RFC 0023.
fn extract_tables(text: &str) -> Vec<ExtractedTable> {
    let mut tables = Vec::new();
    let mut current_rows: Vec<Vec<String>> = Vec::new();

    let flush = |rows: &mut Vec<Vec<String>>, tables: &mut Vec<ExtractedTable>| {
        if rows.len() >= 2 {
            tables.push(ExtractedTable {
                page: None,
                rows: std::mem::take(rows),
            });
        } else {
            rows.clear();
        }
    };

    for line in text.lines() {
        match split_table_row(line) {
            Some(fields) => current_rows.push(fields),
            None => flush(&mut current_rows, &mut tables),
        }
    }
    flush(&mut current_rows, &mut tables);

    tables
}

/// Splits a line into ≥2 fields on runs of ≥2 whitespace characters, or
/// returns `None` if the line doesn't look like a table row.
fn split_table_row(line: &str) -> Option<Vec<String>> {
    if line.trim().is_empty() {
        return None;
    }
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut space_run = 0usize;

    for ch in line.chars() {
        if ch == ' ' || ch == '\t' {
            space_run += 1;
            if space_run == 2 && !current.trim().is_empty() {
                fields.push(current.trim().to_string());
                current.clear();
            }
        } else {
            space_run = 0;
            current.push(ch);
        }
    }
    if !current.trim().is_empty() {
        fields.push(current.trim().to_string());
    }

    if fields.len() >= 2 {
        Some(fields)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_table_row_requires_two_space_gap() {
        assert_eq!(
            split_table_row("Name  Value"),
            Some(vec!["Name".to_string(), "Value".to_string()])
        );
        assert_eq!(split_table_row("just prose here"), None);
        assert_eq!(split_table_row(""), None);
    }

    #[test]
    fn extract_tables_groups_contiguous_rows() {
        let text = "Intro paragraph.\n\nName  Value\na  1\nb  2\n\nMore prose.\n";
        let tables = extract_tables(text);
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].rows.len(), 3);
        assert_eq!(tables[0].rows[0], vec!["Name", "Value"]);
    }

    #[test]
    fn extract_tables_ignores_single_row_matches() {
        let text = "Intro paragraph.\nsingle  row\nmore prose\n";
        assert!(extract_tables(text).is_empty());
    }
}
