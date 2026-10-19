//! RFC 0181 — read scope: what one reader may see of a project's ledger.
//!
//! [`ScopedStore`] wraps a [`KnowledgeStore`] and filters every read by a [`ReadScope`]. It is
//! enforced here, at the store, because everything that reads knowledge — `Runtime`, EKL, graph
//! export, every command — takes a `&dyn KnowledgeStore`. Given the wrapper, none of them can
//! return what it hides, including the ones nobody has written yet.
//!
//! The decision, for an object:
//!
//! * its **origins** are (source, path) pairs: its `sources` property (RFC 0178) crossed with
//!   the `location.path` of each of its evidence records. No source, or no path, is `None`;
//! * it is **denied** when a deny rule names its id or matches any origin. A deny rule matches
//!   an origin whose source or path is unknown: what cannot be placed is treated as inside;
//! * it is **allowed** when the default is allow, or an allow rule names its id, or every origin
//!   is matched by an allow rule. An allow rule with a source does not match "no source", and
//!   one with a path does not match "no path";
//! * it is **visible** when allowed and not denied.
//!
//! A relationship is visible when both its ends are. An evidence record is returned only for an
//! id listed by an object (or relationship) already found visible.
//!
//! A new method on [`KnowledgeStore`] needs a decision here; a test below reads the trait's
//! source and fails when one is missing.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use ekos_kir::{KirEvent, KirEvidence, KirId, KirObject, KirRelationship};
use serde::{Deserialize, Serialize};

use crate::retrieval::{RankedResults, RetrievalRequest};
use crate::{KnowledgeReader, KnowledgeStore, LedgerDiff, LedgerError, RefreshOutcome, provenance};

/// How many times the requested number of search hits is asked of the inner store, so that a
/// page from which hidden hits were removed is not short.
const OVERFETCH: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    Allow,
    Deny,
}

/// One rule. Any of `source`, `path` and `object` may be given; a rule with `object` names
/// that object alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub effect: Effect,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// A pattern over a path inside a source: `*` within one component, `**` across them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<KirId>,
}

/// What one reader may see. `default` has no default: a scope that lost a field in transit must
/// fail to parse, not become "allow everything".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadScope {
    pub default: Effect,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ScopeError {
    #[error("read scope is not valid JSON for a scope: {0}")]
    Parse(String),
    #[error("read scope rule {index}: {reason}")]
    Rule { index: usize, reason: String },
}

impl ReadScope {
    /// Everything is visible.
    pub fn unrestricted() -> Self {
        Self {
            default: Effect::Allow,
            rules: Vec::new(),
        }
    }

    /// True when this scope hides nothing: default allow and no deny rule.
    pub fn is_unrestricted(&self) -> bool {
        self.default == Effect::Allow && self.rules.iter().all(|r| r.effect == Effect::Allow)
    }

    pub fn from_json(text: &str) -> Result<Self, ScopeError> {
        let scope: Self =
            serde_json::from_str(text).map_err(|e| ScopeError::Parse(e.to_string()))?;
        scope.check()?;
        Ok(scope)
    }

    pub fn from_value(value: &serde_json::Value) -> Result<Self, ScopeError> {
        let scope: Self =
            serde_json::from_value(value.clone()).map_err(|e| ScopeError::Parse(e.to_string()))?;
        scope.check()?;
        Ok(scope)
    }

    fn check(&self) -> Result<(), ScopeError> {
        Compiled::new(self).map(|_| ())
    }

    /// A stable text form, for a cache key: two equal scopes give the same string.
    pub fn fingerprint(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// One thing an object was compiled from.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Origin {
    pub source: Option<String>,
    pub path: Option<String>,
}

struct CompiledRule {
    effect: Effect,
    source: Option<String>,
    path: Option<glob::Pattern>,
    object: Option<KirId>,
}

/// A scope with its patterns compiled.
pub struct Compiled {
    default: Effect,
    rules: Vec<CompiledRule>,
    has_path_rules: bool,
}

const MATCH: glob::MatchOptions = glob::MatchOptions {
    case_sensitive: true,
    // `*` stays inside one path component; `**` crosses them.
    require_literal_separator: true,
    require_literal_leading_dot: false,
};

/// A path as rules see it: forward slashes, no leading `./`.
fn normalize(path: &str) -> String {
    let path = path.replace('\\', "/");
    path.strip_prefix("./").unwrap_or(&path).to_string()
}

impl Compiled {
    pub fn new(scope: &ReadScope) -> Result<Self, ScopeError> {
        let mut rules = Vec::with_capacity(scope.rules.len());
        for (index, rule) in scope.rules.iter().enumerate() {
            let fail = |reason: &str| ScopeError::Rule {
                index,
                reason: reason.to_string(),
            };
            if rule.object.is_some() && (rule.source.is_some() || rule.path.is_some()) {
                return Err(fail(
                    "a rule names an object, or a source and path, not both",
                ));
            }
            if rule.source.as_deref() == Some("") || rule.path.as_deref() == Some("") {
                return Err(fail("an empty source or path; leave the field out instead"));
            }
            let path = match &rule.path {
                Some(p) => Some(
                    glob::Pattern::new(&normalize(p))
                        .map_err(|e| fail(&format!("path pattern: {e}")))?,
                ),
                None => None,
            };
            rules.push(CompiledRule {
                effect: rule.effect,
                source: rule.source.clone(),
                path,
                object: rule.object,
            });
        }
        Ok(Self {
            default: scope.default,
            has_path_rules: rules.iter().any(|r| r.path.is_some()),
            rules,
        })
    }

    fn is_unrestricted(&self) -> bool {
        self.default == Effect::Allow && self.rules.iter().all(|r| r.effect == Effect::Allow)
    }

    /// Whether `pattern` matches the origin's path. A path may have been recorded as
    /// `<source>/<path>`, so that form is tried without its source as well.
    fn path_matches(pattern: &glob::Pattern, origin: &Origin, path: &str) -> bool {
        if pattern.matches_with(path, MATCH) {
            return true;
        }
        origin
            .source
            .as_deref()
            .and_then(|s| path.strip_prefix(s))
            .and_then(|rest| rest.strip_prefix('/'))
            .is_some_and(|rest| pattern.matches_with(rest, MATCH))
    }

