//! CIL signature blobs (ECMA-335 II.23.2) into readable type names.
//!
//! Signatures are a compact recursive encoding: a calling convention, counts, then `ELEMENT_TYPE`
//! codes that nest. This module decodes the shapes real business code uses and renders anything
//! it cannot into `"?"` rather than failing — a method whose parameter types did not decode is
//! still a method worth recovering, and losing it over an exotic generic constraint would be a
//! worse outcome than an honest `?`.

use super::metadata::compressed_u32;

// ELEMENT_TYPE codes (ECMA-335 II.23.1.16).
const VOID: u8 = 0x01;
const BOOLEAN: u8 = 0x02;
const CHAR: u8 = 0x03;
const I1: u8 = 0x04;
const U1: u8 = 0x05;
const I2: u8 = 0x06;
const U2: u8 = 0x07;
const I4: u8 = 0x08;
const U4: u8 = 0x09;
const I8: u8 = 0x0A;
const U8: u8 = 0x0B;
const R4: u8 = 0x0C;
const R8: u8 = 0x0D;
const STRING: u8 = 0x0E;
const PTR: u8 = 0x0F;
const BYREF: u8 = 0x10;
const VALUETYPE: u8 = 0x11;
const CLASS: u8 = 0x12;
const VAR: u8 = 0x13;
const ARRAY: u8 = 0x14;
const GENERICINST: u8 = 0x15;
const TYPEDBYREF: u8 = 0x16;
const I: u8 = 0x18;
const U: u8 = 0x19;
const FNPTR: u8 = 0x1B;
const OBJECT: u8 = 0x1C;
const SZARRAY: u8 = 0x1D;
const MVAR: u8 = 0x1E;
const CMOD_REQD: u8 = 0x1F;
const CMOD_OPT: u8 = 0x20;
const PINNED: u8 = 0x45;
const SENTINEL: u8 = 0x41;

/// Calling-convention flag: the signature is preceded by a generic parameter count.
const CALLCONV_GENERIC: u8 = 0x10;

/// Resolves a signature-embedded `TypeDefOrRef` coded index to a type name.
///
/// A closure rather than a trait because the only implementation is the one in
/// [`super::mod`], which closes over the metadata tables.
pub type TypeResolver<'r> = dyn Fn(u32) -> String + 'r;

/// A decoded method signature.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MethodSig {
    pub params: Vec<String>,
    pub return_type: String,
    pub has_this: bool,
    pub generic_params: u32,
}

impl MethodSig {
    /// `(System.String, System.Int32) -> System.Boolean`
    pub fn render(&self) -> String {
        format!("({}) -> {}", self.params.join(", "), self.return_type)
    }
}

/// Decode a `MethodDefSig`/`MethodRefSig` blob.
///
/// Returns `None` only for a blob too short to carry a calling convention; anything else
/// degrades to `"?"` types, because a partially-understood signature still tells a reader the
/// arity and the shape.
pub fn method_signature(blob: &[u8], resolve: &TypeResolver<'_>) -> Option<MethodSig> {
    let mut at = 0usize;
    let conv = *blob.first()?;
    at += 1;

    let mut sig = MethodSig {
        has_this: conv & 0x20 != 0,
        ..Default::default()
    };
    if conv & CALLCONV_GENERIC != 0 {
        let (n, used) = compressed_u32(blob, at)?;
        sig.generic_params = n;
        at += used;
    }
    let (param_count, used) = compressed_u32(blob, at)?;
    at += used;

    sig.return_type = read_type(blob, &mut at, resolve);
    // A corrupt count must not drive a huge allocation; no real signature has thousands of
    // parameters, and the blob length bounds it anyway.
    let param_count = param_count.min(blob.len() as u32) as usize;
    for _ in 0..param_count {
        if at >= blob.len() {
            break;
        }
        sig.params.push(read_type(blob, &mut at, resolve));
    }
    Some(sig)
}

/// Decode a `FieldSig` blob — a `0x06` tag followed by one type.
pub fn field_signature(blob: &[u8], resolve: &TypeResolver<'_>) -> String {
    let mut at = 0usize;
    if blob.first() == Some(&0x06) {
        at = 1;
    }
    read_type(blob, &mut at, resolve)
}

