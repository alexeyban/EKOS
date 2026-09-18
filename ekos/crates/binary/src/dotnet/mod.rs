//! .NET backend (RFC 0148) — managed PE assemblies into [`DecompiledAst`].
//!
//! Hand-written rather than delegated, on evidence: `dotnetdll` is GPL-3.0+ and cannot be linked
//! into this MIT workspace at all, and `dotscope` 0.9.1 — Apache-2.0 and otherwise suitable —
//! **returned nothing for 70 of 147 real Mono assemblies**, aborting the entire file inside its
//! custom-attribute loader. That all-or-nothing failure is the exact shape that silently cost
//! LedgerSMB its whole schema under RFC 0146.
//!
//! So this reader never fails a file for a bad row. Custom attributes are not read at all (v1
//! needs nothing from them), every offset is bounds-checked, and an unreadable signature or body
//! costs that one member and leaves an [`AstDiagnostic`] behind.

mod il;
mod metadata;
mod pe;
mod sig;

use crate::ast::*;
use crate::io_classify;
use crate::{BinaryError, Result, limits};
use metadata::{
    ASSEMBLY, ASSEMBLY_REF, C_MEMBER_REF_PARENT, C_TYPE_DEF_OR_REF, FIELD, INTERFACE_IMPL,
    MEMBER_REF, METHOD_DEF, Metadata, NESTED_CLASS, TYPE_DEF, TYPE_REF, TYPE_SPEC,
};
use std::collections::HashMap;

/// Recorded as `BinarySource::extractor` on every fact derived from this backend.
pub const EXTRACTOR: &str = "ekos-cil-metadata/v1";

// TypeDef flags (ECMA-335 II.23.1.15).
const TYPE_VISIBILITY_MASK: u32 = 0x0000_0007;
const TYPE_INTERFACE: u32 = 0x0000_0020;
const TYPE_ABSTRACT: u32 = 0x0000_0080;

// FieldAttributes / MethodAttributes access masks (II.23.1.5, II.23.1.10).
const MEMBER_ACCESS_MASK: u32 = 0x0007;
const FIELD_STATIC: u32 = 0x0010;
const FIELD_INIT_ONLY: u32 = 0x0020;
const FIELD_LITERAL: u32 = 0x0040;
const METHOD_STATIC: u32 = 0x0010;
const METHOD_ABSTRACT: u32 = 0x0400;

/// Read a managed assembly.
pub fn read_assembly(bytes: &[u8], path: &str, sha256: &str) -> Result<DecompiledAst> {
    if bytes.len() > limits::MAX_FILE_BYTES {
        return Err(BinaryError::TooLarge {
            path: path.into(),
            size: bytes.len(),
            limit: limits::MAX_FILE_BYTES,
        });
    }
    let image = pe::PeImage::parse(bytes)?;
    let md = Metadata::parse(image.metadata()?)?;
    let mut diagnostics = Vec::new();

    let names = TypeNames::build(&md);
    let nesting = nesting_map(&md);

    let assembly_name = md
        .string_cell(ASSEMBLY, 1, 7)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| path.rsplit('/').next().unwrap_or(path).to_string());

    let references = (1..=md.rows(ASSEMBLY_REF))
        .filter_map(|r| md.string_cell(ASSEMBLY_REF, r, 6))
        .filter(|s| !s.is_empty())
        .collect();

    let interfaces = interface_map(&md, &names);

    let mut types = Vec::with_capacity(md.rows(TYPE_DEF) as usize);
    for row in 1..=md.rows(TYPE_DEF) {
        match read_type(
            &md,
            &image,
            &names,
            &nesting,
            &interfaces,
            row,
            &mut diagnostics,
        ) {
            Some(t) => types.push(t),
            None => diagnostics.push(
                AstDiagnostic::new(
                    "BIN_TYPEDEF_UNREADABLE",
                    "TypeDef row could not be read; skipped",
                )
                .at(format!("0x02{row:06X}")),
            ),
        }
    }

    Ok(DecompiledAst {
        source: BinarySource {
            kind: BinaryKind::DotNet,
            path: path.to_string(),
            container_entry: None,
            sha256: sha256.to_string(),
            format_version: md.version.clone(),
            extractor: EXTRACTOR.into(),
        },
        fidelity: Fidelity::Structural,
        assembly_name,
        references,
        types,
        diagnostics,
    })
}

