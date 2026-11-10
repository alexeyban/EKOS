//! RFC 0253: PowerPoint (PPTX) decks — per slide its title, its text and its speaker notes.
//!
//! Read from the package's PresentationML with `roxmltree`, the same way the DOCX reader reads
//! WordprocessingML: slides in the order `ppt/presentation.xml` lists them, each slide's notes
//! and pictures found through the slide's own relationships. Nothing is rendered or run; pictures
//! go to the observer's OCR path as embedded images.

use crate::{
    DocumentParser, DocumentSection, EmbeddedImage, ImageFormat, ParseError, ParsedDocument,
    SECTIONS_MAX, ooxml, scrub,
};
use roxmltree::Node;

/// DrawingML (text) and PresentationML namespaces, transitional and strict.
const A_NS: [&str; 2] = [
    "http://schemas.openxmlformats.org/drawingml/2006/main",
    "http://purl.oclc.org/ooxml/drawingml/main",
];
const P_NS: [&str; 2] = [
    "http://schemas.openxmlformats.org/presentationml/2006/main",
    "http://purl.oclc.org/ooxml/presentationml/main",
];
/// Office document relationships namespaces (the `r:id` attribute).
const R_NS: [&str; 2] = [
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
    "http://purl.oclc.org/ooxml/officeDocument/relationships",
];
/// Placeholders whose text is chrome, not content: slide number, date, footer, header, and the
/// picture of the slide on a notes page.
const CHROME_PLACEHOLDERS: [&str; 5] = ["sldNum", "dt", "ftr", "hdr", "sldImg"];
/// Most slides read from one deck.
const SLIDES_MAX: usize = SECTIONS_MAX;
/// Most pictures handed to OCR from one deck.
const IMAGES_MAX: usize = 50;

pub struct SlidesParser;

/// What one slide says.
#[derive(Debug, Default, PartialEq)]
struct Slide {
    title: Option<String>,
    body: Vec<String>,
    notes: Vec<String>,
}

impl Slide {
    fn text(&self) -> String {
        let mut out = Vec::new();
        if let Some(title) = &self.title {
            out.push(title.clone());
        }
        out.extend(self.body.iter().cloned());
        if !self.notes.is_empty() {
            out.push(String::new());
            out.push("Notes:".to_string());
            out.extend(self.notes.iter().cloned());
        }
        out.join("\n")
    }
}

impl DocumentParser for SlidesParser {
    fn supported_extension(&self) -> &str {
        "pptx"
    }

    fn parse(&self, bytes: &[u8]) -> Result<ParsedDocument, ParseError> {
        let malformed = |e: String| ParseError::Malformed(format!("pptx read failed: {e}"));
        let mut package = ooxml::open(bytes).map_err(malformed)?;
        let slide_parts = slide_order(&mut package).map_err(malformed)?;

        let mut sections = Vec::new();
        let mut images = Vec::new();
        for (i, part) in slide_parts.iter().take(SLIDES_MAX).enumerate() {
            let number = u32::try_from(i + 1).unwrap_or(u32::MAX);
            let (slide, pictures) = read_slide(&mut package, part).map_err(malformed)?;
            for (format, bytes) in pictures {
                if images.len() < IMAGES_MAX {
                    images.push(EmbeddedImage {
                        page: Some(number),
                        bytes,
                        format,
                    });
                }
            }
            let text = scrub(&slide.text());
            if text.trim().is_empty() {
                continue;
            }
            let title = slide.title.as_deref().map(scrub);
            sections.push(DocumentSection {
                page: Some(number),
                index: sections.len(),
                text,
                heading_path: title.iter().cloned().collect(),
                heading: title,
                ..Default::default()
            });
        }
        let text = sections
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        Ok(ParsedDocument {
            page_count: Some(u32::try_from(slide_parts.len()).unwrap_or(u32::MAX)),
            text,
            tables: Vec::new(),
            images,
            sections,
            front_matter: Default::default(),
        })
    }
}

/// Slide part names in presentation order (`p:sldIdLst`), falling back to the slide parts'
/// numbers when the presentation part does not list them.
fn slide_order(package: &mut ooxml::Package<'_>) -> Result<Vec<String>, String> {
    let rels = ooxml::relationships(package, "ppt/_rels/presentation.xml.rels", "ppt");
    let listed: Vec<String> = ooxml::read_text(package, "ppt/presentation.xml")
        .ok()
        .and_then(|xml| {
            let doc = roxmltree::Document::parse(&xml).ok()?;
            let ids: Vec<String> = doc
                .descendants()
                .filter(|n| is(n, &P_NS, "sldId"))
                .filter_map(|n| r_id(&n))
                .collect();
            Some(ids)
        })
        .unwrap_or_default()
        .iter()
        .filter_map(|id| rels.iter().find(|r| &r.id == id && r.kind == "slide"))
        .map(|r| r.target.clone())
        .collect();
    if !listed.is_empty() {
        return Ok(listed);
    }
    let mut numbered: Vec<(u32, String)> = package
        .file_names()
        .filter_map(|name| {
            let name = name.ok()?;
            let n = name
                .strip_prefix("ppt/slides/slide")?
                .strip_suffix(".xml")?
                .parse()
                .ok()?;
            Some((n, name.to_string()))
        })
        .collect();
    if numbered.is_empty() {
        return Err("no slides".into());
    }
    numbered.sort();
    Ok(numbered.into_iter().map(|(_, name)| name).collect())
}

