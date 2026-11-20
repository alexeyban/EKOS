//! The Talend project model (RFC 0263, umbrella RFC 0262).
//!
//! Pure: [`read_project`] takes one project's files as `(path relative to the project root,
//! text)` and returns a [`Project`]. Finding projects and reading files is the observer's job
//! (`ekos-plugin-talend`); turning the model into ledger objects is the analyzer's
//! (`ekos_recovery::talend_analyzer`).
//!
//! Secrets: no value of a `PASSWORD` element parameter, an `id_Password` context parameter or a
//! `password` attribute is ever stored in the model (RFC 0262 §Secrets).
//!
//! Diagnostics:
//! - `TALEND001`: a file is not well-formed XML;
//! - `TALEND002`: a file under `process/` is not a job item;
//! - `TALEND003`: a job item has no `.properties` file (its id is unknown, so a `tRunJob` that
//!   names it by id cannot be resolved);
//! - `TALEND004`: the project has no `talend.project`.

pub mod catalogue;
pub mod java;
mod job;
pub mod model;
mod repository;
pub mod resolve;
mod xml;

use std::collections::BTreeMap;

pub use catalogue::{Category, classify};
pub use java::{Folded, fold, qualified_refs};
pub use model::*;
pub use resolve::{Call, ContextValue, ResolvedJob, RuntimeContext};

use repository::Properties;

/// The marker file of a Talend project directory.
pub const PROJECT_MARKER: &str = "talend.project";

/// Top-level folders holding items v1 recognises and does not read (RFC 0262 §"Not in scope").
pub const OUT_OF_SCOPE_FOLDERS: &[&str] = &[
    "process_mr",
    "process_storm",
    "services",
    "routes",
    "joblets",
    "jobletsMr",
    "businessProcess",
];

/// Directory names never read: generated code, build output, Studio's temporary files.
pub const SKIPPED_DIRS: &[&str] = &["poms", "temp", "target", ".git"];

/// One file of a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectFile {
    /// Relative to the project root, `/`-separated.
    pub path: String,
    pub text: String,
}

/// What a file is, by its path inside a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    ProjectFile,
    Job,
    Context,
    Connection,
    Schema,
    Routine,
    SystemRoutine,
    /// The `.properties` file next to an item.
    Properties,
    /// Talend's own SQL templates (`sqlPatterns/`): counted, not read.
    SqlPattern,
    /// A folder v1 does not read (see [`OUT_OF_SCOPE_FOLDERS`]): counted.
    OutOfScope,
    /// Anything else (settings, screenshots, indexes).
    Other,
}

impl Role {
    /// The name the observer records with each file.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProjectFile => "project",
            Self::Job => "job",
            Self::Context => "context",
            Self::Connection => "connection",
            Self::Schema => "schema",
            Self::Routine => "routine",
            Self::SystemRoutine => "system_routine",
            Self::Properties => "properties",
            Self::SqlPattern => "sql_pattern",
            Self::OutOfScope => "out_of_scope",
            Self::Other => "other",
        }
    }

    /// Whether the observer captures the file's text. Talend's own SQL templates and system
    /// routines are counted from their paths only: they are vendor files, the same in every project.
    pub fn keeps_text(self) -> bool {
        !matches!(
            self,
            Self::SqlPattern | Self::SystemRoutine | Self::OutOfScope | Self::Other
        )
    }
}

/// Classifies a path inside a project. The observer keeps only files whose role is not
/// [`Role::Other`].
pub fn role(path: &str) -> Role {
    if path == PROJECT_MARKER {
        return Role::ProjectFile;
    }
    let parts: Vec<&str> = path.split('/').collect();
    if parts.iter().any(|p| SKIPPED_DIRS.contains(p)) {
        return Role::Other;
    }
    let ext = path.rsplit_once('.').map_or("", |(_, e)| e);
    match ext {
        "properties" if parts.len() > 1 => Role::Properties,
        "item" => item_role(&parts),
        _ => Role::Other,
    }
}

fn item_role(parts: &[&str]) -> Role {
    match parts {
        ["process", ..] => Role::Job,
        ["context", ..] => Role::Context,
        ["metadata", "connections", ..] => Role::Connection,
        ["metadata", ..] => Role::Schema,
        ["code", "routines", "system", ..] => Role::SystemRoutine,
        ["code", "routines", ..] => Role::Routine,
        ["sqlPatterns", ..] => Role::SqlPattern,
        [top, ..] if OUT_OF_SCOPE_FOLDERS.contains(top) => Role::OutOfScope,
        _ => Role::Other,
    }
}

/// Reads one project from its files. `root` is the project directory relative to the scanned
/// tree, recorded as is.
pub fn read_project(root: &str, files: &[ProjectFile]) -> Project {
    let mut project = Project {
        root: root.to_string(),
        ..Project::default()
    };
    let properties = read_properties(files, &mut project);
    let mut sorted: Vec<&ProjectFile> = files.iter().collect();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    for file in sorted {
        read_file(file, &properties, &mut project);
    }
    if project.label.is_empty() && project.technical_label.is_empty() {
        diagnose(
            &mut project,
            "TALEND004",
            root,
            "no talend.project in the project",
        );
    }
    mark_latest(&mut project.jobs);
    project.jobs.sort_by(|a, b| {
        (&a.folder, &a.name, version_key(&a.version)).cmp(&(
            &b.folder,
            &b.name,
            version_key(&b.version),
        ))
    });
    project
}

