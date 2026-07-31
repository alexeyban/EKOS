//! PDF parsing: text via `pdf-extract`, structure/embedded images via
//! `lopdf`, tables via a whitespace-column heuristic on the extracted text
//! (RFC 0023 — approximate, documented in the RFC's Design section).

use crate::{
    DocumentParser, DocumentSection, EmbeddedImage, ExtractedTable, ImageFormat, ParseError,
    ParsedDocument, SECTION_TEXT_MAX_CHARS, SECTIONS_MAX,
};
use lopdf::Document as LoDocument;
use lopdf::Object;

pub struct PdfParser;

impl DocumentParser for PdfParser {
    fn supported_extension(&self) -> &str {
        "pdf"
    }

    fn parse(&self, bytes: &[u8]) -> Result<ParsedDocument, ParseError> {
        // `pdf_extract`/`lopdf` panic (rather than return `Err`) on some
        // malformed real-world PDFs (e.g. a Type3 font missing a glyph
        // width) — observed on actual scanned/converted books in
        // production use. A panic here must not take down the whole
        // `ekos build` run; `catch_unwind` turns it into an ordinary
        // soft-skip for this one file, same as any other `ParseError`.
        std::panic::catch_unwind(|| Self::parse_inner(bytes)).unwrap_or_else(|payload| {
            let msg = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_string());
            Err(ParseError::Malformed(format!("pdf parser panicked: {msg}")))
        })
    }
}

impl PdfParser {
    fn parse_inner(bytes: &[u8]) -> Result<ParsedDocument, ParseError> {
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
        let sections = extract_sections(bytes, pages.len());

        Ok(ParsedDocument {
            page_count,
            text,
            tables,
            images,
            sections,
        })
    }
}

/// One `DocumentSection` per PDF page, via `pdf-extract`'s real per-page
/// API (RFC 0024) — small enough per page to be fully indexed, unlike the
/// one whole-document excerpt every earlier version of this connector
/// shared across however many pages a document had.
///
/// `extract_text_from_mem_by_pages`'s internal loop
/// (`while let Ok(content) = extract_text_by_page(&doc, page_num)`) stops
/// at the *first* page that errors, silently dropping every page after
/// it — verified by reading the vendored `pdf-extract` 0.12.0 source. If
/// fewer pages come back than `expected_page_count` (from the `lopdf`
/// page walk already done by the caller), that truncation is logged; the
/// pages that did extract are still a strict improvement over the old
/// single 600-char whole-document cap.
fn extract_sections(bytes: &[u8], expected_page_count: usize) -> Vec<DocumentSection> {
    let pages = match pdf_extract::extract_text_from_mem_by_pages(bytes) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("pdf per-page text extraction failed, no sections captured: {e}");
            return Vec::new();
        }
    };
    if pages.len() < expected_page_count {
        tracing::warn!(
            "pdf per-page extraction stopped early: got {} of {} pages",
            pages.len(),
            expected_page_count
        );
    }
    pages
        .into_iter()
        .take(SECTIONS_MAX)
        .enumerate()
        .map(|(i, text)| DocumentSection {
            page: Some(i as u32 + 1),
            index: i,
            text: text.chars().take(SECTION_TEXT_MAX_CHARS).collect(),
        })
        .collect()
}

