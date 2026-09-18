# Devlog 190 — RFC 0148 on a real app: .NET call graph was empty, and three more joins were wrong

**Date:** 2026-09-18
**PRs:** (single working tree, uncommitted at time of writing)
**Branch:** `main` (local)

---

## Summary

The first real-world run of RFC 0148's binary recovery used a lost-source Windows CE handheld app,
`alexeyban/tsd` `SmartDeviceProject2/bin/Release`: 12 managed assemblies (a barcode-terminal client, its
desktop sync server, OpenNETCF and five vendor SDK wrappers). The run surfaced four defects that devlog_189's
verification could not have caught. Its only .NET input was `mscorlib` alone, and all 13,077 of its resolved
call edges came from `pdfbox.jar`.

1. **Every .NET call failed to join.** The run produced 36,107 call sites and **0** `Calls` edges.
2. `DependsOn`/`Extends` went to `external` stubs even when the target binary sat in the same folder.
3. The call join was keyed by name across the whole run, so a server and client built from shared source
   swapped calls: **320 of TSDServer's calls landed in TSDClient.exe**.
4. The I/O classifier labelled all of `System.Data.*`, including in-memory DataTable work, as database I/O.
   That produced **4,130 "boundaries" for an app with no database I/O**.

After the fixes the run produces 10,956 call edges. Every cross-binary edge matches a real `AssemblyRef`, and
the run reports 174 I/O boundaries, all genuine.

---

## Fix 1 — .NET call sites never matched their declarations

### Problem / motivation
`binary_analyzer` builds a `Calls` edge only when `owner.method descriptor` matches a declaration's key
character for character. The .NET reader broke that match in two ways:
- **`MethodDef` tokens got an empty owner.** Every intra-assembly call uses one. The code comment said the
  owner was "filled in by the caller's own type context downstream", but no code does that.
- **Call sites and declarations used different descriptor formats.** Call sites used `MethodSig::render()`
  (`(a, b) -> r`), while declarations used `render_descriptor()` (`instance a,b)r`). So even cross-assembly
  `MemberRef` calls could never match.

Neither problem raised an error. The only symptom was a low edge count, and RFC 0148 had already told readers
to expect low edge counts ("calls into code not compiled here").

### What was built
| Component | Change |
|---|---|
| `TypeNames::build` | Nesting-aware names for both `TypeDef` (`NestedClass`) and `TypeRef` (`ResolutionScope` → `TypeRef`), spelled `Ns.Outer+Inner` exactly as `DecompiledType::qualified_name` spells the declaration |
| `TypeNames::method_owners` / `field_owners` | `MethodDef`/`Field` row → declaring type, from the `TypeDef` list ranges |
| `resolve_method_token` | Real owner for `MethodDef`, and `render_descriptor` on both branches |
| `push_field` | Real owner for `Field` tokens (the same empty-owner bug) |
| `MemberRefParent` → `MethodDef` | Resolved to the method's owner instead of `"?"` (vararg call sites) |

**Measured:** on wine-mono's mscorlib, **69,971 of 69,971** intra-assembly calls now join.
`call_sites_join_to_their_declarations` asserts ≥ 99% of them.

---

## Fix 2 — references resolve onto binaries observed in the same run

### Problem / motivation
`add_external` materialized a `BinaryAssembly`/`BinaryType` stub for every reference, whether or not this run
had compiled the target. As a result, `TSDClient → BluetoothLibNet` pointed at an empty stub rather than
`bluetoothlibnet.dll`, which was observed alongside it, and "what depends on bluetoothlibnet.dll" returned
nothing.

### What was built
`DependsOn`/`Extends` are now deferred in the same way `Calls` already was (`PendingRef`) and resolved once
every artifact has been read. Only a name that no binary in the run defines gets a stub. `Extends` prefers a
type declared in the referrer's own binary. On TSD this removed 6 assembly stubs and 39 type stubs.

### Decisions
The RFC 0148 rationale for name-keyed stubs is unchanged: a binary observed in a *later* run is still a
separate, richer object. The change applies only within one run, which is the rule `Calls` already followed.

---

## Fix 3 — calls stay inside the assembly their metadata names

