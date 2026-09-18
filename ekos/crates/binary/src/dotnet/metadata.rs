//! CLI metadata: the `BSJB` root, its streams, the heaps, and the `#~` table stream.
//!
//! # Why the whole schema is here when only a dozen tables are read
//!
//! Metadata tables are stored back-to-back with **no index and no per-table offset**. To find
//! where `MethodDef` (table 0x06) starts, every present table below it must have its exact row
//! size computed — and a row's size depends on the `HeapSizes` byte, on other tables' row counts
//! (an index into a table with more than 2^16 rows widens from 2 to 4 bytes), and on coded-index
//! tag widths. One wrong column width anywhere below the table you want does not fail; it shifts
//! every subsequent read by a few bytes and yields plausible garbage. That is why [`SCHEMA`] is
//! complete rather than partial.
//!
//! # Failure model
//!
//! Every read is bounds-checked and returns `Option`/`Result`. A malformed row costs that row.
//! Nothing here can panic on hostile input, and nothing aborts the file — which is the entire
//! reason this reader exists instead of a dependency (see the crate docs).

use crate::{BinaryError, Result};

// ────────────────────────────────────────────────────────────────────────────
// Little-endian primitives, all bounds-checked
// ────────────────────────────────────────────────────────────────────────────

pub(crate) fn u8_at(b: &[u8], at: usize) -> Option<u8> {
    b.get(at).copied()
}

pub(crate) fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    let s = b.get(at..at + 2)?;
    Some(u16::from_le_bytes([s[0], s[1]]))
}

pub(crate) fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at + 4)?;
    Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

pub(crate) fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    let s = b.get(at..at + 8)?;
    Some(u64::from_le_bytes([
        s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7],
    ]))
}

