# Architecture

## Architecture Summary

_Executive Overview (RFC 0068 §14) — only fields EKOS can back with real compiled evidence are populated; fields the standard names but nothing here computes yet say so explicitly rather than being silently omitted or guessed at._

**Components:** 44865 compiled object(s) across 10 kind(s)

**Containers (crates):** 0

**Primary technologies:** _none compiled_

**Open questions:** 0

**Purpose:** _not yet computed — no real EKOS source for a project's stated purpose today (RFC 0068 §14)_

**Architecture style:** _not yet computed — requires reasoning EKOS doesn't perform yet_

**Major risks:** 'mscorlib' has 12 real compiled dependent(s); 'System.Windows.Forms' has 8 real compiled dependent(s); 'System' has 6 real compiled dependent(s); 'System.Drawing' has 6 real compiled dependent(s); 'System.Data' has 5 real compiled dependent(s); 'System.Xml' has 4 real compiled dependent(s) _(Observed, RFC 0068 §29/RFC 0094 — see each risk's own evidence)_

**Architecture confidence:** _not meaningfully computed — no Crate/Claim/ArchitectureGap objects exist for this project (this dimension is Rust-workspace-specific today, RFC 0065 Phase 3 v1 scope)_

## System Context

_C4 System Context (RFC 0068 §15) — the compiled workspace as one system, and the real external technologies it depends on. One level broader than the Container view below; only technologies with a real compiled dependency edge are shown, not every `Technology` object that happens to exist._

_No external technology dependencies compiled._


## System Decomposition

_C4 Container-level decomposition (RFC 0068 §16/§68), one level inside System Context above — real Backend/Frontend/Database layers, grouped from each compiled `File`/`Table` object's own path or `source_system` (RFC 0056/0083), never guessed. A path can be routed to a specific layer via `[[architecture.system-decomposition.overrides]]` in `ekos.toml` when the convention gets a project's layout wrong._

_No Backend, Frontend, or Database layer data compiled yet._


## Components

- **BinaryAssembly**: 25
- **BinaryField**: 7134
- **BinaryMethod**: 8327
- **BinaryType**: 931
- **Document**: 3
- **ExternalIoBoundary**: 174
- **File**: 31
- **Risk**: 6
- **Section**: 4
- **TransformNode**: 28230

## Subsystems

_Deterministic rollups (RFC 0044) — one per directory/project group with ≥2 member files, zero LLM. Each links to a detail page with real member counts and boundary relationships, so a subsystem can be understood without walking every file inside it._

_No subsystem rollups compiled._

## Crate & Workspace Topology

_C4 mapping (RFC 0065 §23): each crate below is a C4 **Container** — the natural deployable/buildable unit in a Rust workspace — and each entry under Technologies is a C4 **External System** the workspace depends on but doesn't own._

_No crate/workspace manifests or subsystem rollups compiled._

## Component View

_C4 Component (RFC 0068 §18) — one level inside a Container. Each `Crate` below whose manifest directory matches a compiled `Rollup` (RFC 0044) links through to that subsystem's real member-file breakdown; a crate with no matching rollup either has too few member files to summarize (RFC 0044's own ≥2-member threshold) or none were compiled — not fabricated either way._

_No crate/workspace manifests or subsystem rollups compiled._

## Technology Inventory

_C4 External System-level dependencies (RFC 0068 §61's Technology Inventory), each linked to its own detail page where one was compiled._

_No technology dependencies compiled._

## Runtime View

_Basic Runtime View (RFC 0068 §20) — real behavior, not structure. `SequenceDiagrams.md` (generated alongside this page) already renders every real compiled call/data-flow sequence (RFC 0041's `Calls` graph, RFC 0027's Transformation IR); this section links through rather than duplicating it. Naming *which* of those are the system's important business scenarios (RFC 0068's own examples: "Create Order", "Process Payment") needs either an LLM read of real intent or human curation — neither happens in this deterministic view, so no scenario names are invented here._

See [SequenceDiagrams.md](SequenceDiagrams.md) for the real compiled sequences.

## Data Architecture

