//! RFC 0221: Markdown front matter — the YAML block a Docusaurus page or a wiki snapshot opens
//! with. It is the page's own metadata (title, description, provenance), so it becomes document
//! properties rather than the text of a heading-less first section.

use serde_json::Value;
use std::collections::BTreeMap;

/// The closing line must come within this many lines, or the opening `---` is a thematic break.
const MAX_LINES: usize = 200;
/// Keys kept per document.
const MAX_KEYS: usize = 40;
/// Characters kept per value.
const MAX_VALUE_CHARS: usize = 500;

/// A file's front matter: the kept fields and how many lines the block spans, delimiters included.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct FrontMatter {
    pub fields: BTreeMap<String, Value>,
    pub lines: usize,
}

/// `Some` when `text` opens with a `---` line closed by `---` or `...` within [`MAX_LINES`].
/// YAML that does not parse keeps no fields but still counts its lines: it is metadata either way.
pub(crate) fn split(text: &str) -> Option<FrontMatter> {
    let mut lines = text.lines();
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    let close = lines
        .take(MAX_LINES)
        .position(|l| matches!(l.trim_end(), "---" | "..."))?;
    let body: Vec<&str> = text.lines().skip(1).take(close).collect();
    let fields = match serde_yaml::from_str::<serde_yaml::Value>(&body.join("\n")) {
        Ok(serde_yaml::Value::Mapping(map)) => map
            .iter()
            .filter_map(|(k, v)| Some((scalar(k)?, value(v)?)))
            .take(MAX_KEYS)
            .collect(),
        _ => BTreeMap::new(),
    };
    Some(FrontMatter {
        fields,
        lines: close + 2,
    })
}

/// A scalar or a list of scalars, as strings; anything nested is dropped.
fn value(v: &serde_yaml::Value) -> Option<Value> {
    match v {
        serde_yaml::Value::Sequence(items) => {
            let items: Vec<Value> = items.iter().filter_map(scalar).map(Value::String).collect();
            Some(Value::Array(items))
        }
        other => scalar(other).map(Value::String),
    }
}

fn scalar(v: &serde_yaml::Value) -> Option<String> {
    let s = match v {
        serde_yaml::Value::String(s) => s.clone(),
        serde_yaml::Value::Number(n) => n.to_string(),
        serde_yaml::Value::Bool(b) => b.to_string(),
        _ => return None,
    };
    Some(s.chars().take(MAX_VALUE_CHARS).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn docusaurus_front_matter_keeps_scalars_and_lists() {
        let md = "---\nsidebar_position: 36\ntitle: \"Sessions\"\ntags: [technical, admin]\n\
                  draft: false\n---\n# Sessions\n";
        let fm = split(md).unwrap();
        assert_eq!(fm.lines, 6);
        assert_eq!(fm.fields["sidebar_position"], json!("36"));
        assert_eq!(fm.fields["title"], json!("Sessions"));
        assert_eq!(fm.fields["tags"], json!(["technical", "admin"]));
        assert_eq!(fm.fields["draft"], json!("false"));
    }

    #[test]
    fn nested_values_and_nulls_are_dropped() {
        let fm = split("---\nauthor:\n  name: x\nempty:\nkeep: y\n...\nbody\n").unwrap();
        assert_eq!(fm.fields.keys().collect::<Vec<_>>(), ["keep"]);
        assert_eq!(fm.lines, 6);
    }

    #[test]
    fn an_unclosed_or_late_rule_is_not_front_matter() {
        assert_eq!(split("---\ntitle: x\n\nbody with no closing rule\n"), None);
        assert_eq!(split("# Title\n---\na: b\n---\n"), None);
        assert_eq!(split(""), None);
    }

    #[test]
    fn unparseable_yaml_keeps_no_fields_but_still_counts_its_lines() {
        let fm = split("---\n: : [unbalanced\n---\n# T\n").unwrap();
        assert!(fm.fields.is_empty());
        assert_eq!(fm.lines, 3);
    }

    #[test]
    fn long_values_are_truncated() {
        let md = format!("---\ndescription: {}\n---\n", "a".repeat(900));
        let fm = split(&md).unwrap();
        assert_eq!(
            fm.fields["description"].as_str().unwrap().len(),
            MAX_VALUE_CHARS
        );
    }
}