/// ECMA-335 II.23.2 compressed unsigned integer: 1, 2 or 4 bytes, tagged by the top bits of the
/// first. Returns `(value, bytes_consumed)`.
pub(crate) fn compressed_u32(b: &[u8], at: usize) -> Option<(u32, usize)> {
    let first = u8_at(b, at)?;
    if first & 0x80 == 0 {
        Some((first as u32, 1))
    } else if first & 0xC0 == 0x80 {
        let second = u8_at(b, at + 1)?;
        Some(((((first & 0x3F) as u32) << 8) | second as u32, 2))
    } else if first & 0xE0 == 0xC0 {
        let (b1, b2, b3) = (u8_at(b, at + 1)?, u8_at(b, at + 2)?, u8_at(b, at + 3)?);
        Some((
            (((first & 0x1F) as u32) << 24) | ((b1 as u32) << 16) | ((b2 as u32) << 8) | b3 as u32,
            4,
        ))
    } else {
        // 0xF0-tagged values are reserved and appear only in corrupt or hostile blobs.
        None
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Table identifiers
// ────────────────────────────────────────────────────────────────────────────

pub const MODULE: usize = 0x00;
pub const TYPE_REF: usize = 0x01;
pub const TYPE_DEF: usize = 0x02;
pub const FIELD: usize = 0x04;
pub const METHOD_DEF: usize = 0x06;
pub const PARAM: usize = 0x08;
pub const INTERFACE_IMPL: usize = 0x09;
pub const MEMBER_REF: usize = 0x0A;
pub const MODULE_REF: usize = 0x1A;
pub const TYPE_SPEC: usize = 0x1B;
pub const ASSEMBLY: usize = 0x20;
pub const ASSEMBLY_REF: usize = 0x23;
pub const NESTED_CLASS: usize = 0x29;

/// Number of metadata tables defined by ECMA-335. The `Valid` bitmask is 64 bits wide, so the
/// arrays here are sized to it rather than to the highest table actually defined.
const TABLE_COUNT: usize = 64;

// ────────────────────────────────────────────────────────────────────────────
// Column and schema description
// ────────────────────────────────────────────────────────────────────────────

/// A coded index: one of several tables, selected by low tag bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Coded {
    /// Tables the tag selects between, in tag order. A `usize::MAX` entry is a tag value the
    /// spec reserves and never uses (`CustomAttributeType` has three).
    pub tables: &'static [usize],
    /// Bits the tag occupies — `ceil(log2(tables.len()))`.
    pub bits: u32,
}

const TYPE_DEF_OR_REF: Coded = Coded {
    tables: &[TYPE_DEF, TYPE_REF, TYPE_SPEC],
    bits: 2,
};
const HAS_CONSTANT: Coded = Coded {
    tables: &[FIELD, PARAM, 0x17],
    bits: 2,
};
const HAS_CUSTOM_ATTRIBUTE: Coded = Coded {
    tables: &[
        METHOD_DEF,
        FIELD,
        TYPE_REF,
        TYPE_DEF,
        PARAM,
        INTERFACE_IMPL,
        MEMBER_REF,
        MODULE,
        0x0E,
        0x17,
        0x14,
        0x11,
        0x1A,
        TYPE_SPEC,
        ASSEMBLY,
        ASSEMBLY_REF,
        0x26,
        0x27,
        0x28,
        0x2A,
        0x2C,
        0x2B,
    ],
    bits: 5,
};
const HAS_FIELD_MARSHAL: Coded = Coded {
    tables: &[FIELD, PARAM],
    bits: 1,
};
const HAS_DECL_SECURITY: Coded = Coded {
    tables: &[TYPE_DEF, METHOD_DEF, ASSEMBLY],
    bits: 2,
};
const MEMBER_REF_PARENT: Coded = Coded {
    tables: &[TYPE_DEF, TYPE_REF, 0x1A, METHOD_DEF, TYPE_SPEC],
    bits: 3,
};
const HAS_SEMANTICS: Coded = Coded {
    tables: &[0x14, 0x17],
    bits: 1,
};
const METHOD_DEF_OR_REF: Coded = Coded {
    tables: &[METHOD_DEF, MEMBER_REF],
    bits: 1,
};
const MEMBER_FORWARDED: Coded = Coded {
    tables: &[FIELD, METHOD_DEF],
    bits: 1,
};
const IMPLEMENTATION: Coded = Coded {
    tables: &[0x26, ASSEMBLY_REF, 0x27],
    bits: 2,
};
/// Tags 0, 1 and 4 are reserved and unused — represented by `usize::MAX` so the tag-to-table
/// mapping stays positional and a reserved tag decodes to "no table" rather than to whichever
/// table happened to be next in a compacted list.
const CUSTOM_ATTRIBUTE_TYPE: Coded = Coded {
    tables: &[usize::MAX, usize::MAX, METHOD_DEF, MEMBER_REF, usize::MAX],
    bits: 3,
};
const RESOLUTION_SCOPE: Coded = Coded {
    tables: &[MODULE, 0x1A, ASSEMBLY_REF, TYPE_REF],
    bits: 2,
};
const TYPE_OR_METHOD_DEF: Coded = Coded {
    tables: &[TYPE_DEF, METHOD_DEF],
    bits: 1,
};

/// One column of a metadata table row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Col {
    U8,
    U16,
    U32,
    /// `#Strings` heap index.
    Str,
    /// `#GUID` heap index.
    Guid,
    /// `#Blob` heap index.
    Blob,
    /// Simple index into one table.
    Table(usize),
    Coded(Coded),
}