_RFC 0068 §22 ("A major EKOS capability") — real compiled data stores and real compiled transformations/lineage; domain grouping, ownership, lifecycle, and data quality each say explicitly why they're not computed yet rather than being guessed at._

### Data Stores

_No compiled data stores (Tables/Datasets)._

### Transformations & Lineage

See [SequenceDiagrams.md](SequenceDiagrams.md) for real compiled data-flow sequences (RFC 0027 Transformation IR). None of this workspace's `TransformNode` source/sink names matched exactly one compiled table (RFC 0075) — either no name overlaps a compiled `Table`, or every overlapping name is ambiguous across two or more tables, so nothing was linked rather than guessed at.

### Data Domains

_not yet computed — no compiled data stores to derive a domain from._

### Ownership

_not yet computed for data objects — `OwnedBy` edges are compiled from git history (`git_analyzer.rs`), but only from a commit event to the contributor who authored it, never onto a `File`/`Table`/`Dataset` object; there's no compiled per-file ownership signal today for a data store to link to, even setting aside that `Table`/`Dataset` objects also aren't yet linked to the `File` they were defined in. Two real gaps, not one: (1) `git_analyzer.rs` would need to derive a per-file top-contributor relationship, the way it already derives per-file `CoupledWith` coupling; (2) a data store would need the same kind of name/evidence-path linkage RFC 0075 just built for `TransformNode`s, but against `File` objects instead._

### Lifecycle

