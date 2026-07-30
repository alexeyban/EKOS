//! DOCX parsing: document structure (paragraphs, tables) via `docx-rs`;
//! embedded images read directly from the `.docx` zip archive's
//! `word/media/` entries, independent of `docx-rs`'s document-model
//! traversal (RFC 0023).

use crate::{
    DocumentParser, EmbeddedImage, ExtractedTable, ImageFormat, ParseError, ParsedDocument,
};
use docx_rs::{
    DocumentChild, ParagraphChild, RunChild, TableCellContent, TableChild, TableRowChild,
};
use std::io::Read;

pub struct DocxParser;

impl DocumentParser for DocxParser {
    fn supported_extension(&self) -> &str {
        "docx"
    }

    fn parse(&self, bytes: &[u8]) -> Result<ParsedDocument, ParseError> {
        let docx = docx_rs::read_docx(bytes)
            .map_err(|e| ParseError::Malformed(format!("docx read failed: {e:?}")))?;

        let mut text = String::new();
        let mut tables = Vec::new();

        for child in &docx.document.children {
            match child {
                DocumentChild::Paragraph(p) => {
                    let para_text = paragraph_text(&p.children);
                    if !para_text.is_empty() {
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(&para_text);
                    }
                }
                DocumentChild::Table(t) => {
                    let rows = table_rows(&t.rows);
                    if !rows.is_empty() {
                        tables.push(ExtractedTable { page: None, rows });
                    }
                }
                _ => {}
            }
        }

        let images = extract_media_images(bytes);

        Ok(ParsedDocument {
            page_count: None,
            text,
            tables,
            images,
        })
    }
}

fn paragraph_text(children: &[ParagraphChild]) -> String {
    let mut out = String::new();
    for child in children {
        if let ParagraphChild::Run(run) = child {
            for rc in &run.children {
                if let RunChild::Text(t) = rc {
                    out.push_str(&t.text);
                }
            }
        }
    }
    out
}

fn table_rows(rows: &[TableChild]) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    for row in rows {
        let TableChild::TableRow(row) = row;
        let mut cells = Vec::new();
        for cell in &row.cells {
            let TableRowChild::TableCell(cell) = cell;
            let mut cell_text = String::new();
            for content in &cell.children {
                if let TableCellContent::Paragraph(p) = content {
                    let t = paragraph_text(&p.children);
                    if !cell_text.is_empty() && !t.is_empty() {
                        cell_text.push(' ');
                    }
                    cell_text.push_str(&t);
                }
            }
            cells.push(cell_text);
        }
        out.push(cells);
    }
    out
}

/// Reads `word/media/*` entries directly from the `.docx` zip archive.
fn extract_media_images(bytes: &[u8]) -> Vec<EmbeddedImage> {
    let mut images = Vec::new();
    let cursor = std::io::Cursor::new(bytes);
    let Ok(mut archive) = zip::ZipArchive::new(cursor) else {
        return images;
    };
    for i in 0..archive.len() {
        let Ok(mut file) = archive.by_index(i) else {
            continue;
        };
        let name = file.name().to_string();
        if !name.starts_with("word/media/") {
            continue;
        }
        let format = if name.ends_with(".png") {
            Some(ImageFormat::Png)
        } else if name.ends_with(".jpg") || name.ends_with(".jpeg") {
            Some(ImageFormat::Jpeg)
        } else {
            None
        };
        let Some(format) = format else { continue };
        let mut buf = Vec::new();
        if file.read_to_end(&mut buf).is_err() {
            continue;
        }
        images.push(EmbeddedImage {
            page: None,
            bytes: buf,
            format,
        });
    }
    images
}

#[cfg(test)]
mod tests {
    use super::*;
    use docx_rs::Docx;

    #[test]
    fn parses_paragraph_text_and_table_rows() {
        let docx = Docx::new()
            .add_paragraph(
                docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("hello world")),
            )
            .add_table(docx_rs::Table::new(vec![
                docx_rs::TableRow::new(vec![
                    docx_rs::TableCell::new().add_paragraph(
                        docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("Name")),
                    ),
                    docx_rs::TableCell::new().add_paragraph(
                        docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("Value")),
                    ),
                ]),
                docx_rs::TableRow::new(vec![
                    docx_rs::TableCell::new().add_paragraph(
                        docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("a")),
                    ),
                    docx_rs::TableCell::new().add_paragraph(
                        docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("1")),
                    ),
                ]),
            ]));

        let mut buf = Vec::new();
        docx.build().pack(std::io::Cursor::new(&mut buf)).unwrap();

        let parsed = DocxParser.parse(&buf).unwrap();
        assert!(parsed.text.contains("hello world"));
        assert_eq!(parsed.tables.len(), 1);
        assert_eq!(parsed.tables[0].rows[0], vec!["Name", "Value"]);
        assert_eq!(parsed.tables[0].rows[1], vec!["a", "1"]);
    }
}
