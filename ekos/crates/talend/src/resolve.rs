//! What a static reader can resolve (RFC 0263 §"Resolution"): context values with how they were
//! resolved, Java strings folded over them, and `tRunJob` calls.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::java::{Folded, fold};
use crate::model::{Component, ContextVar, Job, Param, Project};

/// A context value as the job would see it by default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextValue {
    pub name: String,
    pub group: String,
    pub value: String,
    /// True when the value can be replaced at run time: implicit context load, a `tContextLoad`,
    /// or an assignment in a `tJava`.
    pub overridable_at_runtime: bool,
}

/// How a job's context can change while it runs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeContext {
    pub implicit_context_load: bool,
    /// Unique names of `tContextLoad` components.
    pub context_loads: Vec<String>,
    /// Variables a `tJava`/`tJavaRow`/`tJavaFlex` assigns (`context.x = …`).
    pub assigned: BTreeSet<String>,
}

impl RuntimeContext {
    pub fn of(job: &Job) -> Self {
        let mut rc = Self {
            implicit_context_load: job.implicit_context_load,
            ..Self::default()
        };
        for c in &job.components {
            if c.component_type == "tContextLoad" {
                rc.context_loads.push(c.unique_name.clone());
            }
            if c.component_type.starts_with("tJava") {
                for code in c.params.values().filter_map(Param::text) {
                    rc.assigned.extend(context_assignments(code));
                }
            }
        }
        rc
    }

    pub fn overridable(&self, name: &str) -> bool {
        self.implicit_context_load || !self.context_loads.is_empty() || self.assigned.contains(name)
    }
}

/// `context.x = …` (not `==`) in Java code.
pub fn context_assignments(code: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = code;
    while let Some(at) = rest.find("context.") {
        let before_ok = rest[..at]
            .chars()
            .next_back()
            .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'));
        let after = &rest[at + "context.".len()..];
        let end = after
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(after.len());
        let name = &after[..end];
        let tail = after[end..].trim_start();
        if before_ok && !name.is_empty() && tail.starts_with('=') && !tail.starts_with("==") {
            out.insert(name.to_string());
        }
        rest = &after[end..];
    }
    out
}

impl Job {
    /// The default context group's variable.
    pub fn context_var(&self, name: &str) -> Option<&ContextVar> {
        let group = self
            .context_groups
            .iter()
            .find(|g| g.name == self.default_context)
            .or_else(|| self.context_groups.first())?;
        group.variables.iter().find(|v| v.name == name)
    }

    /// A non-secret context value from the default group.
    pub fn context_value(&self, name: &str) -> Option<ContextValue> {
        let var = self.context_var(name)?;
        let value = var.value.clone()?;
        Some(ContextValue {
            name: name.to_string(),
            group: self.default_context.clone(),
            value,
            overridable_at_runtime: RuntimeContext::of(self).overridable(name),
        })
    }

    /// Folds a Java expression over this job's default context.
    pub fn fold(&self, expr: &str) -> Folded {
        fold(expr, &|name| {
            self.context_var(name).and_then(|v| v.value.clone())
        })
    }

    pub fn component(&self, unique_name: &str) -> Option<&Component> {
        self.components
            .iter()
            .find(|c| c.unique_name == unique_name)
    }
}

/// One `tRunJob` call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Call {
    /// The calling component's unique name.
    pub component: String,
    /// `PROCESS`: the child's name.
    pub child_name: String,
    /// The child's repository id (`PROCESS:PROCESS_TYPE_PROCESS`, project prefix removed).
    pub child_id: Option<String>,
    /// `Latest` or a version number.
    pub requested_version: Option<String>,
    /// The job version present in the project that the call runs, when found.
    pub resolved: Option<ResolvedJob>,
    pub context: Option<String>,
    pub transmit_whole_context: bool,
    pub die_on_child_error: bool,
    /// The child is chosen at run time (`USE_DYNAMIC_JOB`).
    pub dynamic: bool,
    /// Context parameters passed explicitly: (name, Java value).
    pub context_params: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedJob {
    pub name: String,
    pub version: String,
    pub path: String,
    /// True when `Latest` was resolved to the highest version present: what Studio would pick
    /// here, which a deployed build may not.
    pub via_latest: bool,
}

impl Project {
    /// Every `tRunJob` call of `job`.
    pub fn calls(&self, job: &Job) -> Vec<Call> {
        job.components
            .iter()
            .filter(|c| c.component_type == "tRunJob")
            .map(|c| self.call(c))
            .collect()
    }

    fn call(&self, c: &Component) -> Call {
        let text = |name: &str| {
            c.params
                .get(name)
                .and_then(Param::text)
                .filter(|v| !v.is_empty())
                .map(str::to_string)
        };
        let flag = |name: &str| text(name).as_deref() == Some("true");
        let child_name = text("PROCESS").unwrap_or_default();
        let child_id = text("PROCESS:PROCESS_TYPE_PROCESS").map(|v| {
            v.rsplit_once(':')
                .map_or(v.clone(), |(_, id)| id.to_string())
        });
        let requested_version = text("PROCESS:PROCESS_TYPE_VERSION");
        let resolved = self.resolve_job(
            child_id.as_deref(),
            &child_name,
            requested_version.as_deref(),
        );
        let context_params = c
            .params
            .get("CONTEXTPARAMS")
            .map(Param::rows)
            .unwrap_or_default()
            .iter()
            .map(|row| {
                (
                    row.get("PARAM_NAME_COLUMN").cloned().unwrap_or_default(),
                    row.get("PARAM_VALUE_COLUMN").cloned().unwrap_or_default(),
                )
            })
            .collect();
        Call {
            component: c.unique_name.clone(),
            child_name,
            child_id,
            requested_version,
            resolved,
            context: text("PROCESS:PROCESS_TYPE_CONTEXT").or_else(|| text("CONTEXT_NAME")),
            transmit_whole_context: flag("TRANSMIT_WHOLE_CONTEXT"),
            die_on_child_error: flag("DIE_ON_CHILD_ERROR"),
            dynamic: flag("USE_DYNAMIC_JOB"),
            context_params,
        }
    }

    fn resolve_job(
        &self,
        id: Option<&str>,
        name: &str,
        version: Option<&str>,
    ) -> Option<ResolvedJob> {
        let candidates: Vec<&Job> = self
            .jobs
            .iter()
            .filter(|j| match (id, &j.id) {
                (Some(id), Some(jid)) => id == jid,
                _ => j.name == name,
            })
            .collect();
        let via_latest = version.is_none_or(|v| v == "Latest");
        let job = if via_latest {
            candidates.into_iter().find(|j| j.latest)
        } else {
            candidates
                .into_iter()
                .find(|j| Some(j.version.as_str()) == version)
        }?;
        Some(ResolvedJob {
            name: job.name.clone(),
            version: job.version.clone(),
            path: job.path.clone(),
            via_latest,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_assignments_make_a_context_variable_runtime() {
        let a = context_assignments(
            "context.nbJours = 3; if (context.x == 1) {} String s = context.y; my.context.z = 2;",
        );
        assert_eq!(a.into_iter().collect::<Vec<_>>(), vec!["nbJours"]);
    }
}
