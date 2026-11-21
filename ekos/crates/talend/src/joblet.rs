//! Joblets (RFC 0263 §Joblets): which components are joblet uses, and a job with every use
//! inlined, as Studio does when it generates code.
//!
//! *(doc)*: no fixture holds a joblet yet. The shapes follow Talend's EMF model and two
//! independent readers of Studio files; the tests here are synthetic.

use std::collections::BTreeSet;

use crate::catalogue::Category;
use crate::model::{Component, Diagnostic, Job, JobletOrigin, Link, LinkKind, Param, Project};

/// Deepest joblet nesting expanded; a deeper use is `TALEND006` and stays unexpanded.
pub const MAX_DEPTH: usize = 8;

const INPUT_PORT: &str = "joblet_input";
const OUTPUT_PORT: &str = "joblet_output";

/// Marks every component whose type is the name of one of the project's joblets as a use.
pub(crate) fn mark_uses(project: &mut Project) {
    let names: BTreeSet<String> = project.joblets.iter().map(|j| j.name.clone()).collect();
    if names.is_empty() {
        return;
    }
    for job in project.jobs.iter_mut().chain(project.joblets.iter_mut()) {
        for c in &mut job.components {
            if c.category == Category::Unknown && names.contains(&c.component_type) {
                c.category = Category::Joblet;
                c.logical = "joblet".to_string();
            }
        }
    }
}

/// One joblet use, resolved.
#[derive(Debug, Clone, PartialEq)]
pub struct JobletUse<'a> {
    /// The use's unique name in the job.
    pub component: String,
    pub joblet: &'a Job,
    /// `true` when the use asked for no particular version and got the latest.
    pub via_latest: bool,
}

/// A job with its joblet uses inlined.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Expanded {
    pub job: Job,
    /// Uses inlined, nested ones included.
    pub inlined: usize,
    /// `TALEND005` / `TALEND006`.
    pub diagnostics: Vec<Diagnostic>,
}

impl Project {
    /// The joblet a component uses: the version `PROCESS_TYPE_VERSION` names, else the latest.
    pub fn joblet_for(&self, c: &Component) -> Option<(&Job, bool)> {
        if c.category != Category::Joblet {
            return None;
        }
        let requested = c
            .params
            .get("PROCESS_TYPE_VERSION")
            .and_then(Param::text)
            .map(|v| v.trim_matches('"'))
            .filter(|v| !v.is_empty() && *v != "Latest");
        let same = |j: &&Job| j.name == c.component_type;
        match requested {
            Some(v) => self
                .joblets
                .iter()
                .filter(same)
                .find(|j| j.version == v)
                .map(|j| (j, false)),
            None => self
                .joblets
                .iter()
                .filter(same)
                .find(|j| j.latest)
                .map(|j| (j, true)),
        }
    }

    /// The joblet uses of a job (or joblet) that resolve to a joblet of the project.
    pub fn joblet_uses<'a>(&'a self, job: &Job) -> Vec<JobletUse<'a>> {
        job.components
            .iter()
            .filter_map(|c| {
                let (joblet, via_latest) = self.joblet_for(c)?;
                Some(JobletUse {
                    component: c.unique_name.clone(),
                    joblet,
                    via_latest,
                })
            })
            .collect()
    }

    /// The job with every joblet use inlined (RFC 0263 §Joblets). A job without uses comes back
    /// unchanged.
    pub fn expand(&self, job: &Job) -> Expanded {
        let mut out = Expanded::default();
        let mut stack = vec![job.name.clone()];
        out.job = self.expand_into(job, &mut stack, &mut out.inlined, &mut out.diagnostics);
        out
    }

    fn expand_into(
        &self,
        job: &Job,
        stack: &mut Vec<String>,
        inlined: &mut usize,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Job {
        let mut out = job.clone();
        let uses: Vec<(String, &Job)> = self
            .joblet_uses(job)
            .into_iter()
            .map(|u| (u.component, u.joblet))
            .collect();
        for (use_name, joblet) in uses {
            if stack.contains(&joblet.name) || stack.len() > MAX_DEPTH {
                diagnostics.push(Diagnostic {
                    code: "TALEND006".into(),
                    path: job.path.clone(),
                    message: format!(
                        "{use_name}: joblet {} not expanded (nesting {} deep, or it uses itself)",
                        joblet.name,
                        stack.len()
                    ),
                });
                continue;
            }
            stack.push(joblet.name.clone());
            let body = self.expand_into(joblet, stack, inlined, diagnostics);
            stack.pop();
            splice(&mut out, &use_name, &body, joblet, diagnostics);
            *inlined += 1;
        }
        out
    }
}