/// Every table's column layout, indexed by table id. `&[]` means the table is not defined by the
/// spec at that id — those ids never appear in a valid `Valid` mask.
static SCHEMA: [&[Col]; TABLE_COUNT] = {
    // Written as an explicit initializer rather than a builder so the layout is readable against
    // ECMA-335 II.22 row by row, which is how it has to be checked.
    let mut s: [&[Col]; TABLE_COUNT] = [&[]; TABLE_COUNT];
    s[0x00] = &[Col::U16, Col::Str, Col::Guid, Col::Guid, Col::Guid]; // Module
    s[0x01] = &[Col::Coded(RESOLUTION_SCOPE), Col::Str, Col::Str]; // TypeRef
    s[0x02] = &[
        Col::U32,
        Col::Str,
        Col::Str,
        Col::Coded(TYPE_DEF_OR_REF),
        Col::Table(FIELD),
        Col::Table(METHOD_DEF),
    ]; // TypeDef
    s[0x03] = &[Col::Table(FIELD)]; // FieldPtr
    s[0x04] = &[Col::U16, Col::Str, Col::Blob]; // Field
    s[0x05] = &[Col::Table(METHOD_DEF)]; // MethodPtr
    s[0x06] = &[
        Col::U32,
        Col::U16,
        Col::U16,
        Col::Str,
        Col::Blob,
        Col::Table(PARAM),
    ]; // MethodDef
    s[0x07] = &[Col::Table(PARAM)]; // ParamPtr
    s[0x08] = &[Col::U16, Col::U16, Col::Str]; // Param
    s[0x09] = &[Col::Table(TYPE_DEF), Col::Coded(TYPE_DEF_OR_REF)]; // InterfaceImpl
    s[0x0A] = &[Col::Coded(MEMBER_REF_PARENT), Col::Str, Col::Blob]; // MemberRef
    s[0x0B] = &[Col::U8, Col::U8, Col::Coded(HAS_CONSTANT), Col::Blob]; // Constant
    s[0x0C] = &[
        Col::Coded(HAS_CUSTOM_ATTRIBUTE),
        Col::Coded(CUSTOM_ATTRIBUTE_TYPE),
        Col::Blob,
    ]; // CustomAttribute
    s[0x0D] = &[Col::Coded(HAS_FIELD_MARSHAL), Col::Blob]; // FieldMarshal
    s[0x0E] = &[Col::U16, Col::Coded(HAS_DECL_SECURITY), Col::Blob]; // DeclSecurity
    s[0x0F] = &[Col::U16, Col::U32, Col::Table(TYPE_DEF)]; // ClassLayout
    s[0x10] = &[Col::U32, Col::Table(FIELD)]; // FieldLayout
    s[0x11] = &[Col::Blob]; // StandAloneSig
    s[0x12] = &[Col::Table(TYPE_DEF), Col::Table(0x14)]; // EventMap
    s[0x13] = &[Col::Table(0x14)]; // EventPtr
    s[0x14] = &[Col::U16, Col::Str, Col::Coded(TYPE_DEF_OR_REF)]; // Event
    s[0x15] = &[Col::Table(TYPE_DEF), Col::Table(0x17)]; // PropertyMap
    s[0x16] = &[Col::Table(0x17)]; // PropertyPtr
    s[0x17] = &[Col::U16, Col::Str, Col::Blob]; // Property
    s[0x18] = &[Col::U16, Col::Table(METHOD_DEF), Col::Coded(HAS_SEMANTICS)]; // MethodSemantics
    s[0x19] = &[
        Col::Table(TYPE_DEF),
        Col::Coded(METHOD_DEF_OR_REF),
        Col::Coded(METHOD_DEF_OR_REF),
    ]; // MethodImpl
    s[0x1A] = &[Col::Str]; // ModuleRef
    s[0x1B] = &[Col::Blob]; // TypeSpec
    s[0x1C] = &[
        Col::U16,
        Col::Coded(MEMBER_FORWARDED),
        Col::Str,
        Col::Table(0x1A),
    ]; // ImplMap
    s[0x1D] = &[Col::U32, Col::Table(FIELD)]; // FieldRVA
    s[0x1E] = &[Col::U32, Col::U32]; // EncLog
    s[0x1F] = &[Col::U32]; // EncMap
    s[0x20] = &[
        Col::U32,
        Col::U16,
        Col::U16,
        Col::U16,
        Col::U16,
        Col::U32,
        Col::Blob,
        Col::Str,
        Col::Str,
    ]; // Assembly
    s[0x21] = &[Col::U32]; // AssemblyProcessor
    s[0x22] = &[Col::U32, Col::U32, Col::U32]; // AssemblyOS
    s[0x23] = &[
        Col::U16,
        Col::U16,
        Col::U16,
        Col::U16,
        Col::U32,
        Col::Blob,
        Col::Str,
        Col::Str,
        Col::Blob,
    ]; // AssemblyRef
    s[0x24] = &[Col::U32, Col::Table(ASSEMBLY_REF)]; // AssemblyRefProcessor
    s[0x25] = &[Col::U32, Col::U32, Col::U32, Col::Table(ASSEMBLY_REF)]; // AssemblyRefOS
    s[0x26] = &[Col::U32, Col::Str, Col::Blob]; // File
    s[0x27] = &[
        Col::U32,
        Col::U32,
        Col::Str,
        Col::Str,
        Col::Coded(IMPLEMENTATION),
    ]; // ExportedType
    s[0x28] = &[Col::U32, Col::U32, Col::Str, Col::Coded(IMPLEMENTATION)]; // ManifestResource
    s[0x29] = &[Col::Table(TYPE_DEF), Col::Table(TYPE_DEF)]; // NestedClass
    s[0x2A] = &[Col::U16, Col::U16, Col::Coded(TYPE_OR_METHOD_DEF), Col::Str]; // GenericParam
    s[0x2B] = &[Col::Coded(METHOD_DEF_OR_REF), Col::Blob]; // MethodSpec
    s[0x2C] = &[Col::Table(0x2A), Col::Coded(TYPE_DEF_OR_REF)]; // GenericParamConstraint
    s
};

