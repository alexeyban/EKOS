//! OCR engines for embedded and standalone images (RFC 0023, RFC 0253).

use crate::{EmbeddedImage, OcrBlock, OcrEngine, OcrError};
use std::io::Write;
use std::process::Command;
use std::sync::OnceLock;

/// Default Tesseract language packs (RFC 0253): English, Russian, Serbian.
pub const DEFAULT_OCR_LANGUAGES: &str = "eng+rus+srp";

/// Real `OcrEngine`: shells out to the `tesseract` CLI. No `unsafe`, no FFI,
/// no native build-time dependency — the tradeoff is requiring the binary
/// on `PATH` at run time, which a missing-binary error turns into a
/// soft-skip (see `LocalDocsObserver::scan`) rather than a hard failure.
///
/// RFC 0253: the requested language packs are checked once against
/// `tesseract --list-langs`; a pack that is not installed is dropped with a
/// warning, and when none is left the engine falls back to what is installed.
pub struct TesseractOcr {
    requested: String,
    /// The `-l` argument actually used, resolved on first use.
    resolved: OnceLock<Result<String, String>>,
}

impl Default for TesseractOcr {
    fn default() -> Self {
        Self::new()
    }
}

impl TesseractOcr {
    /// With the default languages, [`DEFAULT_OCR_LANGUAGES`].
    pub fn new() -> Self {
        Self::with_languages(DEFAULT_OCR_LANGUAGES)
    }

    /// With `languages`, `+`-joined Tesseract pack names (`[ocr] languages`).
    pub fn with_languages(languages: &str) -> Self {
        Self {
            requested: languages.to_string(),
            resolved: OnceLock::new(),
        }
    }

    fn languages(&self) -> Result<String, OcrError> {
        self.resolved
            .get_or_init(|| {
                let installed = installed_languages()?;
                let (langs, missing) = resolve_languages(&self.requested, &installed);
                if !missing.is_empty() {
                    tracing::warn!(
                        "OCR language pack(s) {} not installed; reading images with `{}`",
                        missing.join(", "),
                        langs.as_deref().unwrap_or("-")
                    );
                }
                langs.ok_or_else(|| "tesseract has no language packs installed".to_string())
            })
            .clone()
            .map_err(OcrError::Unavailable)
    }