fn is_port(c: &Component) -> bool {
    matches!(c.logical.as_str(), INPUT_PORT | OUTPUT_PORT)
        || c.logical.starts_with("joblet_trigger_")
}

/// Which of `ports` a caller link goes through: the only one, else the one its connector or
/// `metaname` names.
fn choose_port<'a>(ports: &[&'a str], link: &Link) -> Option<&'a str> {
    if ports.len() == 1 {
        return Some(ports[0]);
    }
    let named = |name: &str| {
        matches!(&link.kind, LinkKind::Other(c) if c == name)
            || link.metaname.as_deref() == Some(name)
    };
    ports.iter().copied().find(|p| named(p))
}

/// A link out of a joblet's output port joined to a caller's: kind and target end from the
/// caller (`downstream`), source end from inside (`upstream`).
fn joined(upstream: &Link, downstream: &Link) -> Link {
    let kind = if upstream.kind == LinkKind::Lookup || downstream.kind == LinkKind::Lookup {
        LinkKind::Lookup
    } else {
        match &downstream.kind {
            LinkKind::Other(_) => upstream.kind.clone(),
            k => k.clone(),
        }
    };
    let kind = match kind {
        LinkKind::Other(_) => LinkKind::Main,
        k => k,
    };
    Link {
        kind,
        from: upstream.from.clone(),
        to: downstream.to.clone(),
        label: downstream.label.clone(),
        metaname: upstream.metaname.clone(),
        line: downstream.line,
    }
}

/// One joblet use being inlined: the expanded joblet and its ports.
struct Splice<'a> {
    use_name: &'a str,
    body: &'a Job,
    joblet: &'a Job,
    inputs: Vec<&'a str>,
    outputs: Vec<&'a str>,
    ports: BTreeSet<&'a str>,
    inner: BTreeSet<&'a str>,
}

