//! RFC 0154 — the migration project and its units, as ledger facts.
//!
//! Every write here goes through `&dyn KnowledgeStore` with an RFC 0135 Part B [`WriteContext`]
//! whose `stage` is `migrate:<verb>`, so `ekos ledger audit` and the `ekos_audit` MCP tool explain
//! any migration fact without this crate doing anything further.
//!
//! Ids are deterministic (`Uuid::new_v5` over a structural seed), following
//! `semantic::transform_ir` and `KirRelationship::deterministic`: re-running `ekos migrate init`
//! against the same project must append the same object id, not accumulate duplicates in an
//! append-only ledger that has no dedup.

use crate::kinds;
use crate::state::UnitState;
use crate::{Error, connection::ConnectionRef};
use ekos_kir::{KirEvent, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind};
use ekos_ledger::{KnowledgeStore, provenance::WriteContext};
use serde_json::json;
use uuid::Uuid;

/// The RFC 0135 stage prefix every migration write carries.
pub const STAGE_PREFIX: &str = "migrate";

/// Build the `WriteContext` for one `ekos migrate <verb>` invocation. `run_id` groups every write
/// that verb makes, exactly as `build` and `commit` do.
pub fn write_context(verb: &str, run_id: &str) -> WriteContext {
    WriteContext {
        run_id: run_id.to_string(),
        stage: format!("{STAGE_PREFIX}:{verb}"),
        source_artifact_id: None,
    }
}

fn det_id(seed: &str) -> KirId {
    KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, seed.as_bytes()))
}

pub fn project_id(name: &str) -> KirId {
    det_id(&format!("migration-project:{name}"))
}

pub fn unit_id(project: &str, key: &str) -> KirId {
    det_id(&format!("migration-unit:{project}:{key}"))
}

pub fn connection_id(project: &str, role: &str, dsn: &str) -> KirId {
    det_id(&format!("migration-connection:{project}:{role}:{dsn}"))
}

/// A migration project: one source, one target, a policy and a name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub name: String,
    pub source: ConnectionRef,
    pub target: ConnectionRef,
    pub created_by: String,
}

impl Project {
    pub fn id(&self) -> KirId {
        project_id(&self.name)
    }

    /// Write the project and its two connection references. Idempotent: deterministic ids mean a
    /// second `init` re-appends identical content rather than creating a second project.
    pub fn create(&self, store: &dyn KnowledgeStore, run_id: &str) -> Result<KirId, Error> {
        store.set_write_context(Some(write_context("init", run_id)));
        let result = self.create_inner(store);
        store.set_write_context(None);
        result
    }

    fn create_inner(&self, store: &dyn KnowledgeStore) -> Result<KirId, Error> {
        let src = self.write_connection(store, "source", &self.source)?;
        let tgt = self.write_connection(store, "target", &self.target)?;

        let mut obj = KirObject::new(
            self.name.clone(),
            ObjectKind::Custom(kinds::PROJECT_KIND.into()),
        );
        obj.id = self.id();
        obj.properties
            .insert("source".into(), json!(self.source.dsn()));
        obj.properties
            .insert("target".into(), json!(self.target.dsn()));
        obj.properties
            .insert("created_by".into(), json!(self.created_by));
        store.append_object(&obj)?;

        for (role, id) in [("source", src), ("target", tgt)] {
            store.append_relationship(&KirRelationship::deterministic(
                RelationshipKind::References,
                obj.id,
                id,
                role,
            ))?;
        }
        Ok(obj.id)
    }

    fn write_connection(
        &self,
        store: &dyn KnowledgeStore,
        role: &str,
        conn: &ConnectionRef,
    ) -> Result<KirId, Error> {
        let mut obj = KirObject::new(
            conn.dsn(),
            ObjectKind::Custom(kinds::CONNECTION_KIND.into()),
        );
        obj.id = connection_id(&self.name, role, &conn.dsn());
        obj.properties
            .insert("engine".into(), json!(conn.engine.as_str()));
        obj.properties.insert("alias".into(), json!(conn.alias));
        obj.properties
            .insert("database".into(), json!(conn.database));
        obj.properties.insert("role".into(), json!(role));
        // The *name* of the variable, never its value. A ledger-scan test asserts the difference.
        obj.properties
            .insert("secret_env".into(), json!(conn.secret_env));
        store.append_object(&obj)?;
        Ok(obj.id)
    }

    /// Load a project by name.
    pub fn load(store: &dyn KnowledgeStore, name: &str) -> Result<Option<KirObject>, Error> {
        let obj = store.get_object(&project_id(name))?;
        Ok(obj.filter(|o| is_kind(o, kinds::PROJECT_KIND)))
    }
}

fn is_kind(o: &KirObject, kind: &str) -> bool {
    matches!(&o.kind, ObjectKind::Custom(k) if k == kind)
}

/// One migration unit: a table, view, function, or a group migrated together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    pub project: String,
    /// Structural key within the project — a qualified object name, or a group name for a cycle.
    pub key: String,
    pub state: UnitState,
    pub wave: Option<u32>,
}