// ────────────────────────────────────────────────────────────────────────────
// Heaps
// ────────────────────────────────────────────────────────────────────────────

/// The metadata heaps, as raw slices. Indices into them are validated on every access.
#[derive(Debug, Default, Clone, Copy)]
pub struct Heaps<'a> {
    pub strings: &'a [u8],
    pub user_strings: &'a [u8],
    pub blobs: &'a [u8],
    pub guids: &'a [u8],
}

impl<'a> Heaps<'a> {
    /// A null-terminated UTF-8 string from `#Strings`.
    ///
    /// Invalid UTF-8 is replaced rather than rejected: identifier heaps in real assemblies from
    /// older toolchains are not always clean, and losing a type's name over one bad byte would
    /// lose the type.
    pub fn string(&self, index: u32) -> Option<String> {
        let start = index as usize;
        let rest = self.strings.get(start..)?;
        let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
        Some(String::from_utf8_lossy(&rest[..end]).into_owned())
    }

    /// A length-prefixed blob from `#Blob`.
    pub fn blob(&self, index: u32) -> Option<&'a [u8]> {
        let at = index as usize;
        let (len, used) = compressed_u32(self.blobs, at)?;
        self.blobs.get(at + used..at + used + len as usize)
    }

    /// A user string from `#US`, addressed by the low 24 bits of a `ldstr` token.
    ///
    /// Stored as UTF-16LE with a one-byte trailing flag, so the length is always odd and the
    /// payload is `len - 1` bytes. A malformed length yields `None`, never a panic.
    pub fn user_string(&self, index: u32) -> Option<String> {
        let at = index as usize;
        let (len, used) = compressed_u32(self.user_strings, at)?;
        if len == 0 {
            return Some(String::new());
        }
        let payload = self
            .user_strings
            .get(at + used..at + used + (len as usize).saturating_sub(1))?;
        let units: Vec<u16> = payload
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        Some(String::from_utf16_lossy(&units))
    }
}

// ────────────────────────────────────────────────────────────────────────────
// The metadata root and table stream
// ────────────────────────────────────────────────────────────────────────────

/// Per-table geometry, computed once from the `#~` header.
#[derive(Debug, Clone)]
struct TableInfo {
    rows: u32,
    row_size: usize,
    /// Offset of row 1 within the table-stream slice.
    start: usize,
    /// Byte offset of each column within a row.
    col_offsets: Vec<usize>,
    col_sizes: Vec<usize>,
}

/// Fully parsed CLI metadata, ready to be read table by table.
#[derive(Debug)]
pub struct Metadata<'a> {
    pub version: String,
    pub heaps: Heaps<'a>,
    stream: &'a [u8],
    tables: Vec<Option<TableInfo>>,
}

