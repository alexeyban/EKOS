//! JVM backend (RFC 0148) — class files and jar/war/ear archives into [`DecompiledAst`].
//!
//! Built on `cafebabe` 0.9 (0BSD, pure safe Rust), which parses the class file per JVMS §4 and
//! decodes the `Code` attribute's opcodes with their constant-pool operands already resolved.
//! It was chosen over the alternatives on a real corpus rather than on its README: **652 of 652**
//! classes in `/usr/share/java/pdfbox.jar` parsed, with methods, fields and bytecode.
//!
//! Archive members are read with `zip` (already a workspace dependency for DOCX in
//! `plugins/localdocs`). One [`DecompiledAst`] is produced **per class**, not per archive: a
//! fat jar holds tens of thousands of classes, and one artifact that size would be unreadable,
//! un-diffable, and would re-hash in its entirety when a single class changed.

use crate::ast::*;
use crate::io_classify;
use crate::limits;
use crate::{BinaryError, Result};
use cafebabe::attributes::AttributeData;
use cafebabe::bytecode::Opcode;
use cafebabe::constant_pool::{LiteralConstant, Loadable, MemberRef};
use cafebabe::descriptors::{FieldDescriptor, FieldType, MethodDescriptor, ReturnDescriptor};
use cafebabe::{ClassAccessFlags, FieldAccessFlags, MethodAccessFlags};

/// Recorded as `BinarySource::extractor` on every fact derived from this backend, so a query can
/// always tell a deterministically-read fact from an LLM-reconstructed one.
pub const EXTRACTOR: &str = "ekos-jvm-classfile/v1";

// ────────────────────────────────────────────────────────────────────────────
// Entry points
// ────────────────────────────────────────────────────────────────────────────

/// Read one loose `.class` file.
///
/// `path` is the workspace-relative path and `sha256` the hash of the bytes, both carried
/// straight onto [`BinarySource`] to close the provenance chain.
pub fn read_class(bytes: &[u8], path: &str, sha256: &str) -> Result<DecompiledAst> {
    read_class_member(bytes, path, None, sha256)
}

/// Read every `.class` member of a jar/war/ear.
///
/// Returns one AST per class, in the archive's own entry order — deterministic for a given
/// archive, because zip central directories are ordered and this does not sort or parallelise.
///
/// A member that fails to parse costs that member only: it becomes a diagnostic on the *next*
/// successfully-read AST is not good enough (it could be lost entirely if it is the last), so
/// failures are collected and returned alongside, in [`ArchiveRead::diagnostics`].
pub fn read_archive(bytes: &[u8], path: &str, sha256: &str) -> Result<ArchiveRead> {
    if bytes.len() > limits::MAX_FILE_BYTES {
        return Err(BinaryError::TooLarge {
            path: path.into(),
            size: bytes.len(),
            limit: limits::MAX_FILE_BYTES,
        });
    }
    let reader = std::io::Cursor::new(bytes);
    let mut zip =
        zip::ZipArchive::new(reader).map_err(|e| BinaryError::Container(format!("{path}: {e}")))?;

    let mut out = ArchiveRead::default();
    let member_count = zip.len().min(limits::MAX_ARCHIVE_MEMBERS);
    if zip.len() > limits::MAX_ARCHIVE_MEMBERS {
        out.diagnostics.push(AstDiagnostic::new(
            "BIN_ARCHIVE_TRUNCATED",
            format!(
                "{path}: {} members exceeds the {} cap; the remainder was not read",
                zip.len(),
                limits::MAX_ARCHIVE_MEMBERS
            ),
        ));
    }

    for i in 0..member_count {
        let mut entry = match zip.by_index(i) {
            Ok(e) => e,
            Err(e) => {
                out.diagnostics.push(AstDiagnostic::new(
                    "BIN_ARCHIVE_MEMBER",
                    format!("{path}: member {i} unreadable: {e}"),
                ));
                continue;
            }
        };
        if !entry.is_file() || !entry.name().ends_with(".class") {
            continue;
        }
        // Declared size is checked *before* decompressing: this is the zip-bomb guard, and
        // checking after the read would defeat the point of having it.
        if entry.size() > limits::MAX_MEMBER_BYTES as u64 {
            out.diagnostics.push(AstDiagnostic::new(
                "BIN_MEMBER_TOO_LARGE",
                format!(
                    "{path}!{}: {} bytes exceeds the {} cap; skipped",
                    entry.name(),
                    entry.size(),
                    limits::MAX_MEMBER_BYTES
                ),
            ));
            continue;
        }
        let name = entry.name().to_string();
        let mut buf = Vec::with_capacity(entry.size() as usize);
        if let Err(e) = std::io::Read::read_to_end(&mut entry, &mut buf) {
            out.diagnostics.push(AstDiagnostic::new(
                "BIN_ARCHIVE_MEMBER",
                format!("{path}!{name}: {e}"),
            ));
            continue;
        }
        match read_class_member(&buf, path, Some(&name), sha256) {
            Ok(ast) => out.asts.push(ast),
            Err(e) => out.diagnostics.push(
                AstDiagnostic::new("BIN_CLASS_UNREADABLE", format!("{path}!{name}: {e}")).at(name),
            ),
        }
    }
    Ok(out)
}

