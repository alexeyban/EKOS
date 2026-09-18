//! Binary format detection by **magic bytes**, never by file extension (RFC 0148).
//!
//! Extension-based detection is wrong here in both directions and expensively so. The vast
//! majority of `.dll` files on any real machine are *native* PE images with no CLI metadata at
//! all — this crate cannot read them, and must say so rather than emit an empty AST that looks
//! like an assembly with no types. The reverse also happens: managed assemblies ship as `.exe`,
//! and JVM classes live inside `.jar`/`.war`/`.ear` (and occasionally `.zip`) containers whose
//! names say nothing about their contents.

use crate::ast::BinaryKind;

/// A JVM class file's first four bytes.
const CLASS_MAGIC: [u8; 4] = [0xCA, 0xFE, 0xBA, 0xBE];
/// Local file header of a zip entry — the start of every jar/war/ear.
const ZIP_MAGIC: [u8; 2] = *b"PK";

/// What a file's leading bytes say it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detected {
    /// A loose `.class` file.
    ClassFile,
    /// A zip container that may hold class files — jar/war/ear. Membership is confirmed by
    /// actually finding `.class` members, not by this detection alone.
    Archive,
    /// A PE image carrying a CLI header: a managed .NET assembly.
    ManagedPe,
    /// A PE image with no CLI header — native code. Explicitly *not* an error: this is the
    /// common case for `.dll`, and reporting it distinctly is what keeps "we skipped 400 native
    /// DLLs" from looking like "we found nothing in 400 assemblies".
    NativePe,
    /// Anything else.
    Unknown,
}

impl Detected {
    /// The `BinaryKind` this maps to, or `None` for formats this crate does not read.
    pub fn kind(self) -> Option<BinaryKind> {
        match self {
            Self::ClassFile | Self::Archive => Some(BinaryKind::Jvm),
            Self::ManagedPe => Some(BinaryKind::DotNet),
            Self::NativePe | Self::Unknown => None,
        }
    }
}

/// Classify a file from its bytes.
///
/// Cheap by construction: the JVM and zip cases need 4 bytes, and the PE case walks a handful of
/// headers without parsing metadata. Safe on any input — a truncated or hostile file returns
/// [`Detected::Unknown`] rather than panicking, because every read here is bounds-checked.
pub fn detect(bytes: &[u8]) -> Detected {
    if bytes.len() >= 4 && bytes[..4] == CLASS_MAGIC {
        return Detected::ClassFile;
    }
    if bytes.len() >= 2 && bytes[..2] == ZIP_MAGIC {
        return Detected::Archive;
    }
    match pe_cli_directory(bytes) {
        PeProbe::NotPe => Detected::Unknown,
        PeProbe::Native => Detected::NativePe,
        PeProbe::Managed { .. } => Detected::ManagedPe,
    }
}

/// Result of probing a file for a PE header and its CLI data directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PeProbe {
    NotPe,
    Native,
    /// A managed image: `rva`/`size` locate the CLI header within the image's virtual layout.
    Managed {
        rva: u32,
        size: u32,
    },
}

pub(crate) fn read_u16(bytes: &[u8], at: usize) -> Option<u16> {
    let slice = bytes.get(at..at + 2)?;
    Some(u16::from_le_bytes([slice[0], slice[1]]))
}