    /// Whether a deny rule covers this origin. An unknown source or an unknown path counts as
    /// covered: what cannot be placed is treated as inside the denied area. Without that, an
    /// object the compiler could not attribute (a commit and the files it lists, say) would stay
    /// visible to a reader who was denied exactly those files.
    fn denies(rule: &CompiledRule, origin: &Origin) -> bool {
        if let (Some(wanted), Some(actual)) = (&rule.source, &origin.source)
            && wanted != actual
        {
            return false;
        }
        match (&rule.path, &origin.path) {
            (None, _) => true,
            (Some(_), None) => true,
            (Some(pattern), Some(path)) => Self::path_matches(pattern, origin, path),
        }
    }

    fn allows(rule: &CompiledRule, origin: &Origin) -> bool {
        if let Some(source) = &rule.source
            && origin.source.as_deref() != Some(source.as_str())
        {
            return false;
        }
        match (&rule.path, &origin.path) {
            (None, _) => true,
            (Some(_), None) => false,
            (Some(pattern), Some(path)) => Self::path_matches(pattern, origin, path),
        }
    }

    /// The decision for one object, given its id and origins. `origins` is never empty: an
    /// object nothing is known about has the single origin (no source, no path).
    pub fn visible(&self, id: &KirId, origins: &[Origin]) -> bool {
        let named = |r: &CompiledRule| r.object.as_ref() == Some(id);
        let denied = self
            .rules
            .iter()
            .filter(|r| r.effect == Effect::Deny)
            .any(|r| match r.object {
                Some(_) => named(r),
                None => origins.iter().any(|o| Self::denies(r, o)),
            });
        if denied {
            return false;
        }
        if self.default == Effect::Allow {
            return true;
        }
        let allow = || self.rules.iter().filter(|r| r.effect == Effect::Allow);
        if allow().any(named) {
            return true;
        }
        !origins.is_empty()
            && origins
                .iter()
                .all(|o| allow().any(|r| r.object.is_none() && Self::allows(r, o)))
    }
}

/// The sources an object records (RFC 0178): `sources`, or the single `source`.
fn sources_of(obj: &KirObject) -> Vec<String> {
    let mut out: Vec<String> = obj
        .properties
        .get("sources")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    if out.is_empty()
        && let Some(one) = obj.properties.get("source").and_then(|v| v.as_str())
    {
        out.push(one.to_string());
    }
    out.sort();
    out.dedup();
    out
}

/// Sources crossed with paths; `None` stands in for an empty side.
pub fn origins(sources: &[String], paths: &[String]) -> Vec<Origin> {
    let sources: Vec<Option<String>> = if sources.is_empty() {
        vec![None]
    } else {
        sources.iter().cloned().map(Some).collect()
    };
    let paths: Vec<Option<String>> = if paths.is_empty() {
        vec![None]
    } else {
        paths.iter().map(|p| Some(normalize(p))).collect()
    };
    let mut out = Vec::with_capacity(sources.len() * paths.len());
    for s in &sources {
        for p in &paths {
            out.push(Origin {
                source: s.clone(),
                path: p.clone(),
            });
        }
    }
    out
}

#[derive(Default)]
struct State {
    scopes: Vec<Compiled>,
    /// A cache key for whoever caches results computed through this store.
    fingerprint: String,
    /// The decision for the current version of an object, by id.
    seen: HashMap<KirId, bool>,
    /// Evidence ids listed by something already found visible.
    evidence: HashSet<KirId>,
}

/// A [`KnowledgeStore`] that shows only what its scopes allow. With no restricting scope every
/// call goes straight to the inner store.
pub struct ScopedStore {
    inner: Box<dyn KnowledgeStore>,
    state: Mutex<State>,
}

impl ScopedStore {
    /// Wrap `inner`, unrestricted until [`Self::set_scopes`] says otherwise.
    pub fn new(inner: Box<dyn KnowledgeStore>) -> Self {
        Self {
            inner,
            state: Mutex::new(State::default()),
        }
    }

    pub fn with_scopes(
        inner: Box<dyn KnowledgeStore>,
        scopes: &[ReadScope],
    ) -> Result<Self, ScopeError> {
        let store = Self::new(inner);
        store.set_scopes(scopes)?;
        Ok(store)
    }

    /// Replace the scopes. An object must be visible under every one of them, so adding a scope
    /// can only hide more. Scopes that hide nothing are dropped.
    pub fn set_scopes(&self, scopes: &[ReadScope]) -> Result<(), ScopeError> {
        let mut compiled = Vec::new();
        let mut fingerprint = String::new();
        for scope in scopes {
            let c = Compiled::new(scope)?;
            if !c.is_unrestricted() {
                fingerprint.push_str(&scope.fingerprint());
                fingerprint.push('\n');
                compiled.push(c);
            }
        }
        let mut state = self.lock();
        state.scopes = compiled;
        state.fingerprint = fingerprint;
        state.seen.clear();
        state.evidence.clear();
        Ok(())
    }

    /// True when some scope hides something.
    pub fn is_restricted(&self) -> bool {
        !self.lock().scopes.is_empty()
    }

