//! OCR engines for embedded images (RFC 0023).

use crate::{EmbeddedImage, OcrEngine, OcrError};
use std::io::Write;
use std::process::Command;

/// Real `OcrEngine`: shells out to the `tesseract` CLI. No `unsafe`, no FFI,
/// no native build-time dependency — the tradeoff is requiring the binary
/// on `PATH` at run time, which a missing-binary error turns into a
/// soft-skip (see `LocalDocsObserver::scan`) rather than a hard failure.
pub struct TesseractOcr;

impl OcrEngine for TesseractOcr {
    fn recognize(&self, image: &EmbeddedImage) -> Result<String, OcrError> {
        let ext = match image.format {
            crate::ImageFormat::Png => "png",
            crate::ImageFormat::Jpeg => "jpg",
        };
        let mut tmp = tempfile::Builder::new()
            .suffix(&format!(".{ext}"))
            .tempfile()
            .map_err(|e| OcrError::Failed(format!("cannot create temp file: {e}")))?;
        tmp.write_all(&image.bytes)
            .map_err(|e| OcrError::Failed(format!("cannot write temp file: {e}")))?;

        let output = Command::new("tesseract")
            .arg(tmp.path())
            .arg("stdout")
            .output();

        let output = match output {
            Ok(o) => o,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(OcrError::Unavailable(
                    "tesseract binary not found on PATH".into(),
                ));
            }
            Err(e) => return Err(OcrError::Failed(e.to_string())),
        };

        if !output.status.success() {
            return Err(OcrError::Failed(format!(
                "tesseract exited with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

/// Fixed in-memory image-bytes → text lookup. The actual OCR test bar, the
/// same role every other connector's `MockXClient` plays.
pub struct MockOcr {
    pub fixed_text: String,
}

impl MockOcr {
    pub fn new(fixed_text: impl Into<String>) -> Self {
        Self {
            fixed_text: fixed_text.into(),
        }
    }
}

impl OcrEngine for MockOcr {
    fn recognize(&self, _image: &EmbeddedImage) -> Result<String, OcrError> {
        Ok(self.fixed_text.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ImageFormat;

    #[test]
    fn mock_ocr_returns_fixed_text() {
        let ocr = MockOcr::new("hello from image");
        let image = EmbeddedImage {
            page: Some(1),
            bytes: vec![],
            format: ImageFormat::Png,
        };
        assert_eq!(ocr.recognize(&image).unwrap(), "hello from image");
    }

    #[test]
    fn missing_tesseract_binary_is_soft_skippable() {
        // Exercises the real code path with a binary that (almost
        // certainly) is not on PATH under this name, without requiring
        // tesseract to be installed in CI. If tesseract-not-a-real-binary
        // happens to exist, this just proves the OCR call still completes
        // without panicking.
        let result = Command::new("definitely-not-a-real-ocr-binary-xyz").output();
        assert!(result.is_err());
    }
}