impl<'a> Metadata<'a> {
    pub fn parse(md: &'a [u8]) -> Result<Self> {
        if u32_at(md, 0) != Some(0x424A_5342) {
            return Err(BinaryError::Parse(
                "metadata root does not start with BSJB".into(),
            ));
        }
        let version_len = u32_at(md, 12)
            .ok_or_else(|| BinaryError::Parse("truncated metadata root".into()))?
            as usize;
        let version_bytes = md
            .get(16..16 + version_len)
            .ok_or_else(|| BinaryError::Parse("truncated version string".into()))?;
        let version = String::from_utf8_lossy(
            &version_bytes[..version_bytes
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(version_bytes.len())],
        )
        .into_owned();

        // Version is padded to a 4-byte boundary, then Flags(2) and StreamCount(2).
        let after_version = 16 + version_len.next_multiple_of(4);
        let stream_count = u16_at(md, after_version + 2)
            .ok_or_else(|| BinaryError::Parse("truncated stream count".into()))?;

        let mut heaps = Heaps::default();
        let mut table_stream: Option<&[u8]> = None;
        let mut at = after_version + 4;
        for _ in 0..stream_count {
            let (Some(offset), Some(size)) = (u32_at(md, at), u32_at(md, at + 4)) else {
                break;
            };
            let name_start = at + 8;
            let name_bytes = match md.get(name_start..) {
                Some(rest) => {
                    let end = rest.iter().position(|&b| b == 0).unwrap_or(0);
                    &rest[..end]
                }
                None => break,
            };
            let name = String::from_utf8_lossy(name_bytes).into_owned();
            // Stream names are null-terminated and padded to a 4-byte boundary.
            at = name_start + (name_bytes.len() + 1).next_multiple_of(4);

            let slice = md
                .get(offset as usize..)
                .map(|s| &s[..(size as usize).min(s.len())])
                .unwrap_or(&[]);
            match name.as_str() {
                "#Strings" => heaps.strings = slice,
                "#US" => heaps.user_strings = slice,
                "#Blob" => heaps.blobs = slice,
                "#GUID" => heaps.guids = slice,
                // `#~` is the compressed (normal) table stream; `#-` is the uncompressed
                // edit-and-continue variant, which has identical row layout and appears in
                // assemblies built by some older toolchains. Reading both costs one match arm.
                "#~" | "#-" => table_stream = Some(slice),
                _ => {}
            }
        }

        let stream = table_stream
            .ok_or_else(|| BinaryError::Parse("no #~ table stream in metadata".into()))?;
        let tables = Self::layout_tables(stream, &heaps)?;

        Ok(Self {
            version,
            heaps,
            stream,
            tables,
        })
    }

    /// Compute every present table's row size and start offset.
    ///
    /// This is the part that must be exactly right for *all* tables, not just the ones read —
    /// see the module docs.
    fn layout_tables(stream: &[u8], _heaps: &Heaps<'_>) -> Result<Vec<Option<TableInfo>>> {
        let heap_sizes =
            u8_at(stream, 6).ok_or_else(|| BinaryError::Parse("truncated #~ header".into()))?;
        let valid =
            u64_at(stream, 8).ok_or_else(|| BinaryError::Parse("truncated #~ header".into()))?;

        let str_size = if heap_sizes & 0x01 != 0 { 4 } else { 2 };
        let guid_size = if heap_sizes & 0x02 != 0 { 4 } else { 2 };
        let blob_size = if heap_sizes & 0x04 != 0 { 4 } else { 2 };

        // Row counts follow the 24-byte header, one u32 per set bit in `Valid`.
        let mut rows = [0u32; TABLE_COUNT];
        let mut at = 24;
        for (t, row) in rows.iter_mut().enumerate() {
            if valid & (1u64 << t) != 0 {
                *row = u32_at(stream, at)
                    .ok_or_else(|| BinaryError::Parse("truncated row-count array".into()))?;
                at += 4;
            }
        }

        let col_size = |c: &Col| -> usize {
            match c {
                Col::U8 => 1,
                Col::U16 => 2,
                Col::U32 => 4,
                Col::Str => str_size,
                Col::Guid => guid_size,
                Col::Blob => blob_size,
                // An index into a table needs 4 bytes once that table can hold more rows than a
                // u16 addresses.
                Col::Table(t) => {
                    if rows.get(*t).copied().unwrap_or(0) > u16::MAX as u32 {
                        4
                    } else {
                        2
                    }
                }
                // A coded index needs 4 bytes once the largest candidate table's row count no
                // longer fits in the bits left after the tag.
                Col::Coded(c) => {
                    let max = c
                        .tables
                        .iter()
                        .filter(|&&t| t != usize::MAX)
                        .map(|&t| rows.get(t).copied().unwrap_or(0))
                        .max()
                        .unwrap_or(0);
                    if max as u64 >= (1u64 << (16 - c.bits)) {
                        4
                    } else {
                        2
                    }
                }
            }
        };

        let mut infos: Vec<Option<TableInfo>> = vec![None; TABLE_COUNT];
        let mut cursor = at;
        for t in 0..TABLE_COUNT {
            if valid & (1u64 << t) == 0 {
                continue;
            }
            let cols = SCHEMA[t];
            if cols.is_empty() {
                // A `Valid` bit for a table the spec does not define. Everything after it is
                // unreadable because its row size is unknowable, so stop rather than guess.
                return Err(BinaryError::Parse(format!(
                    "metadata declares unknown table 0x{t:02X}"
                )));
            }
            let mut col_offsets = Vec::with_capacity(cols.len());
            let mut col_sizes = Vec::with_capacity(cols.len());
            let mut width = 0usize;
            for c in cols {
                col_offsets.push(width);
                let size = col_size(c);
                col_sizes.push(size);
                width += size;
            }
            infos[t] = Some(TableInfo {
                rows: rows[t],
                row_size: width,
                start: cursor,
                col_offsets,
                col_sizes,
            });
            cursor += width * rows[t] as usize;
        }
        Ok(infos)
    }