    /// Empty when unrestricted; otherwise a string that differs between different scope sets.
    pub fn scope_fingerprint(&self) -> String {
        self.lock().fingerprint.clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        // A panic while the lock was held leaves the caches in an unknown state; the scopes
        // themselves are only ever replaced whole, so reading on is safe once they are dropped.
        match self.state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                guard.seen.clear();
                guard.evidence.clear();
                guard
            }
        }
    }

    fn refuse(&self, what: &str) -> LedgerError {
        LedgerError::ReadScope(what.to_string())
    }

    /// The decision for this version of an object. Loads its evidence paths only when a scope
    /// has a path rule.
    fn decide(&self, obj: &KirObject) -> Result<bool, LedgerError> {
        let needs_paths = self.lock().scopes.iter().any(|s| s.has_path_rules);
        let mut paths = Vec::new();
        if needs_paths {
            for id in &obj.evidence {
                if let Some(ev) = self.inner.get_evidence(id)?
                    && !ev.location.path.is_empty()
                {
                    paths.push(ev.location.path);
                }
            }
            paths.sort();
            paths.dedup();
        }
        let origins = origins(&sources_of(obj), &paths);
        let state = self.lock();
        Ok(state.scopes.iter().all(|s| s.visible(&obj.id, &origins)))
    }

    /// Decide for an object value and, when visible, remember that its evidence may be read.
    fn admit(&self, obj: &KirObject, current: bool) -> Result<bool, LedgerError> {
        let visible = self.decide(obj)?;
        let mut state = self.lock();
        if current {
            state.seen.insert(obj.id, visible);
        }
        if visible {
            state.evidence.extend(obj.evidence.iter().copied());
        }
        Ok(visible)
    }

    /// Whether the current version of the object with this id is visible.
    fn visible_id(&self, id: &KirId) -> Result<bool, LedgerError> {
        if let Some(known) = self.lock().seen.get(id) {
            return Ok(*known);
        }
        match self.inner.get_object(id)? {
            Some(obj) => self.admit(&obj, true),
            None => {
                self.lock().seen.insert(*id, false);
                Ok(false)
            }
        }
    }

    fn admit_relationship(&self, rel: &KirRelationship) -> Result<bool, LedgerError> {
        let visible = self.visible_id(&rel.from)? && self.visible_id(&rel.to)?;
        if visible {
            self.lock().evidence.extend(rel.evidence.iter().copied());
        }
        Ok(visible)
    }

    fn keep_objects(
        &self,
        objects: Vec<KirObject>,
        current: bool,
    ) -> Result<Vec<KirObject>, LedgerError> {
        let mut out = Vec::with_capacity(objects.len());
        for obj in objects {
            if self.admit(&obj, current)? {
                out.push(obj);
            }
        }
        Ok(out)
    }

    fn keep_relationships(
        &self,
        rels: Vec<KirRelationship>,
    ) -> Result<Vec<KirRelationship>, LedgerError> {
        let mut out = Vec::with_capacity(rels.len());
        for rel in rels {
            if self.admit_relationship(&rel)? {
                out.push(rel);
            }
        }
        Ok(out)
    }
}

impl KnowledgeReader for ScopedStore {
    fn refresh_snapshot(&self) -> Result<RefreshOutcome, LedgerError> {
        let outcome = self.inner.refresh_snapshot()?;
        // Decisions were made on the versions the old snapshot held.
        let mut state = self.lock();
        state.seen.clear();
        state.evidence.clear();
        Ok(outcome)
    }

    // ── objects ──────────────────────────────────────────────────────────────

    fn get_object(&self, id: &KirId) -> Result<Option<KirObject>, LedgerError> {
        let found = self.inner.get_object(id)?;
        if !self.is_restricted() {
            return Ok(found);
        }
        match found {
            Some(obj) if self.admit(&obj, true)? => Ok(Some(obj)),
            _ => Ok(None),
        }
    }

    fn all_objects(&self) -> Result<Vec<KirObject>, LedgerError> {
        let all = self.inner.all_objects()?;
        if !self.is_restricted() {
            return Ok(all);
        }
        self.keep_objects(all, true)
    }

    fn object_at(&self, id: &KirId, at: DateTime<Utc>) -> Result<Option<KirObject>, LedgerError> {
        let found = self.inner.object_at(id, at)?;
        if !self.is_restricted() {
            return Ok(found);
        }
        match found {
            // Both the version asked for and today's must be visible: an object moved into a
            // hidden area is hidden in its history too.
            Some(obj) if self.admit(&obj, false)? && self.visible_id(id)? => Ok(Some(obj)),
            _ => Ok(None),
        }
    }

    fn all_objects_at(&self, at: DateTime<Utc>) -> Result<Vec<KirObject>, LedgerError> {
        let all = self.inner.all_objects_at(at)?;
        if !self.is_restricted() {
            return Ok(all);
        }
        let mut out = Vec::with_capacity(all.len());
        for obj in all {
            if self.admit(&obj, false)? && self.visible_id(&obj.id)? {
                out.push(obj);
            }
        }
        Ok(out)
    }

    fn object_history(&self, id: &KirId) -> Result<Vec<KirObject>, LedgerError> {
        let versions = self.inner.object_history(id)?;
        if !self.is_restricted() {
            return Ok(versions);
        }
        if !self.visible_id(id)? {
            return Ok(Vec::new());
        }
        self.keep_objects(versions, false)
    }

    fn fact(&self, entity: &KirId, attr: &str) -> Result<Option<serde_json::Value>, LedgerError> {
        if self.is_restricted() && !self.visible_id(entity)? {
            return Ok(None);
        }
        self.inner.fact(entity, attr)
    }

    fn audit_trail(&self, id: &KirId) -> Result<Vec<provenance::AuditRecord>, LedgerError> {
        if self.is_restricted() && !self.visible_id(id)? {
            return Ok(Vec::new());
        }
        self.inner.audit_trail(id)
    }

    // ── relationships: both ends visible ─────────────────────────────────────

    fn get_relationship(&self, id: &KirId) -> Result<Option<KirRelationship>, LedgerError> {
        let found = self.inner.get_relationship(id)?;
        if !self.is_restricted() {
            return Ok(found);
        }
        match found {
            Some(rel) if self.admit_relationship(&rel)? => Ok(Some(rel)),
            _ => Ok(None),
        }
    }

    fn all_relationships(&self) -> Result<Vec<KirRelationship>, LedgerError> {
        let all = self.inner.all_relationships()?;
        if !self.is_restricted() {
            return Ok(all);
        }
        self.keep_relationships(all)
    }

