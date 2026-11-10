//! RFC 0253: reading parts of an Office Open XML package (a zip), shared by the PPTX reader.
//! Same limits as the DOCX reader: a part is read only up to a fixed size, so a zip bomb cannot
//! exhaust memory, and nothing in the package is executed or followed outside it.

use std::io::Read;

/// Largest decompressed part read.
const MAX_PART_BYTES: u64 = 256 * 1024 * 1024;

pub(crate) type Package<'a> = zip::ZipArchive<std::io::Cursor<&'a [u8]>>;

pub(crate) fn open(bytes: &[u8]) -> Result<Package<'_>, String> {
    zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())
}

/// A part's bytes, or an error naming it.
pub(crate) fn read_bytes(package: &mut Package<'_>, name: &str) -> Result<Vec<u8>, String> {
    let file = package.by_name(name).map_err(|e| format!("{name}: {e}"))?;
    let mut buf = Vec::new();
    file.take(MAX_PART_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|e| format!("{name}: {e}"))?;
    if buf.len() as u64 > MAX_PART_BYTES {
        return Err(format!("{name}: larger than {MAX_PART_BYTES} bytes"));
    }
    Ok(buf)
}

/// A part as text.
pub(crate) fn read_text(package: &mut Package<'_>, name: &str) -> Result<String, String> {
    let bytes = read_bytes(package, name)?;
    String::from_utf8(bytes).map_err(|e| format!("{name}: {e}"))
}

/// One package relationship: its id, the last segment of its type URI (`slide`, `image`,
/// `notesSlide`), and its target resolved to a part name.
pub(crate) struct Relationship {
    pub id: String,
    pub kind: String,
    pub target: String,
}

/// The internal relationships in the `.rels` part `rels_name`, for a part in `base_dir`
/// (targets are relative to it). Empty when the part is missing or unreadable.
pub(crate) fn relationships(
    package: &mut Package<'_>,
    rels_name: &str,
    base_dir: &str,
) -> Vec<Relationship> {
    let Ok(xml) = read_text(package, rels_name) else {
        return Vec::new();
    };
    let Ok(doc) = roxmltree::Document::parse(&xml) else {
        return Vec::new();
    };
    doc.descendants()
        .filter(|n| n.tag_name().name() == "Relationship")
        .filter(|n| n.attribute("TargetMode") != Some("External"))
        .filter_map(|n| {
            Some(Relationship {
                id: n.attribute("Id")?.to_string(),
                kind: n.attribute("Type")?.rsplit('/').next()?.to_string(),
                target: resolve(base_dir, n.attribute("Target")?),
            })
        })
        .collect()
}

/// `target` relative to `base_dir`, with `..` segments applied; an absolute target (`/ppt/…`)
/// is taken from the package root.
pub(crate) fn resolve(base_dir: &str, target: &str) -> String {
    let mut parts: Vec<&str> = if target.starts_with('/') {
        Vec::new()
    } else {
        base_dir.split('/').filter(|p| !p.is_empty()).collect()
    };
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_targets_resolve_against_the_part_directory() {
        assert_eq!(
            resolve("ppt/slides", "../notesSlides/notesSlide1.xml"),
            "ppt/notesSlides/notesSlide1.xml"
        );
        assert_eq!(
            resolve("ppt/slides", "/ppt/media/image1.png"),
            "ppt/media/image1.png"
        );
        assert_eq!(resolve("ppt", "slides/slide2.xml"), "ppt/slides/slide2.xml");
    }
}