impl<'a> Splice<'a> {
    fn new(use_name: &'a str, body: &'a Job, joblet: &'a Job) -> Self {
        let names = |keep: &dyn Fn(&Component) -> bool| -> Vec<&'a str> {
            body.components
                .iter()
                .filter(|c| keep(c))
                .map(|c| c.unique_name.as_str())
                .collect()
        };
        Self {
            use_name,
            body,
            joblet,
            inputs: names(&|c| c.logical == INPUT_PORT),
            outputs: names(&|c| c.logical == OUTPUT_PORT),
            ports: names(&is_port).into_iter().collect(),
            inner: names(&|_| true).into_iter().collect(),
        }
    }

    fn prefix(&self, name: &str) -> String {
        format!("{}__{name}", self.use_name)
    }

    fn is_port(&self, name: &str) -> bool {
        self.ports.contains(name)
    }

    fn copy_component(&self, c: &Component) -> Component {
        let mut copy = c.clone();
        copy.unique_name = self.prefix(&c.unique_name);
        for p in copy.params.values_mut() {
            if let Param::Value { value, .. } = p
                && self.inner.contains(value.as_str())
            {
                *value = self.prefix(value);
            }
        }
        copy.joblet = Some(match c.joblet.clone() {
            Some(mut nested) => {
                nested.uses.insert(0, self.use_name.to_string());
                nested
            }
            None => JobletOrigin {
                name: self.joblet.name.clone(),
                version: self.joblet.version.clone(),
                uses: vec![self.use_name.to_string()],
                component: c.unique_name.clone(),
            },
        });
        copy
    }

    /// The joblet's own links and triggers, neither end a port.
    fn inner_links(&self) -> (Vec<Link>, Vec<crate::model::Trigger>) {
        let links = self
            .body
            .links
            .iter()
            .filter(|l| !self.is_port(&l.from) && !self.is_port(&l.to))
            .map(|l| Link {
                from: self.prefix(&l.from),
                to: self.prefix(&l.to),
                ..l.clone()
            })
            .collect();
        let triggers = self
            .body
            .triggers
            .iter()
            .filter(|t| !self.is_port(&t.from) && !self.is_port(&t.to))
            .map(|t| crate::model::Trigger {
                from: self.prefix(&t.from),
                to: self.prefix(&t.to),
                ..t.clone()
            })
            .collect();
        (links, triggers)
    }

    /// Caller links into the use, joined to the joblet's links out of the port each enters.
    fn inbound(&self, entering: &[(&Link, &str)]) -> Vec<Link> {
        let mut out = Vec::new();
        for (l, port) in entering {
            for inside in self
                .body
                .links
                .iter()
                .filter(|i| i.from == *port && !self.is_port(&i.to))
            {
                let inside = Link {
                    to: self.prefix(&inside.to),
                    ..inside.clone()
                };
                out.push(joined(l, &inside));
            }
        }
        out
    }

    /// A caller link out of the use, joined to the joblet's links into its output port.
    fn outbound(&self, l: &Link, port: &str, entering: &[(&Link, &str)]) -> Vec<Link> {
        let mut out = Vec::new();
        for inside in self.body.links.iter().filter(|i| i.to == port) {
            if self.inputs.contains(&inside.from.as_str()) {
                // Straight through the joblet: input port → output port.
                out.extend(
                    entering
                        .iter()
                        .filter(|(_, p)| *p == inside.from)
                        .map(|(upstream, _)| joined(upstream, l)),
                );
            } else if !self.is_port(&inside.from) {
                let inside = Link {
                    from: self.prefix(&inside.from),
                    ..inside.clone()
                };
                out.push(joined(&inside, l));
            }
        }
        out
    }

    fn unrouted(&self, job: &Job, link: &Link, side: &str) -> Diagnostic {
        Diagnostic {
            code: "TALEND005".into(),
            path: job.path.clone(),
            message: format!(
                "{}: the link {} {side} joblet {} names none of its ports; dropped",
                self.use_name,
                link.label.as_deref().unwrap_or("(unnamed)"),
                self.joblet.name
            ),
        }
    }
}

/// Replaces `use_name` in `out` with `body` (an already expanded joblet).
fn splice(
    out: &mut Job,
    use_name: &str,
    body: &Job,
    joblet: &Job,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let sp = Splice::new(use_name, body, joblet);
    let copied: Vec<Component> = body
        .components
        .iter()
        .filter(|c| !is_port(c))
        .map(|c| sp.copy_component(c))
        .collect();
    let (mut links, triggers) = sp.inner_links();
    let mut entering: Vec<(&Link, &str)> = Vec::new();
    for l in out.links.iter().filter(|l| l.to == use_name) {
        match choose_port(&sp.inputs, l) {
            Some(p) => entering.push((l, p)),
            None => diagnostics.push(sp.unrouted(out, l, "into")),
        }
    }
    links.extend(sp.inbound(&entering));
    for l in out.links.iter().filter(|l| l.from == use_name) {
        match choose_port(&sp.outputs, l) {
            Some(port) => links.extend(sp.outbound(l, port, &entering)),
            None => diagnostics.push(sp.unrouted(out, l, "out of")),
        }
    }
    out.triggers.extend(triggers);
    out.links.retain(|l| l.from != use_name && l.to != use_name);
    out.links.extend(links);
    let at = out
        .components
        .iter()
        .position(|c| c.unique_name == use_name)
        .unwrap_or(out.components.len());
    let end = (at + 1).min(out.components.len());
    out.components.splice(at..end, copied);
    adopt_context(out, body);
}

