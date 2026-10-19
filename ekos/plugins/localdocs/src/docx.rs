//! DOCX parsing: document structure (paragraphs, tables) read from the main
//! document part's WordprocessingML with `roxmltree`; embedded images read
//! directly from the `.docx` zip archive's `word/media/` entries (RFC 0023).
//!
//! This used to go through `docx-rs`, which pins `zip` 8 — affected by
//! RUSTSEC-2026-0367/0368/0369 with no release on `zip` 9. Only the text of
//! body-level paragraphs and tables is needed, so the reader keeps exactly
//! what the `docx-rs` traversal kept: `w:t` inside a paragraph's direct `w:r`
//! children, and table cells' direct paragraphs.

use crate::{
    DOCX_CHUNK_CHAR_BUDGET, DocumentParser, DocumentSection, EmbeddedImage, ExtractedTable,
    ImageFormat, ParseError, ParsedDocument, SECTIONS_MAX,
};
use roxmltree::Node;
use std::io::Read;

/// Transitional and Strict OOXML namespaces for WordprocessingML.
const W_NS: [&str; 2] = [
    "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
    "http://purl.oclc.org/ooxml/wordprocessingml/main",
];
/// Largest decompressed XML part read, so a zip bomb cannot exhaust memory.
const MAX_XML_PART_BYTES: u64 = 256 * 1024 * 1024;

pub struct DocxParser;

impl DocumentParser for DocxParser {
    fn supported_extension(&self) -> &str {
        "docx"
    }

    fn parse(&self, bytes: &[u8]) -> Result<ParsedDocument, ParseError> {
        let xml = main_document_xml(bytes)?;
        let doc = roxmltree::Document::parse(&xml)
            .map_err(|e| ParseError::Malformed(format!("docx read failed: {e}")))?;
        let body = doc
            .root_element()
            .children()
            .find(|n| is_w(n, "body"))
            .ok_or_else(|| ParseError::Malformed("docx read failed: no w:body".into()))?;

        let mut text = String::new();
        let mut tables = Vec::new();
        let mut sections = Vec::new();
        let mut current_section = String::new();

        for child in body.children() {
            if is_w(&child, "p") {
                let para_text = paragraph_text(child);
                if !para_text.is_empty() {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(&para_text);

                    if !current_section.is_empty() {
                        current_section.push('\n');
                    }
                    current_section.push_str(&para_text);
                    if current_section.len() >= DOCX_CHUNK_CHAR_BUDGET
                        && sections.len() < SECTIONS_MAX
                    {
                        sections.push(DocumentSection {
                            page: None,
                            index: sections.len(),
                            text: std::mem::take(&mut current_section),
                            ..Default::default()
                        });
                    }
                }
            } else if is_w(&child, "tbl") {
                let rows = table_rows(child);
                if !rows.is_empty() {
                    tables.push(ExtractedTable { page: None, rows });
                }
            }
        }
        if !current_section.trim().is_empty() && sections.len() < SECTIONS_MAX {
            sections.push(DocumentSection {
                page: None,
                index: sections.len(),
                text: current_section,
                ..Default::default()
            });
        }

        let images = extract_media_images(bytes);

        Ok(ParsedDocument {
            page_count: None,
            text,
            tables,
            images,
            sections,
        })
    }
}

fn is_w(node: &Node, name: &str) -> bool {
    node.is_element()
        && node.tag_name().name() == name
        && node
            .tag_name()
            .namespace()
            .is_some_and(|ns| W_NS.contains(&ns))
}

fn w_children<'a, 'i>(
    node: Node<'a, 'i>,
    name: &'static str,
) -> impl Iterator<Item = Node<'a, 'i>> {
    node.children().filter(move |n| is_w(n, name))
}

/// The main document part: the package relationship of type `officeDocument`
/// (normally `word/document.xml`).
fn main_document_xml(bytes: &[u8]) -> Result<String, ParseError> {
    let malformed = |e: String| ParseError::Malformed(format!("docx read failed: {e}"));
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| malformed(e.to_string()))?;
    let part = read_part(&mut archive, "_rels/.rels")
        .ok()
        .and_then(|rels| office_document_target(&rels))
        .unwrap_or_else(|| "word/document.xml".to_string());
    read_part(&mut archive, &part).map_err(malformed)
}

fn office_document_target(rels: &str) -> Option<String> {
    let doc = roxmltree::Document::parse(rels).ok()?;
    doc.descendants()
        .filter(|n| n.tag_name().name() == "Relationship")
        .find(|n| {
            n.attribute("Type")
                .is_some_and(|t| t.ends_with("/officeDocument"))
        })
        .and_then(|n| n.attribute("Target"))
        .map(|t| t.trim_start_matches('/').to_string())
}

