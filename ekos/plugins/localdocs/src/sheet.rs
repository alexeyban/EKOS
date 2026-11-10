//! RFC 0253: spreadsheets (XLSX) and CSV files.
//!
//! Each sheet becomes one or more sections headed by the sheet's name: its header row and up to
//! `[inbox] sheet-rows` rows as a pipe table, split into sections of a bounded size with the
//! header repeated in each, so no row is cut off by the section text cap. Every cell is redacted
//! before it is held. Formulas are read as their cached values; nothing is evaluated, and macros
//! are never read.

use crate::{
    DocumentParser, DocumentSection, ExtractedTable, ParseError, ParsedDocument, SECTIONS_MAX,
    TABLES_MAX, TEXT_CHUNK_CHAR_BUDGET, scrub,
};
use calamine::{Reader, Xlsx};

/// Default for `[inbox] sheet-rows`.
pub const DEFAULT_SHEET_ROWS: usize = 200;
/// Longest cell text kept.
const CELL_MAX_CHARS: usize = 500;

/// Reads `.xlsx` workbooks or `.csv` files, depending on the extension it is registered for.
pub struct SheetParser {
    ext: &'static str,
    max_rows: usize,
}

impl SheetParser {
    /// A parser for `ext` (`xlsx` or `csv`) reading at most `max_rows` data rows per sheet.
    pub fn new(ext: &'static str, max_rows: usize) -> Self {
        Self { ext, max_rows }
    }
}

/// One sheet: its name (none for a CSV file) and its non-empty rows, header first.
struct Sheet {
    name: Option<String>,
    rows: Vec<Vec<String>>,
}

impl DocumentParser for SheetParser {
    fn supported_extension(&self) -> &str {
        self.ext
    }