/// Adds the joblet's default-context variables the job does not define to the job's default
/// group, so the joblet's expressions fold.
fn adopt_context(out: &mut Job, body: &Job) {
    let Some(source) = body
        .context_groups
        .iter()
        .find(|g| g.name == body.default_context)
        .or_else(|| body.context_groups.first())
    else {
        return;
    };
    let default = out.default_context.clone();
    if out.context_groups.is_empty() {
        out.context_groups.push(crate::model::ContextGroup {
            name: default.clone(),
            variables: Vec::new(),
        });
    }
    let at = out
        .context_groups
        .iter()
        .position(|g| g.name == default)
        .unwrap_or(0);
    let group = &mut out.context_groups[at];
    for v in &source.variables {
        if !group.variables.iter().any(|x| x.name == v.name) {
            group.variables.push(v.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{ProjectFile, read_project};

    fn file(path: &str, text: &str) -> ProjectFile {
        ProjectFile {
            path: path.into(),
            text: text.into(),
        }
    }

    fn node(kind: &str, name: &str, extra: &str) -> String {
        format!(
            r#"<node componentName="{kind}"><elementParameter field="TEXT" name="UNIQUE_NAME" value="{name}"/>{extra}</node>"#
        )
    }

    fn link(label: &str, from: &str, to: &str) -> String {
        format!(
            r#"<connection connectorName="FLOW" label="{label}" lineStyle="0" source="{from}" target="{to}"/>"#
        )
    }

    /// Synthetic, in the shapes RFC 0263 §Joblets describes: a joblet that maps its input
    /// through a tMap reading the label `row1`, used by a job whose own link is also `row1`.
    fn files() -> Vec<ProjectFile> {
        let joblet = format!(
            r#"<model:JobletProcess xmlns:model="http://www.talend.com/joblet.ecore" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:TalendMapper="http://www.talend.org/mapper" defaultContext="Default"><context name="Default"><contextParameter name="suffix" type="id_String" value="_X"/></context><jobletNodes componentName="INPUT"><elementParameter field="TEXT" name="UNIQUE_NAME" value="INPUT_1"/></jobletNodes>{}{}<jobletNodes componentName="OUTPUT"><elementParameter field="TEXT" name="UNIQUE_NAME" value="OUTPUT_1"/></jobletNodes>{}{}{}</model:JobletProcess>"#,
            node(
                "tMap",
                "tMap_1",
                r#"<nodeData xsi:type="TalendMapper:MapperData"><outputTables name="clean"><mapperTableEntries name="CODE" expression="row1.CODE.trim()"/></outputTables><inputTables name="row1"/></nodeData>"#
            ),
            node("tFilterRow", "tFilterRow_1", ""),
            link("row1", "INPUT_1", "tMap_1"),
            r#"<connection connectorName="FLOW" label="clean" lineStyle="0" metaname="clean" source="tMap_1" target="tFilterRow_1"/>"#,
            link("row2", "tFilterRow_1", "OUTPUT_1"),
        );
        let job = format!(
            r#"<talendfile:ProcessType xmlns:talendfile="platform:/resource/org.talend.model/model/TalendFile.xsd" defaultContext="Default">{}{}{}{}{}</talendfile:ProcessType>"#,
            node("tFileInputDelimited", "tFileInputDelimited_1", ""),
            node("jlClean", "jlClean_1", ""),
            node("tLogRow", "tLogRow_1", ""),
            link("row1", "tFileInputDelimited_1", "jlClean_1"),
            link("row3", "jlClean_1", "tLogRow_1"),
        );
        vec![
            file(
                "talend.project",
                r#"<xmi:XMI xmlns:xmi="http://www.omg.org/XMI" xmlns:TalendProperties="http://www.talend.org/properties"><TalendProperties:Project label="Demo" technicalLabel="DEMO"/></xmi:XMI>"#,
            ),
            file("joblets/jlClean_0.1.item", &joblet),
            file("joblets/jlClean_0.2.item", &joblet),
            file("process/j_0.1.item", &job),
        ]
    }

    #[test]
    fn a_joblet_is_read_and_its_use_is_marked() {
        let p = read_project("", &files());
        assert_eq!(p.joblets.len(), 2);
        assert!(p.joblets.iter().any(|j| j.version == "0.2" && j.latest));
        let ports: Vec<&str> = p.joblets[0]
            .components
            .iter()
            .map(|c| c.logical.as_str())
            .collect();
        assert!(ports.contains(&"joblet_input") && ports.contains(&"joblet_output"));
        let job = &p.jobs[0];
        let uses = p.joblet_uses(job);
        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].joblet.version, "0.2");
        assert!(uses[0].via_latest);
    }

    #[test]
    fn expansion_inlines_the_joblet_and_joins_links_through_its_ports() {
        let p = read_project("", &files());
        let e = p.expand(&p.jobs[0]);
        assert!(e.diagnostics.is_empty(), "{:?}", e.diagnostics);
        assert_eq!(e.inlined, 1);
        let names: Vec<&str> = e
            .job
            .components
            .iter()
            .map(|c| c.unique_name.as_str())
            .collect();
        assert_eq!(
            names,
            [
                "tFileInputDelimited_1",
                "jlClean_1__tMap_1",
                "jlClean_1__tFilterRow_1",
                "tLogRow_1"
            ]
        );
        let edge = |from: &str, to: &str| {
            e.job
                .links
                .iter()
                .find(|l| l.from == from && l.to == to)
                .unwrap_or_else(|| panic!("no link {from} -> {to}: {:?}", e.job.links))
        };
        // Into the joblet: the caller's source, the joblet's label (what its tMap reads).
        assert_eq!(
            edge("tFileInputDelimited_1", "jlClean_1__tMap_1")
                .label
                .as_deref(),
            Some("row1")
        );
        assert_eq!(
            edge("jlClean_1__tMap_1", "jlClean_1__tFilterRow_1")
                .metaname
                .as_deref(),
            Some("clean")
        );
        // Out of it: the caller's label.
        assert_eq!(
            edge("jlClean_1__tFilterRow_1", "tLogRow_1")
                .label
                .as_deref(),
            Some("row3")
        );
        assert!(!e.job.links.iter().any(|l| l.to == "jlClean_1"));
        let origin = e.job.components[1].joblet.as_ref().unwrap();
        assert_eq!(
            (origin.name.as_str(), origin.version.as_str()),
            ("jlClean", "0.2")
        );
        assert_eq!(origin.uses, ["jlClean_1"]);
        // The joblet's context variable is now the job's.
        assert!(e.job.context_var("suffix").is_some());
    }

    #[test]
    fn a_joblet_that_uses_itself_is_reported_not_expanded_forever() {
        let joblet = r#"<model:JobletProcess xmlns:model="http://www.talend.com/joblet.ecore"><node componentName="jlLoop"><elementParameter field="TEXT" name="UNIQUE_NAME" value="jlLoop_1"/></node></model:JobletProcess>"#;
        let job = r#"<talendfile:ProcessType xmlns:talendfile="platform:/resource/org.talend.model/model/TalendFile.xsd"><node componentName="jlLoop"><elementParameter field="TEXT" name="UNIQUE_NAME" value="jlLoop_1"/></node></talendfile:ProcessType>"#;
        let p = read_project(
            "",
            &[
                file("joblets/jlLoop_0.1.item", joblet),
                file("process/j_0.1.item", job),
            ],
        );
        let e = p.expand(&p.jobs[0]);
        assert!(e.diagnostics.iter().any(|d| d.code == "TALEND006"));
    }

    #[test]
    fn a_job_without_joblets_expands_to_itself() {
        let job = r#"<talendfile:ProcessType xmlns:talendfile="platform:/resource/org.talend.model/model/TalendFile.xsd"><node componentName="tLogRow"><elementParameter field="TEXT" name="UNIQUE_NAME" value="tLogRow_1"/></node></talendfile:ProcessType>"#;
        let p = read_project("", &[file("process/j_0.1.item", job)]);
        let e = p.expand(&p.jobs[0]);
        assert_eq!(e.job, p.jobs[0]);
        assert_eq!(e.inlined, 0);
    }
}