    fn relationships_for(&self, id: &KirId) -> Result<Vec<KirRelationship>, LedgerError> {
        if !self.is_restricted() {
            return self.inner.relationships_for(id);
        }
        if !self.visible_id(id)? {
            return Ok(Vec::new());
        }
        self.keep_relationships(self.inner.relationships_for(id)?)
    }

    fn relationships_at(
        &self,
        id: &KirId,
        at: DateTime<Utc>,
    ) -> Result<Vec<KirRelationship>, LedgerError> {
        if !self.is_restricted() {
            return self.inner.relationships_at(id, at);
        }
        if !self.visible_id(id)? {
            return Ok(Vec::new());
        }
        self.keep_relationships(self.inner.relationships_at(id, at)?)
    }

    fn all_relationships_at(&self, at: DateTime<Utc>) -> Result<Vec<KirRelationship>, LedgerError> {
        let all = self.inner.all_relationships_at(at)?;
        if !self.is_restricted() {
            return Ok(all);
        }
        self.keep_relationships(all)
    }

    fn relationship_history(&self, id: &KirId) -> Result<Vec<KirRelationship>, LedgerError> {
        let versions = self.inner.relationship_history(id)?;
        if !self.is_restricted() {
            return Ok(versions);
        }
        self.keep_relationships(versions)
    }

    // ── evidence and events ──────────────────────────────────────────────────

    fn get_evidence(&self, id: &KirId) -> Result<Option<KirEvidence>, LedgerError> {
        // Evidence carries a path and no source, so it cannot be judged alone. It is readable
        // through something visible that lists it, and not otherwise.
        if self.is_restricted() && !self.lock().evidence.contains(id) {
            return Ok(None);
        }
        self.inner.get_evidence(id)
    }

    fn get_event(&self, id: &KirId) -> Result<Option<KirEvent>, LedgerError> {
        // An event names what happened to other things; nothing ties it to a source.
        if self.is_restricted() {
            return Ok(None);
        }
        self.inner.get_event(id)
    }

    // ── search ───────────────────────────────────────────────────────────────

    fn find_objects(&self, query: &str) -> Result<Vec<(KirId, String)>, LedgerError> {
        let found = self.inner.find_objects(query)?;
        if !self.is_restricted() {
            return Ok(found);
        }
        let mut out = Vec::with_capacity(found.len());
        for (id, name) in found {
            if self.visible_id(&id)? {
                out.push((id, name));
            }
        }
        Ok(out)
    }

    fn retrieve(&self, req: &RetrievalRequest) -> Result<RankedResults, LedgerError> {
        if !self.is_restricted() {
            return self.inner.retrieve(req);
        }
        let mut wider = req.clone();
        wider.limit = req.limit.saturating_mul(OVERFETCH);
        wider.per_arm_limit = req.per_arm_limit.saturating_mul(OVERFETCH);
        let mut results = self.inner.retrieve(&wider)?;
        let mut kept = Vec::with_capacity(req.limit);
        for hit in results.hits {
            if kept.len() >= req.limit {
                break;
            }
            if self.visible_id(&hit.id)? {
                kept.push(hit);
            }
        }
        results.hits = kept;
        Ok(results)
    }

    // ── counts and diff: over what is visible ────────────────────────────────

    fn entry_count(&self) -> Result<usize, LedgerError> {
        if !self.is_restricted() {
            return self.inner.entry_count();
        }
        Ok(self.object_count()? + self.relationship_count()? + self.evidence_count()?)
    }

    fn object_count(&self) -> Result<usize, LedgerError> {
        if !self.is_restricted() {
            return self.inner.object_count();
        }
        Ok(self.all_objects()?.len())
    }

    fn relationship_count(&self) -> Result<usize, LedgerError> {
        if !self.is_restricted() {
            return self.inner.relationship_count();
        }
        Ok(self.all_relationships()?.len())
    }

    fn evidence_count(&self) -> Result<usize, LedgerError> {
        if !self.is_restricted() {
            return self.inner.evidence_count();
        }
        let mut ids: HashSet<KirId> = HashSet::new();
        for obj in self.all_objects()? {
            ids.extend(obj.evidence);
        }
        for rel in self.all_relationships()? {
            ids.extend(rel.evidence);
        }
        Ok(ids.len())
    }

    fn diff(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<LedgerDiff, LedgerError> {
        let diff = self.inner.diff(from, to)?;
        if !self.is_restricted() {
            return Ok(diff);
        }
        let mut touched = Vec::new();
        for id in diff.touched {
            let Ok(uuid) = uuid::Uuid::parse_str(&id) else {
                continue;
            };
            let kir_id = KirId(uuid);
            let visible = match self.inner.get_relationship(&kir_id)? {
                Some(rel) => self.admit_relationship(&rel)?,
                None => self.visible_id(&kir_id)?,
            };
            if visible {
                touched.push(id);
            }
        }
        let tracked = self.object_count()? + self.relationship_count()?;
        Ok(LedgerDiff {
            // Entry ids cannot be tied back to an object here, so none are given.
            added: Vec::new(),
            unchanged: tracked.saturating_sub(touched.len()),
            touched,
        })
    }
}

impl KnowledgeStore for ScopedStore {
    // ── writes: never through a restricted view ──────────────────────────────

    fn append_object(&self, obj: &KirObject) -> Result<bool, LedgerError> {
        if self.is_restricted() {
            return Err(self.refuse("writing an object"));
        }
        self.inner.append_object(obj)
    }

    fn append_evidence(&self, ev: &KirEvidence) -> Result<(), LedgerError> {
        if self.is_restricted() {
            return Err(self.refuse("writing evidence"));
        }
        self.inner.append_evidence(ev)
    }

    fn append_relationship(&self, rel: &KirRelationship) -> Result<bool, LedgerError> {
        if self.is_restricted() {
            return Err(self.refuse("writing a relationship"));
        }
        self.inner.append_relationship(rel)
    }

    fn append_event(&self, ev: &KirEvent) -> Result<(), LedgerError> {
        if self.is_restricted() {
            return Err(self.refuse("writing an event"));
        }
        self.inner.append_event(ev)
    }

