//! The Talend project model (RFC 0263 §"Job items"). Every type serialises, so the analyzer can
//! keep a project as evidence and the translation path (RFC 0266–0268) can read the same model.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::catalogue::Category;

/// One Talend project: everything under a directory holding `talend.project`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Project {
    /// The project directory, relative to the scanned root (`""` when the root is the project).
    pub root: String,
    pub label: String,
    pub technical_label: String,
    /// `productVersion` of `talend.project`, e.g. `Talend Open Studio for Data Integration-8.0.1…`.
    pub product_version: Option<String>,
    pub jobs: Vec<Job>,
    pub contexts: Vec<ContextItem>,
    pub connections: Vec<DbConnection>,
    pub schemas: Vec<RepoSchema>,
    pub routines: Vec<Routine>,
    /// Joblet versions (`joblets/**`), read like jobs, latest marked (RFC 0263 §Joblets).
    pub joblets: Vec<Job>,
    /// Items recognised and not read in v1, by top-level folder (`process_mr`, `services`, …),
    /// and generated or vendor files skipped (`system_sql_patterns`, `system_routines`, …).
    pub not_read: BTreeMap<String, usize>,
    pub diagnostics: Vec<Diagnostic>,
}

/// One version of one job (`process/**/<Name>_<version>.item` + its `.properties`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Job {
    /// The repository id from the `.properties` file (what `tRunJob` refers to).
    pub id: Option<String>,
    pub name: String,
    pub version: String,
    /// True for the highest version of this job present in the project.
    pub latest: bool,
    /// Repository folder (`ODS`, `DWH/facts`), from the item's path under `process/`.
    pub folder: String,
    /// Path of the `.item` file, relative to the project root.
    pub path: String,
    /// `jobType` of the root element (`Standard` for Data Integration jobs).
    pub job_type: Option<String>,
    pub purpose: Option<String>,
    pub description: Option<String>,
    pub created: Option<String>,
    pub modified: Option<String>,
    pub default_context: String,
    pub context_groups: Vec<ContextGroup>,
    /// Implicit context load (`IMPLICIT_TCONTEXTLOAD`): context values may be replaced at run time.
    pub implicit_context_load: bool,
    pub components: Vec<Component>,
    pub links: Vec<Link>,
    pub triggers: Vec<Trigger>,
    pub subjob_count: usize,
}

/// One context group of a job or of a context item.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ContextGroup {
    pub name: String,
    pub variables: Vec<ContextVar>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ContextVar {
    pub name: String,
    /// Talend type id: `id_String`, `id_Integer`, `id_Password`, `id_Directory`, …
    pub talend_type: String,
    /// The default value; always `None` for a password (RFC 0262 §Secrets).
    pub value: Option<String>,
    /// True when the variable is a password: its value is never read.
    pub secret: bool,
    pub prompt: Option<String>,
    pub comment: Option<String>,
    /// The repository context item this variable comes from (`built-in` when the job owns it).
    pub repository_context_id: Option<String>,
}

/// One component (`node`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Component {
    /// `UNIQUE_NAME`, the component's name inside its job (`tDBInput_1`).
    pub unique_name: String,
    /// `componentName`, the component type (`tPostgresqlInput`).
    pub component_type: String,
    pub type_version: Option<String>,
    pub category: Category,
    /// The component with its engine taken off (`db_input` for `tPostgresqlInput`).
    pub logical: String,
    /// The database engine for engine-specific and generic database components.
    pub engine: Option<String>,
    /// The label shown in Studio when one is set (`LABEL`), with `__UNIQUE_NAME__` left as is.
    pub label: Option<String>,
    /// Every `elementParameter` by name.
    pub params: BTreeMap<String, Param>,
    /// Output schemas by connector (`FLOW`, `REJECT`, a tMap output's name, …).
    pub schemas: Vec<ConnectorSchema>,
    pub mapper: Option<Mapper>,
    /// 1-based line of the `node` element in the item file.
    pub line: u32,
    /// Set on a component an expanded job took from a joblet (RFC 0263 §Joblets).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub joblet: Option<JobletOrigin>,
}

/// Where an expanded component came from: the joblet, its version, and the chain of uses that
/// inlined it (outermost first).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobletOrigin {
    pub name: String,
    pub version: String,
    /// The use's unique name in the job, then any nested use inside the joblet.
    pub uses: Vec<String>,
    /// The component's own unique name inside the joblet.
    pub component: String,
}

/// The value of one `elementParameter`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Param {
    /// A plain value: text as written in the item (a Java expression for most `TEXT` fields).
    Value { field: String, value: String },
    /// A `TABLE` parameter: its rows, each `elementRef → value`.
    Table {
        field: String,
        rows: Vec<BTreeMap<String, String>>,
    },
    /// A password parameter: whether a value is set, never the value.
    Secret { field: String, set: bool },
}

