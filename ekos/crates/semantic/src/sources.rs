//! RFC 0178 Part 2 — which source an object comes from.
//!
//! A project's `[[sources]]` are named inputs. `ekos commit` records on each object the sources
//! it was observed in: `sources` (sorted list) and, when there is exactly one, `source`. The
//! attribution is deterministic and reads only what the object already carries:
//!
//! 1. its `project` property, when an analyzer recorded the key of the directory it read
//!    (RFC 0044 / 0079) — exact;
//! 2. otherwise the sources holding a file one of its evidence records names. A path that exists
//!    in two sources attributes the object to both: the list may be too long, never too short;
//! 3. otherwise, in a project with one source, that source.
//!
//! An object none of these reach has no `sources`. A reader restricted by source must treat it
//! as not visible (RFC 0181).

use std::collections::{BTreeMap, BTreeSet, HashMap};

/// The files each named source holds, from the build's source manifest.
#[derive(Debug, Default)]
pub struct SourceIndex {
    names: BTreeSet<String>,
    by_file: HashMap<String, BTreeSet<String>>,
}

impl SourceIndex {
    /// `manifest_paths` are the source manifest's keys, `<source>/<path in source>`. A key whose
    /// first component is not a source name is ignored.
    pub fn new<'a>(
        names: impl IntoIterator<Item = &'a str>,
        manifest_paths: impl IntoIterator<Item = &'a str>,
    ) -> Self {
        let names: BTreeSet<String> = names.into_iter().map(str::to_string).collect();
        let mut by_file: HashMap<String, BTreeSet<String>> = HashMap::new();
        for key in manifest_paths {
            if let Some((name, rel)) = key.split_once('/')
                && names.contains(name)
            {
                by_file
                    .entry(rel.to_string())
                    .or_default()
                    .insert(name.to_string());
            }
        }
        Self { names, by_file }
    }

    /// No `[[sources]]`: nothing is attributed and nothing is written.
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// The sources of an object with this `project` property and these evidence paths.
    pub fn attribute<'a>(
        &self,
        project: Option<&str>,
        evidence_paths: impl IntoIterator<Item = &'a str>,
    ) -> BTreeSet<String> {
        if let Some(p) = project
            && self.names.contains(p)
        {
            return BTreeSet::from([p.to_string()]);
        }
        let mut found = BTreeSet::new();
        for path in evidence_paths {
            let path = path.replace('\\', "/");
            let path = path.strip_prefix("./").unwrap_or(&path);
            if let Some(sources) = self.by_file.get(path) {
                found.extend(sources.iter().cloned());
            } else if let Some((name, rel)) = path.split_once('/')
                && self.by_file.get(rel).is_some_and(|s| s.contains(name))
            {
                // A path already written as `<source>/<path>`.
                found.insert(name.to_string());
            }
        }
        if found.is_empty() && self.names.len() == 1 {
            found.extend(self.names.iter().cloned());
        }
        found
    }
}

/// Write `sources` (and `source` when there is exactly one) onto an object's properties,
/// replacing what an earlier commit wrote. An empty set removes both.
pub fn stamp(properties: &mut HashMap<String, serde_json::Value>, sources: &BTreeSet<String>) {
    properties.remove("source");
    properties.remove("sources");
    if sources.is_empty() {
        return;
    }
    properties.insert("sources".into(), serde_json::json!(sources));
    if let Some(only) = sources.iter().next()
        && sources.len() == 1
    {
        properties.insert("source".into(), serde_json::json!(only));
    }
}

/// How one commit's objects were attributed, for the commit summary.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SourceStats {
    pub attributed: usize,
    /// Objects with no source, counted per kind.
    pub unattributed: BTreeMap<String, usize>,
}

impl SourceStats {
    pub fn record(&mut self, kind: &str, sources: &BTreeSet<String>) {
        if sources.is_empty() {
            *self.unattributed.entry(kind.to_string()).or_default() += 1;
        } else {
            self.attributed += 1;
        }
    }

    pub fn unattributed_total(&self) -> usize {
        self.unattributed.values().sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> SourceIndex {
        SourceIndex::new(
            ["api", "web", "docs"],
            [
                "api/src/main.rs",
                "api/README.md",
                "web/src/main.rs",
                "web/package.json",
                "docs/guide/setup.md",
                "stray/file.txt",
            ],
        )
    }

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_recorded_key_wins_over_evidence() {
        let idx = index();
        assert_eq!(idx.attribute(Some("web"), ["src/main.rs"]), set(&["web"]));
        // A key that is not a source name (an older ledger's path key) falls back to evidence.
        assert_eq!(
            idx.attribute(Some("services/api"), ["README.md"]),
            set(&["api"])
        );
    }

    #[test]
    fn evidence_paths_attribute_and_a_shared_path_names_every_source_holding_it() {
        let idx = index();
        assert_eq!(idx.attribute(None, ["package.json"]), set(&["web"]));
        assert_eq!(idx.attribute(None, ["src/main.rs"]), set(&["api", "web"]));
        assert_eq!(
            idx.attribute(None, ["README.md", "guide/setup.md"]),
            set(&["api", "docs"])
        );
        // Written with the source in front, with a leading `./`, or with backslashes.
        assert_eq!(idx.attribute(None, ["web/src/main.rs"]), set(&["web"]));
        assert_eq!(idx.attribute(None, ["./package.json"]), set(&["web"]));
        assert_eq!(idx.attribute(None, ["guide\\setup.md"]), set(&["docs"]));
    }

    #[test]
    fn what_nothing_reaches_stays_unattributed_unless_there_is_one_source() {
        let idx = index();
        assert!(idx.attribute(None, ["nowhere.txt"]).is_empty());
        assert!(idx.attribute(None, []).is_empty());
        assert!(idx.attribute(None, ["stray/file.txt"]).is_empty());

        let one = SourceIndex::new(["api"], ["api/a.rs"]);
        assert_eq!(one.attribute(None, []), set(&["api"]));
        assert!(SourceIndex::default().is_empty());
        assert!(SourceIndex::default().attribute(None, ["a.rs"]).is_empty());
    }

    #[test]
    fn stamping_writes_one_or_many_and_replaces_an_earlier_stamp() {
        let mut props = HashMap::new();
        stamp(&mut props, &set(&["web", "api"]));
        assert_eq!(props["sources"], serde_json::json!(["api", "web"]));
        assert!(!props.contains_key("source"));

        stamp(&mut props, &set(&["api"]));
        assert_eq!(props["sources"], serde_json::json!(["api"]));
        assert_eq!(props["source"], serde_json::json!("api"));

        stamp(&mut props, &BTreeSet::new());
        assert!(!props.contains_key("source") && !props.contains_key("sources"));
    }

    #[test]
    fn stats_count_unattributed_objects_per_kind() {
        let mut stats = SourceStats::default();
        stats.record("File", &set(&["api"]));
        stats.record("Person", &BTreeSet::new());
        stats.record("Person", &BTreeSet::new());
        assert_eq!(stats.attributed, 1);
        assert_eq!(stats.unattributed_total(), 2);
        assert_eq!(stats.unattributed["Person"], 2);
    }
}