/// Read one type, advancing `at`. Never panics and never loops forever: every arm either
/// consumes at least one byte or returns.
fn read_type(blob: &[u8], at: &mut usize, resolve: &TypeResolver<'_>) -> String {
    let Some(&code) = blob.get(*at) else {
        return "?".into();
    };
    *at += 1;
    match code {
        VOID => "System.Void".into(),
        BOOLEAN => "System.Boolean".into(),
        CHAR => "System.Char".into(),
        I1 => "System.SByte".into(),
        U1 => "System.Byte".into(),
        I2 => "System.Int16".into(),
        U2 => "System.UInt16".into(),
        I4 => "System.Int32".into(),
        U4 => "System.UInt32".into(),
        I8 => "System.Int64".into(),
        U8 => "System.UInt64".into(),
        R4 => "System.Single".into(),
        R8 => "System.Double".into(),
        STRING => "System.String".into(),
        OBJECT => "System.Object".into(),
        I => "System.IntPtr".into(),
        U => "System.UIntPtr".into(),
        TYPEDBYREF => "System.TypedReference".into(),
        CLASS | VALUETYPE => match compressed_u32(blob, *at) {
            Some((coded, used)) => {
                *at += used;
                resolve(coded)
            }
            None => "?".into(),
        },
        SZARRAY => format!("{}[]", read_type(blob, at, resolve)),
        PTR => format!("{}*", read_type(blob, at, resolve)),
        BYREF => format!("ref {}", read_type(blob, at, resolve)),
        ARRAY => {
            // Type, rank, then bounds/lo-bound arrays that carry no naming information.
            let inner = read_type(blob, at, resolve);
            let rank = match compressed_u32(blob, *at) {
                Some((r, used)) => {
                    *at += used;
                    r
                }
                None => return format!("{inner}[?]"),
            };
            skip_counted_list(blob, at);
            skip_counted_list(blob, at);
            format!("{inner}[{}]", ",".repeat(rank.saturating_sub(1) as usize))
        }
        GENERICINST => {
            // The `CLASS`/`VALUETYPE` tag of the open type, then the type itself, then the
            // argument count and the arguments.
            if matches!(blob.get(*at), Some(&CLASS) | Some(&VALUETYPE)) {
                *at += 1;
            }
            let base = match compressed_u32(blob, *at) {
                Some((coded, used)) => {
                    *at += used;
                    resolve(coded)
                }
                None => "?".into(),
            };
            let argc = match compressed_u32(blob, *at) {
                Some((n, used)) => {
                    *at += used;
                    n.min(64)
                }
                None => 0,
            };
            let args: Vec<String> = (0..argc).map(|_| read_type(blob, at, resolve)).collect();
            if args.is_empty() {
                base
            } else {
                format!("{base}<{}>", args.join(", "))
            }
        }
        // Generic parameters keep their position rather than an invented name: `!0` and `!!0`
        // are how they are written everywhere in the .NET tooling ecosystem.
        VAR => match compressed_u32(blob, *at) {
            Some((n, used)) => {
                *at += used;
                format!("!{n}")
            }
            None => "!?".into(),
        },
        MVAR => match compressed_u32(blob, *at) {
            Some((n, used)) => {
                *at += used;
                format!("!!{n}")
            }
            None => "!!?".into(),
        },
        // Custom modifiers and `pinned` decorate the type that follows; they carry no
        // information a business reader needs, so they are stepped over.
        CMOD_REQD | CMOD_OPT => {
            if let Some((_, used)) = compressed_u32(blob, *at) {
                *at += used;
            }
            read_type(blob, at, resolve)
        }
        PINNED => read_type(blob, at, resolve),
        // A vararg sentinel separates fixed from variable parameters.
        SENTINEL => read_type(blob, at, resolve),
        FNPTR => {
            // A function-pointer signature nests a whole method signature. Its shape adds
            // nothing to a business reading, so it is named and skipped rather than expanded.
            *at = blob.len();
            "method*".into()
        }
        _ => "?".into(),
    }
}