// ────────────────────────────────────────────────────────────────────────────
// Name resolution
// ────────────────────────────────────────────────────────────────────────────

/// Fully-qualified names for every `TypeDef` and `TypeRef`, resolved once.
///
/// Built up front rather than on demand because signature decoding resolves type references
/// constantly, and each resolution would otherwise re-read two string-heap cells.
struct TypeNames {
    defs: Vec<String>,
    refs: Vec<String>,
}

impl TypeNames {
    fn build(md: &Metadata<'_>) -> Self {
        let qualify = |ns: Option<String>, name: Option<String>| -> String {
            let name = name.unwrap_or_default();
            match ns {
                Some(ns) if !ns.is_empty() => format!("{ns}.{name}"),
                _ => name,
            }
        };
        Self {
            defs: (1..=md.rows(TYPE_DEF))
                .map(|r| {
                    qualify(
                        md.string_cell(TYPE_DEF, r, 2),
                        md.string_cell(TYPE_DEF, r, 1),
                    )
                })
                .collect(),
            refs: (1..=md.rows(TYPE_REF))
                .map(|r| {
                    qualify(
                        md.string_cell(TYPE_REF, r, 2),
                        md.string_cell(TYPE_REF, r, 1),
                    )
                })
                .collect(),
        }
    }

    /// Resolve a `TypeDefOrRef` coded index to a name.
    ///
    /// A `TypeSpec` is a signature blob describing a constructed type (`List<Invoice>`). Fully
    /// expanding it needs the recursive decoder, which would recurse back through here — so it
    /// is named by its kind instead. The concrete element types that matter are already visible
    /// on the members that use them.
    fn resolve(&self, md: &Metadata<'_>, coded: u32) -> String {
        match md.decode_coded(C_TYPE_DEF_OR_REF, coded) {
            Some((TYPE_DEF, row)) => self
                .defs
                .get(row as usize - 1)
                .cloned()
                .unwrap_or_else(|| "?".into()),
            Some((TYPE_REF, row)) => self
                .refs
                .get(row as usize - 1)
                .cloned()
                .unwrap_or_else(|| "?".into()),
            Some((TYPE_SPEC, _)) => "constructed type".into(),
            _ => "?".into(),
        }
    }

    /// Resolve a `MemberRefParent` coded index — the owner of a called method or accessed field.
    fn resolve_member_parent(&self, md: &Metadata<'_>, coded: u32) -> String {
        match md.decode_coded(C_MEMBER_REF_PARENT, coded) {
            Some((TYPE_DEF, row)) => self
                .defs
                .get(row as usize - 1)
                .cloned()
                .unwrap_or_else(|| "?".into()),
            Some((TYPE_REF, row)) => self
                .refs
                .get(row as usize - 1)
                .cloned()
                .unwrap_or_else(|| "?".into()),
            Some((TYPE_SPEC, _)) => "constructed type".into(),
            _ => "?".into(),
        }
    }
}

/// `nested TypeDef row → enclosing TypeDef row`.
fn nesting_map(md: &Metadata<'_>) -> HashMap<u32, u32> {
    (1..=md.rows(NESTED_CLASS))
        .filter_map(|r| Some((md.cell(NESTED_CLASS, r, 0)?, md.cell(NESTED_CLASS, r, 1)?)))
        .collect()
}

/// `TypeDef row → implemented interface names`.
fn interface_map(md: &Metadata<'_>, names: &TypeNames) -> HashMap<u32, Vec<String>> {
    let mut out: HashMap<u32, Vec<String>> = HashMap::new();
    for r in 1..=md.rows(INTERFACE_IMPL) {
        let (Some(class), Some(coded)) =
            (md.cell(INTERFACE_IMPL, r, 0), md.cell(INTERFACE_IMPL, r, 1))
        else {
            continue;
        };
        out.entry(class).or_default().push(names.resolve(md, coded));
    }
    out
}

