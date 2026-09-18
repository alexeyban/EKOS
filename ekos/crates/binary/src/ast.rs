//! The `DecompiledAst` — RFC 0148's shared backend-agnostic projection of a compiled binary.
//!
//! Both backends (`jvm`, `dotnet`) normalize into these types, so `BinaryAnalyzerPass` and the
//! LLM reconstruction stage contain no backend-specific logic. Everything here is plain data:
//! serializable, deterministic, and carrying a locator on every node that can be cited as
//! evidence.
//!
//! **Determinism is a hard requirement, not a nicety.** These structures are serialized into a
//! content-addressed `ObservationArtifact`, so two reads of the same unchanged binary must
//! produce byte-identical JSON. That is why every collection here is an ordered `Vec` populated
//! in file order and never a `HashMap`/`HashSet`, and why nothing carries a timestamp.

use serde::{Deserialize, Serialize};

// ────────────────────────────────────────────────────────────────────────────
// Source identity
// ────────────────────────────────────────────────────────────────────────────

/// Which binary format an AST was read from. Decided by magic bytes (see [`crate::detect`]),
/// never by file extension — a `.dll` is very often native code with no CLI metadata at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BinaryKind {
    /// JVM class file (`0xCAFEBABE`), loose or inside a jar/war/ear.
    Jvm,
    /// PE image carrying a CLI header — a managed .NET assembly.
    DotNet,
}

impl BinaryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Jvm => "jvm",
            Self::DotNet => "dotnet",
        }
    }
}

/// How much of the original program this AST actually represents.
///
/// Carried on every artifact and propagated onto every derived fact, because the LLM stage must
/// be told what it is *not* looking at. A model handed only `Structural` facts and not told so
/// will narrate control flow it was never given; being explicit is the cheapest guard available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Fidelity {
    /// v1, in-process: types, members, signatures, call graph, field access, branch structure,
    /// literals and I/O boundaries — with offsets, but no statement or expression trees.
    Structural,
    /// Reserved for the future out-of-process decompiler backend (RFC 0148 "Fidelity levels"):
    /// the above plus reconstructed statements. Nothing produces this today; it exists so
    /// consumers are written against the enum rather than against the assumption.
    Statements,
}

/// Identity and provenance of the binary an AST was read from — the far end of RFC 0148's
/// two-hop provenance chain (`fact → locator → this`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinarySource {
    pub kind: BinaryKind,
    /// Workspace-relative path of the file on disk. For a class inside an archive this is the
    /// *archive's* path; [`Self::container_entry`] carries the member.
    pub path: String,
    /// Member path inside a jar/war/ear, when the type came from one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_entry: Option<String>,
    /// SHA-256 of the whole file on disk (the archive, not the member) — what a recovered fact
    /// ultimately traces back to, and what proves which build it came from.
    pub sha256: String,
    /// Format version as the file itself declares it: `"52.0"` for a JVM class file's
    /// major.minor, or the CLI metadata version string (e.g. `"v4.0.30319"`) for .NET.
    pub format_version: String,
    /// Identity of the reader that produced this AST, recorded as the `extractor` on every
    /// derived fact so stage-1 and stage-2 facts can never be conflated by a query.
    pub extractor: String,
}

// ────────────────────────────────────────────────────────────────────────────
// Diagnostics
// ────────────────────────────────────────────────────────────────────────────

/// One thing the reader could not understand, recorded rather than discarded.
///
/// This is the mechanism behind RFC 0148's central design constraint: a binary reader must
/// **degrade per item, never per file**. `dotscope` was rejected for aborting the whole assembly
/// on one malformed custom-attribute blob, which is the same all-or-nothing failure that silently
/// cost LedgerSMB its entire schema under RFC 0146. Every recoverable problem lands here and the
/// rest of the file is still read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AstDiagnostic {
    /// Stable short code, e.g. `"BIN_SIG_UNREADABLE"`.
    pub code: String,
    /// The locator of the node this concerns, when it is about a specific one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locator: Option<String>,
    pub message: String,
}

impl AstDiagnostic {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            locator: None,
            message: message.into(),
        }
    }

    pub fn at(mut self, locator: impl Into<String>) -> Self {
        self.locator = Some(locator.into());
        self
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Program structure
// ────────────────────────────────────────────────────────────────────────────

/// Everything recovered from one binary file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecompiledAst {
    pub source: BinarySource,
    pub fidelity: Fidelity,
    /// Assembly/module display name as the binary declares it (`Assembly.Name` for .NET, the jar
    /// or class file name for JVM — class files carry no module identity before Java 9).
    pub assembly_name: String,
    /// Assemblies/modules this one references by name. `DependsOn` edges downstream.
    #[serde(default)]
    pub references: Vec<String>,
    pub types: Vec<DecompiledType>,
    #[serde(default)]
    pub diagnostics: Vec<AstDiagnostic>,
}