    fn set_write_context(&self, ctx: Option<provenance::WriteContext>) {
        self.inner.set_write_context(ctx);
    }

    fn vacuum_into(&self, dest: &Path) -> Result<(), LedgerError> {
        if self.is_restricted() {
            return Err(self.refuse("copying the ledger"));
        }
        self.inner.vacuum_into(dest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::{ObjectKind, RelationshipKind, SourceLocation};

    fn scope(json: &str) -> Compiled {
        Compiled::new(&ReadScope::from_json(json).unwrap()).unwrap()
    }

    fn o(source: Option<&str>, path: Option<&str>) -> Origin {
        Origin {
            source: source.map(str::to_string),
            path: path.map(str::to_string),
        }
    }

    fn id() -> KirId {
        KirId::new()
    }

    // ── parsing ──────────────────────────────────────────────────────────────

    #[test]
    fn a_scope_without_a_default_does_not_parse() {
        assert!(matches!(
            ReadScope::from_json(r#"{"rules": []}"#),
            Err(ScopeError::Parse(_))
        ));
        assert!(ReadScope::from_json("{}").is_err());
        assert!(ReadScope::from_json("null").is_err());
        assert!(ReadScope::from_json("").is_err());
    }

    #[test]
    fn unknown_fields_and_bad_rules_are_refused() {
        for bad in [
            r#"{"default":"deny","rule":[]}"#,
            r#"{"default":"deny","rules":[{"effect":"allow","sources":"a"}]}"#,
            r#"{"default":"deny","rules":[{"effect":"permit","source":"a"}]}"#,
            r#"{"default":"deny","rules":[{"effect":"allow","source":""}]}"#,
            r#"{"default":"deny","rules":[{"effect":"allow","path":"[unclosed"}]}"#,
            r#"{"default":"maybe"}"#,
        ] {
            assert!(ReadScope::from_json(bad).is_err(), "{bad}");
        }
        let both = format!(
            r#"{{"default":"deny","rules":[{{"effect":"allow","source":"a","object":"{}"}}]}}"#,
            id().0
        );
        assert!(matches!(
            ReadScope::from_json(&both),
            Err(ScopeError::Rule { index: 0, .. })
        ));
    }

    #[test]
    fn only_default_allow_without_a_deny_is_unrestricted() {
        assert!(ReadScope::unrestricted().is_unrestricted());
        assert!(
            ReadScope::from_json(r#"{"default":"allow"}"#)
                .unwrap()
                .is_unrestricted()
        );
        assert!(
            !ReadScope::from_json(r#"{"default":"deny"}"#)
                .unwrap()
                .is_unrestricted()
        );
        assert!(
            !ReadScope::from_json(
                r#"{"default":"allow","rules":[{"effect":"deny","source":"a"}]}"#
            )
            .unwrap()
            .is_unrestricted()
        );
    }

    // ── the decision ─────────────────────────────────────────────────────────

    #[test]
    fn default_deny_hides_everything_without_a_rule() {
        let s = scope(r#"{"default":"deny"}"#);
        assert!(!s.visible(&id(), &[o(Some("api"), Some("a.rs"))]));
        assert!(!s.visible(&id(), &[o(None, None)]));
    }

    #[test]
    fn an_allowed_source_shows_its_objects_and_no_others() {
        let s = scope(r#"{"default":"deny","rules":[{"effect":"allow","source":"api"}]}"#);
        assert!(s.visible(&id(), &[o(Some("api"), Some("src/a.rs"))]));
        assert!(s.visible(&id(), &[o(Some("api"), None)]));
        assert!(!s.visible(&id(), &[o(Some("web"), Some("src/a.rs"))]));
    }

    #[test]
    fn an_object_without_a_source_is_hidden_from_a_reader_limited_by_source() {
        let s = scope(r#"{"default":"deny","rules":[{"effect":"allow","source":"api"}]}"#);
        assert!(!s.visible(&id(), &[o(None, None)]));
        assert!(!s.visible(&id(), &[o(None, Some("src/a.rs"))]));
    }

    #[test]
    fn an_object_in_two_sources_needs_both() {
        let one = scope(r#"{"default":"deny","rules":[{"effect":"allow","source":"api"}]}"#);
        let both = scope(
            r#"{"default":"deny","rules":[{"effect":"allow","source":"api"},
                                          {"effect":"allow","source":"web"}]}"#,
        );
        let origins = [o(Some("api"), Some("x")), o(Some("web"), Some("x"))];
        assert!(!one.visible(&id(), &origins));
        assert!(both.visible(&id(), &origins));
    }

    #[test]
    fn deny_wins_over_allow_whatever_the_order() {
        for rules in [
            r#"[{"effect":"allow","source":"api"},{"effect":"deny","source":"api","path":"secrets/**"}]"#,
            r#"[{"effect":"deny","source":"api","path":"secrets/**"},{"effect":"allow","source":"api"}]"#,
        ] {
            let s = scope(&format!(r#"{{"default":"deny","rules":{rules}}}"#));
            assert!(s.visible(&id(), &[o(Some("api"), Some("src/a.rs"))]));
            assert!(!s.visible(&id(), &[o(Some("api"), Some("secrets/key.pem"))]));
            assert!(!s.visible(&id(), &[o(Some("api"), Some("secrets/deep/er/x"))]));
        }
    }

    #[test]
    fn one_denied_origin_hides_an_object_with_several_paths() {
        let s = scope(r#"{"default":"allow","rules":[{"effect":"deny","path":"secrets/**"}]}"#);
        let origins = [
            o(Some("api"), Some("src/a.rs")),
            o(Some("api"), Some("secrets/k")),
        ];
        assert!(!s.visible(&id(), &origins));
    }

    #[test]
    fn a_path_deny_hides_what_has_no_path() {
        let s = scope(
            r#"{"default":"allow","rules":[{"effect":"deny","source":"api","path":"secrets/**"}]}"#,
        );
        // In the source the rule is about, with nothing to place it: hidden.
        assert!(!s.visible(&id(), &[o(Some("api"), None)]));
        // Another source: the rule does not concern it.
        assert!(s.visible(&id(), &[o(Some("web"), None)]));
        assert!(s.visible(&id(), &[o(Some("web"), Some("secrets/x"))]));
        // No source at all: it may be from `api`, so it is hidden, whatever its path.
        assert!(!s.visible(&id(), &[o(None, None)]));
        assert!(!s.visible(&id(), &[o(None, Some("secrets/x"))]));
        assert!(s.visible(&id(), &[o(None, Some("src/a.rs"))]));
    }

    #[test]
    fn a_path_deny_without_a_source_applies_to_every_source_and_to_none() {
        let s = scope(r#"{"default":"allow","rules":[{"effect":"deny","path":"hr/**"}]}"#);
        assert!(!s.visible(&id(), &[o(Some("handbook"), Some("hr/salaries.md"))]));
        assert!(!s.visible(&id(), &[o(None, Some("hr/salaries.md"))]));
        assert!(!s.visible(&id(), &[o(None, None)]));
        assert!(s.visible(&id(), &[o(Some("handbook"), Some("public/a.md"))]));
    }

    #[test]
    fn a_path_allow_needs_a_path_that_matches() {
        let s = scope(
            r#"{"default":"deny","rules":[{"effect":"allow","source":"docs","path":"public/**"}]}"#,
        );
        assert!(s.visible(&id(), &[o(Some("docs"), Some("public/a.md"))]));
        assert!(!s.visible(&id(), &[o(Some("docs"), Some("hr/a.md"))]));
        assert!(!s.visible(&id(), &[o(Some("docs"), None)]));
        // Every path must be covered.
        assert!(!s.visible(
            &id(),
            &[
                o(Some("docs"), Some("public/a.md")),
                o(Some("docs"), Some("hr/a.md"))
            ]
        ));
    }

    #[test]
    fn a_star_stays_in_one_component_and_two_cross_them() {
        let one = scope(r#"{"default":"deny","rules":[{"effect":"allow","path":"src/*.rs"}]}"#);
        assert!(one.visible(&id(), &[o(None, Some("src/a.rs"))]));
        assert!(!one.visible(&id(), &[o(None, Some("src/deep/a.rs"))]));
        let two = scope(r#"{"default":"deny","rules":[{"effect":"allow","path":"src/**"}]}"#);
        assert!(two.visible(&id(), &[o(None, Some("src/deep/a.rs"))]));
        assert!(!two.visible(&id(), &[o(None, Some("srcs/a.rs"))]));
        assert!(!two.visible(&id(), &[o(None, Some("other/src/a.rs"))]));
    }

    #[test]
    fn a_path_recorded_with_its_source_in_front_still_matches() {
        let s = scope(
            r#"{"default":"allow","rules":[{"effect":"deny","source":"api","path":"secrets/**"}]}"#,
        );
        assert!(!s.visible(&id(), &[o(Some("api"), Some("api/secrets/k"))]));
        let normalized = origins(&["api".into()], &["./secrets\\k".into()]);
        assert!(!s.visible(&id(), &normalized));
    }

    #[test]
    fn object_rules_name_one_object() {
        let wanted = id();
        let allow = scope(&format!(
            r#"{{"default":"deny","rules":[{{"effect":"allow","object":"{}"}}]}}"#,
            wanted.0
        ));
        assert!(allow.visible(&wanted, &[o(None, None)]));
        assert!(!allow.visible(&id(), &[o(None, None)]));

        let deny = scope(&format!(
            r#"{{"default":"allow","rules":[{{"effect":"deny","object":"{}"}}]}}"#,
            wanted.0
        ));
        assert!(!deny.visible(&wanted, &[o(Some("api"), Some("a"))]));
        assert!(deny.visible(&id(), &[o(Some("api"), Some("a"))]));
    }

    #[test]
    fn a_deny_on_a_source_beats_an_allow_on_the_object() {
        let wanted = id();
        let s = scope(&format!(
            r#"{{"default":"deny","rules":[{{"effect":"allow","object":"{}"}},
                                           {{"effect":"deny","source":"api"}}]}}"#,
            wanted.0
        ));
        assert!(!s.visible(&wanted, &[o(Some("api"), None)]));
        assert!(s.visible(&wanted, &[o(Some("web"), None)]));
    }

    #[test]
    fn origins_cross_sources_with_paths() {
        assert_eq!(origins(&[], &[]), vec![o(None, None)]);
        assert_eq!(
            origins(&["a".into(), "b".into()], &["x".into()]),
            vec![o(Some("a"), Some("x")), o(Some("b"), Some("x"))]
        );
    }

    // ── the store ────────────────────────────────────────────────────────────

    struct World {
        _dir: tempfile::TempDir,
        public: KirObject,
        secret: KirObject,
        other: KirObject,
        unplaced: KirObject,
        pub_secret: KirRelationship,
        pub_other: KirRelationship,
        path: std::path::PathBuf,
    }

    fn object(
        name: &str,
        source: Option<&str>,
        file: Option<&str>,
        ledger: &crate::Ledger,
    ) -> KirObject {
        let mut obj = KirObject::new(name, ObjectKind::Table);
        if let Some(s) = source {
            obj.properties.insert("source".into(), serde_json::json!(s));
            obj.properties
                .insert("sources".into(), serde_json::json!([s]));
        }
        if let Some(f) = file {
            let ev = KirEvidence::new(SourceLocation::file(f), format!("text of {name}"));
            ledger.append_evidence(&ev).unwrap();
            obj.evidence.push(ev.id);
        }
        ledger.append_object(&obj).unwrap();
        obj
    }

    fn world() -> World {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.db");
        let ledger = crate::Ledger::open(&path).unwrap();
        let public = object("public_orders", Some("api"), Some("db/schema.sql"), &ledger);
        let secret = object(
            "secret_payroll",
            Some("api"),
            Some("secrets/payroll.sql"),
            &ledger,
        );
        let other = object("other_pages", Some("web"), Some("pages/index.sql"), &ledger);
        let unplaced = object("unplaced_person", None, None, &ledger);
        let rel = |from: &KirObject, to: &KirObject| {
            let r = KirRelationship::new(RelationshipKind::DependsOn, from.id, to.id);
            ledger.append_relationship(&r).unwrap();
            r
        };
        let pub_secret = rel(&public, &secret);
        let pub_other = rel(&public, &other);
        World {
            _dir: dir,
            public,
            secret,
            other,
            unplaced,
            pub_secret,
            pub_other,
            path,
        }
    }

    const HIDE_SECRETS_AND_WEB: &str = r#"{"default":"deny","rules":[
        {"effect":"allow","source":"api"},
        {"effect":"deny","source":"api","path":"secrets/**"}]}"#;

    fn scoped(w: &World, json: &str) -> ScopedStore {
        let inner: Box<dyn KnowledgeStore> = Box::new(crate::Ledger::open(&w.path).unwrap());
        ScopedStore::with_scopes(inner, &[ReadScope::from_json(json).unwrap()]).unwrap()
    }

    fn names(objects: &[KirObject]) -> Vec<String> {
        let mut n: Vec<String> = objects.iter().map(|o| o.name.clone()).collect();
        n.sort();
        n
    }

    #[test]
    fn an_unrestricted_store_returns_what_the_inner_store_returns() {
        let w = world();
        let plain = crate::Ledger::open(&w.path).unwrap();
        let s = scoped(&w, r#"{"default":"allow"}"#);
        assert!(!s.is_restricted());
        assert_eq!(s.scope_fingerprint(), "");
        assert_eq!(
            names(&s.all_objects().unwrap()),
            names(&plain.all_objects().unwrap())
        );
        assert_eq!(s.all_relationships().unwrap().len(), 2);
        assert_eq!(s.object_count().unwrap(), plain.object_count().unwrap());
        assert_eq!(s.entry_count().unwrap(), plain.entry_count().unwrap());
        assert_eq!(s.evidence_count().unwrap(), plain.evidence_count().unwrap());
        assert!(s.get_evidence(&w.secret.evidence[0]).unwrap().is_some());
        // And it still writes.
        assert!(
            s.append_object(&KirObject::new("new", ObjectKind::Table))
                .unwrap()
        );
    }

    #[test]
    fn objects_are_filtered_on_every_object_read() {
        let w = world();
        let s = scoped(&w, HIDE_SECRETS_AND_WEB);
        assert!(s.is_restricted());
        assert_eq!(names(&s.all_objects().unwrap()), ["public_orders"]);
        assert!(s.get_object(&w.public.id).unwrap().is_some());
        for hidden in [&w.secret, &w.other, &w.unplaced] {
            assert!(
                s.get_object(&hidden.id).unwrap().is_none(),
                "{}",
                hidden.name
            );
            assert!(s.object_history(&hidden.id).unwrap().is_empty());
            assert!(s.object_at(&hidden.id, Utc::now()).unwrap().is_none());
            assert!(s.fact(&hidden.id, "name").unwrap().is_none());
            assert!(s.audit_trail(&hidden.id).unwrap().is_empty());
            assert!(s.relationships_for(&hidden.id).unwrap().is_empty());
            assert!(
                s.relationships_at(&hidden.id, Utc::now())
                    .unwrap()
                    .is_empty()
            );
        }
        assert_eq!(
            names(&s.all_objects_at(Utc::now()).unwrap()),
            ["public_orders"]
        );
        assert_eq!(
            names(&s.object_history(&w.public.id).unwrap()),
            ["public_orders"]
        );
        assert_eq!(
            s.fact(&w.public.id, "name").unwrap(),
            Some(serde_json::json!("public_orders"))
        );
    }

    #[test]
    fn a_relationship_needs_both_ends() {
        let w = world();
        let s = scoped(&w, HIDE_SECRETS_AND_WEB);
        assert!(s.all_relationships().unwrap().is_empty());
        assert!(s.all_relationships_at(Utc::now()).unwrap().is_empty());
        assert!(s.relationships_for(&w.public.id).unwrap().is_empty());
        assert!(s.get_relationship(&w.pub_secret.id).unwrap().is_none());
        assert!(s.relationship_history(&w.pub_secret.id).unwrap().is_empty());

        // With `web` allowed too, the edge to it appears and the one to the secret does not.
        let wider = scoped(
            &w,
            r#"{"default":"deny","rules":[{"effect":"allow","source":"api"},
                {"effect":"allow","source":"web"},
                {"effect":"deny","source":"api","path":"secrets/**"}]}"#,
        );
        let ids: Vec<KirId> = wider
            .all_relationships()
            .unwrap()
            .iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(ids, vec![w.pub_other.id]);
        assert_eq!(wider.relationships_for(&w.public.id).unwrap().len(), 1);
    }

    #[test]
    fn evidence_is_readable_only_through_something_visible() {
        let w = world();
        let s = scoped(&w, HIDE_SECRETS_AND_WEB);
        // Asked for cold, by id, before any object was read: nothing.
        assert!(s.get_evidence(&w.public.evidence[0]).unwrap().is_none());
        assert!(s.get_evidence(&w.secret.evidence[0]).unwrap().is_none());
        s.get_object(&w.public.id).unwrap();
        s.get_object(&w.secret.id).unwrap();
        assert!(s.get_evidence(&w.public.evidence[0]).unwrap().is_some());
        assert!(s.get_evidence(&w.secret.evidence[0]).unwrap().is_none());
    }

    #[test]
    fn search_returns_only_visible_hits() {
        let w = world();
        let s = scoped(&w, HIDE_SECRETS_AND_WEB);
        for query in [
            "secret_payroll",
            "payroll",
            "other_pages",
            "unplaced_person",
        ] {
            assert!(s.find_objects(query).unwrap().is_empty(), "{query}");
            let hits = s.retrieve(&RetrievalRequest::lexical(query)).unwrap().hits;
            assert!(hits.is_empty(), "{query}");
        }
        assert_eq!(s.find_objects("public_orders").unwrap().len(), 1);
        assert_eq!(
            s.retrieve(&RetrievalRequest::lexical("public_orders"))
                .unwrap()
                .hits
                .len(),
            1
        );
    }

    #[test]
    fn counts_and_diff_cover_only_what_is_visible() {
        let w = world();
        let s = scoped(&w, HIDE_SECRETS_AND_WEB);
        assert_eq!(s.object_count().unwrap(), 1);
        assert_eq!(s.relationship_count().unwrap(), 0);
        assert_eq!(s.evidence_count().unwrap(), 1);
        assert_eq!(s.entry_count().unwrap(), 2);

        let diff = s
            .diff(Utc::now() - chrono::Duration::hours(1), Utc::now())
            .unwrap();
        assert_eq!(diff.touched, vec![w.public.id.0.to_string()]);
        assert!(diff.added.is_empty());
    }

    #[test]
    fn a_restricted_store_refuses_writes_events_and_copies() {
        let w = world();
        let s = scoped(&w, HIDE_SECRETS_AND_WEB);
        let obj = KirObject::new("x", ObjectKind::Table);
        assert!(matches!(
            s.append_object(&obj),
            Err(LedgerError::ReadScope(_))
        ));
        let ev = KirEvidence::new(SourceLocation::file("a"), "t");
        assert!(matches!(
            s.append_evidence(&ev),
            Err(LedgerError::ReadScope(_))
        ));
        let rel = KirRelationship::new(RelationshipKind::DependsOn, w.public.id, w.public.id);
        assert!(matches!(
            s.append_relationship(&rel),
            Err(LedgerError::ReadScope(_))
        ));
        assert!(matches!(
            s.vacuum_into(&w.path.with_extension("copy")),
            Err(LedgerError::ReadScope(_))
        ));
        assert!(s.get_event(&id()).unwrap().is_none());
    }

    #[test]
    fn several_scopes_must_all_agree_and_can_be_replaced() {
        let w = world();
        let s = scoped(
            &w,
            r#"{"default":"deny","rules":[{"effect":"allow","source":"api"}]}"#,
        );
        assert_eq!(
            names(&s.all_objects().unwrap()),
            ["public_orders", "secret_payroll"]
        );
        let first = s.scope_fingerprint();

        s.set_scopes(&[
            ReadScope::from_json(
                r#"{"default":"deny","rules":[{"effect":"allow","source":"api"}]}"#,
            )
            .unwrap(),
            ReadScope::from_json(
                r#"{"default":"allow","rules":[{"effect":"deny","path":"secrets/**"}]}"#,
            )
            .unwrap(),
        ])
        .unwrap();
        assert_eq!(names(&s.all_objects().unwrap()), ["public_orders"]);
        assert_ne!(s.scope_fingerprint(), first);
        // The earlier decision for the secret is not remembered across a scope change.
        assert!(s.get_object(&w.secret.id).unwrap().is_none());
        assert!(s.get_evidence(&w.secret.evidence[0]).unwrap().is_none());

        s.set_scopes(&[]).unwrap();
        assert_eq!(s.all_objects().unwrap().len(), 4);
    }

    #[test]
    fn every_store_method_has_a_decision_here() {
        // Both traits as written in lib.rs (issue #13 split them). A method added there must be
        // handled in this file: a required one fails to compile, and one with a default body
        // would otherwise be inherited without anyone having decided what a restricted reader
        // gets from it.
        let lib = include_str!("lib.rs");
        let methods_of = |header: &str| -> Vec<String> {
            let body = lib.split_once(header).expect(header).1;
            let end = body.find("\n}\n").expect("end of the trait");
            body[..end]
                .lines()
                .filter_map(|l| l.strip_prefix("    fn "))
                .filter_map(|l| l.split(['(', '<']).next())
                .map(str::to_string)
                .collect()
        };
        let mut declared = methods_of("pub trait KnowledgeReader: Send {");
        declared.extend(methods_of("pub trait KnowledgeStore: KnowledgeReader {"));
        assert!(
            declared.len() >= 25,
            "the traits were not read: {declared:?}"
        );

        let here = include_str!("scope.rs");
        let block = |header: &str| -> &str {
            let body = here.split_once(header).expect(header).1;
            &body[..body.find("\n}\n").expect("end of the impl")]
        };
        let wrapper = format!(
            "{}\n{}",
            block("impl KnowledgeReader for ScopedStore {"),
            block("impl KnowledgeStore for ScopedStore {")
        );
        let missing: Vec<&String> = declared
            .iter()
            .filter(|name| !wrapper.contains(&format!("    fn {name}(")))
            .collect();
        assert!(
            missing.is_empty(),
            "KnowledgeStore method(s) {missing:?} have no decision in ScopedStore (RFC 0181): \
             decide what a restricted reader gets, then implement it there"
        );

        // Implemented is not decided: a bare `self.inner.x()` passthrough would pass the check
        // above. Each body must name the scope (issue #13 comment), or be listed here with why.
        const PASSTHROUGH: [(&str, &str); 2] = [
            (
                "set_write_context",
                "stamps provenance on later writes, which are refused anyway",
            ),
            (
                "refresh_snapshot",
                "reads nothing; clears the remembered decisions instead",
            ),
        ];
        const DECISIONS: [&str; 5] = ["is_restricted", "admit", "visible", "refuse", "keep_"];
        let mut undecided = Vec::new();
        for name in &declared {
            let at = wrapper.find(&format!("    fn {name}(")).unwrap();
            let rest = &wrapper[at + 1..];
            let body = &rest[..rest.find("\n    fn ").unwrap_or(rest.len())];
            let decided = DECISIONS.iter().any(|d| body.contains(d));
            let listed = PASSTHROUGH.iter().any(|(n, _)| n == name);
            if !decided && !listed {
                undecided.push(name);
            }
        }
        assert!(
            undecided.is_empty(),
            "ScopedStore method(s) {undecided:?} pass through without consulting the scope"
        );
    }
}