// ────────────────────────────────────────────────────────────────────────────
// Types and members
// ────────────────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn read_type(
    md: &Metadata<'_>,
    image: &pe::PeImage<'_>,
    names: &TypeNames,
    nesting: &HashMap<u32, u32>,
    interfaces: &HashMap<u32, Vec<String>>,
    row: u32,
    diagnostics: &mut Vec<AstDiagnostic>,
) -> Option<DecompiledType> {
    let flags = md.cell(TYPE_DEF, row, 0)?;
    let name = md.string_cell(TYPE_DEF, row, 1)?;
    let mut namespace = md.string_cell(TYPE_DEF, row, 2).unwrap_or_default();
    let locator = format!("0x02{row:06X}");

    // A nested type's own `Namespace` column is empty — the namespace belongs to the enclosing
    // type. Reading the column alone would put every nested type in the global namespace, which
    // is how a `Foo.Bar+Inner` becomes an orphan in every grouped view.
    let mut display_name = name.clone();
    if let Some(&outer_row) = nesting.get(&row)
        && let Some(outer) = names.defs.get(outer_row as usize - 1)
    {
        match outer.rsplit_once('.') {
            Some((ns, outer_name)) => {
                namespace = ns.to_string();
                display_name = format!("{outer_name}+{name}");
            }
            None => display_name = format!("{outer}+{name}"),
        }
    }

    let extends = md
        .cell(TYPE_DEF, row, 3)
        .map(|c| names.resolve(md, c))
        .filter(|s| s != "?");

    let category = if flags & TYPE_INTERFACE != 0 {
        TypeCategory::Interface
    } else {
        match extends.as_deref() {
            Some("System.Enum") => TypeCategory::Enum,
            Some("System.ValueType") => TypeCategory::Struct,
            _ => TypeCategory::Class,
        }
    };

    // `System.Object` is the implicit root of every class, and `System.ValueType`/`System.Enum`
    // are already carried by `category` — re-emitting them would add one edge per type to a
    // single node and say nothing.
    let super_type = extends.filter(|s| {
        !matches!(
            s.as_str(),
            "System.Object" | "System.ValueType" | "System.Enum"
        )
    });

    let (field_start, field_end) = md.list_range(TYPE_DEF, row, 4, FIELD);
    let (method_start, method_end) = md.list_range(TYPE_DEF, row, 5, METHOD_DEF);

    let fields = (field_start..field_end)
        .filter_map(|f| read_field(md, names, f))
        .collect();
    let methods = (method_start..method_end)
        .filter_map(|m| read_method(md, image, names, m, diagnostics))
        .collect();

    Some(DecompiledType {
        namespace,
        compiler_generated: is_generated_clr_name(&name),
        name: display_name,
        locator,
        category,
        visibility: type_visibility(flags),
        is_abstract: flags & TYPE_ABSTRACT != 0,
        super_type,
        interfaces: interfaces.get(&row).cloned().unwrap_or_default(),
        fields,
        methods,
    })
}

fn read_field(md: &Metadata<'_>, names: &TypeNames, row: u32) -> Option<DecompiledField> {
    let flags = md.cell(FIELD, row, 0)?;
    let name = md.string_cell(FIELD, row, 1)?;
    let resolve = |coded: u32| names.resolve(md, coded);
    let type_name = md
        .blob_cell(FIELD, row, 2)
        .map(|b| sig::field_signature(b, &resolve))
        .unwrap_or_else(|| "?".into());
    Some(DecompiledField {
        compiler_generated: is_generated_clr_name(&name),
        name,
        locator: format!("0x04{row:06X}"),
        type_name,
        visibility: member_visibility(flags),
        is_static: flags & FIELD_STATIC != 0,
        // `const` implies `readonly` for a reader's purposes: both mean "this value does not
        // change after construction", which is the only distinction a recovered fact draws.
        is_final: flags & (FIELD_INIT_ONLY | FIELD_LITERAL) != 0,
    })
}

