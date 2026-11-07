//! Reading Ossie documents: `0.2.0.dev0` (one model at the root) and `0.1.1` (a
//! `semantic_model` array), as the spec's "Migrating earlier document shapes" describes.
//! Every model of a `0.1.1` array is kept, never only the first.

use crate::model::SemanticModel;
use serde_yaml::Value;

/// A document that is not an Ossie model, or that is one but does not parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadError(pub String);

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ReadError {}

fn key<'a>(m: &'a serde_yaml::Mapping, k: &str) -> Option<&'a Value> {
    m.get(Value::String(k.into()))
}

/// The version string of a top-level mapping (`0.1.1` was often written unquoted-numeric-safe,
/// but a bare `0.2` would read as a number).
fn version_of(m: &serde_yaml::Mapping) -> Option<String> {
    match key(m, "version")? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Whether `text` looks like an Ossie document: a mapping with an Ossie `version` (`0.1.*` or
/// `0.2.*`) and a `datasets` or `semantic_model` list. Cheap enough to run on every YAML file of a
/// repository during discovery; a file that does not parse is simply not one.
pub fn sniff(text: &str) -> bool {
    if !text.contains("datasets") && !text.contains("semantic_model") {
        return false;
    }
    let Ok(Value::Mapping(m)) = serde_yaml::from_str::<Value>(text) else {
        return false;
    };
    let versioned = version_of(&m).is_some_and(|v| v.starts_with("0.1") || v.starts_with("0.2"));
    let listed = matches!(key(&m, "datasets"), Some(Value::Sequence(_)))
        || matches!(key(&m, "semantic_model"), Some(Value::Sequence(_)));
    versioned && listed
}

/// Every model in an Ossie document (YAML or JSON).
pub fn parse(text: &str) -> Result<Vec<SemanticModel>, ReadError> {
    let doc: Value =
        serde_yaml::from_str(text).map_err(|e| ReadError(format!("not YAML or JSON: {e}")))?;
    let Value::Mapping(m) = &doc else {
        return Err(ReadError("an Ossie document is a mapping".into()));
    };
    let version = version_of(m).ok_or_else(|| ReadError("no `version`".into()))?;
    match key(m, "semantic_model") {
        Some(Value::Sequence(models)) => {
            if models.is_empty() {
                return Err(ReadError(
                    "`semantic_model` is empty: it holds no model to read".into(),
                ));
            }
            models
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let mut v = v.clone();
                    if let Value::Mapping(mm) = &mut v {
                        mm.insert("version".into(), Value::String(version.clone()));
                    }
                    serde_yaml::from_value::<SemanticModel>(v)
                        .map_err(|e| ReadError(format!("semantic_model[{i}]: {e}")))
                })
                .collect()
        }
        Some(_) => Err(ReadError("`semantic_model` must be a list".into())),
        None => {
            let mut root = m.clone();
            // `0.1.1` root declarations the flat shape drops (spec: remove them).
            root.remove(Value::String("dialects".into()));
            root.remove(Value::String("vendors".into()));
            root.insert("version".into(), Value::String(version));
            serde_yaml::from_value::<SemanticModel>(Value::Mapping(root))
                .map(|m| vec![m])
                .map_err(|e| ReadError(e.to_string()))
        }
    }
}
