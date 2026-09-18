//! PE image layout: section table, RVA→file-offset translation, and the CLI header.
//!
//! A managed assembly is an ordinary PE image whose data directory 14 points at a CLI header
//! (ECMA-335 II.25.3.3), which in turn points at the metadata root. Everything inside the
//! metadata is addressed by **RVA** — an address in the image's *virtual* layout — while the file
//! on disk is laid out by section. Translating between the two is what this module exists for,
//! and getting it wrong reads plausible-looking garbage rather than failing, so every lookup is
//! explicit and bounds-checked.

use super::super::detect::{PeProbe, pe_cli_directory, read_u16, read_u32};
use crate::{BinaryError, Result};

/// One PE section, reduced to what RVA translation needs.
#[derive(Debug, Clone)]
struct Section {
    virtual_address: u32,
    virtual_size: u32,
    raw_pointer: u32,
    raw_size: u32,
}

/// A parsed managed PE image, able to resolve RVAs against the file bytes.
#[derive(Debug)]
pub struct PeImage<'a> {
    bytes: &'a [u8],
    sections: Vec<Section>,
    /// RVA and size of the metadata root, from the CLI header.
    pub metadata_rva: u32,
    pub metadata_size: u32,
}

impl<'a> PeImage<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        let PeProbe::Managed { rva: cli_rva, .. } = pe_cli_directory(bytes) else {
            return Err(BinaryError::UnsupportedFormat(
                "not a managed PE image".into(),
            ));
        };

        let sections = read_sections(bytes)?;
        let image = Self {
            bytes,
            sections,
            metadata_rva: 0,
            metadata_size: 0,
        };

        // The CLI header itself is addressed by RVA, so the section table must be read first.
        let cli = image
            .rva_to_offset(cli_rva)
            .ok_or_else(|| BinaryError::Parse("CLI header RVA is outside every section".into()))?;
        let metadata_rva = read_u32(bytes, cli + 8)
            .ok_or_else(|| BinaryError::Parse("truncated CLI header".into()))?;
        let metadata_size = read_u32(bytes, cli + 12)
            .ok_or_else(|| BinaryError::Parse("truncated CLI header".into()))?;
        Ok(Self {
            metadata_rva,
            metadata_size,
            ..image
        })
    }

    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Translate a virtual address to a file offset, or `None` when it falls outside every
    /// section — which a corrupt or hostile file will do, and which must never index blindly.
    pub fn rva_to_offset(&self, rva: u32) -> Option<usize> {
        for s in &self.sections {
            // `virtual_size` can legitimately exceed `raw_size` (BSS-style padding the loader
            // zero-fills). Clamping to the raw size is what keeps a valid RVA that points into
            // that padding from resolving to a file offset that does not exist.
            let span = s.virtual_size.min(s.raw_size);
            if rva >= s.virtual_address && rva < s.virtual_address.saturating_add(span) {
                let delta = rva - s.virtual_address;
                return Some(s.raw_pointer as usize + delta as usize);
            }
        }
        None
    }

    /// The bytes of the metadata root, bounds-checked against the real file length.
    pub fn metadata(&self) -> Result<&'a [u8]> {
        let start = self
            .rva_to_offset(self.metadata_rva)
            .ok_or_else(|| BinaryError::Parse("metadata RVA is outside every section".into()))?;
        let end = start
            .checked_add(self.metadata_size as usize)
            .unwrap_or(self.bytes.len())
            .min(self.bytes.len());
        self.bytes
            .get(start..end)
            .ok_or_else(|| BinaryError::Parse("metadata root is truncated".into()))
    }
}