_not yet computed — blocked on the same missing `Table`→`File` link Ownership above is (a real last-modified/commit-recency signal already exists per file via git history, RFC 0020's coupling analysis touches the same commit data, but nothing connects a compiled data store to the file whose history that would be)._

### Data Quality

_not yet computed — no data-quality signal (completeness, freshness, validation-rule pass/fail) is extractable from static DDL/transformation-logic recovery at all; this needs runtime data profiling (row counts, null rates, constraint violations against actual data), which is explicitly RFC 0068 §63 Phase 3 scope (runtime telemetry), not yet built._

## Open Questions

_Explicit knowledge gaps (RFC 0065 §17) — not errors, and not silently dropped: each entry below is something a deterministic pass found it could not resolve, evidence-backed like everything else on this page. Unless resolved, they stay here rather than being guessed at._

_No open architecture questions compiled._

## CI/CD Pipelines

_No CI/CD pipeline definitions compiled._

## Entity Relationships

_No table foreign-key relationships compiled._

## Dependency Graph

### Calls

_7825 `Calls` relationships compiled — diagram omitted, too large to render usefully. First 15 shown below; every object's own detail page (linked) lists its full relationship set._

- [.ctor](entities/binarymethod/ct/ctor-b121ad77.md) → [get_Default](entities/binarymethod/ge/get-default.md)
- [Initialize](entities/binarymethod/in/initialize-b4a775d0.md) → [get_StandardDate](entities/binarymethod/ge/get-standarddate.md)
- [navCodeTB_KeyDown](entities/binarymethod/na/navcodetb-keydown-73398e09.md) → [get_DocType](entities/binarymethod/ge/get-doctype-cb092e53.md)
- [PulseEvent](entities/binarymethod/pu/pulseevent-e8e2ee2e.md) → [CEEventModify](entities/binarymethod/ce/ceeventmodify.md)
- [set_CalendarTitleBackColor](entities/binarymethod/se/set-calendartitlebackcolor.md) → [ToWin32](entities/binarymethod/to/towin32.md)
- [CreateNewReturnBox](entities/binarymethod/cr/createnewreturnbox.md) → [AddScannedRow](entities/binarymethod/ad/addscannedrow-ef73f7b4.md)
- [.ctor](entities/binarymethod/ct/ctor-3651d2bb.md) → [.ctor](entities/binarymethod/ct/ctor-b3ed9d40.md)
- [System.Collections.IEnumerable.GetEnumerator](entities/binarymethod/sy/system-collections-ienumerable-getenumerator-a78143bd.md) → [System.Collections.Generic.IEnumerable<System.Int32>.GetEnumerator](entities/binarymethod/sy/system-collections-generic-ienumerable-system-int32-getenumerator-4db6741f.md)
- [.ctor](entities/binarymethod/ct/ctor-30a82351.md) → [GetErrorMessage](entities/binarymethod/ge/geterrormessage.md)
- [InternalValuesFormat](entities/binarymethod/in/internalvaluesformat.md) → [GetValues](entities/binarymethod/ge/getvalues.md)
- [OpenScanned](entities/binarymethod/op/openscanned-4071d024.md) → [set_Barcode](entities/binarymethod/se/set-barcode-15584a8d.md)
- [MenuEvents](entities/binarymethod/me/menuevents-b9ed731f.md) → [get_Action](entities/binarymethod/ge/get-action-8d266d67.md)
- [get_VirtualScreen](entities/binarymethod/ge/get-virtualscreen.md) → [GetSystemMetrics](entities/binarymethod/ge/getsystemmetrics.md)
- [get_FactQuantity](entities/binarymethod/ge/get-factquantity-2fb2fa86.md) → [get_FactQuantityColumn](entities/binarymethod/ge/get-factquantitycolumn-96729cd8.md)
- [InitClass](entities/binarymethod/in/initclass-2c7a7b50.md) → [.ctor](entities/binarymethod/ct/ctor-0b724093.md)

### Contains

_44583 `Contains` relationships compiled — diagram omitted, too large to render usefully. First 15 shown below; every object's own detail page (linked) lists its full relationship set._

- [AddRange](entities/binarymethod/ad/addrange-68406f1a.md) → Release/TSDServer.exe:0x0600021A:15
- [.ctor](entities/binarymethod/ct/ctor-212e4941.md) → Release/MoFlinkLibNet.dll:0x06000048:0
- [TSDServer.ProductsDataSet+ProductsBinTblDataTable](entities/binarytype/ts/tsdserver-productsdataset-productsbintbldatatable.md) → [get_MessageColumn](entities/binarymethod/ge/get-messagecolumn-82121bc9.md)
- [OpenNETCF.IO.Serial.GPS.Position](entities/binarytype/op/opennetcf-io-serial-gps-position.md) → [set_Longitude_Fractional](entities/binarymethod/se/set-longitude-fractional.md)
- [OnPaint](entities/binarymethod/on/onpaint-714d1958.md) → Release/OpenNETCF.Windows.Forms.dll:0x06000203:12
- [ConnToPrinter](entities/binarymethod/co/conntoprinter-84fd6147.md) → Release/TSDClient - ª®¯¨ï.exe:0x06000249:16
- [OpenNETCF.Diagnostics.ProcessStartInfo](entities/binarytype/op/opennetcf-diagnostics-processstartinfo.md) → [set_WindowStyle](entities/binarymethod/se/set-windowstyle.md)
- [.ctor](entities/binarymethod/ct/ctor-373dcbe5.md) → Release/OpenNETCF.Windows.Forms.dll:0x06000188:5
- [RefreshData](entities/binarymethod/re/refreshdata-63352073.md) → Release/TSDClient.exe:0x06000443:64
- [ExportParameters](entities/binarymethod/ex/exportparameters-a6c29cc6.md) → Release/OpenNETCF.dll:0x060009BA:20
- [MoveNext](entities/binarymethod/mo/movenext-907b84f4.md) → Release/TSDServer.exe:0x06000324:83
- [InitializeComponent](entities/binarymethod/in/initializecomponent-f9bdc3b6.md) → Release/TSDClient.exe:0x060001D6:47
- [InitializeComponent](entities/binarymethod/in/initializecomponent-150f036c.md) → Release/TSDClient.exe:0x0600043E:48
- [InitClass](entities/binarymethod/in/initclass-2769fb2b.md) → Release/TSDServer.exe:0x0600011B:11
- [TSDServer.ProductsDataSet+DocsBinTbl1DataTable](entities/binarytype/ts/tsdserver-productsdataset-docsbintbl1datatable.md) → [get_PriorityColumn](entities/binarymethod/ge/get-prioritycolumn-66e35418.md)

### DependsOn

_60 `DependsOn` relationships compiled — diagram omitted, too large to render usefully. First 15 shown below; every object's own detail page (linked) lists its full relationship set._

- [OpenNETCF](entities/binaryassembly/op/opennetcf.md) → [Microsoft.WindowsCE.Forms](entities/binaryassembly/mi/microsoft-windowsce-forms.md)
- [ControlsLib](entities/binaryassembly/co/controlslib.md) → [System.Drawing](entities/binaryassembly/sy/system-drawing.md)
- [OpenNETCF.Windows.Forms](entities/binaryassembly/op/opennetcf-windows-forms.md) → [System.Drawing](entities/binaryassembly/sy/system-drawing.md)
- [TSDClient](entities/binaryassembly/ts/tsdclient.md) → [BluetoothLibNet](entities/binaryassembly/bl/bluetoothlibnet.md)
- [OpenNETCF.Windows.Forms](entities/binaryassembly/op/opennetcf-windows-forms.md) → [System](entities/binaryassembly/sy/system.md)
- [ControlsLib](entities/binaryassembly/co/controlslib.md) → [TsdCommandsLib](entities/binaryassembly/ts/tsdcommandslib.md)
- [TSDServer](entities/binaryassembly/ts/tsdserver.md) → [System.Drawing](entities/binaryassembly/sy/system-drawing.md)
- [TSDClient](entities/binaryassembly/ts/tsdclient.md) → [SystemLibnet](entities/binaryassembly/sy/systemlibnet.md)
- [OpenNETCF.Drawing](entities/binaryassembly/op/opennetcf-drawing.md) → [System.Windows.Forms](entities/binaryassembly/sy/system-windows-forms.md)
- [TSDClient](entities/binaryassembly/ts/tsdclient.md) → [mscorlib](entities/binaryassembly/ms/mscorlib.md)
- [SystemLibnet](entities/binaryassembly/sy/systemlibnet.md) → [mscorlib](entities/binaryassembly/ms/mscorlib.md)
- [TSDClient](entities/binaryassembly/ts/tsdclient-9a9ae2f8.md) → [OBReadLibNet](entities/binaryassembly/ob/obreadlibnet.md)
- [TSDServer](entities/binaryassembly/ts/tsdserver.md) → [System.Data](entities/binaryassembly/sy/system-data.md)
- [OpenNETCF.Drawing](entities/binaryassembly/op/opennetcf-drawing.md) → [mscorlib](entities/binaryassembly/ms/mscorlib.md)
- [ControlsLib](entities/binaryassembly/co/controlslib.md) → [NetworkLib](entities/binaryassembly/ne/networklib.md)

### Extends

_443 `Extends` relationships compiled — diagram omitted, too large to render usefully. First 15 shown below; every object's own detail page (linked) lists its full relationship set._

- [FamilTsdDB.Index+<FindIndexes>d__c](entities/binarytype/fa/familtsddb-index-findindexes-d-c.md) → [System.IDisposable](entities/binarytype/sy/system-idisposable.md)
- [OpenNETCF.Windows.Forms.AnimateCtl](entities/binarytype/op/opennetcf-windows-forms-animatectl.md) → [OpenNETCF.Windows.Forms.IWin32Window](entities/binarytype/op/opennetcf-windows-forms-iwin32window.md)
- [TSDServer.FileCopyProgressForm](entities/binarytype/ts/tsdserver-filecopyprogressform.md) → [System.Windows.Forms.Form](entities/binarytype/sy/system-windows-forms-form.md)
- [TSDServer.ProductsDataSet+ProductsBinTblRow](entities/binarytype/ts/tsdserver-productsdataset-productsbintblrow.md) → [System.Data.DataRow](entities/binarytype/sy/system-data-datarow.md)
- [TSDServer.ProductsDataSet+DocsTblRow](entities/binarytype/ts/tsdserver-productsdataset-docstblrow.md) → [System.Data.DataRow](entities/binarytype/sy/system-data-datarow.md)
- [TSDServer.DialogForm](entities/binarytype/ts/tsdserver-dialogform.md) → [System.Windows.Forms.Form](entities/binarytype/sy/system-windows-forms-form.md)
- [TSDServer.SettingsDataSet+TypedSettingsRowChangeEvent](entities/binarytype/ts/tsdserver-settingsdataset-typedsettingsrowchangeevent-b606234c.md) → [System.EventArgs](entities/binarytype/sy/system-eventargs.md)
- [TSDServer.ScannedProductsDataSetTableAdapters.ScannedBarcodesTableAdapter](entities/binarytype/ts/tsdserver-scannedproductsdatasettableadapters-scannedbarcodestableadapter-f05ea158.md) → [TSDServer.BaseTableAdapter](entities/binarytype/ts/tsdserver-basetableadapter.md)
- [OpenNETCF.IO.Serial.WinCommAPI](entities/binarytype/op/opennetcf-io-serial-wincommapi.md) → [OpenNETCF.IO.Serial.CommAPI](entities/binarytype/op/opennetcf-io-serial-commapi.md)
- [TSDServer.ProductsDataSet+ProductsBinTblRowChangeEvent](entities/binarytype/ts/tsdserver-productsdataset-productsbintblrowchangeevent.md) → [System.EventArgs](entities/binarytype/sy/system-eventargs.md)
- [OpenNETCF.IO.StreamInterfaceDriver](entities/binarytype/op/opennetcf-io-streaminterfacedriver.md) → [System.IDisposable](entities/binarytype/sy/system-idisposable.md)
- [TSDServer.ScannedProductsDataSet+ScannedBarcodesDataTable](entities/binarytype/ts/tsdserver-scannedproductsdataset-scannedbarcodesdatatable-cbb102b1.md) → [System.Data.DataTable](entities/binarytype/sy/system-data-datatable.md)
- [OpenNETCF.Security.Cryptography.DSASignatureDeformatter](entities/binarytype/op/opennetcf-security-cryptography-dsasignaturedeformatter.md) → [OpenNETCF.Security.Cryptography.AsymmetricSignatureDeformatter](entities/binarytype/op/opennetcf-security-cryptography-asymmetricsignaturedeformatter.md)
- [FamilTsdDB.DataRowItem](entities/binarytype/fa/familtsddb-datarowitem-e46cae4a.md) → [System.IComparable](entities/binarytype/sy/system-icomparable.md)
- [TSDServer.ProductsDataSet](entities/binarytype/ts/tsdserver-productsdataset-b7588ff0.md) → [System.Data.DataSet](entities/binarytype/sy/system-data-dataset.md)

### References

_180 `References` relationships compiled — diagram omitted, too large to render usefully. First 15 shown below; every object's own detail page (linked) lists its full relationship set._

- [TestFileArray](entities/binarymethod/te/testfilearray.md) → System.IO.FileSystemInfo.Refresh
- [TestFileArray](entities/binarymethod/te/testfilearray-9693047a.md) → System.IO.FileSystemInfo.Refresh
- [UploadResults](entities/binarymethod/up/uploadresults.md) → System.IO.File.Exists
- [SetErrorEvent](entities/binarymethod/se/seterrorevent-18c26517.md) → System.IO.StreamWriter..ctor
- [FindFirstByPartIndexes](entities/binarymethod/fi/findfirstbypartindexes-99e636c5.md) → System.IO.FileStream..ctor
- [CheckForClear](entities/binarymethod/ch/checkforclear.md) → System.IO.FileInfo.get_Length
- [Copy](entities/binarymethod/co/copy-ed076b30.md) → System.IO.File.Copy
- [ReadTableDef](entities/binarymethod/re/readtabledef.md) → System.IO.Directory.GetFiles
- [FindAllByPartIndexes](entities/binarymethod/fi/findallbypartindexes.md) → System.IO.FileStream..ctor
- [UploadResults](entities/binarymethod/up/uploadresults.md) → System.IO.File.Exists
- [FindByIndexDirectScan](entities/binarymethod/fi/findbyindexdirectscan-35db3aab.md) → System.IO.FileStream..ctor
- [ClearScannedData](entities/binarymethod/cl/clearscanneddata-c3e55bff.md) → System.IO.File.Exists
- [PlayVibro](entities/binarymethod/pl/playvibro.md) → System.IO.StreamReader..ctor
- [FindByIndex](entities/binarymethod/fi/findbyindex-cf24cc1a.md) → System.IO.FileStream..ctor
- [set_SoundLocation](entities/binarymethod/se/set-soundlocation.md) → System.IO.File.Exists

