//! RFC 0209 §6 — the application model of the iDempiere / ADempiere / Compiere family, read from
//! its generated Java classes.
//!
//! These ERPs generate one interface `I_<Table>` and one class `X_<Table>` per table of their
//! Application Dictionary, and hand-write a business class `M<Name> extends X_<Table>`. The
//! generated pair restates the dictionary as code: table identity, every column, the business name
//! and description the dictionary gives it, its list values with labels, which columns are
//! mandatory and with what default, which are not updatable, and which table a column references.
//! Reading them gives the data model's meaning with no database connection and no model call.
//!
//! The trigger is narrow: an interface declaring both `Table_Name` and `Table_ID`, or a class
//! annotated `@…Model(table = "…")`. Other Java estates never meet it.
//!
//! What comes out is a **claim by code** ([`ModelTable`]). It is never merged into a database
//! `Table`; the catalog is the authority, and RFC 0211 compares the two.

use crate::java_syntax::{
    FieldDecl, JavaFile, MethodDecl, TokKind, Token, TypeDecl, TypeKind, doc_description,
    doc_params,
};
use std::collections::{BTreeMap, HashMap};

/// The application family this recognizer reads. Part of a `ModelTable`'s identity.
pub const FAMILY: &str = "idempiere";

/// `DocAction` methods: the document lifecycle entry points a business class may implement.
pub const LIFECYCLE_METHODS: &[&str] = &[
    "prepareIt",
    "approveIt",
    "rejectIt",
    "completeIt",
    "voidIt",
    "closeIt",
    "reverseCorrectIt",
    "reverseAccrualIt",
    "reActivateIt",
    "invalidateIt",
    "unlockIt",
];

/// One column as the generated code describes it. Absent facts stay `None`, never guessed.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct ModelColumn {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub java_type: Option<String>,
    /// A `boolean` accessor: in the database this is a `CHAR(1)` holding `Y`/`N`.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub boolean: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub business_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Listed in the generated constructor's new-record block.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub mandatory: bool,
    /// The default written beside it in that block (`@#Date@`, `CO`, `N`), as written.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// `false` when the setter uses `set_ValueNoCheck` — the dictionary marks it not updatable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updatable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference_id: Option<i64>,
    /// List values: code → label.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub values: BTreeMap<String, String>,
    /// The table a code-declared logical foreign key points at.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub references: Option<String>,
    /// `path:line` of the declaration this column was read from.
    pub source: String,
}

/// One table of the application model.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelTable {
    pub table_name: String,
    pub table_id: Option<i64>,
    pub access_level: Option<String>,
    pub columns: BTreeMap<String, ModelColumn>,
    /// Fully qualified names of the generated interface and class, and the business classes.
    pub interface: Option<String>,
    pub model_class: Option<String>,
    pub business_classes: Vec<String>,
    /// `(path, line)` of the generated class header (else the interface's).
    pub evidence: Option<(String, u32)>,
}

/// A parsed file and where it came from.
pub struct SourceFile<'a> {
    pub path: &'a str,
    pub file: &'a JavaFile,
}

/// A business class implementing the document lifecycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lifecycle {
    pub class: String,
    pub table_name: String,
    /// `(method, start_line, end_line)`, in [`LIFECYCLE_METHODS`] order.
    pub methods: Vec<(String, u32, u32)>,
}

#[derive(Debug, Default)]
pub struct AppModel {
    /// By table name.
    pub tables: BTreeMap<String, ModelTable>,
    pub lifecycles: Vec<Lifecycle>,
}