pub(crate) fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    let slice = bytes.get(at..at + 4)?;
    Some(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

/// Walk `MZ` → `e_lfanew` → `PE\0\0` → COFF → optional header → data directory 14.
///
/// Data directory index 14 is the CLI header (ECMA-335 II.25.3.3); its presence with a non-zero
/// RVA is the definition of a managed image.
pub(crate) fn pe_cli_directory(bytes: &[u8]) -> PeProbe {
    if bytes.len() < 0x40 || bytes[0] != b'M' || bytes[1] != b'Z' {
        return PeProbe::NotPe;
    }
    let Some(pe_off) = read_u32(bytes, 0x3C).map(|v| v as usize) else {
        return PeProbe::NotPe;
    };
    if bytes.get(pe_off..pe_off + 4) != Some(&[b'P', b'E', 0, 0]) {
        return PeProbe::NotPe;
    }

    // COFF header is 20 bytes; the optional header follows it.
    let coff = pe_off + 4;
    let Some(opt_size) = read_u16(bytes, coff + 16).map(|v| v as usize) else {
        return PeProbe::NotPe;
    };
    let opt = coff + 20;
    if opt_size == 0 {
        // An object file, not an image — it has no data directories at all.
        return PeProbe::Native;
    }
    let Some(magic) = read_u16(bytes, opt) else {
        return PeProbe::NotPe;
    };

    // The data directories sit at a different offset in PE32 and PE32+ because eight fields
    // between them widen from 4 to 8 bytes. Getting this wrong reads garbage rather than
    // failing, which is why it is spelled out rather than inferred.
    let dir_off = match magic {
        0x10B => opt + 96,  // PE32
        0x20B => opt + 112, // PE32+
        _ => return PeProbe::NotPe,
    };
    let Some(num_dirs) = read_u32(bytes, dir_off - 4) else {
        return PeProbe::NotPe;
    };
    if num_dirs < 15 {
        return PeProbe::Native;
    }
    // Each directory entry is (rva: u32, size: u32); the CLI header is index 14.
    let cli = dir_off + 14 * 8;
    match (read_u32(bytes, cli), read_u32(bytes, cli + 4)) {
        (Some(rva), Some(size)) if rva != 0 && size != 0 => PeProbe::Managed { rva, size },
        (Some(_), Some(_)) => PeProbe::Native,
        _ => PeProbe::NotPe,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_class_file_is_detected_by_its_magic() {
        assert_eq!(detect(&[0xCA, 0xFE, 0xBA, 0xBE, 0, 0]), Detected::ClassFile);
        assert_eq!(
            detect(&[0xCA, 0xFE, 0xBA, 0xBE]).kind(),
            Some(BinaryKind::Jvm)
        );
    }

    #[test]
    fn a_zip_container_is_detected_by_its_magic() {
        assert_eq!(detect(b"PK\x03\x04rest"), Detected::Archive);
    }

    #[test]
    fn short_and_empty_input_is_unknown_not_a_panic() {
        // The whole point of bounds-checking every read: a truncated file is a diagnostic, never
        // a crash in a compiler pass.
        assert_eq!(detect(&[]), Detected::Unknown);
        assert_eq!(detect(&[0xCA]), Detected::Unknown);
        assert_eq!(detect(&[0xCA, 0xFE]), Detected::Unknown);
        assert_eq!(detect(b"M"), Detected::Unknown);
    }

    #[test]
    fn plain_text_is_unknown() {
        assert_eq!(detect(b"#!/usr/bin/perl\nuse strict;\n"), Detected::Unknown);
    }

    #[test]
    fn an_mz_stub_with_no_pe_signature_is_not_a_pe() {
        let mut bytes = vec![0u8; 0x80];
        bytes[0] = b'M';
        bytes[1] = b'Z';
        bytes[0x3C..0x40].copy_from_slice(&0x40u32.to_le_bytes());
        // 0x40 holds zeroes, not "PE\0\0".
        assert_eq!(detect(&bytes), Detected::Unknown);
    }

    /// Builds a minimal PE32 image whose CLI data directory is present or absent, to prove the
    /// managed/native split is decided by directory 14 and nothing else.
    fn synthetic_pe(num_dirs: u32, cli_rva: u32) -> Vec<u8> {
        let pe_off = 0x80usize;
        let mut b = vec![0u8; 0x400];
        b[0] = b'M';
        b[1] = b'Z';
        b[0x3C..0x40].copy_from_slice(&(pe_off as u32).to_le_bytes());
        b[pe_off..pe_off + 4].copy_from_slice(b"PE\0\0");
        let coff = pe_off + 4;
        b[coff + 16..coff + 18].copy_from_slice(&224u16.to_le_bytes()); // SizeOfOptionalHeader
        let opt = coff + 20;
        b[opt..opt + 2].copy_from_slice(&0x10Bu16.to_le_bytes()); // PE32
        let dir_off = opt + 96;
        b[dir_off - 4..dir_off].copy_from_slice(&num_dirs.to_le_bytes());
        if num_dirs >= 15 {
            let cli = dir_off + 14 * 8;
            b[cli..cli + 4].copy_from_slice(&cli_rva.to_le_bytes());
            b[cli + 4..cli + 8].copy_from_slice(&72u32.to_le_bytes());
        }
        b
    }

    #[test]
    fn a_pe_with_a_cli_directory_is_managed() {
        assert_eq!(detect(&synthetic_pe(16, 0x2008)), Detected::ManagedPe);
    }

    #[test]
    fn a_pe_with_a_zero_cli_directory_is_native() {
        // This is the overwhelmingly common `.dll` — reported distinctly so a run that skipped
        // 400 native libraries cannot be mistaken for 400 empty assemblies.
        assert_eq!(detect(&synthetic_pe(16, 0)), Detected::NativePe);
        assert_eq!(detect(&synthetic_pe(16, 0)).kind(), None);
    }

    #[test]
    fn a_pe_with_too_few_data_directories_is_native() {
        assert_eq!(detect(&synthetic_pe(10, 0x2008)), Detected::NativePe);
    }
}
