//! The Apache Ossie core document, spec `0.2.0.dev0` (`core-spec/spec.md`).
//!
//! The types read both spec versions (`0.1.1` expressions carry only `dialects`, and `0.1.1` has no
//! `datatype` or dataset `metrics`) and write `0.2.0.dev0`. Empty lists and absent values are not
//! written, so an export validates against the official schema, which rejects an empty
//! `primary_key` and has `additionalProperties: false` everywhere except inside `ai_context`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The spec version EKOS writes: the official schema pins `version` to this constant.
pub const SPEC_VERSION: &str = "0.2.0.dev0";

/// The `vendor_name` of the extension EKOS writes on every element (RFC 0245 §3).
pub const EKOS_VENDOR: &str = "EKOS";

/// The one dialect whose semantics the spec defines.
pub const OSSIE_SQL: &str = "OSSIE_SQL_2026";

/// One semantic model: one document.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SemanticModel {
    pub version: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_context: Option<AiContext>,
    pub datasets: Vec<Dataset>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relationships: Vec<Relationship>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub metrics: Vec<Metric>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_extensions: Vec<CustomExtension>,
}

/// A logical dataset over one physical table, view or query.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Dataset {
    pub name: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub primary_key: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unique_keys: Vec<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_context: Option<AiContext>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<Field>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub metrics: Vec<Metric>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_extensions: Vec<CustomExtension>,
}

/// A row-level attribute: a scalar expression over the dataset's source columns.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    pub expression: Expression,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dimension: Option<Dimension>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub datatype: Option<DataType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_context: Option<AiContext>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_extensions: Vec<CustomExtension>,
}

/// `dimension`: only the temporal-role flag today.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Dimension {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_time: Option<bool>,
}

/// An expression: the portable `sql` form and/or per-dialect variants.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Expression {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sql: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dialects: Vec<DialectExpression>,
}

impl Expression {
    /// A portable `OSSIE_SQL_2026` expression.
    pub fn sql(sql: impl Into<String>) -> Self {
        Self {
            sql: Some(sql.into()),
            dialects: Vec::new(),
        }
    }

    /// The portable text: the top-level `sql`, else an `OSSIE_SQL_2026` or `ANSI_SQL` variant
    /// (the `0.1.1` examples wrote `ANSI_SQL`), else the first variant. The spec makes the
    /// top-level `sql` authoritative when both are present.
    pub fn text(&self) -> Option<&str> {
        if let Some(s) = self.sql.as_deref() {
            return Some(s);
        }
        ["OSSIE_SQL_2026", "ANSI_SQL"]
            .iter()
            .find_map(|d| {
                self.dialects
                    .iter()
                    .find(|x| x.dialect.eq_ignore_ascii_case(d))
            })
            .or_else(|| self.dialects.first())
            .map(|d| d.expression.as_str())
    }
}

/// One dialect's form of an expression.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct DialectExpression {
    pub dialect: String,
    pub expression: String,
}

/// A foreign-key path between two datasets, over their **fields**.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Relationship {
    pub name: String,
    pub from: String,
    pub to: String,
    pub from_columns: Vec<String>,
    pub to_columns: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_context: Option<AiContext>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_extensions: Vec<CustomExtension>,
}

/// An aggregation, dataset- or model-scoped.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Metric {
    pub name: String,
    pub expression: Expression,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub datatype: Option<DataType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_context: Option<AiContext>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_extensions: Vec<CustomExtension>,
}

/// The logical value type of a field or metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataType {
    String,
    Integer,
    Decimal,
    Float,
    Boolean,
    Date,
    Time,
    DateTime,
    DateTimeTz,
    Opaque,
}

impl DataType {
    /// `Date`, `Time`, `DateTime`, `DateTimeTz`: what makes `is_time` default to `true`.
    pub fn is_temporal(self) -> bool {
        matches!(
            self,
            DataType::Date | DataType::Time | DataType::DateTime | DataType::DateTimeTz
        )
    }
}

/// `ai_context`: free text, or `instructions`/`synonyms`/`examples` plus anything else
/// (the schema allows additional keys here only).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AiContext {
    Text(String),
    Structured(AiContextFields),
}

/// The structured form of [`AiContext`].
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct AiContextFields {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub synonyms: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<String>,
    #[serde(flatten)]
    pub other: BTreeMap<String, serde_json::Value>,
}

impl AiContext {
    /// The instructions: the free text, or the structured `instructions`.
    pub fn instructions(&self) -> Option<&str> {
        match self {
            AiContext::Text(t) => Some(t.as_str()),
            AiContext::Structured(f) => f.instructions.as_deref(),
        }
    }

    /// The synonyms (none for the free-text form).
    pub fn synonyms(&self) -> &[String] {
        match self {
            AiContext::Text(_) => &[],
            AiContext::Structured(f) => &f.synonyms,
        }
    }

    /// A structured context from parts, or `None` when every part is empty.
    pub fn from_parts(instructions: Option<String>, synonyms: Vec<String>) -> Option<Self> {
        let instructions = instructions.filter(|i| !i.trim().is_empty());
        if instructions.is_none() && synonyms.is_empty() {
            return None;
        }
        Some(AiContext::Structured(AiContextFields {
            instructions,
            synonyms,
            ..Default::default()
        }))
    }
}

/// Vendor metadata: `data` is a JSON **string** Ossie carries but does not interpret.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CustomExtension {
    pub vendor_name: String,
    pub data: String,
}

impl CustomExtension {
    /// The `EKOS` extension carrying `data` (keys sorted: `serde_json`'s map is ordered).
    pub fn ekos(data: &serde_json::Value) -> Self {
        Self {
            vendor_name: EKOS_VENDOR.into(),
            data: data.to_string(),
        }
    }
}

/// The parsed data of the `EKOS` extension in `exts`, if there is one and it is JSON.
pub fn ekos_data(exts: &[CustomExtension]) -> Option<serde_json::Value> {
    exts.iter()
        .find(|e| e.vendor_name == EKOS_VENDOR)
        .and_then(|e| serde_json::from_str(&e.data).ok())
}

impl SemanticModel {
    /// The document as YAML.
    pub fn to_yaml(&self) -> String {
        serde_yaml::to_string(self).expect("an Ossie model always serialises")
    }

    /// The document as a JSON value.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("an Ossie model always serialises")
    }
}

impl Field {
    /// The effective temporal role: explicit `is_time`, else a temporal `datatype`.
    pub fn is_time(&self) -> bool {
        self.dimension
            .as_ref()
            .and_then(|d| d.is_time)
            .unwrap_or_else(|| self.datatype.is_some_and(DataType::is_temporal))
    }
}
