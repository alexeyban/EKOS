//! RFC 0253: standalone images (PNG, JPEG, TIFF, WebP) read through the OCR engine.
//!
//! A screenshot or a photographed page becomes a document with one section per recognised block
//! of text (a paragraph, as the engine sees it), each carrying the engine's confidence. Recognition
//! is local (Tesseract); an image is never sent anywhere. Scanned PDFs stay on the PDF parser's
//! embedded-image path.

use crate::{
    DocumentParser, DocumentSection, EmbeddedImage, ImageFormat, OcrEngine, OcrError, ParseError,
    ParsedDocument, SECTIONS_MAX, scrub,
};
use std::sync::Arc;

/// Every extension [`ImageParser`] is registered for.
pub(crate) const IMAGE_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "tif", "tiff", "webp"];

/// Reads one image format through an [`OcrEngine`]. Registered once per extension, like the
/// other multi-extension formats (RFC 0025).
pub struct ImageParser {
    ext: &'static str,
    format: ImageFormat,
    ocr: Arc<dyn OcrEngine>,
}

impl ImageParser {
    /// A parser for `ext` (one of `png`, `jpg`, `jpeg`, `tif`, `tiff`, `webp`; anything else is
    /// read as PNG).
    pub fn new(ext: &'static str, ocr: Arc<dyn OcrEngine>) -> Self {
        let format = ImageFormat::from_name(ext).unwrap_or(ImageFormat::Png);
        Self { ext, format, ocr }
    }
}

impl DocumentParser for ImageParser {
    fn supported_extension(&self) -> &str {
        self.ext
    }

    fn parse(&self, bytes: &[u8]) -> Result<ParsedDocument, ParseError> {
        let image = EmbeddedImage {
            page: None,
            bytes: bytes.to_vec(),
            format: self.format,
        };
        let blocks = match self.ocr.recognize_blocks(&image) {
            Ok(blocks) => blocks,
            Err(OcrError::Unavailable(why)) => return Err(ParseError::Skipped(why)),
            Err(OcrError::Failed(why)) => return Err(ParseError::Malformed(why)),
        };
        let sections: Vec<DocumentSection> = blocks
            .into_iter()
            .filter(|b| !b.text.trim().is_empty())
            .take(SECTIONS_MAX)
            .enumerate()
            .map(|(index, block)| DocumentSection {
                page: None,
                index,
                text: scrub(block.text.trim()),
                ocr_confidence: block.confidence,
                ..Default::default()
            })
            .collect();
        let text = sections
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        Ok(ParsedDocument {
            page_count: Some(1),
            text,
            tables: Vec::new(),
            images: Vec::new(),
            sections,
            front_matter: Default::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MockOcr, OcrBlock, TesseractOcr};

    const FIXTURE: &[u8] = include_bytes!("../tests/fixtures/order-status-scan.png");

    struct Blocks(Vec<OcrBlock>);
    impl OcrEngine for Blocks {
        fn recognize(&self, _: &EmbeddedImage) -> Result<String, OcrError> {
            unreachable!("ImageParser asks for blocks")
        }
        fn recognize_blocks(&self, _: &EmbeddedImage) -> Result<Vec<OcrBlock>, OcrError> {
            Ok(self.0.clone())
        }
    }

    struct Missing;
    impl OcrEngine for Missing {
        fn recognize(&self, _: &EmbeddedImage) -> Result<String, OcrError> {
            Err(OcrError::Unavailable(
                "tesseract binary not found on PATH".into(),
            ))
        }
    }

    #[test]
    fn each_block_is_a_section_with_its_confidence() {
        let ocr = Arc::new(Blocks(vec![
            OcrBlock {
                text: "Order status codes".into(),
                confidence: Some(95.5),
            },
            OcrBlock {
                text: "   ".into(),
                confidence: Some(10.0),
            },
            OcrBlock {
                text: "Status 7 means cancelled".into(),
                confidence: None,
            },
        ]));
        let doc = ImageParser::new("png", ocr).parse(b"fake").unwrap();
        assert_eq!(doc.sections.len(), 2);
        assert_eq!(doc.sections[0].ocr_confidence, Some(95.5));
        assert_eq!(doc.sections[1].index, 1);
        assert_eq!(doc.sections[1].ocr_confidence, None);
        assert_eq!(doc.text, "Order status codes\n\nStatus 7 means cancelled");
        assert_eq!(doc.page_count, Some(1));
    }

    #[test]
    fn personal_data_in_recognised_text_is_redacted_before_it_is_held() {
        let ocr = Arc::new(MockOcr::new(
            "Contact: jane.doe@example.com\n\nCard: 4111 1111 1111 1111",
        ));
        let doc = ImageParser::new("jpg", ocr).parse(b"fake").unwrap();
        assert!(!doc.text.contains("jane.doe"));
        assert!(!doc.text.contains("4111"));
        assert!(doc.text.contains("[REDACTED:email]"));
        assert!(doc.text.contains("[REDACTED:payment-card]"));
    }

    #[test]
    fn no_ocr_engine_skips_the_image_instead_of_failing() {
        let err = ImageParser::new("webp", Arc::new(Missing))
            .parse(b"fake")
            .unwrap_err();
        assert!(matches!(err, ParseError::Skipped(_)), "{err:?}");
    }

    fn tesseract_installed() -> bool {
        std::process::Command::new("tesseract")
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    }

    /// The real engine on a committed fixture of known text, with a seeded e-mail address and a
    /// (test) card number. Skips when tesseract is not installed.
    #[test]
    fn real_tesseract_reads_the_fixture_and_the_seeded_personal_data_is_gone() {
        if !tesseract_installed() {
            eprintln!("skipped: tesseract is not installed");
            return;
        }
        let ocr = Arc::new(TesseractOcr::new());
        let doc = ImageParser::new("png", ocr).parse(FIXTURE).unwrap();
        assert!(
            doc.text.contains("Status 7 means cancelled"),
            "{}",
            doc.text
        );
        assert!(!doc.text.contains("jane.doe"), "{}", doc.text);
        assert!(!doc.text.contains("example.com"), "{}", doc.text);
        assert!(!doc.text.contains("4111"), "{}", doc.text);
        assert!(doc.text.contains("[REDACTED:email]"), "{}", doc.text);
        assert!(doc.text.contains("[REDACTED:payment-card]"), "{}", doc.text);
        assert!(doc.sections.len() >= 2);
        for section in &doc.sections {
            let confidence = section
                .ocr_confidence
                .expect("tesseract reports confidence");
            assert!(confidence > 50.0, "low confidence {confidence}");
        }
        eprintln!(
            "fixture confidence per block: {:?}",
            doc.sections
                .iter()
                .map(|s| s.ocr_confidence)
                .collect::<Vec<_>>()
        );
    }
}