    fn parse(&self, bytes: &[u8]) -> Result<ParsedDocument, ParseError> {
        let sheets = if self.ext.eq_ignore_ascii_case("csv") {
            vec![read_csv(bytes, self.max_rows)?]
        } else {
            read_xlsx(bytes, self.max_rows)?
        };
        let mut sections = Vec::new();
        let mut tables = Vec::new();
        for sheet in &sheets {
            if sheet.rows.is_empty() {
                continue;
            }
            for text in chunks(&sheet.rows) {
                if sections.len() >= SECTIONS_MAX {
                    break;
                }
                sections.push(DocumentSection {
                    page: None,
                    index: sections.len(),
                    text,
                    heading: sheet.name.clone(),
                    heading_path: sheet.name.iter().cloned().collect(),
                    ..Default::default()
                });
            }
            if tables.len() < TABLES_MAX {
                tables.push(ExtractedTable {
                    page: None,
                    rows: sheet.rows.clone(),
                });
            }
        }
        let text = sections
            .iter()
            .map(|s| match &s.heading {
                Some(h) => format!("{h}\n{}", s.text),
                None => s.text.clone(),
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        Ok(ParsedDocument {
            page_count: Some(u32::try_from(sheets.len()).unwrap_or(u32::MAX)),
            text,
            tables,
            images: Vec::new(),
            sections,
            front_matter: Default::default(),
        })
    }
}

/// A cell as held: redacted, single-line, capped.
fn cell(raw: &str) -> String {
    let flat = raw.trim().replace(['\r', '\n'], " ");
    let capped: String = flat.chars().take(CELL_MAX_CHARS).collect();
    scrub(&capped)
}

/// Keeps non-empty rows: the first one (the header) and up to `max_rows` after it.
fn keep_rows(rows: impl Iterator<Item = Vec<String>>, max_rows: usize) -> Vec<Vec<String>> {
    rows.filter(|r| r.iter().any(|c| !c.is_empty()))
        .take(max_rows.saturating_add(1))
        .collect()
}

fn read_xlsx(bytes: &[u8], max_rows: usize) -> Result<Vec<Sheet>, ParseError> {
    let malformed = |e: String| ParseError::Malformed(format!("xlsx read failed: {e}"));
    let mut workbook: Xlsx<_> =
        Xlsx::new(std::io::Cursor::new(bytes)).map_err(|e| malformed(e.to_string()))?;
    let mut sheets = Vec::new();
    for name in workbook.sheet_names() {
        let range = workbook
            .worksheet_range(&name)
            .map_err(|e| malformed(format!("{name}: {e}")))?;
        let rows = keep_rows(
            range
                .rows()
                .map(|row| row.iter().map(|c| cell(&c.to_string())).collect()),
            max_rows,
        );
        sheets.push(Sheet {
            name: Some(cell(&name)),
            rows,
        });
    }
    Ok(sheets)
}

fn read_csv(bytes: &[u8], max_rows: usize) -> Result<Sheet, ParseError> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(bytes);
    let mut records = Vec::new();
    for record in reader.byte_records() {
        let record = record.map_err(|e| ParseError::Malformed(format!("csv read failed: {e}")))?;
        records.push(
            record
                .iter()
                .map(|f| cell(&String::from_utf8_lossy(f)))
                .collect::<Vec<_>>(),
        );
        if records.len() > max_rows.saturating_add(1) * 4 {
            // Enough rows to fill the cap even with blank lines among them.
            break;
        }
    }
    Ok(Sheet {
        name: None,
        rows: keep_rows(records.into_iter(), max_rows),
    })
}

/// A row as a pipe-table line.
fn line(row: &[String]) -> String {
    let cells: Vec<String> = row.iter().map(|c| c.replace('|', "\\|")).collect();
    format!("| {} |", cells.join(" | "))
}

/// The rows as pipe-table text, split into chunks of about [`TEXT_CHUNK_CHAR_BUDGET`] characters,
/// each starting with the header row.
fn chunks(rows: &[Vec<String>]) -> Vec<String> {
    let Some((header, data)) = rows.split_first() else {
        return Vec::new();
    };
    let header = line(header);
    let mut out = Vec::new();
    let mut current = header.clone();
    for row in data {
        let next = line(row);
        if current.len() + next.len() + 1 > TEXT_CHUNK_CHAR_BUDGET && current != header {
            out.push(std::mem::replace(&mut current, header.clone()));
        }
        current.push('\n');
        current.push_str(&next);
    }
    out.push(current);
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    fn esc(s: &str) -> String {
        s.replace('&', "&amp;").replace('<', "&lt;")
    }

    /// A minimal XLSX workbook: one sheet per `(name, rows)`, cells as inline strings.
    pub(crate) fn workbook(sheets: &[(&str, Vec<Vec<&str>>)]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        let mut put = |name: &str, body: String| {
            zip.start_file(name, opts).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        };
        let mut types = String::new();
        let mut entries = String::new();
        let mut rels = String::new();
        for (i, (name, rows)) in sheets.iter().enumerate() {
            let n = i + 1;
            types.push_str(&format!(
                r#"<Override PartName="/xl/worksheets/sheet{n}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#
            ));
            entries.push_str(&format!(
                r#"<sheet name="{}" sheetId="{n}" r:id="rId{n}"/>"#,
                esc(name)
            ));
            rels.push_str(&format!(
                r#"<Relationship Id="rId{n}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{n}.xml"/>"#
            ));
            let data: String = rows
                .iter()
                .enumerate()
                .map(|(r, row)| {
                    let cells: String = row
                        .iter()
                        .enumerate()
                        .map(|(c, v)| {
                            let col = char::from(b'A' + u8::try_from(c).unwrap());
                            format!(
                                r#"<c r="{col}{}" t="inlineStr"><is><t>{}</t></is></c>"#,
                                r + 1,
                                esc(v)
                            )
                        })
                        .collect();
                    format!(r#"<row r="{}">{cells}</row>"#, r + 1)
                })
                .collect();
            put(
                &format!("xl/worksheets/sheet{n}.xml"),
                format!(
                    r#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>{data}</sheetData></worksheet>"#
                ),
            );
        }
        put(
            "[Content_Types].xml",
            format!(
                r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>{types}</Types>"#
            ),
        );
        put(
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#.to_string(),
        );
        put(
            "xl/workbook.xml",
            format!(
                r#"<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>{entries}</sheets></workbook>"#
            ),
        );
        put(
            "xl/_rels/workbook.xml.rels",
            format!(
                r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{rels}</Relationships>"#
            ),
        );
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn each_sheet_is_a_section_with_its_header_and_rows() {
        let bytes = workbook(&[
            (
                "Orders",
                vec![
                    vec!["id", "status", "meaning"],
                    vec!["1", "7", "cancelled by the customer"],
                    vec!["2", "9", "refunded"],
                ],
            ),
            ("Notes", vec![vec!["note"], vec!["amounts are in cents"]]),
        ]);
        let doc = SheetParser::new("xlsx", 200).parse(&bytes).unwrap();
        assert_eq!(doc.page_count, Some(2));
        assert_eq!(doc.sections.len(), 2);
        let orders = &doc.sections[0];
        assert_eq!(orders.heading.as_deref(), Some("Orders"));
        assert_eq!(
            orders.text,
            "| id | status | meaning |\n| 1 | 7 | cancelled by the customer |\n| 2 | 9 | refunded |"
        );
        assert_eq!(doc.sections[1].heading_path, vec!["Notes".to_string()]);
        assert_eq!(doc.tables.len(), 2);
        assert_eq!(doc.tables[0].rows[1][2], "cancelled by the customer");
    }

    #[test]
    fn a_secret_and_personal_data_in_a_cell_are_redacted() {
        let bytes = workbook(&[(
            "Contacts",
            vec![
                vec!["name", "email", "config"],
                vec![
                    "Ivana",
                    "ivana@example.com",
                    "token AKIAABCDEFGHIJKLMNOP here",
                ],
            ],
        )]);
        let doc = SheetParser::new("xlsx", 200).parse(&bytes).unwrap();
        let all = format!("{}{:?}", doc.text, doc.tables);
        assert!(!all.contains("ivana@example.com"));
        assert!(!all.contains("AKIAABCDEFGHIJKLMNOP"));
        assert!(all.contains("[REDACTED:email]"));
        assert!(all.contains("[REDACTED:aws-access-key-id]"));
    }

    #[test]
    fn rows_are_capped_at_the_configured_number() {
        let mut rows = vec![vec!["n"]];
        let numbers: Vec<String> = (0..50).map(|i| format!("row{i}")).collect();
        rows.extend(numbers.iter().map(|n| vec![n.as_str()]));
        let bytes = workbook(&[("Big", rows)]);
        let doc = SheetParser::new("xlsx", 10).parse(&bytes).unwrap();
        assert_eq!(doc.tables[0].rows.len(), 11);
        assert!(doc.text.contains("row9"));
        assert!(!doc.text.contains("row10"));
    }

    #[test]
    fn a_long_sheet_splits_into_sections_that_each_repeat_the_header() {
        let mut rows = vec![vec!["code", "meaning"]];
        let long = "x".repeat(200);
        for _ in 0..40 {
            rows.push(vec!["7", long.as_str()]);
        }
        let bytes = workbook(&[("Codes", rows)]);
        let doc = SheetParser::new("xlsx", 200).parse(&bytes).unwrap();
        assert!(doc.sections.len() > 1);
        for s in &doc.sections {
            assert!(s.text.starts_with("| code | meaning |"));
            assert!(s.text.len() <= TEXT_CHUNK_CHAR_BUDGET + 300);
            assert_eq!(s.heading.as_deref(), Some("Codes"));
        }
    }

    #[test]
    fn csv_is_one_unnamed_sheet_with_quoting_and_pipes_handled() {
        let csv = "code,meaning\n7,\"cancelled, by the customer\"\n\n9,a|b\n";
        let doc = SheetParser::new("csv", 200).parse(csv.as_bytes()).unwrap();
        assert_eq!(doc.sections.len(), 1);
        assert_eq!(doc.sections[0].heading, None);
        assert_eq!(
            doc.sections[0].text,
            "| code | meaning |\n| 7 | cancelled, by the customer |\n| 9 | a\\|b |"
        );
    }

    #[test]
    fn a_file_that_is_not_a_workbook_is_malformed() {
        assert!(matches!(
            SheetParser::new("xlsx", 200).parse(b"not a zip"),
            Err(ParseError::Malformed(_))
        ));
    }
}