fn read_properties(files: &[ProjectFile], project: &mut Project) -> BTreeMap<String, Properties> {
    let mut out = BTreeMap::new();
    for file in files.iter().filter(|f| role(&f.path) == Role::Properties) {
        match repository::parse_properties(&file.text) {
            Ok(p) => {
                let stem = file.path.trim_end_matches(".properties").to_string();
                out.insert(stem, p);
            }
            Err(e) => diagnose(project, "TALEND001", &file.path, &e),
        }
    }
    out
}

fn read_file(file: &ProjectFile, properties: &BTreeMap<String, Properties>, project: &mut Project) {
    let path = file.path.as_str();
    let props = properties.get(path.trim_end_matches(".item"));
    let result = match role(path) {
        Role::ProjectFile => repository::parse_project_file(&file.text).map(|(l, t, v)| {
            project.label = l;
            project.technical_label = t;
            project.product_version = v;
        }),
        Role::Job => read_job(file, props, project),
        Role::Context => repository::parse_context_item(path, &file.text).map(|mut c| {
            c.id = props.and_then(|p| p.id.clone());
            project.contexts.push(c);
        }),
        Role::Connection => repository::parse_connection(path, &file.text).map(|mut c| {
            c.id = props.and_then(|p| p.id.clone()).or(c.id);
            project.connections.push(c);
        }),
        Role::Schema => {
            let family = path.split('/').nth(1).unwrap_or_default();
            repository::parse_schema(path, family, &file.text).map(|mut s| {
                s.id = props.and_then(|p| p.id.clone());
                project.schemas.push(s);
            })
        }
        Role::Routine => {
            project
                .routines
                .push(repository::parse_routine(path, &file.text, false));
            Ok(())
        }
        Role::SystemRoutine => {
            project
                .routines
                .push(repository::parse_routine(path, &file.text, true));
            count(project, "system_routines");
            Ok(())
        }
        Role::SqlPattern => {
            count(project, "sql_patterns");
            Ok(())
        }
        Role::OutOfScope => {
            count(project, path.split('/').next().unwrap_or_default());
            Ok(())
        }
        Role::Properties | Role::Other => Ok(()),
    };
    if let Err(e) = result {
        let code = if e.starts_with("not a job item") {
            "TALEND002"
        } else {
            "TALEND001"
        };
        diagnose(project, code, path, &e);
    }
}

fn read_job(
    file: &ProjectFile,
    props: Option<&Properties>,
    project: &mut Project,
) -> Result<(), String> {
    let mut job = job::parse_job(&file.path, &file.text)?;
    match props {
        Some(p) => {
            job.id = p.id.clone();
            job.name = p.label.clone().unwrap_or(job.name);
            job.version = p.version.clone().unwrap_or(job.version);
            job.folder = p.folder.clone().unwrap_or(job.folder);
            job.purpose = p.purpose.clone();
            job.description = p.description.clone();
            job.created = p.created.clone();
            job.modified = p.modified.clone();
        }
        None => diagnose(
            project,
            "TALEND003",
            &file.path,
            "job item without a .properties file",
        ),
    }
    project.jobs.push(job);
    Ok(())
}

fn count(project: &mut Project, key: &str) {
    *project.not_read.entry(key.to_string()).or_insert(0) += 1;
}

fn diagnose(project: &mut Project, code: &str, path: &str, message: &str) {
    project.diagnostics.push(Diagnostic {
        code: code.to_string(),
        path: path.to_string(),
        message: message.to_string(),
    });
}

