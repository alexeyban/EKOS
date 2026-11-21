//! The component catalogue (RFC 0263 §"Component catalogue"): a component type → its category,
//! a logical name with the database engine taken off, and the engine.
//!
//! Only the 24 types in fixture B are verified against a real project ([`VERIFIED`]); every other
//! row is from Talend's documentation (RFC 0262 principle 4). An unknown type is
//! [`Category::Unknown`], never an error.

use serde::{Deserialize, Serialize};

/// What a component does, coarsely: drives coverage, lowering and the assessment.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Input,
    Output,
    RowTransform,
    SetTransform,
    Join,
    Flow,
    Code,
    Elt,
    Orchestration,
    Connection,
    File,
    Logging,
    /// A use of a joblet of the project (RFC 0263 §Joblets): set once the joblets are known.
    Joblet,
    #[default]
    Unknown,
}

impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Output => "output",
            Self::RowTransform => "row_transform",
            Self::SetTransform => "set_transform",
            Self::Join => "join",
            Self::Flow => "flow",
            Self::Code => "code",
            Self::Elt => "elt",
            Self::Orchestration => "orchestration",
            Self::Connection => "connection",
            Self::File => "file",
            Self::Logging => "logging",
            Self::Joblet => "joblet",
            Self::Unknown => "unknown",
        }
    }

    /// Categories that move or change rows, as opposed to orchestration, connections and logging.
    pub fn is_data(self) -> bool {
        matches!(
            self,
            Self::Input
                | Self::Output
                | Self::RowTransform
                | Self::SetTransform
                | Self::Join
                | Self::Flow
                | Self::Elt
                | Self::Joblet
        )
    }
}

/// Component types seen in a real project (fixture B, Talend Open Studio 8.0.1).
pub const VERIFIED: &[&str] = &[
    "tAggregateRow",
    "tChronometerStart",
    "tChronometerStop",
    "tDie",
    "tFileInputDelimited",
    "tFileInputExcel",
    "tFileList",
    "tFilterRow",
    "tHashInput",
    "tHashOutput",
    "tJava",
    "tLogRow",
    "tMap",
    "tPostgresqlClose",
    "tPostgresqlCommit",
    "tPostgresqlConnection",
    "tPostgresqlInput",
    "tPostgresqlOutput",
    "tPostjob",
    "tPrejob",
    "tRowGenerator",
    "tRunJob",
    "tSetGlobalVar",
    "tUniqRow",
];

/// Engine names as they appear inside component type names, longest first so `PostgresPlus`
/// wins over `Postgresql`'s prefix and `MSSql` is tried before nothing else matches.
const ENGINES: &[(&str, &str)] = &[
    ("PostgresPlus", "postgresplus"),
    ("Postgresql", "postgresql"),
    ("Greenplum", "greenplum"),
    ("Snowflake", "snowflake"),
    ("Redshift", "redshift"),
    ("Teradata", "teradata"),
    ("Informix", "informix"),
    ("BigQuery", "bigquery"),
    ("Firebird", "firebird"),
    ("Netezza", "netezza"),
    ("Vertica", "vertica"),
    ("Oracle", "oracle"),
    ("Sybase", "sybase"),
    ("Exasol", "exasol"),
    ("Ingres", "ingres"),
    ("SQLite", "sqlite"),
    ("Access", "access"),
    ("MSSql", "mssql"),
    ("Mssql", "mssql"),
    ("Mysql", "mysql"),
    ("MySQL", "mysql"),
    ("Hive", "hive"),
    ("DB2", "db2"),
    ("DB", "generic"),
];

/// Database component suffixes → (category, logical name), longest first.
const DB_SUFFIXES: &[(&str, Category, &str)] = &[
    ("OutputBulkExec", Category::Output, "db_output_bulk_exec"),
    ("LastInsertId", Category::RowTransform, "db_last_insert_id"),
    ("OutputBulk", Category::Output, "db_output_bulk"),
    ("Connection", Category::Connection, "db_connection"),
    ("BulkExec", Category::Output, "db_bulk_exec"),
    ("Rollback", Category::Connection, "db_rollback"),
    ("Commit", Category::Connection, "db_commit"),
    ("Output", Category::Output, "db_output"),
    ("Input", Category::Input, "db_input"),
    ("Close", Category::Connection, "db_close"),
    ("SCD", Category::Output, "db_scd"),
    ("Row", Category::Elt, "db_row"),
    ("SP", Category::Code, "db_sp"),
];