impl DecompiledAst {
    /// Total methods across all types — the unit stage 2 batches on, so it is also the cost
    /// signal a `max_slices` budget is set against.
    pub fn method_count(&self) -> usize {
        self.types.iter().map(|t| t.methods.len()).sum()
    }

    /// Split a multi-type AST into one AST per type, returning the diagnostics that belong to no
    /// single type alongside.
    ///
    /// The JVM backend is already one-type-per-AST because a class file holds exactly one class.
    /// A .NET assembly holds thousands, and keeping them together produced a **34 MB artifact**
    /// for `mscorlib.dll` in the first end-to-end run — one blob that re-hashes in its entirety
    /// when a single method changes, cannot be diffed usefully, and defeats the content-addressed
    /// store's whole purpose. Splitting makes both backends emit the same unit.
    ///
    /// `source` and `references` are copied onto every part: `references` is a handful of
    /// assembly names, and the alternative — putting them on one part only — would make the
    /// assembly-level facts depend on which artifact the analyzer happened to read first.
    pub fn split_by_type(self) -> (Vec<Self>, Vec<AstDiagnostic>) {
        if self.types.len() <= 1 {
            let diagnostics = self.diagnostics.clone();
            return (vec![self], diagnostics);
        }

        // A diagnostic names the locator of the type, method or field it concerns, so each one
        // can be returned to the part that owns it instead of being dumped on the first.
        let mut owner_of: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        for (i, ty) in self.types.iter().enumerate() {
            owner_of.insert(ty.locator.as_str(), i);
            for m in &ty.methods {
                owner_of.insert(m.locator.as_str(), i);
            }
            for f in &ty.fields {
                owner_of.insert(f.locator.as_str(), i);
            }
        }

        let mut per_type: Vec<Vec<AstDiagnostic>> = vec![Vec::new(); self.types.len()];
        let mut orphaned = Vec::new();
        for d in &self.diagnostics {
            match d.locator.as_deref().and_then(|l| owner_of.get(l)) {
                Some(&i) => per_type[i].push(d.clone()),
                None => orphaned.push(d.clone()),
            }
        }

        let parts = self
            .types
            .into_iter()
            .zip(per_type)
            .map(|(ty, diagnostics)| Self {
                source: self.source.clone(),
                fidelity: self.fidelity,
                assembly_name: self.assembly_name.clone(),
                references: self.references.clone(),
                types: vec![ty],
                diagnostics,
            })
            .collect();
        (parts, orphaned)
    }
}

/// Visibility, normalized across the two type systems. `.NET`'s `assembly`/`famorassem` and the
/// JVM's package-private both collapse to [`Visibility::Internal`]: the distinction they draw is
/// the same one (visible within the compilation unit, not outside it), and preserving both
/// spellings would make every downstream filter backend-specific for no gain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    Public,
    Protected,
    Internal,
    Private,
}

impl Visibility {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Protected => "protected",
            Self::Internal => "internal",
            Self::Private => "private",
        }
    }
}

/// What kind of type declaration this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TypeCategory {
    Class,
    Interface,
    Enum,
    /// .NET value types (`struct`). The JVM has no equivalent and never produces this.
    Struct,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecompiledType {
    /// Namespace (.NET) or package (JVM), dotted. Empty for the default/global namespace.
    pub namespace: String,
    /// Simple name, without the namespace.
    pub name: String,
    /// Stable provenance locator — `"pkg/Cls"` for JVM, `"0x02000004"` (the TypeDef metadata
    /// token) for .NET. Unique within one binary, and the hop a recovered fact cites.
    pub locator: String,
    pub category: TypeCategory,
    pub visibility: Visibility,
    pub is_abstract: bool,
    /// Fully-qualified superclass, absent for interfaces and for the root type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub super_type: Option<String>,
    #[serde(default)]
    pub interfaces: Vec<String>,
    /// True for a type the *compiler* emitted rather than a developer: lambda closures
    /// (`$$Lambda$`, `<>c`), async/iterator state machines (`<Foo>d__3`), synthetic accessors.
    ///
    /// Recovered and flagged rather than dropped, deliberately: these types carry real business
    /// logic (a lambda body is the predicate of a rule), so hiding them would lose it. Marking
    /// them lets a consumer filter without this crate deciding on its behalf what is "real".
    pub compiler_generated: bool,
    #[serde(default)]
    pub fields: Vec<DecompiledField>,
    #[serde(default)]
    pub methods: Vec<DecompiledMethod>,
}