impl Unit {
    pub fn id(&self) -> KirId {
        unit_id(&self.project, &self.key)
    }

    pub fn create(&self, store: &dyn KnowledgeStore, run_id: &str) -> Result<KirId, Error> {
        store.set_write_context(Some(write_context("discover", run_id)));
        let result = self.create_inner(store);
        store.set_write_context(None);
        result
    }

    fn create_inner(&self, store: &dyn KnowledgeStore) -> Result<KirId, Error> {
        let mut obj = KirObject::new(
            self.key.clone(),
            ObjectKind::Custom(kinds::UNIT_KIND.into()),
        );
        obj.id = self.id();
        obj.properties.insert("project".into(), json!(self.project));
        obj.properties
            .insert("state".into(), json!(self.state.as_str()));
        obj.properties.insert("wave".into(), json!(self.wave));
        store.append_object(&obj)?;
        store.append_relationship(&KirRelationship::deterministic(
            RelationshipKind::Custom(kinds::HAS_UNIT.into()),
            project_id(&self.project),
            obj.id,
            "",
        ))?;
        Ok(obj.id)
    }

    /// Read a unit's current state. A point lookup on the latest object version — never a fold
    /// over the transition event log.
    pub fn state_of(store: &dyn KnowledgeStore, id: &KirId) -> Result<Option<UnitState>, Error> {
        let Some(obj) = store.get_object(id)? else {
            return Ok(None);
        };
        if !is_kind(&obj, kinds::UNIT_KIND) {
            return Err(Error::NotAUnit(id.to_string()));
        }
        let raw = obj
            .properties
            .get("state")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::NotAUnit(id.to_string()))?;
        raw.parse().map(Some).map_err(Error::UnknownState)
    }

    /// Every unit in a project, with its current state.
    pub fn all_in(
        store: &dyn KnowledgeStore,
        project: &str,
    ) -> Result<Vec<(KirObject, UnitState)>, Error> {
        let mut out = Vec::new();
        for obj in store.all_objects()? {
            if !is_kind(&obj, kinds::UNIT_KIND) {
                continue;
            }
            if obj.properties.get("project").and_then(|v| v.as_str()) != Some(project) {
                continue;
            }
            let state = obj
                .properties
                .get("state")
                .and_then(|v| v.as_str())
                .unwrap_or("discovered")
                .parse()
                .map_err(Error::UnknownState)?;
            out.push((obj, state));
        }
        out.sort_by(|a, b| a.0.name.cmp(&b.0.name));
        Ok(out)
    }
}

/// Move a unit to a new state: **re-append the object, append the event**. Never an edit.
///
/// Refuses a transition the state machine does not allow, so an illegal move is a caller error at
/// the point it happens rather than an inconsistent ledger discovered later.
///
/// Returns the id of the appended `MigrationTransition` event, or `None` when the unit was already
/// in `to` and nothing was written. `KnowledgeStore` can only fetch an event by id, so returning it
/// is the only way a caller — or a test — can read back what was recorded.
pub fn transition(
    store: &dyn KnowledgeStore,
    unit_id: &KirId,
    to: UnitState,
    actor: &str,
    reason: &str,
    run_id: &str,
) -> Result<Option<KirId>, Error> {
    store.set_write_context(Some(write_context("transition", run_id)));
    let result = transition_inner(store, unit_id, to, actor, reason);
    store.set_write_context(None);
    result
}

fn transition_inner(
    store: &dyn KnowledgeStore,
    unit_id: &KirId,
    to: UnitState,
    actor: &str,
    reason: &str,
) -> Result<Option<KirId>, Error> {
    let mut obj = store
        .get_object(unit_id)?
        .ok_or_else(|| Error::NotAUnit(unit_id.to_string()))?;
    if !is_kind(&obj, kinds::UNIT_KIND) {
        return Err(Error::NotAUnit(unit_id.to_string()));
    }
    let from: UnitState = obj
        .properties
        .get("state")
        .and_then(|v| v.as_str())
        .unwrap_or("discovered")
        .parse()
        .map_err(Error::UnknownState)?;

    if from == to {
        return Ok(None);
    }
    if !from.can_move_to(to) {
        return Err(Error::IllegalTransition { from, to });
    }

    let now = chrono::Utc::now();
    obj.properties.insert("state".into(), json!(to.as_str()));
    obj.properties
        .insert("state_changed_at".into(), json!(now.to_rfc3339()));
    store.append_object(&obj)?;

    let event_id = KirId::new();
    store.append_event(&KirEvent {
        id: event_id,
        kind: ekos_kir::EventKind::Custom(kinds::TRANSITION_EVENT.into()),
        subject: obj.id,
        payload: json!({
            "from": from.as_str(),
            "to": to.as_str(),
            "actor": actor,
            "reason": reason,
        }),
        evidence: Vec::new(),
        occurred_at: now,
    })?;
    Ok(Some(event_id))
}