fn read_method(
    md: &Metadata<'_>,
    image: &pe::PeImage<'_>,
    names: &TypeNames,
    row: u32,
    diagnostics: &mut Vec<AstDiagnostic>,
) -> Option<DecompiledMethod> {
    let rva = md.cell(METHOD_DEF, row, 0)?;
    let flags = md.cell(METHOD_DEF, row, 2)?;
    let name = md.string_cell(METHOD_DEF, row, 3)?;
    let locator = format!("0x06{row:06X}");

    let resolve = |coded: u32| names.resolve(md, coded);
    let blob = md.blob_cell(METHOD_DEF, row, 4);
    let parsed = blob.and_then(|b| sig::method_signature(b, &resolve));
    let (signature, descriptor) = match &parsed {
        Some(s) => (s.render(), render_descriptor(s)),
        None => {
            diagnostics.push(
                AstDiagnostic::new(
                    "BIN_SIG_UNREADABLE",
                    "method signature blob could not be decoded",
                )
                .at(&locator),
            );
            ("(?) -> ?".to_string(), "?".to_string())
        }
    };

    // RVA 0 means there is no body here at all: abstract, interface, or `extern`/P-Invoke.
    let body = if rva == 0 {
        None
    } else {
        match image.rva_to_offset(rva) {
            Some(offset) => read_method_body(md, image, names, offset, &locator, diagnostics),
            None => {
                diagnostics.push(
                    AstDiagnostic::new(
                        "BIN_BODY_RVA",
                        format!("method body RVA 0x{rva:08X} is outside every section"),
                    )
                    .at(&locator),
                );
                None
            }
        }
    };

    Some(DecompiledMethod {
        compiler_generated: is_generated_clr_name(&name),
        name,
        locator,
        signature,
        descriptor,
        visibility: member_visibility(flags),
        is_static: flags & METHOD_STATIC != 0,
        is_abstract: flags & METHOD_ABSTRACT != 0,
        body,
    })
}

fn read_method_body(
    md: &Metadata<'_>,
    image: &pe::PeImage<'_>,
    names: &TypeNames,
    offset: usize,
    locator: &str,
    diagnostics: &mut Vec<AstDiagnostic>,
) -> Option<MethodBody> {
    let Some(raw) = il::read_body(image.bytes(), offset) else {
        diagnostics.push(
            AstDiagnostic::new("BIN_BODY_HEADER", "method body header is unreadable").at(locator),
        );
        return None;
    };

    let code = raw.code;
    let mut body = MethodBody {
        code_size: code.len(),
        ..Default::default()
    };

    for ins in il::walk(code) {
        let token = || metadata::u32_at(code, ins.operand_at);
        match (ins.prefix, ins.opcode) {
            // ── calls ───────────────────────────────────────────────────────
            (None, 0x28) => push_call(&mut body, md, names, ins.offset, CallKind::Static, token()),
            (None, 0x6F) => push_call(&mut body, md, names, ins.offset, CallKind::Virtual, token()),
            (None, 0x73) => push_call(
                &mut body,
                md,
                names,
                ins.offset,
                CallKind::Constructor,
                token(),
            ),

            // ── field access ────────────────────────────────────────────────
            (None, 0x7B) => push_field(
                &mut body,
                md,
                names,
                ins.offset,
                AccessMode::Read,
                false,
                token(),
            ),
            (None, 0x7E) => push_field(
                &mut body,
                md,
                names,
                ins.offset,
                AccessMode::Read,
                true,
                token(),
            ),
            (None, 0x7D) => push_field(
                &mut body,
                md,
                names,
                ins.offset,
                AccessMode::Write,
                false,
                token(),
            ),
            (None, 0x80) => push_field(
                &mut body,
                md,
                names,
                ins.offset,
                AccessMode::Write,
                true,
                token(),
            ),

            // ── literals ────────────────────────────────────────────────────
            (None, 0x72) => {
                // `ldstr`'s token addresses the `#US` heap directly rather than a table row.
                if let Some(t) = token() {
                    let (table, index) = il::split_token(t);
                    if table == 0x70
                        && let Some(value) = md.heaps.user_string(index)
                    {
                        body.string_literals.push(StringLiteral {
                            offset: ins.offset,
                            value,
                        });
                    }
                }
            }
            // `ldc.i4.0`-`ldc.i4.8` and `ldc.i4.m1` are excluded for the same reason the JVM
            // backend excludes `iconst_*`: they are loop bounds and booleans, and emitting them
            // would bury the thresholds and rates a business rule actually turns on.
            (None, 0x1F) => push_numeric(
                &mut body,
                ins.offset,
                metadata::u8_at(code, ins.operand_at).map(|v| (v as i8).to_string()),
            ),
            (None, 0x20) => push_numeric(
                &mut body,
                ins.offset,
                metadata::u32_at(code, ins.operand_at).map(|v| (v as i32).to_string()),
            ),
            (None, 0x21) => push_numeric(
                &mut body,
                ins.offset,
                metadata::u64_at(code, ins.operand_at).map(|v| (v as i64).to_string()),
            ),
            (None, 0x22) => push_numeric(
                &mut body,
                ins.offset,
                metadata::u32_at(code, ins.operand_at).map(|v| format!("{:?}", f32::from_bits(v))),
            ),
            (None, 0x23) => push_numeric(
                &mut body,
                ins.offset,
                metadata::u64_at(code, ins.operand_at).map(|v| format!("{:?}", f64::from_bits(v))),
            ),

            // ── branches ────────────────────────────────────────────────────
            (None, 0x45) => body.branches.push(Branch {
                offset: ins.offset,
                opcode: "switch".into(),
                case_count: metadata::u32_at(code, ins.operand_at).unwrap_or(0) as usize,
            }),
            (prefix, opcode) => {
                if let Some(name) = il::conditional_branch_name(prefix, opcode) {
                    body.branches.push(Branch {
                        offset: ins.offset,
                        opcode: name.into(),
                        case_count: 1,
                    });
                }
            }
        }
    }
    Some(body)
}

