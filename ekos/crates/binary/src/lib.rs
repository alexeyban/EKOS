//! `ekos-binary` — RFC 0148 readers for compiled .NET assemblies and JVM bytecode.
//!
//! Produces a [`DecompiledAst`]: a backend-agnostic, deterministic, fully serializable projection
//! of a binary's types, members, call graph, field access, literals, branch structure and
//! external I/O boundaries, each node carrying a locator that downstream facts cite as evidence.
//!
//! # Why this is a library, not a subprocess
//!
//! The RFC this crate implements deliberately departs from its own originating draft, which
//! specified out-of-process decompiler sidecars (ICSharpCode.Decompiler and Vineflower). Four
//! reasons, three of them this project's own established precedent (RFC 0147 rejected shelling
//! out to `perl` on the first three):
//!
//! 1. A sidecar needs a .NET SDK and a JRE on every machine and CI runner that runs `ekos
//!    recover`.
//! 2. Decompiler output varies by decompiler version, so ledger facts would depend on what
//!    happened to be installed — the opposite of reproducible builds.
//! 3. `CompilerPass`es must be deterministic and side-effect-free; spawning a subprocess per
//!    file is neither.
//! 4. A customer's DLL is untrusted input. Reading its bytes in-process with no `unsafe` and no
//!    execution means there is nothing to sandbox — the sidecar design needs a security review
//!    first, and this one does not.
//!
//! What is given up is statement-level reconstruction, and that is named rather than hidden:
//! see [`Fidelity`]. Everything the LLM stage actually consumes — signatures, the call graph,
//! literals, branch counts, I/O boundaries — is in the metadata and bytecode.
//!
//! # Failure model
//!
//! **Degrade per item, never per file.** This is the crate's central constraint, and the reason
//! the .NET reader is written by hand: `dotscope` 0.9.1 aborts the entire assembly on one
//! malformed custom-attribute blob and so returned nothing for 70 of 147 real Mono assemblies.
//! That is the same all-or-nothing parse failure that silently cost LedgerSMB its whole schema
//! under RFC 0146, and an analyzer that returns zero facts for half a customer's estate without
//! an error they will notice is worse than one that does not exist.
//!
//! So: every offset read is bounds-checked, nothing panics on hostile input, and a problem
//! becomes an [`AstDiagnostic`] on the AST while the rest of the file is still read.

#![forbid(unsafe_code)]

pub mod ast;
pub mod detect;
pub mod dotnet;
pub mod io_classify;
pub mod jvm;

pub use ast::*;
pub use detect::{Detected, detect};

use thiserror::Error;

/// Hard caps applied to every container and file, so a zip bomb or a corrupt length field
/// cannot make the observer allocate without bound. These are guards, not tuning knobs: a real
/// binary never approaches them.
pub mod limits {
    /// Largest file (or archive) read at all.
    pub const MAX_FILE_BYTES: usize = 512 * 1024 * 1024;
    /// Largest single class file, loose or as an archive member.
    pub const MAX_MEMBER_BYTES: usize = 32 * 1024 * 1024;
    /// Most archive members inspected. Fat jars reach five figures legitimately; beyond this the
    /// remainder is skipped with a diagnostic rather than silently.
    pub const MAX_ARCHIVE_MEMBERS: usize = 50_000;
    /// No nested-archive recursion: a jar inside a jar is not opened. Recursion is the mechanism
    /// every zip bomb relies on, and a nested jar is a dependency — something the workspace
    /// should observe in its own right, not through its container.
    pub const ARCHIVE_RECURSION: usize = 0;
}

#[derive(Debug, Error)]
pub enum BinaryError {
    #[error("not a format this crate reads: {0}")]
    UnsupportedFormat(String),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("container error: {0}")]
    Container(String),
    #[error("{path}: {size} bytes exceeds the {limit} byte limit")]
    TooLarge {
        path: String,
        size: usize,
        limit: usize,
    },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, BinaryError>;

/// Read any supported binary from its bytes, dispatching on magic bytes.
///
/// Returns one AST per type-bearing unit: a single-element `Vec` for a loose class file or an
/// assembly, and one element per class for an archive. A native PE or an unrecognized file
/// returns [`BinaryError::UnsupportedFormat`] — the caller decides whether that is worth
/// reporting, since skipping native DLLs is normal and expected.
pub fn read(bytes: &[u8], path: &str, sha256: &str) -> Result<ReadResult> {
    match detect(bytes) {
        Detected::ClassFile => Ok(ReadResult {
            asts: vec![jvm::read_class(bytes, path, sha256)?],
            diagnostics: Vec::new(),
        }),
        Detected::Archive => {
            let read = jvm::read_archive(bytes, path, sha256)?;
            if read.asts.is_empty() && read.diagnostics.is_empty() {
                // A zip with no class members: a source archive, a resource bundle, a docx. Not
                // an error and not a jar — just not ours.
                return Err(BinaryError::UnsupportedFormat(format!(
                    "{path}: zip container with no .class members"
                )));
            }
            Ok(ReadResult {
                asts: read.asts,
                diagnostics: read.diagnostics,
            })
        }
        Detected::ManagedPe => {
            // One artifact per type, matching the JVM backend — see `split_by_type` for the 34 MB
            // artifact that made this non-negotiable.
            let (asts, diagnostics) = dotnet::read_assembly(bytes, path, sha256)?.split_by_type();
            Ok(ReadResult { asts, diagnostics })
        }
        Detected::NativePe => Err(BinaryError::UnsupportedFormat(format!(
            "{path}: native PE image, no CLI metadata"
        ))),
        Detected::Unknown => Err(BinaryError::UnsupportedFormat(format!(
            "{path}: not a class file, jar or managed assembly"
        ))),
    }
}

/// What [`read`] produced.
#[derive(Debug, Default)]
pub struct ReadResult {
    pub asts: Vec<DecompiledAst>,
    /// Container-level diagnostics that belong to no single AST.
    pub diagnostics: Vec<AstDiagnostic>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_unsupported_not_a_panic() {
        let err = read(b"hello world", "notes.txt", "d").unwrap_err();
        assert!(matches!(err, BinaryError::UnsupportedFormat(_)));
    }

    #[test]
    fn an_empty_file_is_unsupported() {
        assert!(matches!(
            read(&[], "empty.dll", "d").unwrap_err(),
            BinaryError::UnsupportedFormat(_)
        ));
    }

    /// Skipping a native DLL must be distinguishable from finding an empty assembly — the whole
    /// reason `Detected` separates the two.
    #[test]
    fn a_native_pe_reports_that_it_is_native() {
        let mut b = vec![0u8; 0x400];
        b[0] = b'M';
        b[1] = b'Z';
        b[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        b[0x80..0x84].copy_from_slice(b"PE\0\0");
        b[0x80 + 4 + 16..0x80 + 4 + 18].copy_from_slice(&224u16.to_le_bytes());
        b[0x98..0x9A].copy_from_slice(&0x10Bu16.to_le_bytes());
        let dir_off = 0x98 + 96;
        b[dir_off - 4..dir_off].copy_from_slice(&16u32.to_le_bytes());
        let err = read(&b, "native.dll", "d").unwrap_err();
        assert!(
            err.to_string().contains("native PE"),
            "expected a native-PE message, got: {err}"
        );
    }
}