/// Everything a single archive yielded.
#[derive(Debug, Default)]
pub struct ArchiveRead {
    pub asts: Vec<DecompiledAst>,
    /// Archive-level problems, and per-member failures that have no AST to hang off.
    pub diagnostics: Vec<AstDiagnostic>,
}

fn read_class_member(
    bytes: &[u8],
    path: &str,
    entry: Option<&str>,
    sha256: &str,
) -> Result<DecompiledAst> {
    if bytes.len() > limits::MAX_MEMBER_BYTES {
        return Err(BinaryError::TooLarge {
            path: entry.unwrap_or(path).into(),
            size: bytes.len(),
            limit: limits::MAX_MEMBER_BYTES,
        });
    }
    let class =
        cafebabe::parse_class(bytes).map_err(|e| BinaryError::Parse(format!("class file: {e}")))?;

    let mut diagnostics = Vec::new();
    let internal = class.this_class.to_string();
    let (namespace, simple) = split_internal_name(&internal);

    let category = if class.access_flags.contains(ClassAccessFlags::INTERFACE) {
        TypeCategory::Interface
    } else if class.access_flags.contains(ClassAccessFlags::ENUM) {
        TypeCategory::Enum
    } else {
        TypeCategory::Class
    };

    let super_type = class
        .super_class
        .as_ref()
        .map(|c| dotted(&c.to_string()))
        // `java.lang.Object` is the implicit root of every class. Emitting it would add one edge
        // per type to a single node, which is noise in every graph view and tells a reader
        // nothing they did not already know.
        .filter(|s| s != "java.lang.Object");

    let ty = DecompiledType {
        namespace,
        name: simple,
        locator: internal.clone(),
        category,
        visibility: class_visibility(class.access_flags),
        is_abstract: class.access_flags.contains(ClassAccessFlags::ABSTRACT),
        super_type,
        interfaces: class
            .interfaces
            .iter()
            .map(|i| dotted(&i.to_string()))
            .collect(),
        compiler_generated: class.access_flags.contains(ClassAccessFlags::SYNTHETIC)
            || is_generated_jvm_name(&internal),
        fields: class
            .fields
            .iter()
            .map(|f| DecompiledField {
                name: f.name.to_string(),
                locator: format!("{internal}#{}", f.name),
                type_name: render_field_type(&f.descriptor),
                visibility: field_visibility(f.access_flags),
                is_static: f.access_flags.contains(FieldAccessFlags::STATIC),
                is_final: f.access_flags.contains(FieldAccessFlags::FINAL),
                compiler_generated: f.access_flags.contains(FieldAccessFlags::SYNTHETIC),
            })
            .collect(),
        methods: class
            .methods
            .iter()
            .map(|m| {
                let descriptor = m.descriptor.to_string();
                let locator = format!("{internal}.{}:{descriptor}", m.name);
                DecompiledMethod {
                    name: m.name.to_string(),
                    signature: render_method_signature(&m.descriptor),
                    descriptor,
                    visibility: method_visibility(m.access_flags),
                    is_static: m.access_flags.contains(MethodAccessFlags::STATIC),
                    is_abstract: m.access_flags.contains(MethodAccessFlags::ABSTRACT),
                    compiler_generated: m
                        .access_flags
                        .intersects(MethodAccessFlags::SYNTHETIC | MethodAccessFlags::BRIDGE),
                    body: read_body(&m.attributes, &locator, &mut diagnostics),
                    locator,
                }
            })
            .collect(),
    };

    Ok(DecompiledAst {
        source: BinarySource {
            kind: BinaryKind::Jvm,
            path: path.to_string(),
            container_entry: entry.map(str::to_string),
            sha256: sha256.to_string(),
            format_version: format!("{}.{}", class.major_version, class.minor_version),
            extractor: EXTRACTOR.into(),
        },
        fidelity: Fidelity::Structural,
        // A class file carries no module identity before Java 9, and even then only in a separate
        // `module-info.class`. The archive (or the file itself) is the honest unit of assembly
        // here, so the caller's path is used rather than a name invented from the package.
        assembly_name: entry.map_or_else(|| path.to_string(), |_| path.to_string()),
        references: Vec::new(),
        types: vec![ty],
        diagnostics,
    })
}