impl DecompiledType {
    /// `namespace.name`, or just `name` in the global namespace.
    pub fn qualified_name(&self) -> String {
        if self.namespace.is_empty() {
            self.name.clone()
        } else {
            format!("{}.{}", self.namespace, self.name)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecompiledField {
    pub name: String,
    pub locator: String,
    /// Rendered type, source-like (`java.lang.String`, `System.Int32`, `int[]`). Best-effort: a
    /// signature blob this crate cannot decode yields `"?"` plus a diagnostic, never a failure.
    pub type_name: String,
    pub visibility: Visibility,
    pub is_static: bool,
    pub is_final: bool,
    pub compiler_generated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecompiledMethod {
    pub name: String,
    pub locator: String,
    /// Human-readable signature, e.g. `(java.lang.String, int) -> boolean`. Rendered from the
    /// JVM descriptor or the CIL signature blob.
    pub signature: String,
    /// The raw descriptor/blob rendering, kept verbatim as evidence alongside the friendly form.
    pub descriptor: String,
    pub visibility: Visibility,
    pub is_static: bool,
    pub is_abstract: bool,
    pub compiler_generated: bool,
    /// Absent for abstract, interface and extern methods — a real "no body here", distinct from
    /// an empty one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<MethodBody>,
}

/// What a method's bytecode yields, per RFC 0148 "What a method body yields".
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MethodBody {
    /// Size of the bytecode in bytes.
    pub code_size: usize,
    /// Conditional branches and switches, in offset order.
    #[serde(default)]
    pub branches: Vec<Branch>,
    /// Call sites, in offset order — the real call graph.
    #[serde(default)]
    pub calls: Vec<CallSite>,
    /// Field reads and writes, in offset order.
    #[serde(default)]
    pub field_access: Vec<FieldAccess>,
    /// String literals loaded by this method, in offset order. Duplicates are kept: two loads of
    /// the same string at different offsets are two distinct pieces of evidence.
    #[serde(default)]
    pub string_literals: Vec<StringLiteral>,
    /// Numeric literals, in offset order. These become rule parameters in stage 2 instead of
    /// being left as magic numbers.
    #[serde(default)]
    pub numeric_literals: Vec<NumericLiteral>,
}

impl MethodBody {
    /// Branch count + 1. This is the real cyclomatic complexity of the compiled method, counted
    /// from its actual branch opcodes — not an estimate from source text, which is the usual way
    /// this number is produced and the reason it is usually wrong.
    ///
    /// A switch counts once per case, since each is a distinct path.
    pub fn cyclomatic_complexity(&self) -> u32 {
        1 + self
            .branches
            .iter()
            .map(|b| b.case_count.max(1) as u32)
            .sum::<u32>()
    }
}

/// A conditional branch or switch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Branch {
    /// Bytecode offset within the method body.
    pub offset: u32,
    /// Mnemonic as the format spells it (`ifeq`, `tableswitch`, `brtrue.s`, `switch`).
    pub opcode: String,
    /// Number of alternative targets for a switch; 1 for a plain conditional branch.
    pub case_count: usize,
}

/// How a call reaches its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CallKind {
    /// Statically bound: `invokestatic`, `invokespecial`, `call`.
    Static,
    /// Dynamically dispatched: `invokevirtual`, `invokeinterface`, `callvirt`.
    Virtual,
    /// Construction: `new`+`invokespecial <init>`, `newobj`.
    Constructor,
    /// `invokedynamic` — the target is decided at runtime by a bootstrap method, so the
    /// recovered name is the call site's *descriptor*, not a resolvable method. Kept distinct so
    /// nothing downstream treats a lambda call site as a resolved edge.
    Dynamic,
}

/// One call site.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallSite {
    pub offset: u32,
    pub kind: CallKind,
    /// Fully-qualified owner of the callee (`java.sql.Connection`, `System.Data.SqlClient.SqlCommand`).
    pub owner: String,
    pub method: String,
    pub descriptor: String,
    /// The assembly that defines the callee, when the format records it. .NET does — a
    /// `MethodDef` token is always this assembly, a `TypeRef` names its `AssemblyRef` — and
    /// without it two binaries declaring the same type name (a client and server built from
    /// shared source, or two copies of one exe) are indistinguishable to a name-keyed join. The
    /// JVM has no such scope (a class is found on the classpath), so it is `None` there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_assembly: Option<String>,
    /// Set when [`crate::io_classify`] recognizes the owner as an I/O boundary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub io: Option<IoBoundary>,
}

/// The category of resource a call leaves the binary to reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IoBoundary {
    Database,
    Http,
    File,
    Messaging,
    /// Process/shell execution — worth surfacing separately because it is both a real
    /// integration boundary and the thing a security reviewer looks for first.
    Process,
}