    /// Number of rows in a table (0 when absent).
    pub fn rows(&self, table: usize) -> u32 {
        self.tables
            .get(table)
            .and_then(|t| t.as_ref())
            .map_or(0, |t| t.rows)
    }

    /// Read one column of one row. `row` is **1-based**, matching metadata token conventions.
    ///
    /// Returns `None` for an absent table, an out-of-range row, an out-of-range column, or a
    /// truncated stream — every one of which a corrupt file can produce, and none of which may
    /// panic in a compiler pass.
    pub fn cell(&self, table: usize, row: u32, col: usize) -> Option<u32> {
        let info = self.tables.get(table)?.as_ref()?;
        if row == 0 || row > info.rows {
            return None;
        }
        let at = info.start + (row as usize - 1) * info.row_size + *info.col_offsets.get(col)?;
        match info.col_sizes.get(col)? {
            1 => u8_at(self.stream, at).map(u32::from),
            2 => u16_at(self.stream, at).map(u32::from),
            4 => u32_at(self.stream, at),
            _ => None,
        }
    }

    /// Decode a coded index cell into `(table, row)`, or `None` for a reserved tag or an empty
    /// reference.
    pub fn decode_coded(&self, coded: Coded, value: u32) -> Option<(usize, u32)> {
        let tag = (value & ((1 << coded.bits) - 1)) as usize;
        let row = value >> coded.bits;
        let table = *coded.tables.get(tag)?;
        if table == usize::MAX || row == 0 {
            return None;
        }
        Some((table, row))
    }

    /// A `#Strings` cell, resolved.
    pub fn string_cell(&self, table: usize, row: u32, col: usize) -> Option<String> {
        self.heaps.string(self.cell(table, row, col)?)
    }

    /// A `#Blob` cell, resolved.
    pub fn blob_cell(&self, table: usize, row: u32, col: usize) -> Option<&'a [u8]> {
        self.heaps.blob(self.cell(table, row, col)?)
    }

    /// The half-open row range a "list" column denotes.
    ///
    /// Metadata encodes one-to-many relationships (a type's fields, a type's methods) as a
    /// start index per row, with the end implied by the *next* row's start. The last row runs to
    /// the end of the target table. Getting the "next row" wrong is why this is one shared
    /// helper rather than repeated inline.
    pub fn list_range(&self, table: usize, row: u32, col: usize, target: usize) -> (u32, u32) {
        let start = self.cell(table, row, col).unwrap_or(1);
        let end = match self.cell(table, row + 1, col) {
            Some(next) => next,
            None => self.rows(target) + 1,
        };
        (start, end.max(start))
    }
}