/// The `r:id` attribute of a node.
fn r_id(node: &Node) -> Option<String> {
    node.attributes()
        .find(|a| a.name() == "id" && a.namespace().is_some_and(|ns| R_NS.contains(&ns)))
        .map(|a| a.value().to_string())
}

fn is(node: &Node, namespaces: &[&str], name: &str) -> bool {
    node.is_element()
        && node.tag_name().name() == name
        && node
            .tag_name()
            .namespace()
            .is_some_and(|ns| namespaces.contains(&ns))
}

/// The slide at `part`: its text, its notes, and the pictures it shows (format, bytes).
#[allow(clippy::type_complexity)]
fn read_slide(
    package: &mut ooxml::Package<'_>,
    part: &str,
) -> Result<(Slide, Vec<(ImageFormat, Vec<u8>)>), String> {
    let xml = ooxml::read_text(package, part)?;
    let doc = roxmltree::Document::parse(&xml).map_err(|e| format!("{part}: {e}"))?;
    let mut slide = Slide::default();
    let mut titles = Vec::new();
    for (role, line) in paragraphs(&doc) {
        match role {
            Role::Title => titles.push(line),
            Role::Body => slide.body.push(line),
            Role::Chrome => {}
        }
    }
    if !titles.is_empty() {
        slide.title = Some(titles.join(" "));
    }

    let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
    let rels = ooxml::relationships(package, &format!("{dir}/_rels/{file}.rels"), dir);
    let mut pictures = Vec::new();
    for rel in rels {
        if rel.kind == "notesSlide" {
            slide.notes = notes(package, &rel.target);
        } else if rel.kind == "image"
            && let Some(format) = ImageFormat::from_name(&rel.target)
            && let Ok(bytes) = ooxml::read_bytes(package, &rel.target)
        {
            pictures.push((format, bytes));
        }
    }
    Ok((slide, pictures))
}

/// The speaker notes on the notes page `part`; empty when it is missing or unreadable.
fn notes(package: &mut ooxml::Package<'_>, part: &str) -> Vec<String> {
    let Ok(xml) = ooxml::read_text(package, part) else {
        return Vec::new();
    };
    let Ok(doc) = roxmltree::Document::parse(&xml) else {
        return Vec::new();
    };
    paragraphs(&doc)
        .into_iter()
        .filter(|(role, _)| *role != Role::Chrome)
        .map(|(_, line)| line)
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Title,
    Body,
    Chrome,
}

/// Every non-empty text paragraph (`a:p`) of a slide or notes page, in document order, with the
/// role of the shape it sits in. Paragraphs outside any shape (a table cell) are body text.
fn paragraphs(doc: &roxmltree::Document) -> Vec<(Role, String)> {
    doc.descendants()
        .filter(|n| is(n, &A_NS, "p"))
        .filter_map(|p| {
            let line: String = p
                .descendants()
                .filter(|n| is(n, &A_NS, "t"))
                .filter_map(|t| t.text())
                .collect();
            let line = line.trim().to_string();
            (!line.is_empty()).then(|| (role_of(p), line))
        })
        .collect()
}