/// Resolve a method token to `(owner, name, signature)`.
fn resolve_method_token(
    md: &Metadata<'_>,
    names: &TypeNames,
    token: u32,
) -> Option<(String, String, String)> {
    let (table, row) = il::split_token(token);
    let resolve = |coded: u32| names.resolve(md, coded);
    match table as usize {
        MEMBER_REF => {
            let owner = names.resolve_member_parent(md, md.cell(MEMBER_REF, row, 0)?);
            let name = md.string_cell(MEMBER_REF, row, 1)?;
            let sig = md
                .blob_cell(MEMBER_REF, row, 2)
                .and_then(|b| sig::method_signature(b, &resolve))
                .map(|s| s.render())
                .unwrap_or_else(|| "?".into());
            Some((owner, name, sig))
        }
        METHOD_DEF => {
            let name = md.string_cell(METHOD_DEF, row, 3)?;
            let sig = md
                .blob_cell(METHOD_DEF, row, 4)
                .and_then(|b| sig::method_signature(b, &resolve))
                .map(|s| s.render())
                .unwrap_or_else(|| "?".into());
            // A `MethodDef` token names a method in *this* assembly. Its owner is the type whose
            // method list contains it, which needs a reverse scan; the owner is filled in by the
            // caller's own type context downstream, so an empty owner here is honest rather than
            // a guess at the wrong type.
            Some((String::new(), name, sig))
        }
        // MethodSpec (0x2B): a generic method instantiation. Its `Method` column is itself a
        // MethodDefOrRef, but resolving it adds only the type arguments — the callee name and
        // owner are identical — so it is left unresolved rather than half-resolved.
        _ => None,
    }
}

fn push_call(
    body: &mut MethodBody,
    md: &Metadata<'_>,
    names: &TypeNames,
    offset: u32,
    kind: CallKind,
    token: Option<u32>,
) {
    let Some((owner, method, descriptor)) = token.and_then(|t| resolve_method_token(md, names, t))
    else {
        return;
    };
    body.calls.push(CallSite {
        offset,
        kind,
        io: io_classify::classify(&owner),
        owner,
        method,
        descriptor,
    });
}

fn push_field(
    body: &mut MethodBody,
    md: &Metadata<'_>,
    names: &TypeNames,
    offset: u32,
    mode: AccessMode,
    is_static: bool,
    token: Option<u32>,
) {
    let Some(token) = token else { return };
    let (table, row) = il::split_token(token);
    let resolve = |coded: u32| names.resolve(md, coded);
    let (owner, name, type_name) = match table as usize {
        MEMBER_REF => (
            names.resolve_member_parent(md, md.cell(MEMBER_REF, row, 0).unwrap_or(0)),
            md.string_cell(MEMBER_REF, row, 1).unwrap_or_default(),
            md.blob_cell(MEMBER_REF, row, 2)
                .map(|b| sig::field_signature(b, &resolve))
                .unwrap_or_else(|| "?".into()),
        ),
        FIELD => (
            String::new(),
            md.string_cell(FIELD, row, 1).unwrap_or_default(),
            md.blob_cell(FIELD, row, 2)
                .map(|b| sig::field_signature(b, &resolve))
                .unwrap_or_else(|| "?".into()),
        ),
        _ => return,
    };
    if name.is_empty() {
        return;
    }
    body.field_access.push(FieldAccess {
        offset,
        mode,
        owner,
        name,
        type_name,
        is_static,
    });
}