/// Types that are not engine-specific, by exact name.
const FIXED: &[(&str, Category)] = &[
    ("tMap", Category::RowTransform),
    ("tXMLMap", Category::RowTransform),
    ("tFilterColumns", Category::RowTransform),
    ("tConvertType", Category::RowTransform),
    ("tReplace", Category::RowTransform),
    ("tExtractDelimitedFields", Category::RowTransform),
    ("tExtractRegexFields", Category::RowTransform),
    ("tSchemaComplianceCheck", Category::RowTransform),
    ("tAggregateRow", Category::SetTransform),
    ("tAggregateSortedRow", Category::SetTransform),
    ("tUniqRow", Category::SetTransform),
    ("tSortRow", Category::SetTransform),
    ("tNormalize", Category::SetTransform),
    ("tDenormalize", Category::SetTransform),
    ("tSampleRow", Category::SetTransform),
    ("tJoin", Category::Join),
    ("tFilterRow", Category::Flow),
    ("tUnite", Category::Flow),
    ("tReplicate", Category::Flow),
    ("tRowGenerator", Category::Input),
    ("tFixedFlowInput", Category::Input),
    ("tHashInput", Category::Input),
    ("tBufferInput", Category::Input),
    ("tHashOutput", Category::Output),
    ("tBufferOutput", Category::Output),
    ("tJava", Category::Code),
    ("tJavaRow", Category::Code),
    ("tJavaFlex", Category::Code),
    ("tGroovy", Category::Code),
    ("tSystem", Category::Code),
    ("tRunJob", Category::Orchestration),
    ("tPrejob", Category::Orchestration),
    ("tPostjob", Category::Orchestration),
    ("tDie", Category::Orchestration),
    ("tWarn", Category::Orchestration),
    ("tLoop", Category::Orchestration),
    ("tInfiniteLoop", Category::Orchestration),
    ("tFileList", Category::Orchestration),
    ("tSetGlobalVar", Category::Orchestration),
    ("tChronometerStart", Category::Orchestration),
    ("tChronometerStop", Category::Orchestration),
    ("tFlowToIterate", Category::Orchestration),
    ("tIterateToFlow", Category::Orchestration),
    ("tParallelize", Category::Orchestration),
    ("tSleep", Category::Orchestration),
    ("tContextLoad", Category::Orchestration),
    ("tContextDump", Category::Orchestration),
    ("tWaitForFile", Category::Orchestration),
    ("tLibraryLoad", Category::Orchestration),
    ("tFileCopy", Category::File),
    ("tFileDelete", Category::File),
    ("tFileArchive", Category::File),
    ("tFileUnarchive", Category::File),
    ("tFileExist", Category::File),
    ("tFileTouch", Category::File),
    ("tFileProperties", Category::File),
    ("tFileRowCount", Category::File),
    ("tCreateTemporaryFile", Category::File),
    ("tLogRow", Category::Logging),
    ("tLogCatcher", Category::Logging),
    ("tStatCatcher", Category::Logging),
    ("tFlowMeter", Category::Logging),
    ("tFlowMeterCatcher", Category::Logging),
    ("tAssertCatcher", Category::Logging),
];

/// A joblet's boundary ports (RFC 0263 §Joblets) → (category, logical name). Studio writes
/// `INPUT` / `OUTPUT`; the `tJoblet*` spellings are accepted too.
const JOBLET_PORTS: &[(&str, Category, &str)] = &[
    ("INPUT", Category::Flow, "joblet_input"),
    ("OUTPUT", Category::Flow, "joblet_output"),
    ("tJobletInput", Category::Flow, "joblet_input"),
    ("tJobletOutput", Category::Flow, "joblet_output"),
    (
        "TRIGGER_INPUT",
        Category::Orchestration,
        "joblet_trigger_input",
    ),
    (
        "TRIGGER_OUTPUT",
        Category::Orchestration,
        "joblet_trigger_output",
    ),
    (
        "tJobletTriggerInput",
        Category::Orchestration,
        "joblet_trigger_input",
    ),
    (
        "tJobletTriggerOutput",
        Category::Orchestration,
        "joblet_trigger_output",
    ),
];

/// A component type, classified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classified {
    pub category: Category,
    /// `db_input` for `tPostgresqlInput`; the type itself (`tMap`) for a non-database component.
    pub logical: String,
    pub engine: Option<String>,
}

/// Classifies a component type. `db_type` is the component's `TYPE` parameter, which names the
/// engine of the generic `tDB*` components.
pub fn classify(component_type: &str, db_type: Option<&str>) -> Classified {
    if let Some(&(_, category, logical)) =
        JOBLET_PORTS.iter().find(|(n, _, _)| *n == component_type)
    {
        return Classified {
            category,
            logical: logical.to_string(),
            engine: None,
        };
    }
    if let Some(&(_, category)) = FIXED.iter().find(|(name, _)| *name == component_type) {
        return Classified {
            category,
            logical: component_type.to_string(),
            engine: None,
        };
    }
    if let Some(elt) = classify_elt(component_type) {
        return elt;
    }
    if let Some(db) = classify_db(component_type, db_type) {
        return db;
    }
    let category = if component_type.starts_with("tFileInput") {
        Category::Input
    } else if component_type.starts_with("tFileOutput") {
        Category::Output
    } else {
        Category::Unknown
    };
    Classified {
        category,
        logical: component_type.to_string(),
        engine: None,
    }
}