/// Whitespace-column table heuristic: contiguous lines that each split into
/// ≥2 fields on a run of ≥2 whitespace characters are grouped into one
/// table, then kept only if every row in the group has the *same* field
/// count. Approximate — operates on the flat text stream, not per-page
/// glyph coordinates — documented as such in RFC 0023.
///
/// The uniform-column-count requirement was added after running against a
/// real book library (RFC 0023's devlog): without it, justified body
/// prose routinely misfires as a "table" — PDF text extraction leaves
/// irregular multi-space gaps in justified paragraphs, but unlike an
/// actual table, the field count varies wildly line to line. Real tables
/// (including simple two-column ones like a table of contents) keep a
/// constant column count; prose does not.
fn extract_tables(text: &str) -> Vec<ExtractedTable> {
    let mut tables = Vec::new();
    let mut current_rows: Vec<Vec<String>> = Vec::new();

    let flush = |rows: &mut Vec<Vec<String>>, tables: &mut Vec<ExtractedTable>| {
        let taken = std::mem::take(rows);
        if taken.len() >= 2 && has_uniform_column_count(&taken) {
            tables.push(ExtractedTable {
                page: None,
                rows: taken,
            });
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

/// True if every row has the same number of fields as the first row.
fn has_uniform_column_count(rows: &[Vec<String>]) -> bool {
    match rows.first() {
        Some(first) => rows.iter().all(|r| r.len() == first.len()),
        None => false,
    }
}

/// Splits a line into ≥2 fields on runs of ≥2 whitespace characters, or
/// returns `None` if the line doesn't look like a table row. A single
/// space/tab is kept as part of the current field's text (real cell
/// content like "Putting it all together" has internal single spaces);
/// only a run of ≥2 is treated as a column delimiter.
fn split_table_row(line: &str) -> Option<Vec<String>> {
    if line.trim().is_empty() {
        return None;
    }
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut space_run = 0usize;

    let push_field = |current: &mut String, fields: &mut Vec<String>, trailing_spaces: usize| {
        let cut = current.len() - trailing_spaces;
        let field = current[..cut].trim().to_string();
        if !field.is_empty() {
            fields.push(field);
        }
        current.clear();
    };

    for ch in line.chars() {
        if ch == ' ' || ch == '\t' {
            space_run += 1;
            current.push(ch);
        } else {
            if space_run >= 2 {
                push_field(&mut current, &mut fields, space_run);
            }
            space_run = 0;
            current.push(ch);
        }
    }
    if space_run >= 2 {
        push_field(&mut current, &mut fields, space_run);
    } else if !current.trim().is_empty() {
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

    // The following two fixtures are real `pdf-extract` output captured
    // from public technical PDFs in an end-to-end run against a real
    // document library (RFC 0023's devlog) — not hand-crafted synthetic
    // text. They pin down the heuristic's real-world precision: before the
    // uniform-column-count check, justified body prose like
    // `JUSTIFIED_PROSE_EXCERPT` was misdetected as a table on real books.

    /// Real `pdf-extract` output: justified body prose. PDF text extraction
    /// leaves irregular multi-space gaps between justified words — a naive
    /// "≥2 spaces = column boundary" heuristic misfires on this, producing
    /// rows with wildly different field counts per line.
    const JUSTIFIED_PROSE_EXCERPT: &str = "The  world  of  enterprise  computing  is  now  starting  to  see  the  incorporation  of  new  technologies  that  have  been\ndeveloped and adopted by  the new media companies like Google, Yahoo, Facebook, LinkedIn et al. These technologies\ninclude  sy stems  like  large-scale  distributed  ﬁle  sy stems  like  Hadoop,  which  can  handle  enormously   large  data  sets,\ncloud  computing  and  virtualiz ation,  html  5 ,  smart  mobile  devices  and  many   more.  We  are  now  on  the  cusp  of  y et\n";

    /// Real `pdf-extract` output: a table-of-contents fragment (two
    /// consistently 2-field lines: entry title, page number).
    const TOC_EXCERPT: &str = "Putting it all together   34\nAdditional resources   36\n";

    #[test]
    fn real_justified_prose_produces_no_table() {
        // Before the uniform-column-count check, this excerpt produced a
        // single bogus "table" spanning the whole paragraph.
        assert!(extract_tables(JUSTIFIED_PROSE_EXCERPT).is_empty());
    }

    #[test]
    fn real_toc_fragment_is_still_detected_as_a_table() {
        let tables = extract_tables(TOC_EXCERPT);
        assert_eq!(tables.len(), 1);
        assert_eq!(
            tables[0].rows,
            vec![
                vec!["Putting it all together".to_string(), "34".to_string()],
                vec!["Additional resources".to_string(), "36".to_string()],
            ]
        );
    }

    #[test]
    fn has_uniform_column_count_rejects_mismatched_rows() {
        let rows = vec![
            vec!["a".to_string(), "b".to_string()],
            vec!["c".to_string()],
        ];
        assert!(!has_uniform_column_count(&rows));
    }

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

    #[test]
    fn extract_sections_returns_empty_on_garbage_bytes_without_panicking() {
        assert!(extract_sections(b"not a pdf", 0).is_empty());
    }

    #[test]
    fn parse_real_multipage_pdf_produces_one_section_per_page() {
        // A minimal, hand-assembled 2-page PDF (no external generator
        // dependency at test time) — real bytes lopdf/pdf-extract parse,
        // not a mock.
        let pdf = build_two_page_pdf();
        let parsed = PdfParser.parse(&pdf).unwrap();
        assert_eq!(parsed.sections.len(), 2);
        assert_eq!(parsed.sections[0].page, Some(1));
        assert_eq!(parsed.sections[1].page, Some(2));
        assert!(parsed.sections[0].text.contains("Page One"));
        assert!(parsed.sections[1].text.contains("Page Two"));
    }

    /// Builds a minimal valid 2-page PDF with real, distinct text content
    /// per page via `lopdf`'s own writer — exercising the real parser
    /// against real bytes end to end, not a byte-string fixture.
    fn build_two_page_pdf() -> Vec<u8> {
        use lopdf::{Dictionary, Object as LoObject, Stream, dictionary};

        let mut doc = LoDocument::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
        });
        let resources_id = doc.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });

        let mut page_ids = Vec::new();
        for label in ["Page One", "Page Two"] {
            let content = format!("BT /F1 24 Tf 72 700 Td ({label} content here) Tj ET");
            let content_id = doc.add_object(Stream::new(Dictionary::new(), content.into_bytes()));
            let page_id = doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => content_id,
                "Resources" => resources_id,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            });
            page_ids.push(LoObject::Reference(page_id));
        }

        doc.objects.insert(
            pages_id,
            LoObject::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => page_ids.clone(),
                "Count" => page_ids.len() as i64,
            }),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);

        let mut buf = Vec::new();
        doc.save_to(&mut buf).unwrap();
        buf
    }
}