### Problem / motivation
`TSDClient.exe` declares its types in the `TSDServer` namespace. Both executables compile the same
`ProductsDataSet`/`FamilTsdDB` source, and the folder also holds two copies of TSDClient (`TSDClient.exe`,
`TSDClient - копия.exe`). A run-wide first-wins `method_index` therefore sent calls into whichever binary was
read first:

| Phantom cross-binary edges (before) | |
|---|---|
| TSDClient.exe → TSDClient (copy) | 817 |
| TSDClient (copy) → TSDClient | 631 |
| TSDServer.exe → TSDClient | 320 |
| TSDClient.exe → TSDServer | 180 |
| TSDClient (copy) → TSDServer | 157 |

None of these binaries references another in that table.

### What was built
- `CallSite::target_assembly: Option<String>`, which is `#[serde(default)]` so the AST stays
  backward-compatible. .NET knows the target exactly: a `MethodDef` is always this assembly, and a
  `TypeRef` names its `AssemblyRef` (walked through nested `TypeRef` scopes; `Module`/`ModuleRef` mean
  this assembly). The JVM has no equivalent, so it stays `None`.
- The analyzer now keeps three indexes: per binary, per assembly name, and run-wide.
  - A call that names its own assembly joins within the caller's own binary.
  - A call that names another assembly joins within that assembly.
  - A call with `None` (JVM) tries the caller's own binary first, then the run.

After the fix, every cross-binary edge on TSD matches a declared reference. The mutation check was run:
reverting the join to the run-wide index fails
`calls_stay_inside_the_assembly_their_metadata_names` with `left: "Server.exe"`.

---

## Fix 4 — the I/O classifier stops claiming in-memory ADO.NET

### Problem / motivation
`io_classify`'s own module doc says it lists concrete types because a bare `java.io.` prefix would claim
`StringWriter` as file I/O. The CLR table broke that rule twice:
- `("System.Data.", Database)` claimed `DataTable.get_Columns`, `DataRow.set_Item`,
  `StrongTypingException..ctor` and similar calls. That is roughly 2,000 in-process calls per TSD executable.
- `("System.IO.Path", File)` claimed `Path.Combine`, which is string manipulation.

### What was built
The table now lists providers (`SqlClient`, `SqlServerCe`, `OleDb`, `Odbc`, `OracleClient`, `SQLite`,
`EntityClient`, `Entity`, `Linq`) and connection/command abstractions (`Common.Db*`, `IDb*`,
`IDataReader`). `System.IO.Path` is removed. TSD went from **4,130 → 174** I/O boundaries.

### Known gap, left deliberately
`DataSet.ReadXml(path)`/`WriteXml(path)` *are* file I/O, and they are exactly how TSD persists its data. The
classifier is owner-prefix only and cannot distinguish `DataSet.ReadXml` from `DataSet.Tables`. Method-level
patterns would be the fix, but that is a table-shape change and belongs in its own change.

---

## Knowledge Captured

- **A verification corpus with one binary per format cannot test a cross-binary join.** Devlog_189's
  headline call-edge count was real, but it came entirely from the JVM backend. A bug that zeroes one
  backend's joins hides behind the other backend's numbers. For any multi-format feature, report the
  metrics per format.
- **A string-keyed join fails silently in both directions.** A mismatch produces zero edges and no error
  (Fix 1). An over-broad key produces confident, wrong edges and no error (Fix 3). The test that catches
  both states "every call site into a declared type joins to a declaration *in its own assembly*". Asserting
  "the edge count is high" catches neither.
- **Real `bin/Release` folders contain duplicates**: `.ex_` renamed backups, `- Copy.exe` files, and a
  server and client compiled from the same source. Any join or merge over binaries must assume the same
  qualified name appears in several binaries.