fn read_part(
    archive: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>,
    name: &str,
) -> Result<String, String> {
    let file = archive.by_name(name).map_err(|e| format!("{name}: {e}"))?;
    let mut xml = String::new();
    file.take(MAX_XML_PART_BYTES + 1)
        .read_to_string(&mut xml)
        .map_err(|e| format!("{name}: {e}"))?;
    if xml.len() as u64 > MAX_XML_PART_BYTES {
        return Err(format!("{name}: larger than {MAX_XML_PART_BYTES} bytes"));
    }
    Ok(xml)
}

fn paragraph_text(p: Node) -> String {
    let mut out = String::new();
    for run in w_children(p, "r") {
        for t in w_children(run, "t") {
            out.push_str(t.text().unwrap_or(""));
        }
    }
    out
}

fn table_rows(tbl: Node) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    for row in w_children(tbl, "tr") {
        let mut cells = Vec::new();
        for cell in w_children(row, "tc") {
            let mut cell_text = String::new();
            for p in w_children(cell, "p") {
                let t = paragraph_text(p);
                if !cell_text.is_empty() && !t.is_empty() {
                    cell_text.push(' ');
                }
                cell_text.push_str(&t);
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
        let Ok(name) = file.name().map(|n| n.into_owned()) else {
            continue;
        };
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
    use std::io::Write;

    const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

    fn run(text: &str) -> String {
        format!("<w:r><w:t xml:space=\"preserve\">{text}</w:t></w:r>")
    }

    fn para(text: &str) -> String {
        format!("<w:p>{}</w:p>", run(text))
    }

    fn table(rows: &[&[&str]]) -> String {
        let rows: String = rows
            .iter()
            .map(|r| {
                let cells: String = r
                    .iter()
                    .map(|c| format!("<w:tc>{}</w:tc>", para(c)))
                    .collect();
                format!("<w:tr>{cells}</w:tr>")
            })
            .collect();
        format!("<w:tbl>{rows}</w:tbl>")
    }

    /// A minimal package: `parts` are (path, contents) pairs.
    fn package(parts: &[(&str, String)]) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        for (name, body) in parts {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        buf
    }

    fn document(body: &str) -> String {
        format!(
            "<?xml version=\"1.0\"?><w:document xmlns:w=\"{W}\"><w:body>{body}</w:body></w:document>"
        )
    }

    fn rels(target: &str) -> String {
        format!(
            "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
             <Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"{target}\"/>\
             </Relationships>"
        )
    }

    #[test]
    fn parses_paragraph_text_and_table_rows() {
        let body = format!(
            "{}{}",
            para("hello world"),
            table(&[&["Name", "Value"], &["a", "1"]])
        );
        let bytes = package(&[
            ("_rels/.rels", rels("word/document.xml")),
            ("word/document.xml", document(&body)),
        ]);

        let parsed = DocxParser.parse(&bytes).unwrap();
        assert_eq!(parsed.text, "hello world");
        assert_eq!(parsed.tables.len(), 1);
        assert_eq!(parsed.tables[0].rows[0], vec!["Name", "Value"]);
        assert_eq!(parsed.tables[0].rows[1], vec!["a", "1"]);
    }

    #[test]
    fn follows_the_office_document_relationship() {
        let bytes = package(&[
            ("_rels/.rels", rels("/word/document2.xml")),
            ("word/document.xml", document(&para("not this one"))),
            ("word/document2.xml", document(&para("main part"))),
        ]);
        assert_eq!(DocxParser.parse(&bytes).unwrap().text, "main part");
    }

    #[test]
    fn keeps_only_direct_run_text_like_the_docx_rs_traversal_did() {
        // Hyperlink and tracked-insertion text sit outside a paragraph's direct
        // runs; deleted text is `w:delText`. None of it was ever extracted.
        let body = format!(
            "<w:p>{}<w:hyperlink>{}</w:hyperlink><w:ins>{}</w:ins><w:r><w:delText>gone</w:delText></w:r>{}</w:p>",
            run("a"),
            run("link"),
            run("inserted"),
            run("b")
        );
        let bytes = package(&[("word/document.xml", document(&body))]);
        assert_eq!(DocxParser.parse(&bytes).unwrap().text, "ab");
    }

    #[test]
    fn rejects_bytes_that_are_not_a_package() {
        assert!(matches!(
            DocxParser.parse(b"PK-fake"),
            Err(ParseError::Malformed(_))
        ));
    }
}