// ────────────────────────────────────────────────────────────────────────────
// Method bodies
// ────────────────────────────────────────────────────────────────────────────

fn read_body(
    attributes: &[cafebabe::attributes::AttributeInfo<'_>],
    locator: &str,
    diagnostics: &mut Vec<AstDiagnostic>,
) -> Option<MethodBody> {
    let code = attributes.iter().find_map(|a| match &a.data {
        AttributeData::Code(c) => Some(c),
        _ => None,
    })?;

    let mut body = MethodBody {
        code_size: code.code.len(),
        ..Default::default()
    };

    let Some(bytecode) = code.bytecode.as_ref() else {
        // A `Code` attribute whose opcodes did not decode: the method exists and its size is
        // known, so it is kept with an empty body rather than dropped, and the gap is recorded.
        diagnostics.push(
            AstDiagnostic::new(
                "BIN_BYTECODE_UNDECODED",
                "Code attribute present but opcodes were not decoded",
            )
            .at(locator),
        );
        return Some(body);
    };

    for (raw_offset, op) in &bytecode.opcodes {
        let offset = *raw_offset as u32;
        match op {
            // ── calls ───────────────────────────────────────────────────────
            Opcode::Invokestatic(r) => body.calls.push(call(offset, CallKind::Static, r)),
            Opcode::Invokespecial(r) => {
                // `invokespecial` covers three different things: constructor invocation,
                // `super.m()`, and private-method calls. Only the first is construction, and the
                // method name is what distinguishes them.
                let kind = if r.name_and_type.name == "<init>" {
                    CallKind::Constructor
                } else {
                    CallKind::Static
                };
                body.calls.push(call(offset, kind, r));
            }
            Opcode::Invokevirtual(r) => body.calls.push(call(offset, CallKind::Virtual, r)),
            Opcode::Invokeinterface(r, _) => body.calls.push(call(offset, CallKind::Virtual, r)),
            Opcode::Invokedynamic(d) => {
                // The real target is chosen at runtime by a bootstrap method, so there is no
                // resolvable owner. Recording the call-site name and descriptor keeps the
                // evidence without pretending an edge was resolved.
                body.calls.push(CallSite {
                    offset,
                    kind: CallKind::Dynamic,
                    owner: String::new(),
                    method: d.name_and_type.name.to_string(),
                    descriptor: d.name_and_type.descriptor.to_string(),
                    io: None,
                });
            }

            // ── field access ────────────────────────────────────────────────
            Opcode::Getfield(r) => {
                body.field_access
                    .push(field_access(offset, AccessMode::Read, r, false))
            }
            Opcode::Getstatic(r) => {
                body.field_access
                    .push(field_access(offset, AccessMode::Read, r, true))
            }
            Opcode::Putfield(r) => {
                body.field_access
                    .push(field_access(offset, AccessMode::Write, r, false))
            }
            Opcode::Putstatic(r) => {
                body.field_access
                    .push(field_access(offset, AccessMode::Write, r, true))
            }

            // ── literals ────────────────────────────────────────────────────
            //
            // `iconst_0..5`/`lconst_*`/`fconst_*`/`dconst_*` are deliberately excluded. They are
            // overwhelmingly loop bounds, array indices and boolean returns — emitting them would
            // bury the literals that actually matter under thousands of 0s and 1s. The numbers a
            // business rule turns on (thresholds, rates, limits, codes) do not fit in those
            // opcodes and arrive as `bipush`/`sipush`/`ldc` instead.
            Opcode::Bipush(v) => body.numeric_literals.push(NumericLiteral {
                offset,
                value: v.to_string(),
            }),
            Opcode::Sipush(v) => body.numeric_literals.push(NumericLiteral {
                offset,
                value: v.to_string(),
            }),
            Opcode::Ldc(l) | Opcode::LdcW(l) | Opcode::Ldc2W(l) => {
                push_loadable(&mut body, offset, l)
            }

            // ── branches ────────────────────────────────────────────────────
            Opcode::Tableswitch(t) => body.branches.push(Branch {
                offset,
                opcode: "tableswitch".into(),
                case_count: t.jumps.len(),
            }),
            Opcode::Lookupswitch(t) => body.branches.push(Branch {
                offset,
                opcode: "lookupswitch".into(),
                case_count: t.match_offsets.len(),
            }),
            other => {
                if let Some(name) = conditional_branch_name(other) {
                    body.branches.push(Branch {
                        offset,
                        opcode: name.into(),
                        case_count: 1,
                    });
                }
            }
        }
    }
    Some(body)
}

/// The mnemonic of a conditional branch opcode, or `None` for everything else.
///
/// `goto`/`jsr` are intentionally absent: they are unconditional, so counting them would inflate
/// cyclomatic complexity without a decision point behind it — and the JVM emits a `goto` at the
/// end of virtually every `if` block and loop body.
fn conditional_branch_name(op: &Opcode<'_>) -> Option<&'static str> {
    Some(match op {
        Opcode::Ifeq(_) => "ifeq",
        Opcode::Ifne(_) => "ifne",
        Opcode::Iflt(_) => "iflt",
        Opcode::Ifge(_) => "ifge",
        Opcode::Ifgt(_) => "ifgt",
        Opcode::Ifle(_) => "ifle",
        Opcode::Ifnull(_) => "ifnull",
        Opcode::Ifnonnull(_) => "ifnonnull",
        Opcode::IfIcmpeq(_) => "if_icmpeq",
        Opcode::IfIcmpne(_) => "if_icmpne",
        Opcode::IfIcmplt(_) => "if_icmplt",
        Opcode::IfIcmpge(_) => "if_icmpge",
        Opcode::IfIcmpgt(_) => "if_icmpgt",
        Opcode::IfIcmple(_) => "if_icmple",
        Opcode::IfAcmpeq(_) => "if_acmpeq",
        Opcode::IfAcmpne(_) => "if_acmpne",
        _ => return None,
    })
}

fn push_loadable(body: &mut MethodBody, offset: u32, l: &Loadable<'_>) {
    match l {
        Loadable::LiteralConstant(LiteralConstant::String(s)) => {
            body.string_literals.push(StringLiteral {
                offset,
                value: s.to_string(),
            })
        }
        Loadable::LiteralConstant(c) => {
            // Rendered here, once, rather than carried as a float through JSON — see
            // `NumericLiteral`'s own note on content-addressing and float formatting.
            let value = match c {
                LiteralConstant::Integer(v) => v.to_string(),
                LiteralConstant::Long(v) => v.to_string(),
                LiteralConstant::Float(v) => format!("{v:?}"),
                LiteralConstant::Double(v) => format!("{v:?}"),
                LiteralConstant::String(_) => unreachable!("matched above"),
                // A `CONSTANT_String` whose bytes are not valid modified UTF-8. Real, and the
                // reason this arm exists rather than a panic.
                LiteralConstant::StringBytes(_) => return,
            };
            body.numeric_literals.push(NumericLiteral { offset, value });
        }
        // Class/method handles and types are not literals a business rule turns on.
        _ => {}
    }
}

fn call(offset: u32, kind: CallKind, r: &MemberRef<'_>) -> CallSite {
    let owner = dotted(&r.class_name);
    CallSite {
        offset,
        kind,
        io: io_classify::classify(&owner),
        owner,
        method: r.name_and_type.name.to_string(),
        descriptor: r.name_and_type.descriptor.to_string(),
    }
}

fn field_access(offset: u32, mode: AccessMode, r: &MemberRef<'_>, is_static: bool) -> FieldAccess {
    FieldAccess {
        offset,
        mode,
        owner: dotted(&r.class_name),
        name: r.name_and_type.name.to_string(),
        type_name: r.name_and_type.descriptor.to_string(),
        is_static,
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Names, types and flags
// ────────────────────────────────────────────────────────────────────────────

/// JVM internal form (`com/acme/Foo`) to source form (`com.acme.Foo`).
///
/// `$` is left alone. It separates a nested class from its outer one, but it is also a legal
/// identifier character, so rewriting it would silently corrupt the (rare but real) class
/// genuinely named `Foo$Bar`.
fn dotted(internal: &str) -> String {
    internal.replace('/', ".")
}

/// Splits `com/acme/Foo` into `("com.acme", "Foo")`.
fn split_internal_name(internal: &str) -> (String, String) {
    match internal.rsplit_once('/') {
        Some((pkg, name)) => (pkg.replace('/', "."), name.to_string()),
        None => (String::new(), internal.to_string()),
    }
}

/// Names the compiler invents, as opposed to names a developer wrote.
///
/// `Foo$1` (an anonymous inner class) is deliberately **not** included: it is compiled *from*
/// developer-written source and usually holds the body of a listener or comparator — real logic
/// that must not be filtered away as machinery.
fn is_generated_jvm_name(internal: &str) -> bool {
    internal.contains("$$Lambda$")
        || internal
            .rsplit('/')
            .next()
            .is_some_and(|n| n.starts_with("access$"))
}

fn class_visibility(f: ClassAccessFlags) -> Visibility {
    if f.contains(ClassAccessFlags::PUBLIC) {
        Visibility::Public
    } else {
        // Package-private. The JVM has no `internal`, but the concept is identical.
        Visibility::Internal
    }
}

fn field_visibility(f: FieldAccessFlags) -> Visibility {
    if f.contains(FieldAccessFlags::PUBLIC) {
        Visibility::Public
    } else if f.contains(FieldAccessFlags::PROTECTED) {
        Visibility::Protected
    } else if f.contains(FieldAccessFlags::PRIVATE) {
        Visibility::Private
    } else {
        Visibility::Internal
    }
}

fn method_visibility(f: MethodAccessFlags) -> Visibility {
    if f.contains(MethodAccessFlags::PUBLIC) {
        Visibility::Public
    } else if f.contains(MethodAccessFlags::PROTECTED) {
        Visibility::Protected
    } else if f.contains(MethodAccessFlags::PRIVATE) {
        Visibility::Private
    } else {
        Visibility::Internal
    }
}

fn render_field_type(d: &FieldDescriptor<'_>) -> String {
    let base = match &d.field_type {
        FieldType::Byte => "byte".to_string(),
        FieldType::Char => "char".to_string(),
        FieldType::Double => "double".to_string(),
        FieldType::Float => "float".to_string(),
        FieldType::Integer => "int".to_string(),
        FieldType::Long => "long".to_string(),
        FieldType::Short => "short".to_string(),
        FieldType::Boolean => "boolean".to_string(),
        FieldType::Object(c) => dotted(&c.to_string()),
    };
    format!("{base}{}", "[]".repeat(d.dimensions as usize))
}

/// `(java.lang.String, int) -> boolean` — the readable form the LLM stage and every rendered page
/// use, alongside the raw descriptor kept verbatim as evidence.
fn render_method_signature(d: &MethodDescriptor<'_>) -> String {
    let params: Vec<String> = d.parameters.iter().map(render_field_type).collect();
    let ret = match &d.return_type {
        ReturnDescriptor::Void => "void".to_string(),
        ReturnDescriptor::Return(f) => render_field_type(f),
    };
    format!("({}) -> {ret}", params.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_names_become_source_names() {
        assert_eq!(
            dotted("com/acme/billing/Invoice"),
            "com.acme.billing.Invoice"
        );
        assert_eq!(dotted("Invoice"), "Invoice");
        // `$` survives: a nested class and a class legitimately named with `$` are
        // indistinguishable here, so neither is rewritten.
        assert_eq!(dotted("com/acme/Outer$Inner"), "com.acme.Outer$Inner");
    }

    #[test]
    fn split_internal_name_separates_package_from_simple_name() {
        assert_eq!(
            split_internal_name("com/acme/billing/Invoice"),
            ("com.acme.billing".into(), "Invoice".into())
        );
        assert_eq!(
            split_internal_name("Invoice"),
            (String::new(), "Invoice".into())
        );
    }

    #[test]
    fn lambda_and_accessor_names_are_compiler_generated() {
        assert!(is_generated_jvm_name("com/acme/Foo$$Lambda$14"));
        assert!(is_generated_jvm_name("com/acme/access$000"));
    }

    /// An anonymous inner class holds real developer-written logic. Filtering it out as
    /// machinery would silently drop the body of every listener and comparator in a codebase.
    #[test]
    fn an_anonymous_inner_class_is_not_compiler_generated() {
        assert!(!is_generated_jvm_name("com/acme/Foo$1"));
        assert!(!is_generated_jvm_name("com/acme/Outer$Inner"));
    }

    #[test]
    fn a_non_class_file_is_a_parse_error_not_a_panic() {
        let err = read_class(b"not a class file at all", "x.class", "deadbeef").unwrap_err();
        assert!(matches!(err, BinaryError::Parse(_)));
    }

    #[test]
    fn an_empty_or_truncated_class_file_is_an_error() {
        assert!(read_class(&[], "x.class", "d").is_err());
        assert!(read_class(&[0xCA, 0xFE, 0xBA, 0xBE], "x.class", "d").is_err());
    }

    #[test]
    fn a_non_zip_input_is_a_container_error() {
        let err = read_archive(b"definitely not a zip", "x.jar", "d").unwrap_err();
        assert!(matches!(err, BinaryError::Container(_)));
    }

    /// The real corpus check. `/usr/share/java/pdfbox.jar` is present on the development machine
    /// and was the evidence RFC 0148 accepted `cafebabe` on; when it is absent (CI, another
    /// machine) the test skips rather than fails, because it asserts about a file this repo does
    /// not own.
    #[test]
    fn reads_a_real_jar_when_one_is_available() {
        let path = "/usr/share/java/pdfbox.jar";
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("skipping: {path} not present on this machine");
            return;
        };
        let read = read_archive(&bytes, path, "sha-not-checked").unwrap();
        assert!(
            read.asts.len() > 500,
            "expected hundreds of classes, got {}",
            read.asts.len()
        );
        assert!(
            read.diagnostics.is_empty(),
            "real jar produced diagnostics: {:?}",
            read.diagnostics
        );

        let doc = read
            .asts
            .iter()
            .find(|a| a.types[0].locator == "org/apache/pdfbox/pdmodel/PDDocument")
            .expect("PDDocument must be present");
        let ty = &doc.types[0];
        assert_eq!(ty.namespace, "org.apache.pdfbox.pdmodel");
        assert_eq!(ty.name, "PDDocument");
        assert_eq!(ty.category, TypeCategory::Class);
        assert!(ty.methods.len() > 50, "got {} methods", ty.methods.len());
        assert_eq!(
            doc.source.container_entry.as_deref(),
            Some("org/apache/pdfbox/pdmodel/PDDocument.class")
        );

        // Real bodies with real call graphs — the whole point of the backend.
        let with_calls = ty
            .methods
            .iter()
            .filter(|m| m.body.as_ref().is_some_and(|b| !b.calls.is_empty()))
            .count();
        assert!(with_calls > 20, "only {with_calls} methods had call sites");

        // PDFBox writes files, so the I/O classifier must find file boundaries somewhere in it.
        let io_found = read.asts.iter().any(|a| {
            a.types.iter().any(|t| {
                t.methods.iter().any(|m| {
                    m.body
                        .as_ref()
                        .is_some_and(|b| b.calls.iter().any(|c| c.io.is_some()))
                })
            })
        });
        assert!(io_found, "expected at least one I/O boundary in pdfbox");
    }

    /// Two reads of the same archive must produce identical JSON, or `ekos build` is not
    /// idempotent for binaries and every run re-writes the ledger.
    #[test]
    fn reading_the_same_jar_twice_is_byte_identical() {
        let path = "/usr/share/java/pdfbox.jar";
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("skipping: {path} not present on this machine");
            return;
        };
        let a = read_archive(&bytes, path, "d").unwrap();
        let b = read_archive(&bytes, path, "d").unwrap();
        assert_eq!(
            serde_json::to_string(&a.asts).unwrap(),
            serde_json::to_string(&b.asts).unwrap()
        );
    }
}