/// Step over a compressed-count-prefixed list of compressed integers (array bounds).
fn skip_counted_list(blob: &[u8], at: &mut usize) {
    let Some((n, used)) = compressed_u32(blob, *at) else {
        return;
    };
    *at += used;
    for _ in 0..n.min(64) {
        match compressed_u32(blob, *at) {
            Some((_, u)) => *at += u,
            None => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_types(_: u32) -> String {
        "Acme.Thing".into()
    }

    fn sig(blob: &[u8]) -> MethodSig {
        method_signature(blob, &no_types).expect("signature must decode")
    }

    #[test]
    fn a_parameterless_void_instance_method_decodes() {
        // HASTHIS, 0 params, void
        let s = sig(&[0x20, 0x00, VOID]);
        assert!(s.has_this);
        assert!(s.params.is_empty());
        assert_eq!(s.return_type, "System.Void");
        assert_eq!(s.render(), "() -> System.Void");
    }

    #[test]
    fn primitive_parameters_and_return_types_decode() {
        // DEFAULT, 2 params, returns bool, takes (string, int32)
        let s = sig(&[0x00, 0x02, BOOLEAN, STRING, I4]);
        assert_eq!(s.return_type, "System.Boolean");
        assert_eq!(s.params, vec!["System.String", "System.Int32"]);
        assert_eq!(
            s.render(),
            "(System.String, System.Int32) -> System.Boolean"
        );
    }

    #[test]
    fn a_static_method_has_no_this() {
        assert!(!sig(&[0x00, 0x00, VOID]).has_this);
    }

    #[test]
    fn arrays_pointers_and_byrefs_render_as_decorations() {
        let s = sig(&[0x00, 0x03, VOID, SZARRAY, I4, BYREF, STRING, PTR, U1]);
        assert_eq!(
            s.params,
            vec!["System.Int32[]", "ref System.String", "System.Byte*"]
        );
    }

    #[test]
    fn a_class_parameter_is_resolved_through_the_caller() {
        let s = sig(&[0x00, 0x01, VOID, CLASS, 0x08]);
        assert_eq!(s.params, vec!["Acme.Thing"]);
    }

    #[test]
    fn a_generic_instantiation_renders_its_arguments() {
        // List<string>: GENERICINST CLASS <tok> 1 STRING
        let s = sig(&[0x00, 0x01, VOID, GENERICINST, CLASS, 0x08, 0x01, STRING]);
        assert_eq!(s.params, vec!["Acme.Thing<System.String>"]);
    }

    #[test]
    fn generic_parameters_keep_their_position() {
        let s = sig(&[0x10, 0x01, 0x01, MVAR, 0x00, VAR, 0x01]);
        assert_eq!(s.generic_params, 1);
        assert_eq!(s.return_type, "!!0");
        assert_eq!(s.params, vec!["!1"]);
    }

    #[test]
    fn custom_modifiers_are_stepped_over_not_rendered() {
        let s = sig(&[0x00, 0x01, VOID, CMOD_OPT, 0x08, I4]);
        assert_eq!(s.params, vec!["System.Int32"]);
    }

    #[test]
    fn a_multidimensional_array_shows_its_rank() {
        // int[,] : ARRAY I4 rank=2, 0 sizes, 0 lobounds
        let s = sig(&[0x00, 0x01, VOID, ARRAY, I4, 0x02, 0x00, 0x00]);
        assert_eq!(s.params, vec!["System.Int32[,]"]);
    }

    #[test]
    fn field_signatures_decode_with_and_without_their_tag() {
        assert_eq!(field_signature(&[0x06, STRING], &no_types), "System.String");
        assert_eq!(field_signature(&[I4], &no_types), "System.Int32");
    }

    /// An unreadable signature must cost the types, never the method.
    #[test]
    fn a_truncated_signature_degrades_to_question_marks() {
        let s = sig(&[0x00, 0x02, BOOLEAN, STRING]);
        assert_eq!(s.return_type, "System.Boolean");
        assert_eq!(
            s.params,
            vec!["System.String"],
            "the missing one is dropped"
        );

        let s = sig(&[0x00, 0x01, 0x99]);
        assert_eq!(s.return_type, "?");
    }

    #[test]
    fn an_empty_blob_is_none_rather_than_a_panic() {
        assert!(method_signature(&[], &no_types).is_none());
    }

    /// A corrupt parameter count must not drive an allocation or a long loop.
    #[test]
    fn an_absurd_parameter_count_is_bounded_by_the_blob_length() {
        let s = sig(&[0x00, 0xC0, 0x00, 0xFF, 0xFF, VOID]);
        assert!(s.params.len() < 16, "got {} params", s.params.len());
    }

    /// Decoding is a pure function of the blob, so it cannot drift between runs.
    #[test]
    fn decoding_is_deterministic() {
        let blob = [0x00, 0x02, BOOLEAN, STRING, I4];
        assert_eq!(sig(&blob), sig(&blob));
    }
}