/// `0.10` > `0.9`: versions compare part by part as numbers.
fn version_key(version: &str) -> Vec<u64> {
    version.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

fn mark_latest(jobs: &mut [Job]) {
    let mut best: BTreeMap<String, Vec<u64>> = BTreeMap::new();
    for job in jobs.iter() {
        let key = job.id.clone().unwrap_or_else(|| job.name.clone());
        let v = version_key(&job.version);
        let entry = best.entry(key).or_insert_with(|| v.clone());
        if v > *entry {
            *entry = v;
        }
    }
    for job in jobs.iter_mut() {
        let key = job.id.clone().unwrap_or_else(|| job.name.clone());
        job.latest = best.get(&key) == Some(&version_key(&job.version));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, text: &str) -> ProjectFile {
        ProjectFile {
            path: path.into(),
            text: text.into(),
        }
    }

    const PROJECT: &str = r#"<xmi:XMI xmlns:xmi="http://www.omg.org/XMI" xmlns:TalendProperties="http://www.talend.org/properties">
  <TalendProperties:Project label="Demo" technicalLabel="DEMO" productVersion="Talend Open Studio for Data Integration-8.0.1"/>
  <TalendProperties:User login="a@b" password="D41D8CD98F00B204E9800998ECF8427E"/>
</xmi:XMI>"#;

    fn job_item(child: Option<(&str, &str)>) -> String {
        let run = child.map_or(String::new(), |(name, id)| {
            format!(
                r#"<node componentName="tRunJob"><elementParameter field="TEXT" name="UNIQUE_NAME" value="tRunJob_1"/><elementParameter field="PROCESS_TYPE" name="PROCESS" value="{name}"/><elementParameter field="TECHNICAL" name="PROCESS:PROCESS_TYPE_PROCESS" value="DEMO:{id}"/><elementParameter field="TECHNICAL" name="PROCESS:PROCESS_TYPE_VERSION" value="Latest"/><elementParameter field="CHECK" name="DIE_ON_CHILD_ERROR" value="true"/></node>"#
            )
        });
        format!(
            r#"<talendfile:ProcessType xmlns:talendfile="platform:/resource/org.talend.model/model/TalendFile.xsd" defaultContext="Default">{run}</talendfile:ProcessType>"#
        )
    }

    fn props(id: &str, label: &str, version: &str, folder: &str) -> String {
        format!(
            r#"<xmi:XMI xmlns:xmi="http://www.omg.org/XMI" xmlns:TalendProperties="http://www.talend.org/properties"><TalendProperties:Property id="{id}" label="{label}" version="{version}"/><TalendProperties:ItemState path="{folder}"/></xmi:XMI>"#
        )
    }

    #[test]
    fn roles_follow_the_project_layout() {
        assert_eq!(role("talend.project"), Role::ProjectFile);
        assert_eq!(role("process/ODS/j_0.1.item"), Role::Job);
        assert_eq!(role("process/ODS/j_0.1.properties"), Role::Properties);
        assert_eq!(role("process/ODS/j_0.1.screenshot"), Role::Other);
        assert_eq!(role("metadata/connections/c_0.1.item"), Role::Connection);
        assert_eq!(role("metadata/fileDelimited/f_0.1.item"), Role::Schema);
        assert_eq!(
            role("code/routines/system/Numeric_0.1.item"),
            Role::SystemRoutine
        );
        assert_eq!(role("code/routines/mine_0.1.item"), Role::Routine);
        assert_eq!(
            role("sqlPatterns/PostgreSQL/system/x_0.1.item"),
            Role::SqlPattern
        );
        assert_eq!(role("process_mr/spark_0.1.item"), Role::OutOfScope);
        assert_eq!(role("poms/jobs/process/x_0.1.item"), Role::Other);
    }

    #[test]
    fn a_project_reads_jobs_marks_latest_and_resolves_calls() {
        let files = vec![
            file("talend.project", PROJECT),
            file(
                "process/run/parent_0.1.item",
                &job_item(Some(("child", "_c"))),
            ),
            file(
                "process/run/parent_0.1.properties",
                &props("_p", "parent", "0.1", "run"),
            ),
            file("process/load/child_0.1.item", &job_item(None)),
            file(
                "process/load/child_0.1.properties",
                &props("_c", "child", "0.1", "load"),
            ),
            file("process/load/child_0.10.item", &job_item(None)),
            file(
                "process/load/child_0.10.properties",
                &props("_c", "child", "0.10", "load"),
            ),
            file("process_mr/big_0.1.item", "<x/>"),
        ];
        let project = read_project("demo", &files);
        assert_eq!(project.technical_label, "DEMO");
        assert!(project.diagnostics.is_empty(), "{:?}", project.diagnostics);
        assert_eq!(project.jobs.len(), 3);
        let latest: Vec<_> = project
            .jobs
            .iter()
            .filter(|j| j.latest)
            .map(|j| format!("{}@{}", j.name, j.version))
            .collect();
        assert_eq!(latest, vec!["child@0.10", "parent@0.1"]);
        let parent = project.jobs.iter().find(|j| j.name == "parent").unwrap();
        let calls = project.calls(parent);
        assert_eq!(calls.len(), 1);
        let resolved = calls[0].resolved.as_ref().unwrap();
        assert_eq!(resolved.version, "0.10");
        assert!(resolved.via_latest);
        assert!(calls[0].die_on_child_error);
        assert_eq!(project.not_read.get("process_mr"), Some(&1));
        assert!(!format!("{project:?}").contains("D41D8CD9"));
    }

    #[test]
    fn a_broken_file_is_a_diagnostic_and_the_rest_is_still_read() {
        let files = vec![
            file("talend.project", PROJECT),
            file("process/a_0.1.item", "<talendfile:ProcessType"),
            file("process/b_0.1.item", &job_item(None)),
        ];
        let project = read_project("", &files);
        assert_eq!(project.jobs.len(), 1);
        let codes: Vec<_> = project
            .diagnostics
            .iter()
            .map(|d| d.code.as_str())
            .collect();
        assert_eq!(codes, vec!["TALEND001", "TALEND003"]);
    }

    #[test]
    fn a_directory_without_talend_project_says_so() {
        let project = read_project("x", &[]);
        assert_eq!(project.diagnostics[0].code, "TALEND004");
    }
}