/// The value of a constant initialiser made only of literals joined by `+`: strings concatenate,
/// a lone number or char is kept as written. `None` for anything else.
pub fn literal_value(tokens: &[Token], (from, to): (usize, usize)) -> Option<String> {
    let parts = &tokens[from..to.min(tokens.len())];
    if parts.len() == 1 {
        let t = &parts[0];
        return match t.kind {
            TokKind::Str | TokKind::Num | TokKind::Char => Some(t.text.clone()),
            TokKind::Ident if t.text == "true" || t.text == "false" => Some(t.text.clone()),
            _ => None,
        };
    }
    if parts.len() == 2 && parts[0].is("-") && parts[1].kind == TokKind::Num {
        return Some(format!("-{}", parts[1].text));
    }
    let mut out = String::new();
    for (i, t) in parts.iter().enumerate() {
        if i % 2 == 1 {
            if !t.is("+") {
                return None;
            }
        } else if t.kind == TokKind::Str {
            out.push_str(&t.text);
        } else {
            return None;
        }
    }
    (!parts.is_empty() && parts.len() % 2 == 1).then_some(out)
}

fn field<'a>(t: &'a TypeDecl, name: &str) -> Option<&'a FieldDecl> {
    t.fields.iter().find(|f| f.name == name)
}

fn field_literal(file: &JavaFile, t: &TypeDecl, name: &str) -> Option<String> {
    field(t, name)?
        .init
        .and_then(|r| literal_value(&file.tokens, r))
}