    /// Runs tesseract over `image`; `tsv` asks for the word table instead of plain text.
    fn run(&self, image: &EmbeddedImage, tsv: bool) -> Result<String, OcrError> {
        let langs = self.languages()?;
        let mut tmp = tempfile::Builder::new()
            .suffix(&format!(".{}", image.format.extension()))
            .tempfile()
            .map_err(|e| OcrError::Failed(format!("cannot create temp file: {e}")))?;
        tmp.write_all(&image.bytes)
            .map_err(|e| OcrError::Failed(format!("cannot write temp file: {e}")))?;

        let mut command = Command::new("tesseract");
        command.arg(tmp.path()).arg("stdout").arg("-l").arg(&langs);
        if tsv {
            command.arg("tsv");
        }
        let output = match command.output() {
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

impl OcrEngine for TesseractOcr {
    fn recognize(&self, image: &EmbeddedImage) -> Result<String, OcrError> {
        self.run(image, false)
    }

    fn recognize_blocks(&self, image: &EmbeddedImage) -> Result<Vec<OcrBlock>, OcrError> {
        Ok(blocks_from_tsv(&self.run(image, true)?))
    }
}

/// The language packs `tesseract --list-langs` reports, without `osd` (orientation only).
fn installed_languages() -> Result<Vec<String>, String> {
    let output = match Command::new("tesseract").arg("--list-langs").output() {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err("tesseract binary not found on PATH".into());
        }
        Err(e) => return Err(e.to_string()),
    };
    // Old versions print the list on stderr.
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push('\n');
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    Ok(parse_language_list(&text))
}

fn parse_language_list(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| {
            !l.is_empty()
                && *l != "osd"
                && l.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        })
        .map(str::to_string)
        .collect()
}

/// The `-l` argument for `requested` given the `installed` packs, and the requested packs that
/// are missing. Falls back to `eng`, then to the first installed pack, when none requested is
/// installed; `None` only when nothing is installed.
pub(crate) fn resolve_languages(
    requested: &str,
    installed: &[String],
) -> (Option<String>, Vec<String>) {
    let mut keep: Vec<&str> = Vec::new();
    let mut missing = Vec::new();
    for lang in requested
        .split('+')
        .map(str::trim)
        .filter(|l| !l.is_empty())
    {
        if !installed.iter().any(|i| i == lang) {
            missing.push(lang.to_string());
        } else if !keep.contains(&lang) {
            keep.push(lang);
        }
    }
    if !keep.is_empty() {
        return (Some(keep.join("+")), missing);
    }
    let fallback = installed
        .iter()
        .find(|i| *i == "eng")
        .or_else(|| installed.first())
        .cloned();
    (fallback, missing)
}

/// A paragraph being assembled from TSV word rows.
struct BlockAcc {
    key: (u32, u32, u32),
    lines: Vec<(u32, Vec<String>)>,
    conf_sum: f64,
    conf_n: u32,
}

impl BlockAcc {
    fn finish(self) -> OcrBlock {
        let text = self
            .lines
            .iter()
            .map(|(_, words)| words.join(" "))
            .collect::<Vec<_>>()
            .join("\n");
        let confidence = (self.conf_n > 0).then(|| {
            let mean = self.conf_sum / f64::from(self.conf_n);
            ((mean * 10.0).round() / 10.0) as f32
        });
        OcrBlock { text, confidence }
    }
}

/// Text blocks from Tesseract's TSV output: one block per paragraph (page, block, paragraph),
/// lines joined by newlines, words by spaces, confidence the mean of the words' (0–100, one
/// decimal). Words Tesseract gives no confidence (`-1`) do not count towards it.
pub(crate) fn blocks_from_tsv(tsv: &str) -> Vec<OcrBlock> {
    let mut blocks: Vec<BlockAcc> = Vec::new();
    for row in tsv.lines().skip(1) {
        let cols: Vec<&str> = row.split('\t').collect();
        if cols.len() < 12 || cols[0] != "5" {
            continue;
        }
        let word = cols[11].trim();
        if word.is_empty() {
            continue;
        }
        let num = |i: usize| cols[i].parse::<u32>().unwrap_or(0);
        let key = (num(1), num(2), num(3));
        let line = num(4);
        let conf = cols[10].parse::<f64>().unwrap_or(-1.0);
        if blocks.last().is_none_or(|b| b.key != key) {
            blocks.push(BlockAcc {
                key,
                lines: Vec::new(),
                conf_sum: 0.0,
                conf_n: 0,
            });
        }
        let acc = blocks.last_mut().expect("pushed above");
        match acc.lines.last_mut() {
            Some((n, words)) if *n == line => words.push(word.to_string()),
            _ => acc.lines.push((line, vec![word.to_string()])),
        }
        if conf >= 0.0 {
            acc.conf_sum += conf;
            acc.conf_n += 1;
        }
    }
    blocks.into_iter().map(BlockAcc::finish).collect()
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
    fn default_blocks_split_plain_text_on_blank_lines_without_confidence() {
        let ocr = MockOcr::new("first para\nline two\n\n\nsecond para\n");
        let image = EmbeddedImage {
            page: None,
            bytes: vec![],
            format: ImageFormat::Png,
        };
        let blocks = ocr.recognize_blocks(&image).unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].text, "first para\nline two");
        assert_eq!(blocks[1].text, "second para");
        assert!(blocks.iter().all(|b| b.confidence.is_none()));
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

    const TSV: &str = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext
1\t1\t0\t0\t0\t0\t0\t0\t900\t360\t-1\t
5\t1\t1\t1\t1\t1\t42\t35\t78\t21\t96\tOrder
5\t1\t1\t1\t1\t2\t130\t37\t83\t19\t96\tstatus
5\t1\t1\t1\t2\t1\t42\t81\t86\t20\t90\tStatus
5\t1\t1\t1\t2\t2\t140\t81\t13\t20\t95\t7
5\t1\t2\t1\t1\t1\t42\t196\t113\t20\t80\tContact:
5\t1\t2\t1\t1\t2\t42\t196\t113\t20\t-1\t
";

    #[test]
    fn tsv_words_group_into_paragraph_blocks_with_mean_confidence() {
        let blocks = blocks_from_tsv(TSV);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].text, "Order status\nStatus 7");
        assert_eq!(blocks[0].confidence, Some(94.3));
        assert_eq!(blocks[1].text, "Contact:");
        assert_eq!(blocks[1].confidence, Some(80.0));
    }

    #[test]
    fn missing_language_packs_are_dropped_and_reported() {
        let installed = vec!["eng".to_string()];
        let (langs, missing) = resolve_languages("eng+rus+srp", &installed);
        assert_eq!(langs.as_deref(), Some("eng"));
        assert_eq!(missing, vec!["rus".to_string(), "srp".to_string()]);
    }

    #[test]
    fn no_requested_pack_installed_falls_back_to_what_is() {
        let installed = vec!["deu".to_string(), "eng".to_string()];
        assert_eq!(
            resolve_languages("srp", &installed).0.as_deref(),
            Some("eng")
        );
        let only_deu = vec!["deu".to_string()];
        assert_eq!(
            resolve_languages("srp", &only_deu).0.as_deref(),
            Some("deu")
        );
        assert_eq!(resolve_languages("srp", &[]).0, None);
    }

    #[test]
    fn language_list_output_is_parsed_without_its_header_or_osd() {
        let out =
            "List of available languages in \"/usr/share/tessdata/\" (3):\neng\nosd\nsrp_latn\n";
        assert_eq!(parse_language_list(out), vec!["eng", "srp_latn"]);
    }
}