fn read_sections(bytes: &[u8]) -> Result<Vec<Section>> {
    let pe_off =
        read_u32(bytes, 0x3C).ok_or_else(|| BinaryError::Parse("no PE offset".into()))? as usize;
    let coff = pe_off + 4;
    let count = read_u16(bytes, coff + 2)
        .ok_or_else(|| BinaryError::Parse("truncated COFF header".into()))?
        as usize;
    let opt_size = read_u16(bytes, coff + 16)
        .ok_or_else(|| BinaryError::Parse("truncated COFF header".into()))?
        as usize;
    let table = coff + 20 + opt_size;

    // A corrupt section count would otherwise drive a huge allocation before any read fails.
    // 96 is the PE format's own documented maximum.
    if count > 96 {
        return Err(BinaryError::Parse(format!(
            "implausible section count: {count}"
        )));
    }

    let mut sections = Vec::with_capacity(count);
    for i in 0..count {
        // Each section header is 40 bytes: an 8-byte name, then the four fields below.
        let at = table + i * 40;
        let (Some(virtual_size), Some(virtual_address), Some(raw_size), Some(raw_pointer)) = (
            read_u32(bytes, at + 8),
            read_u32(bytes, at + 12),
            read_u32(bytes, at + 16),
            read_u32(bytes, at + 20),
        ) else {
            // Truncated section table: keep the sections already read rather than failing the
            // whole file. Metadata living in an earlier section is still reachable.
            break;
        };
        sections.push(Section {
            virtual_address,
            virtual_size,
            raw_pointer,
            raw_size,
        });
    }
    if sections.is_empty() {
        return Err(BinaryError::Parse("no readable sections".into()));
    }
    Ok(sections)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-section image with a CLI header, built by hand so the RVA arithmetic is checkable
    /// against known-correct numbers rather than against whatever a real file happens to contain.
    fn image_bytes() -> Vec<u8> {
        let mut b = vec![0u8; 0x1000];
        let pe_off = 0x80usize;
        b[0] = b'M';
        b[1] = b'Z';
        b[0x3C..0x40].copy_from_slice(&(pe_off as u32).to_le_bytes());
        b[pe_off..pe_off + 4].copy_from_slice(b"PE\0\0");
        let coff = pe_off + 4;
        b[coff + 2..coff + 4].copy_from_slice(&1u16.to_le_bytes()); // one section
        b[coff + 16..coff + 18].copy_from_slice(&224u16.to_le_bytes());
        let opt = coff + 20;
        b[opt..opt + 2].copy_from_slice(&0x10Bu16.to_le_bytes());
        let dir_off = opt + 96;
        b[dir_off - 4..dir_off].copy_from_slice(&16u32.to_le_bytes());
        // CLI header at RVA 0x2000.
        let cli_dir = dir_off + 14 * 8;
        b[cli_dir..cli_dir + 4].copy_from_slice(&0x2000u32.to_le_bytes());
        b[cli_dir + 4..cli_dir + 8].copy_from_slice(&72u32.to_le_bytes());
        // Section: RVA 0x2000 → file offset 0x200.
        let sect = opt + 224;
        b[sect..sect + 8].copy_from_slice(b".text\0\0\0");
        b[sect + 8..sect + 12].copy_from_slice(&0x400u32.to_le_bytes()); // VirtualSize
        b[sect + 12..sect + 16].copy_from_slice(&0x2000u32.to_le_bytes()); // VirtualAddress
        b[sect + 16..sect + 20].copy_from_slice(&0x400u32.to_le_bytes()); // SizeOfRawData
        b[sect + 20..sect + 24].copy_from_slice(&0x200u32.to_le_bytes()); // PointerToRawData
        // CLI header body at file offset 0x200: metadata RVA 0x2100, size 0x80.
        b[0x200 + 8..0x200 + 12].copy_from_slice(&0x2100u32.to_le_bytes());
        b[0x200 + 12..0x200 + 16].copy_from_slice(&0x80u32.to_le_bytes());
        b[0x200 + 16..0x200 + 20].copy_from_slice(&1u32.to_le_bytes()); // ILONLY
        b
    }

    #[test]
    fn rva_translation_follows_the_section_table() {
        let bytes = image_bytes();
        let img = PeImage::parse(&bytes).unwrap();
        assert_eq!(img.rva_to_offset(0x2000), Some(0x200));
        assert_eq!(img.rva_to_offset(0x2100), Some(0x300));
        assert_eq!(img.metadata_rva, 0x2100);
        assert_eq!(img.metadata_size, 0x80);
    }

    #[test]
    fn an_rva_outside_every_section_resolves_to_nothing() {
        let bytes = image_bytes();
        let img = PeImage::parse(&bytes).unwrap();
        // Below the only section, and far above it.
        assert_eq!(img.rva_to_offset(0x1000), None);
        assert_eq!(img.rva_to_offset(0x9000), None);
    }

    /// `VirtualSize` exceeding `SizeOfRawData` is normal (loader-zeroed padding). An RVA landing
    /// in that padding has no file offset, and must not resolve to one past the end of the data.
    #[test]
    fn an_rva_in_virtual_only_padding_does_not_resolve() {
        let mut bytes = image_bytes();
        let sect = 0x80 + 4 + 20 + 224;
        bytes[sect + 8..sect + 12].copy_from_slice(&0x800u32.to_le_bytes()); // VirtualSize 0x800
        bytes[sect + 16..sect + 20].copy_from_slice(&0x400u32.to_le_bytes()); // raw still 0x400
        let img = PeImage::parse(&bytes).unwrap();
        assert_eq!(img.rva_to_offset(0x23FF), Some(0x5FF));
        assert_eq!(img.rva_to_offset(0x2400), None, "virtual-only padding");
    }

    #[test]
    fn metadata_slice_is_clamped_to_the_real_file_length() {
        let mut bytes = image_bytes();
        // Claim a metadata size far past the end of the file.
        bytes[0x200 + 12..0x200 + 16].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        let img = PeImage::parse(&bytes).unwrap();
        let md = img.metadata().unwrap();
        assert!(!md.is_empty());
        assert!(md.len() <= bytes.len());
    }

    #[test]
    fn a_native_image_is_rejected() {
        let mut bytes = image_bytes();
        let dir_off = 0x80 + 4 + 20 + 96;
        let cli_dir = dir_off + 14 * 8;
        bytes[cli_dir..cli_dir + 4].copy_from_slice(&0u32.to_le_bytes());
        assert!(matches!(
            PeImage::parse(&bytes).unwrap_err(),
            BinaryError::UnsupportedFormat(_)
        ));
    }

    #[test]
    fn an_implausible_section_count_is_rejected_before_allocating() {
        let mut bytes = image_bytes();
        let coff = 0x80 + 4;
        bytes[coff + 2..coff + 4].copy_from_slice(&40_000u16.to_le_bytes());
        assert!(PeImage::parse(&bytes).is_err());
    }
}