/// `Set Grand Total.` / `Get Grand Total.` → `Grand Total`.
fn business_name(doc_line: &str) -> Option<String> {
    let rest = doc_line
        .strip_prefix("Set ")
        .or_else(|| doc_line.strip_prefix("Get "))?;
    let name = rest.trim().trim_end_matches('.').trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// The table an `I_<Table>` interface stands for, from its own `Table_Name`.
fn interface_table(file: &JavaFile, t: &TypeDecl) -> Option<String> {
    if t.kind != TypeKind::Interface {
        return None;
    }
    let name = field_literal(file, t, "Table_Name")?;
    field(t, "Table_ID")?;
    Some(name)
}

/// The table a generated class models, from `@…Model(table = "…")`.
fn class_table(t: &TypeDecl) -> Option<String> {
    if t.kind != TypeKind::Class {
        return None;
    }
    t.annotations
        .iter()
        .find(|a| a.simple_name() == "Model")
        .and_then(|a| a.string_arg("table"))
}

fn simple(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

/// Recognize the application model across all parsed files.
pub fn recognize(files: &[SourceFile<'_>]) -> AppModel {
    let mut model = AppModel::default();
    // Interface simple name → table name, for reference getters.
    let mut interface_tables: HashMap<String, String> = HashMap::new();
    // Generated class simple name → table name, for business classes.
    let mut class_tables: HashMap<String, String> = HashMap::new();

    for sf in files {
        for t in &sf.file.types {
            if let Some(table) = interface_table(sf.file, t) {
                interface_tables.insert(t.name.clone(), table.clone());
                read_interface(
                    sf,
                    t,
                    model.tables.entry(table.clone()).or_default(),
                    &table,
                );
            }
        }
    }
    for sf in files {
        for t in &sf.file.types {
            if let Some(table) = class_table(t) {
                class_tables.insert(t.name.clone(), table.clone());
                let mt = model.tables.entry(table.clone()).or_default();
                mt.table_name = table;
                read_class(sf, t, mt, &interface_tables);
            }
        }
    }
    for sf in files {
        for t in &sf.file.types {
            if t.kind != TypeKind::Class || class_table(t).is_some() {
                continue;
            }
            let Some(table) = t
                .extends
                .first()
                .and_then(|e| class_tables.get(simple(e)))
                .cloned()
            else {
                continue;
            };
            if let Some(mt) = model.tables.get_mut(&table) {
                mt.business_classes.push(t.qualified.clone());
            }
            if t.implements.iter().any(|i| simple(i) == "DocAction") {
                let methods = LIFECYCLE_METHODS
                    .iter()
                    .filter_map(|name| {
                        t.methods
                            .iter()
                            .find(|m| m.name == *name && m.body.is_some())
                            .map(|m| (m.name.clone(), m.start_line, m.end_line))
                    })
                    .collect();
                model.lifecycles.push(Lifecycle {
                    class: t.qualified.clone(),
                    table_name: table,
                    methods,
                });
            }
        }
    }
    for mt in model.tables.values_mut() {
        mt.business_classes.sort();
        mt.business_classes.dedup();
    }
    model.lifecycles.sort_by(|a, b| a.class.cmp(&b.class));
    model
}

fn read_interface(sf: &SourceFile<'_>, t: &TypeDecl, mt: &mut ModelTable, table: &str) {
    mt.table_name = table.to_string();
    mt.interface = Some(t.qualified.clone());
    mt.table_id = field_literal(sf.file, t, "Table_ID").and_then(|v| v.parse().ok());
    if let Some(doc) = field(t, "accessLevel").and_then(|f| f.doc.as_deref()) {
        // `AccessLevel = 1 - Org`
        if let Some((_, level)) = doc_description(doc).split_once('=') {
            mt.access_level = Some(level.trim().to_string());
        }
    }
    if mt.evidence.is_none() {
        mt.evidence = Some((sf.path.to_string(), t.start_line));
    }
    for f in &t.fields {
        let Some(col) = f.name.strip_prefix("COLUMNNAME_") else {
            continue;
        };
        let name = f
            .init
            .and_then(|r| literal_value(&sf.file.tokens, r))
            .unwrap_or_else(|| col.to_string());
        let c = mt.columns.entry(name).or_default();
        c.source = format!("{}:{}", sf.path, f.line);
    }
    for m in &t.methods {
        if !m.params.is_empty() {
            continue;
        }
        let column = getter_column(&m.name, &mt.columns);
        let Some(column) = column else {
            // `I_C_BPartner getC_BPartner()` beside a `C_BPartner_ID` column: a logical FK.
            reference_from_getter(m, mt, None);
            continue;
        };
        let c = mt
            .columns
            .get_mut(&column)
            .expect("getter_column checks presence");
        if c.java_type.is_none() {
            c.java_type = m.return_type.clone();
        }
        if m.return_type.as_deref() == Some("boolean") {
            c.boolean = true;
        }
        if let Some(doc) = m.doc.as_deref() {
            let mut lines = doc.lines().filter(|l| !l.is_empty());
            if c.business_name.is_none() {
                c.business_name = lines.next().and_then(business_name);
            }
            if c.description.is_none() {
                c.description = lines
                    .next()
                    .filter(|l| !l.starts_with('@'))
                    .map(str::to_string);
            }
        }
    }
}

/// The column a zero-argument getter reads: `getGrandTotal` → `GrandTotal`, `isApproved` →
/// `IsApproved`. Only when that column exists.
fn getter_column(name: &str, columns: &BTreeMap<String, ModelColumn>) -> Option<String> {
    let candidate = if let Some(rest) = name.strip_prefix("get") {
        rest.to_string()
    } else {
        format!("Is{}", name.strip_prefix("is")?)
    };
    columns.contains_key(&candidate).then_some(candidate)
}

/// `I_Y getX()` with a column `X_ID`: column `X_ID` references `Y`'s table.
fn reference_from_getter(
    m: &MethodDecl,
    mt: &mut ModelTable,
    interface_tables: Option<&HashMap<String, String>>,
) {
    let Some(rest) = m.name.strip_prefix("get") else {
        return;
    };
    let Some(ret) = m.return_type.as_deref() else {
        return;
    };
    let iface = simple(ret);
    let Some(target_hint) = iface.strip_prefix("I_") else {
        return;
    };
    let column = format!("{rest}_ID");
    let Some(c) = mt.columns.get_mut(&column) else {
        return;
    };
    let target = interface_tables
        .and_then(|it| it.get(iface).cloned())
        .unwrap_or_else(|| target_hint.to_string());
    c.references = Some(target);
}

fn read_class(
    sf: &SourceFile<'_>,
    t: &TypeDecl,
    mt: &mut ModelTable,
    interface_tables: &HashMap<String, String>,
) {
    mt.model_class = Some(t.qualified.clone());
    mt.evidence = Some((sf.path.to_string(), t.start_line));
    read_list_values(sf, t, mt);
    for m in &t.methods {
        if let Some(column) = m.name.strip_prefix("set")
            && m.params.len() == 1
            && let Some(c) = mt.columns.get_mut(column)
        {
            read_setter(sf, m, c);
        } else if m.params.is_empty() {
            reference_from_getter(m, mt, Some(interface_tables));
        }
    }
    read_new_record_block(sf, t, mt);
}

/// List values: `X_AD_Reference_ID=131` names the column, `X_Label = "CO"` its values.
fn read_list_values(sf: &SourceFile<'_>, t: &TypeDecl, mt: &mut ModelTable) {
    let upper: HashMap<String, String> = mt
        .columns
        .keys()
        .map(|c| (c.to_ascii_uppercase(), c.clone()))
        .collect();
    let literal = |f: &FieldDecl| f.init.and_then(|r| literal_value(&sf.file.tokens, r));
    for f in &t.fields {
        if let Some(prefix) = f.name.strip_suffix("_AD_Reference_ID")
            && let Some(col) = upper.get(prefix)
            && let Some(id) = literal(f)
        {
            mt.columns.get_mut(col).expect("from keys").reference_id = id.parse().ok();
        }
    }
    for f in &t.fields {
        if f.name.ends_with("_AD_Reference_ID") || f.ty != "String" {
            continue;
        }
        let (Some((prefix, suffix)), Some(code)) =
            (longest_column_prefix(&f.name, &upper), literal(f))
        else {
            continue;
        };
        let label = f
            .doc
            .as_deref()
            .map(doc_description)
            .and_then(|d| d.rsplit_once(" = ").map(|(l, _)| l.trim().to_string()))
            .unwrap_or(suffix);
        let col = upper[&prefix].clone();
        mt.columns
            .get_mut(&col)
            .expect("from keys")
            .values
            .insert(code, label);
    }
}

/// A generated setter: business name and description from its Javadoc, `boolean` from its
/// parameter, updatability from `set_ValueNoCheck` versus `set_Value`.
fn read_setter(sf: &SourceFile<'_>, m: &MethodDecl, c: &mut ModelColumn) {
    if m.params[0].ty == "boolean" {
        c.boolean = true;
    }
    if let Some(doc) = m.doc.as_deref() {
        if let Some(name) = doc.lines().next().and_then(business_name) {
            c.business_name = Some(name);
        }
        if let Some((_, text)) = doc_params(doc).into_iter().next()
            && !text.is_empty()
        {
            c.description = Some(text);
        }
    }
    if let Some((open, close)) = m.body {
        let body = &sf.file.tokens[open..close];
        if body.iter().any(|t| t.is("set_ValueNoCheck")) {
            c.updatable = Some(false);
        } else if body.iter().any(|t| t.is("set_Value")) {
            c.updatable = Some(true);
        }
    }
    c.source = format!("{}:{}", sf.path, m.start_line);
}

/// The new-record block the generator leaves commented out in the first constructor: mandatory
/// columns, each with the default written beside it.
fn read_new_record_block(sf: &SourceFile<'_>, t: &TypeDecl, mt: &mut ModelTable) {
    let Some(ctor) = t
        .methods
        .iter()
        .find(|m| m.kind == crate::java_syntax::MethodKind::Constructor)
    else {
        return;
    };
    let inside = sf
        .file
        .comments
        .iter()
        .filter(|c| c.start_line >= ctor.start_line && c.end_line <= ctor.end_line);
    for comment in inside {
        for (column, default) in new_record_block(&comment.text) {
            if let Some(c) = mt.columns.get_mut(&column) {
                c.mandatory = true;
                if default.is_some() {
                    c.default = default;
                }
            }
        }
    }
}

/// `DOCSTATUS_Completed` → (`DOCSTATUS`, `Completed`) when `DOCSTATUS` is a column, upper-cased.
/// The longest matching prefix wins, so `PAYMENTRULE_X` is not read as a column `PAYMENT`'s value.
fn longest_column_prefix(name: &str, upper: &HashMap<String, String>) -> Option<(String, String)> {
    let mut best: Option<(String, String)> = None;
    for (i, _) in name.match_indices('_') {
        let prefix = &name[..i];
        if upper.contains_key(prefix) {
            best = Some((prefix.to_string(), name[i + 1..].to_string()));
        }
    }
    best
}

/// `setC_Currency_ID (0);` lines of a commented new-record block, each with the `// default`
/// comment on the line after it, if any.
pub fn new_record_block(comment: &str) -> Vec<(String, Option<String>)> {
    if !comment.contains("if (") {
        return Vec::new();
    }
    let mut out: Vec<(String, Option<String>)> = Vec::new();
    for line in comment.lines() {
        let l = line.trim();
        if let Some(rest) = l.strip_prefix("//") {
            if let Some(last) = out.last_mut()
                && last.1.is_none()
            {
                last.1 = Some(rest.trim().to_string());
            }
            continue;
        }
        if let Some(rest) = l.strip_prefix("set")
            && let Some((column, _)) = rest.split_once('(')
        {
            let column = column.trim();
            if !column.is_empty() && column.chars().all(|c| c.is_alphanumeric() || c == '_') {
                out.push((column.to_string(), None));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::java_syntax::parse;

    const I_INVOICE: &str = r#"package org.compiere.model;
public interface I_C_Invoice
{
    /** TableName=C_Invoice */
    public static final String Table_Name = "C_Invoice";
    /** AD_Table_ID=318 */
    public static final int Table_ID = 318;
    /** AccessLevel = 1 - Org
     */
    BigDecimal accessLevel = BigDecimal.valueOf(1);
    /** Column name AD_Client_ID */
    public static final String COLUMNNAME_AD_Client_ID = "AD_Client_ID";
	/** Get Tenant.
	  * Tenant for this installation.
	  */
	public int getAD_Client_ID();
    public static final String COLUMNNAME_C_BPartner_ID = "C_BPartner_ID";
	public org.compiere.model.I_C_BPartner getC_BPartner() throws RuntimeException;
    public static final String COLUMNNAME_DocStatus = "DocStatus";
	public String getDocStatus();
    public static final String COLUMNNAME_GrandTotal = "GrandTotal";
	public BigDecimal getGrandTotal();
    public static final String COLUMNNAME_IsApproved = "IsApproved";
	public boolean isApproved();
}
"#;

    const X_INVOICE: &str = r#"/** Generated Model - DO NOT CHANGE */
package org.compiere.model;
/** Generated Model for C_Invoice */
@org.adempiere.base.Model(table="C_Invoice")
public class X_C_Invoice extends PO implements I_C_Invoice, I_Persistent
{
    public X_C_Invoice (Properties ctx, int C_Invoice_ID, String trxName)
    {
      super (ctx, C_Invoice_ID, trxName);
      /** if (C_Invoice_ID == 0)
        {
			setC_BPartner_ID (0);
			setDocStatus (null);
// DR
			setGrandTotal (Env.ZERO);
			setIsApproved (false);
// N
        } */
    }
	public org.compiere.model.I_C_BPartner getC_BPartner() throws RuntimeException
	{
		return (org.compiere.model.I_C_BPartner)MTable.get(getCtx(), org.compiere.model.I_C_BPartner.Table_ID)
			.getPO(getC_BPartner_ID(), get_TrxName());
	}
	/** Set Business Partner.
		@param C_BPartner_ID Identifies a Business Partner
	*/
	public void setC_BPartner_ID (int C_BPartner_ID)
	{
		set_Value (COLUMNNAME_C_BPartner_ID, Integer.valueOf(C_BPartner_ID));
	}
	/** DocStatus AD_Reference_ID=131 */
	public static final int DOCSTATUS_AD_Reference_ID=131;
	/** Completed = CO */
	public static final String DOCSTATUS_Completed = "CO";
	/** Drafted = DR */
	public static final String DOCSTATUS_Drafted = "DR";
	/** Set Document Status.
		@param DocStatus The current status of the document
	*/
	public void setDocStatus (String DocStatus)
	{
		set_Value (COLUMNNAME_DocStatus, DocStatus);
	}
	/** Set Grand Total.
		@param GrandTotal Total amount of document
	*/
	public void setGrandTotal (BigDecimal GrandTotal)
	{
		set_ValueNoCheck (COLUMNNAME_GrandTotal, GrandTotal);
	}
	/** Set Approved.
		@param IsApproved Indicates if this document requires approval
	*/
	public void setIsApproved (boolean IsApproved)
	{
		set_ValueNoCheck (COLUMNNAME_IsApproved, Boolean.valueOf(IsApproved));
	}
}
"#;

    const M_INVOICE: &str = r#"package org.compiere.model;
public class MInvoice extends X_C_Invoice implements DocAction, IDocsPostProcess
{
    public String prepareIt() { return DocAction.STATUS_InProgress; }
    public String completeIt()
    {
        return DocAction.STATUS_Completed;
    }
    public boolean voidIt() { return true; }
}
"#;

    fn model() -> AppModel {
        let i = parse(I_INVOICE);
        let x = parse(X_INVOICE);
        let m = parse(M_INVOICE);
        recognize(&[
            SourceFile {
                path: "m/MInvoice.java",
                file: &m,
            },
            SourceFile {
                path: "m/X_C_Invoice.java",
                file: &x,
            },
            SourceFile {
                path: "m/I_C_Invoice.java",
                file: &i,
            },
        ])
    }

    #[test]
    fn table_identity_and_columns_come_from_the_generated_pair() {
        let m = model();
        let t = &m.tables["C_Invoice"];
        assert_eq!(t.table_id, Some(318));
        assert_eq!(t.access_level.as_deref(), Some("1 - Org"));
        assert_eq!(
            t.interface.as_deref(),
            Some("org.compiere.model.I_C_Invoice")
        );
        assert_eq!(
            t.model_class.as_deref(),
            Some("org.compiere.model.X_C_Invoice")
        );
        assert_eq!(t.business_classes, vec!["org.compiere.model.MInvoice"]);
        let cols: Vec<&str> = t.columns.keys().map(String::as_str).collect();
        assert_eq!(
            cols,
            vec![
                "AD_Client_ID",
                "C_BPartner_ID",
                "DocStatus",
                "GrandTotal",
                "IsApproved"
            ]
        );
        assert_eq!(t.evidence, Some(("m/X_C_Invoice.java".into(), 4)));
    }

    #[test]
    fn column_semantics_are_read_without_guessing() {
        let m = model();
        let c = &m.tables["C_Invoice"].columns;
        let client = &c["AD_Client_ID"];
        assert_eq!(client.business_name.as_deref(), Some("Tenant"));
        assert_eq!(
            client.description.as_deref(),
            Some("Tenant for this installation.")
        );
        assert_eq!(client.java_type.as_deref(), Some("int"));
        assert!(!client.mandatory);
        assert_eq!(client.updatable, None, "no setter, nothing to read");

        let total = &c["GrandTotal"];
        assert_eq!(total.business_name.as_deref(), Some("Grand Total"));
        assert_eq!(
            total.description.as_deref(),
            Some("Total amount of document")
        );
        assert_eq!(total.updatable, Some(false));
        assert!(total.mandatory);
        assert_eq!(total.default, None);

        let status = &c["DocStatus"];
        assert_eq!(status.reference_id, Some(131));
        assert_eq!(
            status.values.get("CO").map(String::as_str),
            Some("Completed")
        );
        assert_eq!(status.values.get("DR").map(String::as_str), Some("Drafted"));
        assert_eq!(status.default.as_deref(), Some("DR"));
        assert_eq!(status.updatable, Some(true));

        let approved = &c["IsApproved"];
        assert!(approved.boolean);
        assert_eq!(approved.default.as_deref(), Some("N"));

        assert_eq!(c["C_BPartner_ID"].references.as_deref(), Some("C_BPartner"));
    }

    #[test]
    fn a_business_class_implementing_doc_action_carries_its_lifecycle() {
        let m = model();
        assert_eq!(m.lifecycles.len(), 1);
        let l = &m.lifecycles[0];
        assert_eq!(l.table_name, "C_Invoice");
        let names: Vec<&str> = l.methods.iter().map(|(n, ..)| n.as_str()).collect();
        assert_eq!(names, vec!["prepareIt", "completeIt", "voidIt"]);
        assert_eq!(l.methods[1].1, 5);
        assert_eq!(l.methods[1].2, 8);
    }

    #[test]
    fn ordinary_java_never_meets_the_trigger() {
        let f =
            parse("package a; public interface Repo { String Table_Name = \"x\"; } class Model {}");
        let m = recognize(&[SourceFile {
            path: "a.java",
            file: &f,
        }]);
        assert!(
            m.tables.is_empty(),
            "Table_Name without Table_ID is not a model interface"
        );
    }

    #[test]
    fn literal_values_fold_only_literals() {
        let f = parse(
            "class A { String a = \"x\" + \"y\"; String b = \"x\" + B; int c = -1; long d = 20260309L; }",
        );
        let t = &f.types[0];
        let v = |i: usize| t.fields[i].init.and_then(|r| literal_value(&f.tokens, r));
        assert_eq!(v(0).as_deref(), Some("xy"));
        assert_eq!(v(1), None);
        assert_eq!(v(2).as_deref(), Some("-1"));
        assert_eq!(v(3).as_deref(), Some("20260309L"));
    }

    /// `EKOS_JAVA_CORPUS=<dir> cargo test -p ekos-recovery app_model_corpus -- --ignored
    /// --nocapture`: the recognizer over a real tree, counted.
    #[test]
    #[ignore = "needs EKOS_JAVA_CORPUS"]
    fn app_model_corpus() {
        let Ok(root) = std::env::var("EKOS_JAVA_CORPUS") else {
            return;
        };
        let mut parsed = Vec::new();
        for entry in walkdir::WalkDir::new(&root).into_iter().flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("java") {
                let bytes = std::fs::read(path).unwrap();
                parsed.push((
                    path.display().to_string(),
                    parse(&String::from_utf8_lossy(&bytes)),
                ));
            }
        }
        let files: Vec<SourceFile<'_>> = parsed
            .iter()
            .map(|(p, f)| SourceFile { path: p, file: f })
            .collect();
        let m = recognize(&files);
        let cols: Vec<&ModelColumn> = m.tables.values().flat_map(|t| t.columns.values()).collect();
        let count = |f: &dyn Fn(&ModelColumn) -> bool| cols.iter().filter(|c| f(c)).count();
        println!(
            "tables {} (with class {}, with interface {}, with business class {})",
            m.tables.len(),
            m.tables
                .values()
                .filter(|t| t.model_class.is_some())
                .count(),
            m.tables.values().filter(|t| t.interface.is_some()).count(),
            m.tables
                .values()
                .filter(|t| !t.business_classes.is_empty())
                .count(),
        );
        println!(
            "columns {}: business_name {}, description {}, java_type {}, boolean {}, mandatory {}, default {}, not updatable {}, list {}, references {}",
            cols.len(),
            count(&|c| c.business_name.is_some()),
            count(&|c| c.description.is_some()),
            count(&|c| c.java_type.is_some()),
            count(&|c| c.boolean),
            count(&|c| c.mandatory),
            count(&|c| c.default.is_some()),
            count(&|c| c.updatable == Some(false)),
            count(&|c| !c.values.is_empty()),
            count(&|c| c.references.is_some()),
        );
        let unresolved: Vec<String> = m
            .tables
            .values()
            .flat_map(|t| t.columns.values())
            .filter_map(|c| c.references.clone())
            .filter(|r| !m.tables.contains_key(r))
            .collect();
        println!(
            "references to a table with no model: {} {:?}",
            unresolved.len(),
            unresolved.iter().take(10).collect::<Vec<_>>()
        );
        println!("lifecycles {}", m.lifecycles.len());
        for l in m.lifecycles.iter().take(5) {
            println!(
                "  {} {} {:?}",
                l.class,
                l.table_name,
                l.methods.iter().map(|x| &x.0).collect::<Vec<_>>()
            );
        }
        if let Some(t) = m.tables.get("C_Invoice") {
            println!(
                "C_Invoice: id {:?} access {:?} cols {} DocStatus {:?}",
                t.table_id,
                t.access_level,
                t.columns.len(),
                t.columns.get("DocStatus")
            );
        }
    }
}