/// `tELTInput`/`tELTMap`/`tELTOutput` and their engine forms (`tELTPostgresqlMap`).
fn classify_elt(component_type: &str) -> Option<Classified> {
    let rest = component_type.strip_prefix("tELT")?;
    let (part, engine) = ["Input", "Output", "Map"]
        .iter()
        .find_map(|p| rest.strip_suffix(p).map(|engine| (*p, engine)))?;
    Some(Classified {
        category: Category::Elt,
        logical: format!("elt_{}", part.to_ascii_lowercase()),
        engine: engine_id(engine),
    })
}

fn classify_db(component_type: &str, db_type: Option<&str>) -> Option<Classified> {
    let rest = component_type.strip_prefix('t')?;
    let (engine_name, engine) = ENGINES
        .iter()
        .find(|(name, _)| rest.starts_with(name))
        .copied()?;
    let suffix = &rest[engine_name.len()..];
    let &(_, category, logical) = DB_SUFFIXES.iter().find(|(s, _, _)| *s == suffix)?;
    let engine = if engine == "generic" {
        db_type
            .map(|t| t.trim_matches('"').to_ascii_lowercase())
            .filter(|t| !t.is_empty())
    } else {
        Some(engine.to_string())
    };
    Some(Classified {
        category,
        logical: logical.to_string(),
        engine,
    })
}

fn engine_id(name: &str) -> Option<String> {
    if name.is_empty() {
        return None;
    }
    ENGINES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, id)| (*id).to_string())
        .or_else(|| Some(name.to_ascii_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_specific_database_components_normalise_to_a_logical_component_and_an_engine() {
        let c = classify("tPostgresqlInput", Some("POSTGRESQL"));
        assert_eq!(c.category, Category::Input);
        assert_eq!(c.logical, "db_input");
        assert_eq!(c.engine.as_deref(), Some("postgresql"));
        assert_eq!(
            classify("tOracleOutputBulkExec", None).logical,
            "db_output_bulk_exec"
        );
        assert_eq!(
            classify("tMSSqlCommit", None).category,
            Category::Connection
        );
        assert_eq!(
            classify("tPostgresPlusRow", None).engine.as_deref(),
            Some("postgresplus")
        );
    }

    #[test]
    fn the_generic_db_components_take_their_engine_from_the_type_parameter() {
        let c = classify("tDBInput", Some("\"ORACLE\""));
        assert_eq!(c.logical, "db_input");
        assert_eq!(c.engine.as_deref(), Some("oracle"));
        assert_eq!(classify("tDBRow", None).engine, None);
        assert_eq!(classify("tDBRow", None).category, Category::Elt);
    }

    #[test]
    fn elt_components_are_their_own_category() {
        let c = classify("tELTPostgresqlMap", None);
        assert_eq!(c.category, Category::Elt);
        assert_eq!(c.logical, "elt_map");
        assert_eq!(c.engine.as_deref(), Some("postgresql"));
        assert_eq!(classify("tELTInput", None).engine, None);
    }

    #[test]
    fn an_unknown_type_is_kept_as_unknown_and_file_io_by_prefix() {
        assert_eq!(
            classify("tSalesforceInput", None).category,
            Category::Unknown
        );
        assert_eq!(classify("tFileInputJSON", None).category, Category::Input);
        assert_eq!(
            classify("tFileOutputDelimited", None).category,
            Category::Output
        );
        // A fixed name wins over the engine rule: `tDie` is not a `tD…` database component.
        assert_eq!(classify("tDie", None).category, Category::Orchestration);
    }

    #[test]
    fn joblet_ports_are_flow_or_orchestration() {
        assert_eq!(classify("INPUT", None).logical, "joblet_input");
        assert_eq!(classify("tJobletOutput", None).logical, "joblet_output");
        assert_eq!(classify("OUTPUT", None).category, Category::Flow);
        assert_eq!(
            classify("TRIGGER_INPUT", None).category,
            Category::Orchestration
        );
    }

    #[test]
    fn every_verified_type_has_a_known_category() {
        for t in VERIFIED {
            assert_ne!(classify(t, None).category, Category::Unknown, "{t}");
        }
    }
}