pub use self::coded_exports::*;

/// Re-exported coded-index descriptors the type reader needs to decode cells it reads.
mod coded_exports {
    use super::*;
    pub const C_TYPE_DEF_OR_REF: Coded = TYPE_DEF_OR_REF;
    pub const C_MEMBER_REF_PARENT: Coded = MEMBER_REF_PARENT;
    pub const C_RESOLUTION_SCOPE: Coded = RESOLUTION_SCOPE;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compressed_integers_decode_at_all_three_widths() {
        assert_eq!(compressed_u32(&[0x03], 0), Some((0x03, 1)));
        assert_eq!(compressed_u32(&[0x7F], 0), Some((0x7F, 1)));
        // 2-byte form: 0x80 | (value >> 8), value & 0xFF
        assert_eq!(compressed_u32(&[0x80, 0x80], 0), Some((0x80, 2)));
        assert_eq!(compressed_u32(&[0xBF, 0xFF], 0), Some((0x3FFF, 2)));
        // 4-byte form
        assert_eq!(
            compressed_u32(&[0xC0, 0x00, 0x40, 0x00], 0),
            Some((0x4000, 4))
        );
        assert_eq!(
            compressed_u32(&[0xDF, 0xFF, 0xFF, 0xFF], 0),
            Some((0x1FFF_FFFF, 4))
        );
    }

    #[test]
    fn a_reserved_or_truncated_compressed_integer_is_none_not_a_panic() {
        assert_eq!(compressed_u32(&[0xF0], 0), None);
        assert_eq!(compressed_u32(&[0x80], 0), None, "truncated 2-byte form");
        assert_eq!(compressed_u32(&[0xC0, 0x00], 0), None, "truncated 4-byte");
        assert_eq!(compressed_u32(&[], 0), None);
    }

    #[test]
    fn every_read_past_the_end_returns_none() {
        assert_eq!(u16_at(&[0x01], 0), None);
        assert_eq!(u32_at(&[0x01, 0x02, 0x03], 0), None);
        assert_eq!(u64_at(&[0u8; 7], 0), None);
        assert_eq!(u8_at(&[], 0), None);
    }

    #[test]
    fn strings_are_read_up_to_their_null_terminator() {
        let heaps = Heaps {
            strings: b"\0Invoice\0Customer\0",
            ..Default::default()
        };
        assert_eq!(heaps.string(1).as_deref(), Some("Invoice"));
        assert_eq!(heaps.string(9).as_deref(), Some("Customer"));
        assert_eq!(heaps.string(0).as_deref(), Some(""));
        assert_eq!(heaps.string(999), None);
    }

    /// An unterminated heap must yield what is there, not read off the end.
    #[test]
    fn an_unterminated_string_heap_does_not_overrun() {
        let heaps = Heaps {
            strings: b"\0Invoice",
            ..Default::default()
        };
        assert_eq!(heaps.string(1).as_deref(), Some("Invoice"));
    }

    #[test]
    fn user_strings_decode_from_utf16_with_their_trailing_flag_byte() {
        // "Hi" as UTF-16LE is 4 bytes; the stored length includes the 1-byte flag, so 5.
        let us = [0x05, b'H', 0x00, b'i', 0x00, 0x00];
        let heaps = Heaps {
            user_strings: &us,
            ..Default::default()
        };
        assert_eq!(heaps.user_string(0).as_deref(), Some("Hi"));
    }

    #[test]
    fn an_empty_user_string_is_empty_not_none() {
        let heaps = Heaps {
            user_strings: &[0x00],
            ..Default::default()
        };
        assert_eq!(heaps.user_string(0).as_deref(), Some(""));
    }

    #[test]
    fn a_truncated_user_string_is_none() {
        let heaps = Heaps {
            user_strings: &[0x09, b'H', 0x00],
            ..Default::default()
        };
        assert_eq!(heaps.user_string(0), None);
    }

    #[test]
    fn blobs_are_length_prefixed() {
        let blobs = [0x03, 0xAA, 0xBB, 0xCC, 0x01, 0xFF];
        let heaps = Heaps {
            blobs: &blobs,
            ..Default::default()
        };
        assert_eq!(heaps.blob(0), Some(&[0xAA, 0xBB, 0xCC][..]));
        assert_eq!(heaps.blob(4), Some(&[0xFF][..]));
        assert_eq!(heaps.blob(200), None);
    }

