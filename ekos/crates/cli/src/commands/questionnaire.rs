//! RFC 0254 §3.6: the workshop questionnaire as Word (`.docx`) and Excel (`.xlsx`) files.
//!
//! `ekos questions export --format docx|xlsx` writes the same questions the Markdown export does —
//! grouped by table, each with its id, type, the person to ask, its context and an empty answer
//! field — as a minimal Office Open XML package built here (the `zip` crate already in the tree;
//! no Office library). Every question keeps its id, so answers recorded in a workshop go back
//! through the Questions page as operator-recorded testimony (§3.3).
//!
//! Pure: questions (the JSON `ekos questions list --json` prints) in, file bytes out.

use anyhow::Result;
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::Write;
use zip::write::SimpleFileOptions;

fn text<'a>(q: &'a Value, k: &str) -> &'a str {
    q[k].as_str().unwrap_or("")
}

/// XML text: `&`, `<`, `>`, quotes escaped; characters XML 1.0 forbids dropped.
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
    out
}

/// The context facts of a question in words (`key: value`), one per line; nested values as JSON.
fn context_lines(q: &Value) -> Vec<String> {
    q["context"]
        .as_object()
        .map(|m| {
            m.iter()
                .filter(|(k, _)| k.as_str() != "also_origins")
                .map(|(k, v)| match v {
                    Value::String(s) => format!("{k}: {s}"),
                    other => format!("{k}: {other}"),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn by_table(qs: &[Value]) -> BTreeMap<String, Vec<&Value>> {
    let mut m: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
    for q in qs {
        m.entry(text(q, "table").to_string()).or_default().push(q);
    }
    m
}

fn package(parts: &[(&str, String)]) -> Result<Vec<u8>> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        // A fixed timestamp: the same questions make the same file.
        .last_modified_time(zip::DateTime::default());
    for (name, body) in parts {
        zip.start_file(*name, opts)?;
        zip.write_all(body.as_bytes())?;
    }
    Ok(zip.finish()?.into_inner())
}

const XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;

// ── .docx ──────────────────────────────────────────────────────────────────────────────────────

fn para(style: Option<&str>, s: &str) -> String {
    let p_pr = style
        .map(|st| format!(r#"<w:pPr><w:pStyle w:val="{st}"/></w:pPr>"#))
        .unwrap_or_default();
    format!(
        r#"<w:p>{p_pr}<w:r><w:t xml:space="preserve">{}</w:t></w:r></w:p>"#,
        esc(s)
    )
}

/// A one-cell bordered box for the answer.
fn answer_box() -> String {
    let border = r#"w:val="single" w:sz="4" w:space="0" w:color="808080""#;
    format!(
        r#"<w:tbl><w:tblPr><w:tblW w:w="5000" w:type="pct"/><w:tblBorders><w:top {border}/><w:left {border}/><w:bottom {border}/><w:right {border}/></w:tblBorders></w:tblPr><w:tblGrid><w:gridCol w:w="9000"/></w:tblGrid><w:tr><w:trPr><w:trHeight w:val="1200"/></w:trPr><w:tc><w:tcPr><w:tcW w:w="5000" w:type="pct"/></w:tcPr><w:p/></w:tc></w:tr></w:tbl>"#
    )
}

/// The questionnaire as a Word document: a heading per table, per question its text, its id, type
/// and addressee, its context, and an empty answer box.
pub fn docx(qs: &[Value]) -> Result<Vec<u8>> {
    let mut body = String::new();
    body.push_str(&para(Some("Title"), "Questions for the source analysis"));
    body.push_str(&para(
        None,
        "Answer in the box under each question and keep the id: answers are recorded against it.",
    ));
    for (table, items) in by_table(qs) {
        let heading = if table.is_empty() {
            "(project)".to_string()
        } else {
            table
        };
        body.push_str(&para(Some("Heading1"), &heading));
        for q in items {
            body.push_str(&para(Some("Heading2"), text(q, "text")));
            body.push_str(&para(None, &format!("id: {}", text(q, "id"))));
            body.push_str(&para(
                None,
                &format!(
                    "type: {} · ask: {}",
                    text(q, "type"),
                    q["addressee"].as_str().unwrap_or("—")
                ),
            ));
            for line in context_lines(q) {
                body.push_str(&para(None, &line));
            }
            body.push_str(&para(None, "Answer:"));
            body.push_str(&answer_box());
            body.push_str(&para(None, ""));
        }
    }
    let document = format!(
        r#"{XML}<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}<w:sectPr/></w:body></w:document>"#
    );
    let styles = format!(
        r#"{XML}<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style><w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/><w:basedOn w:val="Normal"/><w:rPr><w:b/><w:sz w:val="40"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:pPr><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:b/><w:sz w:val="32"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/><w:pPr><w:outlineLvl w:val="1"/></w:pPr><w:rPr><w:b/><w:sz w:val="26"/></w:rPr></w:style></w:styles>"#
    );
    package(&[
        (
            "[Content_Types].xml",
            format!(
                r#"{XML}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/></Types>"#
            ),
        ),
        (
            "_rels/.rels",
            format!(
                r#"{XML}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#
            ),
        ),
        (
            "word/_rels/document.xml.rels",
            format!(
                r#"{XML}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#
            ),
        ),
        ("word/document.xml", document),
        ("word/styles.xml", styles),
    ])
}

// ── .xlsx ──────────────────────────────────────────────────────────────────────────────────────

/// `A`, `B`, … for a 0-based column (the sheet has fewer than 26).
fn col_letter(i: usize) -> char {
    (b'A' + i as u8) as char
}

fn row_xml(r: usize, cells: &[String], style: Option<u32>) -> String {
    let s = style.map(|s| format!(r#" s="{s}""#)).unwrap_or_default();
    let cells: String = cells
        .iter()
        .enumerate()
        .map(|(i, v)| {
            format!(
                r#"<c r="{}{r}" t="inlineStr"{s}><is><t xml:space="preserve">{}</t></is></c>"#,
                col_letter(i),
                esc(v)
            )
        })
        .collect();
    format!(r#"<row r="{r}">{cells}</row>"#)
}

const COLUMNS: [&str; 8] = [
    "id", "table", "column", "type", "question", "context", "ask", "answer",
];

/// The questionnaire as a workbook: one sheet, one row per question (grouped by table), the last
/// column left for the answer.
pub fn xlsx(qs: &[Value]) -> Result<Vec<u8>> {
    let header: Vec<String> = COLUMNS.iter().map(ToString::to_string).collect();
    let mut rows = vec![row_xml(1, &header, Some(1))];
    let mut r = 2;
    for (_, items) in by_table(qs) {
        for q in items {
            let cells = vec![
                text(q, "id").to_string(),
                text(q, "table").to_string(),
                text(q, "column").to_string(),
                text(q, "type").to_string(),
                text(q, "text").to_string(),
                context_lines(q).join("\n"),
                q["addressee"].as_str().unwrap_or("").to_string(),
                String::new(),
            ];
            rows.push(row_xml(r, &cells, Some(2)));
            r += 1;
        }
    }
    let widths = [38, 24, 18, 18, 70, 50, 24, 50];
    let cols: String = widths
        .iter()
        .enumerate()
        .map(|(i, w)| {
            format!(
                r#"<col min="{n}" max="{n}" width="{w}" customWidth="1"/>"#,
                n = i + 1
            )
        })
        .collect();
    let sheet = format!(
        r#"{XML}<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetViews><sheetView workbookViewId="0"><pane ySplit="1" topLeftCell="A2" activePane="bottomLeft" state="frozen"/></sheetView></sheetViews><cols>{cols}</cols><sheetData>{}</sheetData></worksheet>"#,
        rows.join("")
    );
    let styles = format!(
        r#"{XML}<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fonts count="2"><font><sz val="11"/><name val="Calibri"/></font><font><b/><sz val="11"/><name val="Calibri"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="3"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="0" fontId="1" fillId="0" borderId="0" xfId="0" applyFont="1"/><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0" applyAlignment="1"><alignment vertical="top" wrapText="1"/></xf></cellXfs></styleSheet>"#
    );
    package(&[
        (
            "[Content_Types].xml",
            format!(
                r#"{XML}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/></Types>"#
            ),
        ),
        (
            "_rels/.rels",
            format!(
                r#"{XML}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
            ),
        ),
        (
            "xl/workbook.xml",
            format!(
                r#"{XML}<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Questions" sheetId="1" r:id="rId1"/></sheets></workbook>"#
            ),
        ),
        (
            "xl/_rels/workbook.xml.rels",
            format!(
                r#"{XML}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#
            ),
        ),
        ("xl/worksheets/sheet1.xml", sheet),
        ("xl/styles.xml", styles),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_plugin_localdocs::{DocumentParser, DocxParser, SheetParser};
    use serde_json::json;

    fn questions() -> Vec<Value> {
        vec![
            json!({"id": "6f1c0e1a-0000-4000-8000-000000000001", "table": "orders", "column": "status",
                   "type": "meaning_of_value", "text": "What does `orders.status = 7` mean? <&>",
                   "addressee": "ana@example.com", "context": {"value": "7", "sites": 3}}),
            json!({"id": "6f1c0e1a-0000-4000-8000-000000000002", "table": "account", "column": "",
                   "type": "what_is_a_row", "text": "What does one row of account represent?",
                   "addressee": null, "context": {}}),
        ]
    }

    /// RFC 0254 §5: a questionnaire round-trips its question ids — read back by EKOS's own
    /// document parsers (the inbox reads a returned file the same way).
    #[test]
    fn a_word_questionnaire_round_trips_its_ids() {
        let bytes = docx(&questions()).unwrap();
        let doc = DocxParser.parse(&bytes).expect("a valid .docx");
        let all: String = doc
            .sections
            .iter()
            .map(|s| s.text.as_str())
            .chain([doc.text.as_str()])
            .collect::<Vec<_>>()
            .join("\n");
        for q in questions() {
            assert!(all.contains(q["id"].as_str().unwrap()), "{all}");
        }
        assert!(all.contains("<&>"), "escaped and read back: {all}");
        assert!(
            all.find("account").unwrap() < all.find("orders").unwrap(),
            "grouped by table"
        );
    }

    #[test]
    fn an_excel_questionnaire_round_trips_its_ids() {
        let bytes = xlsx(&questions()).unwrap();
        let doc = SheetParser::new("xlsx", 100)
            .parse(&bytes)
            .expect("a valid .xlsx");
        let all = format!("{}\n{:?}", doc.text, doc.tables);
        for q in questions() {
            assert!(all.contains(q["id"].as_str().unwrap()), "{all}");
        }
        assert!(all.contains("answer"), "{all}");
    }

    #[test]
    fn the_same_questions_make_the_same_file() {
        assert_eq!(docx(&questions()).unwrap(), docx(&questions()).unwrap());
        assert_eq!(xlsx(&questions()).unwrap(), xlsx(&questions()).unwrap());
    }
}