- **A code comment claiming "filled in downstream" is a claim to verify.** Nothing filled the owner in.
- **Bump both version stamps when reader output changes:**
  - `PIPELINE_LOGIC_VERSION` → 3 (otherwise `build`'s fingerprint cache keeps serving stale ASTs).
  - `EXTRACTOR` → `ekos-cil-metadata/v2` (facts record which reader produced them).
  - `BinaryAnalyzerPass::version` → `v2`.
- **`commit` is still the dominant cost:** about 15 minutes for 45–52k objects / 74–86k relationships at
  roughly 100–500 appends/s. This matches RFC 0142. The binary recovery itself takes 5 s. The session had to
  rerun the pipeline four times because each fix invalidated the previous commit. When iterating on
  analyzer output, inspect `.ekos/ckm/model.json.zst` after `compile` (about 10 s) and commit only at the
  end.
- The remaining `resolve` conflict (`System` assembly stub vs. a field named `system`) is a lowercasing
  coincidence. `BinaryAssembly` is deliberately outside `is_expected_binary_declaration_group`, so it still
  surfaces. It was run with `--force`, and the policy was not changed.

---

## Follow-up — TSD documentation built from EKOS output only

The same session produced `docs/presentations/tsd-documentation.html` (18-section system documentation)
and `docs/presentations/tsd-how-it-works.html` (12-slide deck). Every claim in them comes from the ledger
through EKOS's runtime tools, with no decompiler, source code or LLM involved:
- `ekos_binary_explain` over all 931 types, harvested over one MCP stdio session in 155 s;
- `ekos_neighborhood` for enum members;
- `ekos_impact`/`ekos_dependents` for reachability;
- `ekos_state` for observed file excerpts.

Evidence copies are in `docs/presentations/examples/tsd/`. Writing the documentation surfaced two more things
worth knowing:

- **`BinaryMethod.call_targets` is silently capped at 32 entries.** It shares `MAX_LITERALS_PER_METHOD` via
  `capped()`, and unlike the literal lists it records no truncation marker. `Program.Main` in TSDClient has
  more than 32 targets, so its property never mentions `MainForm` or `Application.Run`. A "who calls X"
  check built on `call_targets` produced **false negatives and false positives** (for example, it claimed
  `StorageMemorySize` was never read and `ViewDocsForm` was unreachable). **Answer reachability questions
  from resolved `Calls` edges (`ekos_impact`, filtered to `hop == 1` for direct callers), never from
  `call_targets`.** Filed as a TODO; not fixed here.
- **`ekos_impact`'s depth argument is `max_hops` (default 5), and an unknown `max_depth` is ignored
  without an error.** The first reachability pass silently ran 5 hops deep, and its "direct callers" were
  really transitive ones. Check a tool's schema, not its name, before trusting a bound.
- The graph-verified findings worth keeping: `ReturnBoxForm` is never constructed; `GetDeviceID`, the
  server's `Compressor` and `DocsBinTbl`/`DocsBinTbl1` are never called; and 6 of the 19 terminal
  settings have no reader. The unread settings include both feedback switches and the SQL CE connection
  string.

---

## Files Changed
| File | Change summary |
|---|---|
| `ekos/crates/binary/src/dotnet/mod.rs` | Nesting-aware `TypeDef`/`TypeRef` names; method/field owner maps; `TypeRef` → assembly; one descriptor format; `target_assembly` on call sites; `EXTRACTOR` v2; real-mscorlib join test |
| `ekos/crates/binary/src/dotnet/metadata.rs` | `MODULE_REF` table id, `C_RESOLUTION_SCOPE` export |
| `ekos/crates/binary/src/ast.rs` | `CallSite::target_assembly` |
| `ekos/crates/binary/src/jvm.rs` | `target_assembly: None` |
| `ekos/crates/binary/src/io_classify.rs` | Concrete CLR database types; drop `System.IO.Path`; 2 tests |
| `ekos/crates/recovery/src/binary_analyzer.rs` | Deferred `DependsOn`/`Extends` resolution; per-binary/per-assembly call indexes; pass `v2`; 2 pass-level tests |
| `ekos/crates/common/src/lib.rs` | `PIPELINE_LOGIC_VERSION` 3 |
| `ekos/crates/cli/src/commands/mcp.rs`, `ekos/crates/recovery/src/binary_reconstruction.rs` | Extractor name in docs |
| `docs/presentations/tsd-documentation.html` | New: 18-section TSD system documentation, every claim chip-tagged by provenance |
| `docs/presentations/tsd-how-it-works.html` | New: 12-slide deck on how TSD works |
| `docs/presentations/examples/tsd/**` | EKOS-generated reports + `ekos_binary_explain` JSON for 22 key types + one `ekos_impact` result |
| `docs/presentations.html` | Index entry for the deck and documentation |