    #[test]
    fn a_blob_claiming_more_bytes_than_exist_is_none() {
        let blobs = [0x7F, 0xAA];
        let heaps = Heaps {
            blobs: &blobs,
            ..Default::default()
        };
        assert_eq!(heaps.blob(0), None);
    }

    /// The tag occupies the low bits and the row index the rest. A reserved tag must decode to
    /// nothing rather than to whichever table sits at that position in a compacted list.
    #[test]
    fn coded_indices_split_tag_from_row() {
        let md = Metadata {
            version: String::new(),
            heaps: Heaps::default(),
            stream: &[],
            tables: vec![None; TABLE_COUNT],
        };
        // TypeDefOrRef, 2 tag bits: value 0b101_00 = row 5, tag 0 → TypeDef.
        assert_eq!(
            md.decode_coded(C_TYPE_DEF_OR_REF, 5 << 2),
            Some((TYPE_DEF, 5))
        );
        assert_eq!(
            md.decode_coded(C_TYPE_DEF_OR_REF, (7 << 2) | 1),
            Some((TYPE_REF, 7))
        );
        // Row 0 is the null reference.
        assert_eq!(md.decode_coded(C_TYPE_DEF_OR_REF, 0), None);
        // CustomAttributeType tags 0, 1 and 4 are reserved.
        assert_eq!(md.decode_coded(CUSTOM_ATTRIBUTE_TYPE, 3 << 3), None);
        assert_eq!(
            md.decode_coded(CUSTOM_ATTRIBUTE_TYPE, (3 << 3) | 2),
            Some((METHOD_DEF, 3))
        );
    }

    #[test]
    fn a_non_bsjb_metadata_root_is_rejected() {
        assert!(Metadata::parse(b"not metadata at all").is_err());
        assert!(Metadata::parse(&[]).is_err());
    }

    /// Every table the spec defines must have a schema row, or a `Valid` bit for it aborts the
    /// file. This guards the one mistake that is invisible at runtime: a missing entry shifts
    /// every later table's offset.
    #[test]
    fn every_spec_defined_table_id_has_a_schema() {
        for (t, cols) in SCHEMA.iter().enumerate().take(0x2D) {
            assert!(
                !cols.is_empty(),
                "table 0x{t:02X} has no schema row; every table below the ones actually read \
                 must still have its row size computable"
            );
        }
    }

    #[test]
    fn undefined_table_ids_have_no_schema() {
        for (t, cols) in SCHEMA.iter().enumerate().skip(0x2D) {
            assert!(cols.is_empty(), "table 0x{t:02X} should be undefined");
        }
    }

    /// Coded-index tag widths must be wide enough to address every table in their set. A
    /// too-narrow tag silently misroutes references.
    #[test]
    fn coded_index_tag_widths_cover_their_table_sets() {
        for (name, c) in [
            ("TypeDefOrRef", TYPE_DEF_OR_REF),
            ("HasConstant", HAS_CONSTANT),
            ("HasCustomAttribute", HAS_CUSTOM_ATTRIBUTE),
            ("HasFieldMarshal", HAS_FIELD_MARSHAL),
            ("HasDeclSecurity", HAS_DECL_SECURITY),
            ("MemberRefParent", MEMBER_REF_PARENT),
            ("HasSemantics", HAS_SEMANTICS),
            ("MethodDefOrRef", METHOD_DEF_OR_REF),
            ("MemberForwarded", MEMBER_FORWARDED),
            ("Implementation", IMPLEMENTATION),
            ("CustomAttributeType", CUSTOM_ATTRIBUTE_TYPE),
            ("ResolutionScope", RESOLUTION_SCOPE),
            ("TypeOrMethodDef", TYPE_OR_METHOD_DEF),
        ] {
            assert!(
                c.tables.len() <= (1usize << c.bits),
                "{name}: {} tables do not fit in {} tag bits",
                c.tables.len(),
                c.bits
            );
        }
    }
}