impl Param {
    /// The text of a plain value.
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Value { value, .. } => Some(value),
            _ => None,
        }
    }

    pub fn rows(&self) -> &[BTreeMap<String, String>] {
        match self {
            Self::Table { rows, .. } => rows,
            _ => &[],
        }
    }
}

/// An output schema of a component (`metadata`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConnectorSchema {
    pub connector: String,
    pub name: String,
    /// `label` (for a schema copied from the repository, the repository table's name).
    pub label: Option<String>,
    pub columns: Vec<Column>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    pub talend_type: Option<String>,
    /// The database type (`sourceType`), when the schema came from a database.
    pub db_type: Option<String>,
    pub key: bool,
    pub nullable: bool,
    pub length: Option<i64>,
    pub precision: Option<i64>,
    /// Date pattern or similar, as written (`"dd-MM-yyyy"` with its quotes).
    pub pattern: Option<String>,
    pub comment: Option<String>,
    /// `originalDbColumnName`: the database column a schema column maps to, when it differs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub db_name: Option<String>,
}

/// A tMap's mapping (`nodeData`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Mapper {
    /// The main input first, then the lookups, in the item's order.
    pub inputs: Vec<MapperInput>,
    pub vars: Vec<MapperEntry>,
    pub outputs: Vec<MapperOutput>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MapperInput {
    /// The label of the incoming row link (`row1`).
    pub name: String,
    pub matching_mode: Option<String>,
    pub lookup_mode: Option<String>,
    pub inner_join: bool,
    /// The input's expression filter, when activated.
    pub filter: Option<String>,
    /// Columns; on a lookup, an entry with an expression is a join key.
    pub entries: Vec<MapperEntry>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MapperEntry {
    pub name: String,
    pub talend_type: Option<String>,
    /// The Java expression, trimmed; `None` when empty.
    pub expression: Option<String>,
    /// The join operator on a lookup key (`=`).
    pub operator: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MapperOutput {
    /// The output's name: the `metaname` of the outgoing link that carries it.
    pub name: String,
    /// Catches rows no other output accepted.
    pub reject: bool,
    /// Catches rows an inner-join lookup rejected.
    pub reject_inner_join: bool,
    /// The output's expression filter, when activated.
    pub filter: Option<String>,
    pub entries: Vec<MapperEntry>,
}

/// A row link between two components.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Link {
    pub kind: LinkKind,
    pub from: String,
    pub to: String,
    /// The link's name (`row1`): how a tMap expression refers to the rows it carries.
    pub label: Option<String>,
    /// The source connector's schema name (a tMap output's name).
    pub metaname: Option<String>,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    Main,
    Lookup,
    Filter,
    Reject,
    Unique,
    Duplicate,
    Iterate,
    /// Any other row connector, by its Talend name.
    Other(String),
}

/// A trigger between two components (control flow, no rows).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Trigger {
    pub kind: TriggerKind,
    pub from: String,
    pub to: String,
    /// The `RUN_IF` condition, as written (Java).
    pub condition: Option<String>,
    pub line: u32,
    /// The joblet port the connector names, when the link leaves a joblet use
    /// (`TRIGGER_OUTPUT_1`); Studio then marks the trigger by its line style alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerKind {
    ComponentOk,
    ComponentError,
    SubjobOk,
    SubjobError,
    RunIf,
    Other(String),
}

/// A repository context item (`context/**/*.item`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ContextItem {
    pub id: Option<String>,
    pub name: String,
    pub version: String,
    pub path: String,
    pub groups: Vec<ContextGroup>,
}

/// A repository database connection (`metadata/connections/*.item`). Never a password.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DbConnection {
    pub id: Option<String>,
    pub name: String,
    pub version: String,
    pub path: String,
    /// `DatabaseType` as written (`PostgreSQL`).
    pub engine: Option<String>,
    pub host: Option<String>,
    pub port: Option<String>,
    pub database: Option<String>,
    pub schema: Option<String>,
    pub username: Option<String>,
    /// The repository context item supplying `context.*` values, when the connection is in context mode.
    pub context_id: Option<String>,
    pub has_password: bool,
    /// Tables retrieved from the database into the repository.
    pub tables: Vec<RepoTable>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RepoTable {
    pub name: String,
    pub columns: Vec<Column>,
}

/// A repository schema (`metadata/genericSchema`, file schemas, …).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RepoSchema {
    pub id: Option<String>,
    pub name: String,
    pub version: String,
    pub path: String,
    /// The metadata folder it lives in (`genericSchema`, `fileDelimited`, …).
    pub family: String,
    pub tables: Vec<RepoTable>,
}

/// A routine (`code/routines/**/*.item`). Talend's own (`system/`) routines carry no source.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Routine {
    pub name: String,
    pub version: String,
    pub path: String,
    pub system: bool,
    /// The Java source of a custom routine.
    pub source: Option<String>,
    /// `public static` methods declared by a custom routine.
    pub methods: Vec<String>,
}

/// Something in the project that could not be read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// `TALEND001`…, listed in the crate docs.
    pub code: String,
    pub path: String,
    pub message: String,
}