/// The role of the shape (`p:sp`) a paragraph sits in, from its placeholder type.
fn role_of(paragraph: Node) -> Role {
    let Some(shape) = paragraph.ancestors().find(|n| is(n, &P_NS, "sp")) else {
        return Role::Body;
    };
    let placeholder = shape
        .descendants()
        .find(|n| is(n, &P_NS, "ph"))
        .and_then(|ph| ph.attribute("type"));
    match placeholder {
        Some("title" | "ctrTitle") => Role::Title,
        Some(t) if CHROME_PLACEHOLDERS.contains(&t) => Role::Chrome,
        _ => Role::Body,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    const NS: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#;
    const REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
    const REL_TYPE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

    fn shape(ph: Option<&str>, lines: &[&str]) -> String {
        let ph = ph
            .map(|t| format!(r#"<p:nvPr><p:ph type="{t}"/></p:nvPr>"#))
            .unwrap_or_else(|| "<p:nvPr/>".to_string());
        let paras: String = lines
            .iter()
            .map(|l| format!("<a:p><a:r><a:t>{l}</a:t></a:r></a:p>"))
            .collect();
        format!(
            "<p:sp><p:nvSpPr><p:cNvPr id=\"1\" name=\"s\"/><p:cNvSpPr/>{ph}</p:nvSpPr><p:txBody>{paras}</p:txBody></p:sp>"
        )
    }

    fn sld(root: &str, shapes: &[String]) -> String {
        format!(
            "<?xml version=\"1.0\"?><p:{root} {NS}><p:cSld><p:spTree>{}</p:spTree></p:cSld></p:{root}>",
            shapes.concat()
        )
    }

    fn rels(items: &[(&str, &str, &str)]) -> String {
        let body: String = items
            .iter()
            .map(|(id, kind, target)| {
                format!(r#"<Relationship Id="{id}" Type="{REL_TYPE}/{kind}" Target="{target}"/>"#)
            })
            .collect();
        format!("<?xml version=\"1.0\"?><Relationships xmlns=\"{REL_NS}\">{body}</Relationships>")
    }

    /// A deck of two slides, listed in reverse file order, the second with notes and a picture.
    pub(crate) fn deck(picture: &[u8]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        let mut put = |name: &str, bytes: &[u8]| {
            zip.start_file(name, opts).unwrap();
            zip.write_all(bytes).unwrap();
        };
        put(
            "ppt/presentation.xml",
            format!(
                r#"<?xml version="1.0"?><p:presentation {NS}><p:sldIdLst><p:sldId id="256" r:id="rId3"/><p:sldId id="257" r:id="rId2"/></p:sldIdLst></p:presentation>"#
            )
            .as_bytes(),
        );
        put(
            "ppt/_rels/presentation.xml.rels",
            rels(&[
                ("rId2", "slide", "slides/slide1.xml"),
                ("rId3", "slide", "slides/slide2.xml"),
            ])
            .as_bytes(),
        );
        // Shown first: slide2.xml.
        put(
            "ppt/slides/slide2.xml",
            sld(
                "sld",
                &[
                    shape(Some("ctrTitle"), &["Order lifecycle"]),
                    shape(Some("subTitle"), &["Finance handover, 2026"]),
                    shape(Some("sldNum"), &["1"]),
                ],
            )
            .as_bytes(),
        );
        put(
            "ppt/slides/slide1.xml",
            sld(
                "sld",
                &[
                    shape(Some("title"), &["Status codes"]),
                    shape(None, &["7 = cancelled by the customer", "9 = refunded"]),
                ],
            )
            .as_bytes(),
        );
        put(
            "ppt/slides/_rels/slide1.xml.rels",
            rels(&[
                ("rId1", "notesSlide", "../notesSlides/notesSlide1.xml"),
                ("rId2", "image", "../media/image1.png"),
                ("rId3", "image", "../media/image2.emf"),
            ])
            .as_bytes(),
        );
        put(
            "ppt/notesSlides/notesSlide1.xml",
            sld(
                "notes",
                &[
                    shape(Some("sldImg"), &[]),
                    shape(
                        Some("body"),
                        &["Ask Ivana: amounts are in cents. Mail ivana@example.com"],
                    ),
                    shape(Some("sldNum"), &["2"]),
                ],
            )
            .as_bytes(),
        );
        put("ppt/media/image1.png", picture);
        put("ppt/media/image2.emf", b"not a raster");
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn slides_come_in_presentation_order_with_titles_text_and_notes() {
        let doc = SlidesParser.parse(&deck(b"png bytes")).unwrap();
        assert_eq!(doc.page_count, Some(2));
        assert_eq!(doc.sections.len(), 2);

        let first = &doc.sections[0];
        assert_eq!(first.page, Some(1));
        assert_eq!(first.heading.as_deref(), Some("Order lifecycle"));
        assert_eq!(first.text, "Order lifecycle\nFinance handover, 2026");

        let second = &doc.sections[1];
        assert_eq!(second.heading_path, vec!["Status codes".to_string()]);
        assert!(second.text.contains("7 = cancelled by the customer"));
        assert!(
            second
                .text
                .contains("Notes:\nAsk Ivana: amounts are in cents.")
        );
        // Chrome (slide numbers) is not content.
        assert!(!second.text.contains("\n2"));
    }

    #[test]
    fn notes_pass_redaction() {
        let doc = SlidesParser.parse(&deck(b"png bytes")).unwrap();
        assert!(!doc.text.contains("ivana@example.com"));
        assert!(doc.text.contains("[REDACTED:email]"));
    }

    #[test]
    fn raster_pictures_go_to_ocr_and_vector_ones_do_not() {
        let doc = SlidesParser.parse(&deck(b"png bytes")).unwrap();
        assert_eq!(doc.images.len(), 1);
        assert_eq!(doc.images[0].format, ImageFormat::Png);
        assert_eq!(doc.images[0].page, Some(2));
        assert_eq!(doc.images[0].bytes, b"png bytes");
    }

    #[test]
    fn a_file_that_is_not_a_package_is_malformed() {
        assert!(matches!(
            SlidesParser.parse(b"not a zip"),
            Err(ParseError::Malformed(_))
        ));
    }
}