impl IoBoundary {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Database => "database",
            Self::Http => "http",
            Self::File => "file",
            Self::Messaging => "messaging",
            Self::Process => "process",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccessMode {
    Read,
    Write,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldAccess {
    pub offset: u32,
    pub mode: AccessMode,
    pub owner: String,
    pub name: String,
    pub type_name: String,
    pub is_static: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StringLiteral {
    pub offset: u32,
    pub value: String,
}

/// A numeric constant loaded by the bytecode.
///
/// The value is carried as a string rather than an `f64`/`i64` union for one specific reason:
/// this struct is serialized into a *content-addressed* artifact, and float formatting through
/// JSON is the classic way to make an identical input hash differently across platforms. A
/// string renders once, here, deterministically.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NumericLiteral {
    pub offset: u32,
    pub value: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body_with_branches(cases: &[usize]) -> MethodBody {
        MethodBody {
            code_size: 32,
            branches: cases
                .iter()
                .enumerate()
                .map(|(i, &c)| Branch {
                    offset: i as u32,
                    opcode: "ifeq".into(),
                    case_count: c,
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn a_straight_line_method_has_complexity_one() {
        assert_eq!(MethodBody::default().cyclomatic_complexity(), 1);
    }

    #[test]
    fn each_conditional_branch_adds_one() {
        assert_eq!(body_with_branches(&[1, 1, 1]).cyclomatic_complexity(), 4);
    }

    #[test]
    fn a_switch_counts_once_per_case() {
        // One `tableswitch` with 5 cases is five distinct paths, not one.
        assert_eq!(body_with_branches(&[5]).cyclomatic_complexity(), 6);
    }

    #[test]
    fn a_zero_case_branch_still_counts_as_one_path() {
        // Defensive: a malformed switch that decodes to 0 cases must not make a method look
        // *less* complex than a straight line one.
        assert_eq!(body_with_branches(&[0]).cyclomatic_complexity(), 2);
    }

    #[test]
    fn qualified_name_omits_an_empty_namespace() {
        let mut t = DecompiledType {
            namespace: "com.acme.billing".into(),
            name: "Invoice".into(),
            locator: "com/acme/billing/Invoice".into(),
            category: TypeCategory::Class,
            visibility: Visibility::Public,
            is_abstract: false,
            super_type: None,
            interfaces: vec![],
            compiler_generated: false,
            fields: vec![],
            methods: vec![],
        };
        assert_eq!(t.qualified_name(), "com.acme.billing.Invoice");
        t.namespace = String::new();
        assert_eq!(t.qualified_name(), "Invoice");
    }

    /// The AST is serialized into a content-addressed artifact, so identical input must produce
    /// identical JSON. This is the property that makes `ekos build` idempotent for binaries.
    #[test]
    fn identical_asts_serialize_identically() {
        let make = || DecompiledAst {
            source: BinarySource {
                kind: BinaryKind::Jvm,
                path: "lib/billing.jar".into(),
                container_entry: Some("com/acme/Invoice.class".into()),
                sha256: "abc123".into(),
                format_version: "52.0".into(),
                extractor: "ekos-jvm-classfile/v1".into(),
            },
            fidelity: Fidelity::Structural,
            assembly_name: "billing".into(),
            references: vec!["java.base".into()],
            types: vec![],
            diagnostics: vec![],
        };
        assert_eq!(
            serde_json::to_string(&make()).unwrap(),
            serde_json::to_string(&make()).unwrap()
        );
    }

    #[test]
    fn method_count_sums_across_types() {
        let method = |n: &str| DecompiledMethod {
            name: n.into(),
            locator: format!("T.{n}"),
            signature: "() -> void".into(),
            descriptor: "()V".into(),
            visibility: Visibility::Public,
            is_static: false,
            is_abstract: false,
            compiler_generated: false,
            body: None,
        };
        let ty = |n: &str, ms: Vec<DecompiledMethod>| DecompiledType {
            namespace: String::new(),
            name: n.into(),
            locator: n.into(),
            category: TypeCategory::Class,
            visibility: Visibility::Public,
            is_abstract: false,
            super_type: None,
            interfaces: vec![],
            compiler_generated: false,
            fields: vec![],
            methods: ms,
        };
        let ast = DecompiledAst {
            source: BinarySource {
                kind: BinaryKind::DotNet,
                path: "a.dll".into(),
                container_entry: None,
                sha256: "x".into(),
                format_version: "v4.0.30319".into(),
                extractor: "ekos-cil-metadata/v1".into(),
            },
            fidelity: Fidelity::Structural,
            assembly_name: "a".into(),
            references: vec![],
            types: vec![
                ty("A", vec![method("one"), method("two")]),
                ty("B", vec![method("three")]),
            ],
            diagnostics: vec![],
        };
        assert_eq!(ast.method_count(), 3);
    }
}