fn push_numeric(body: &mut MethodBody, offset: u32, value: Option<String>) {
    if let Some(value) = value {
        body.numeric_literals.push(NumericLiteral { offset, value });
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Flags and names
// ────────────────────────────────────────────────────────────────────────────

fn type_visibility(flags: u32) -> Visibility {
    match flags & TYPE_VISIBILITY_MASK {
        1 | 2 => Visibility::Public,
        3 => Visibility::Private,
        4 | 7 => Visibility::Protected,
        _ => Visibility::Internal,
    }
}

fn member_visibility(flags: u32) -> Visibility {
    match flags & MEMBER_ACCESS_MASK {
        1 => Visibility::Private,
        4 | 5 => Visibility::Protected,
        6 => Visibility::Public,
        _ => Visibility::Internal,
    }
}

/// Names the C#/VB compilers invent. All of them contain `<` or `>`, which the languages do not
/// permit in a source identifier — that is precisely why the compilers use them, and it makes
/// this test exact rather than heuristic.
fn is_generated_clr_name(name: &str) -> bool {
    name.contains('<') || name.contains('>')
}

/// A stable, readable rendering of a signature, kept alongside the friendly form as the evidence
/// a recovered fact cites.
fn render_descriptor(s: &sig::MethodSig) -> String {
    format!(
        "{}({}){}",
        if s.has_this { "instance " } else { "" },
        s.params.join(","),
        s.return_type
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_visibility_maps_every_defined_value() {
        assert_eq!(type_visibility(0), Visibility::Internal, "NotPublic");
        assert_eq!(type_visibility(1), Visibility::Public);
        assert_eq!(type_visibility(2), Visibility::Public, "NestedPublic");
        assert_eq!(type_visibility(3), Visibility::Private, "NestedPrivate");
        assert_eq!(type_visibility(4), Visibility::Protected, "NestedFamily");
        assert_eq!(type_visibility(5), Visibility::Internal, "NestedAssembly");
        assert_eq!(
            type_visibility(7),
            Visibility::Protected,
            "NestedFamORAssem"
        );
    }

    #[test]
    fn member_visibility_maps_every_defined_value() {
        assert_eq!(member_visibility(0), Visibility::Internal, "PrivateScope");
        assert_eq!(member_visibility(1), Visibility::Private);
        assert_eq!(member_visibility(3), Visibility::Internal, "Assembly");
        assert_eq!(member_visibility(4), Visibility::Protected, "Family");
        assert_eq!(member_visibility(6), Visibility::Public);
    }

    /// C# cannot produce an identifier containing `<` or `>`, which is exactly why the compiler
    /// uses them for closures, iterators and async state machines.
    #[test]
    fn compiler_generated_clr_names_are_recognized() {
        assert!(is_generated_clr_name("<>c__DisplayClass0_0"));
        assert!(is_generated_clr_name("<CalculateAsync>d__3"));
        assert!(is_generated_clr_name("<Module>"));
        assert!(is_generated_clr_name("<Price>k__BackingField"));
    }

    #[test]
    fn ordinary_names_are_not_compiler_generated() {
        assert!(!is_generated_clr_name("InvoiceCalculator"));
        assert!(!is_generated_clr_name("CalculateTotal"));
        assert!(!is_generated_clr_name("_price"));
    }

    #[test]
    fn a_non_managed_input_is_an_error_not_a_panic() {
        assert!(read_assembly(b"not a PE at all", "x.dll", "d").is_err());
        assert!(read_assembly(&[], "x.dll", "d").is_err());
    }

    /// The real corpus. These are Mono-compiled assemblies this repository does not own and did
    /// not write — the ones `dotscope` failed on 48% of. When the directory is absent (CI,
    /// another machine) the test skips rather than fails.
    fn mono_dir() -> Option<std::path::PathBuf> {
        let p = std::path::PathBuf::from(
            "/home/legion/.var/app/net.lutris.Lutris/data/lutris/runners/wine/\
             wine-ge-8-25-x86_64/share/wine/mono/wine-mono-8.1.0/lib/mono/4.5",
        );
        p.is_dir().then_some(p)
    }

    #[test]
    fn reads_every_real_mono_assembly_without_failing_a_file() {
        let Some(dir) = mono_dir() else {
            eprintln!("skipping: no Mono assembly corpus on this machine");
            return;
        };
        let mut managed = 0usize;
        let mut failed: Vec<String> = Vec::new();
        let mut total_types = 0usize;
        let mut total_methods = 0usize;

        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("dll") {
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            if crate::detect(&bytes) != crate::Detected::ManagedPe {
                continue;
            }
            managed += 1;
            match read_assembly(&bytes, path.to_str().unwrap(), "d") {
                Ok(ast) => {
                    total_types += ast.types.len();
                    total_methods += ast.method_count();
                }
                Err(e) => failed.push(format!("{}: {e}", path.display())),
            }
        }

        assert!(managed > 50, "expected a real corpus, found {managed}");
        // The bar that `dotscope` failed: every managed assembly must yield an AST.
        assert!(
            failed.is_empty(),
            "{} of {managed} assemblies failed to read: {:#?}",
            failed.len(),
            failed
        );
        assert!(total_types > 10_000, "only {total_types} types recovered");
        assert!(total_methods > 50_000, "only {total_methods} methods");
        eprintln!("{managed} assemblies → {total_types} types, {total_methods} methods");
    }

    #[test]
    fn recovers_real_names_signatures_and_call_graphs() {
        let Some(dir) = mono_dir() else {
            eprintln!("skipping: no Mono assembly corpus on this machine");
            return;
        };
        // `mscorlib.dll` deliberately, not one of the smaller assemblies beside it: most of
        // wine-mono's `4.5` profile ships as type-forwarding *facades* whose every method body is
        // `ldnull; throw` (see `a_facade_assembly_yields_types_but_no_logic`). mscorlib is the
        // real implementation, so it is the only one that exercises the IL walk at scale.
        let Ok(bytes) = std::fs::read(dir.join("mscorlib.dll")) else {
            eprintln!("skipping: mscorlib.dll not present");
            return;
        };
        let ast = read_assembly(&bytes, "mscorlib.dll", "d").unwrap();

        assert_eq!(ast.assembly_name, "mscorlib");
        assert!(ast.types.len() > 2_000, "only {} types", ast.types.len());
        assert!(
            ast.diagnostics.is_empty(),
            "a real assembly produced diagnostics: {:?}",
            &ast.diagnostics[..ast.diagnostics.len().min(5)]
        );

        let string_type = ast
            .types
            .iter()
            .find(|t| t.qualified_name() == "System.String")
            .expect("System.String must be recovered");
        assert_eq!(string_type.category, TypeCategory::Class);
        assert_eq!(string_type.visibility, Visibility::Public);
        assert!(string_type.methods.len() > 50);
        assert!(
            string_type
                .methods
                .iter()
                .any(|m| m.name == "Concat" && m.is_static && m.signature.contains("->")),
            "a real, correctly-flagged static overload must be recovered"
        );

        // An enum and an interface must be categorized from their metadata, not guessed.
        assert_eq!(
            ast.types
                .iter()
                .find(|t| t.qualified_name() == "System.DayOfWeek")
                .map(|t| t.category),
            Some(TypeCategory::Enum)
        );
        assert_eq!(
            ast.types
                .iter()
                .find(|t| t.qualified_name() == "System.IDisposable")
                .map(|t| t.category),
            Some(TypeCategory::Interface)
        );

        let bodies: Vec<&MethodBody> = ast
            .types
            .iter()
            .flat_map(|t| &t.methods)
            .filter_map(|m| m.body.as_ref())
            .collect();
        let sum = |f: fn(&MethodBody) -> usize| bodies.iter().map(|b| f(b)).sum::<usize>();

        // Measured on wine-mono 8.1.0's mscorlib: 24,526 bodies / 1.38 MB of IL / 79,263 calls /
        // 37,821 branches / 13,500 strings / 46,584 field accesses. The thresholds sit well below
        // those so a different build does not fail the suite, but far above zero so a regression
        // that silently stops decoding bodies cannot pass.
        assert!(bodies.len() > 15_000, "only {} bodies", bodies.len());
        assert!(sum(|b| b.code_size) > 500_000, "IL decoded looks too small");
        assert!(sum(|b| b.calls.len()) > 40_000);
        assert!(sum(|b| b.branches.len()) > 15_000);
        assert!(sum(|b| b.string_literals.len()) > 5_000);
        assert!(sum(|b| b.field_access.len()) > 20_000);

        // Field access must be split by direction, or `Source`/`Sink` mapping downstream is
        // meaningless.
        assert!(
            bodies
                .iter()
                .any(|b| b.field_access.iter().any(|f| f.mode == AccessMode::Write))
                && bodies
                    .iter()
                    .any(|b| b.field_access.iter().any(|f| f.mode == AccessMode::Read))
        );

        // Compiler-generated members are flagged, not dropped.
        assert!(
            ast.types.iter().any(|t| t.compiler_generated),
            "expected some compiler-generated types"
        );
    }

    /// Most of wine-mono's `4.5` profile ships as type-forwarding facades: full metadata, and
    /// every method body compiled to `ldnull; throw`. This is not a reader failure and must not
    /// be mistaken for one — it is what a reference assembly *is*. Pinning it here because the
    /// first version of the test above asserted a large call graph against `System.Xml.dll` and
    /// failed, which looked exactly like a broken IL walk for as long as it took to hexdump one.
    #[test]
    fn a_facade_assembly_yields_types_but_no_logic() {
        let Some(dir) = mono_dir() else {
            eprintln!("skipping: no Mono assembly corpus on this machine");
            return;
        };
        let Ok(bytes) = std::fs::read(dir.join("System.Xml.dll")) else {
            return;
        };
        let ast = read_assembly(&bytes, "System.Xml.dll", "d").unwrap();

        // Metadata is fully there ...
        assert_eq!(ast.assembly_name, "System.Xml");
        assert!(ast.references.iter().any(|r| r == "mscorlib"));
        assert!(
            ast.types
                .iter()
                .any(|t| t.qualified_name() == "System.Xml.XmlTextReader"),
            "a facade still declares its types"
        );

        // ... and there is genuinely no logic behind it.
        let branches: usize = ast
            .types
            .iter()
            .flat_map(|t| &t.methods)
            .filter_map(|m| m.body.as_ref())
            .map(|b| b.branches.len())
            .sum();
        assert_eq!(branches, 0, "a facade has no decisions in it");
    }

    #[test]
    fn reading_the_same_assembly_twice_is_byte_identical() {
        let Some(dir) = mono_dir() else {
            eprintln!("skipping: no Mono assembly corpus on this machine");
            return;
        };
        let Ok(bytes) = std::fs::read(dir.join("System.Xml.dll")) else {
            return;
        };
        let a = read_assembly(&bytes, "System.Xml.dll", "d").unwrap();
        let b = read_assembly(&bytes, "System.Xml.dll", "d").unwrap();
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap()
        );
    }

    /// Truncating a real assembly at many points must never panic — this is the hostile-input
    /// property the whole bounds-checked design exists for, exercised against real bytes rather
    /// than a synthetic file.
    #[test]
    fn truncated_assemblies_never_panic() {
        let Some(dir) = mono_dir() else {
            eprintln!("skipping: no Mono assembly corpus on this machine");
            return;
        };
        let Ok(bytes) = std::fs::read(dir.join("Accessibility.dll")) else {
            return;
        };
        for divisor in [2usize, 3, 4, 8, 16, 32, 64, 128] {
            let cut = bytes.len() / divisor;
            let _ = read_assembly(&bytes[..cut], "truncated.dll", "d");
        }
        // And a byte-level sweep near the headers, where the offsets are densest.
        for cut in (0..2048).step_by(37) {
            let _ = read_assembly(&bytes[..cut.min(bytes.len())], "truncated.dll", "d");
        }
    }
}
