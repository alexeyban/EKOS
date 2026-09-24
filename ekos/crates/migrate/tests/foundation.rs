//! RFC 0154 Phase 0 acceptance tests.
//!
//! The four that matter most are not about happy paths: deterministic ids (an append-only ledger
//! has no dedup), the append-only transition shape, the illegal-transition refusal, and the
//! ledger scan proving no credential ever reaches the ledger.

use ekos_kir::{ObjectKind, custom_kinds};
use ekos_ledger::Ledger;
use ekos_migrate::project::{self, Unit};
use ekos_migrate::{
    ConnectionError, ConnectionRef, EngineKind, Environment, Project, UnitState, kinds, lifecycle,
};

fn store() -> (Ledger, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    (Ledger::open(&dir.path().join("ledger.db")).unwrap(), dir)
}

fn project() -> Project {
    Project {
        name: "ledgersmb".into(),
        source: ConnectionRef::parse("postgres://pg-prod/ledgersmb").unwrap(),
        target: ConnectionRef::parse("clickhouse://ch-dev/ledgersmb").unwrap(),
        created_by: "tester".into(),
    }
}

// ── the registry obligation ──────────────────────────────────────────────────

/// The `ekos-identity` guard scans source for `ObjectKind::Custom("…")` **string literals**. This
/// crate builds kinds from `kinds::*` constants instead, so that scan cannot see them — this test
/// is the real enforcement, and the guard's scan of `migrate/src` only catches a stray literal.
#[test]
fn every_migrate_kind_has_a_registry_row() {
    for name in kinds::ALL_KINDS {
        let row = custom_kinds::lookup(name)
            .unwrap_or_else(|| panic!("{name} is missing from ekos_kir::custom_kinds::REGISTRY"));
        assert!(
            row.structurally_keyed,
            "{name} must be structurally_keyed: every migration entity is identified by a \
             structural key, and without the flag DefaultResolver collapses a whole migration's \
             units into one object"
        );
    }
}

// ── connection refs never carry credentials ──────────────────────────────────

#[test]
fn a_dsn_with_embedded_credentials_is_rejected() {
    let err = ConnectionRef::parse("postgres://user:hunter2@pg-prod/ledgersmb").unwrap_err();
    assert_eq!(
        err,
        ConnectionError::EmbeddedCredentials("postgres://user:hunter2@pg-prod/ledgersmb".into())
    );
    // Even without a password: userinfo at all is refused, not silently stripped.
    assert!(matches!(
        ConnectionRef::parse("postgres://user@pg-prod/db"),
        Err(ConnectionError::EmbeddedCredentials(_))
    ));
}

#[test]
fn engines_are_restricted_by_role() {
    assert!(
        ConnectionRef::parse("clickhouse://ch/db")
            .unwrap()
            .require_source()
            .is_err()
    );
    assert!(
        ConnectionRef::parse("postgres://pg/db")
            .unwrap()
            .require_target()
            .is_err()
    );
    assert!(
        ConnectionRef::parse("postgres://pg/db")
            .unwrap()
            .require_source()
            .is_ok()
    );
    assert!(
        ConnectionRef::parse("delta://dbx/main")
            .unwrap()
            .require_target()
            .is_ok()
    );
}

#[test]
fn malformed_dsns_are_rejected() {
    for bad in [
        "pg-prod/ledgersmb",
        "postgres://pg-prod",
        "postgres:///db",
        "postgres://pg/",
    ] {
        assert!(ConnectionRef::parse(bad).is_err(), "{bad} should not parse");
    }
    assert!(matches!(
        ConnectionRef::parse("mysql://h/db"),
        Err(ConnectionError::Engine { .. })
    ));
}

/// The Phase 0 exit criterion: a ledger scan finds no credential. The secret's *variable name* is
/// persisted; its value never is.
#[test]
fn no_credential_ever_reaches_the_ledger() {
    let (s, _d) = store();
    let p = Project {
        source: ConnectionRef::parse("postgres://pg-prod/ledgersmb")
            .unwrap()
            .with_secret_env(Some("EKOS_MIGRATE_PG_PASSWORD".into())),
        ..project()
    };
    // The value a real run would resolve from the environment.
    let secret = "hunter2-do-not-persist";
    unsafe { std::env::set_var("EKOS_MIGRATE_PG_PASSWORD", secret) };
    p.create(&s, "run-1").unwrap();

    let dump = serde_json::to_string(&s.all_objects().unwrap()).unwrap();
    assert!(
        !dump.contains(secret),
        "the secret value reached the ledger"
    );
    assert!(
        dump.contains("EKOS_MIGRATE_PG_PASSWORD"),
        "the secret's variable name should be persisted — it is not a secret"
    );
    unsafe { std::env::remove_var("EKOS_MIGRATE_PG_PASSWORD") };
}

// ── deterministic ids ────────────────────────────────────────────────────────

/// An append-only ledger has no dedup on content, so a non-deterministic id turns a re-run of
/// `ekos migrate init` into a second project. Ids are `Uuid::new_v5` over a structural seed.
#[test]
fn init_is_idempotent_because_ids_are_deterministic() {
    let (s, _d) = store();
    let p = project();
    let first = p.create(&s, "run-1").unwrap();
    let second = p.create(&s, "run-2").unwrap();
    assert_eq!(first, second);

    let projects: Vec<_> = s
        .all_objects()
        .unwrap()
        .into_iter()
        .filter(|o| matches!(&o.kind, ObjectKind::Custom(k) if k == kinds::PROJECT_KIND))
        .collect();
    assert_eq!(projects.len(), 1, "a second init created a second project");
}

#[test]
fn unit_ids_are_scoped_to_their_project() {
    assert_ne!(
        project::unit_id("a", "public.orders"),
        project::unit_id("b", "public.orders")
    );
    assert_eq!(
        project::unit_id("a", "public.orders"),
        project::unit_id("a", "public.orders")
    );
}

// ── the state machine ────────────────────────────────────────────────────────

fn seeded_unit() -> (Ledger, tempfile::TempDir, ekos_kir::KirId) {
    let (s, d) = store();
    project().create(&s, "run-1").unwrap();
    let u = Unit {
        project: "ledgersmb".into(),
        key: "public.orders".into(),
        state: UnitState::Discovered,
        wave: None,
    };
    let id = u.create(&s, "run-1").unwrap();
    (s, d, id)
}

/// A transition is two writes and zero edits: the object is re-appended (a new version, the old
/// one still readable) and an event is appended.
#[test]
fn a_transition_re_appends_the_object_and_appends_an_event() {
    let (s, _d, id) = seeded_unit();
    let event_id = project::transition(
        &s,
        &id,
        UnitState::Profiled,
        "human:tester",
        "profiled it",
        "run-2",
    )
    .unwrap()
    .expect("a real transition appends an event");

    assert_eq!(Unit::state_of(&s, &id).unwrap(), Some(UnitState::Profiled));

    // Write 1: the object is re-appended, and the previous version is still readable.
    let history = s.object_history(&id).unwrap();
    assert_eq!(
        history.len(),
        2,
        "the previous version must still be readable"
    );
    assert_eq!(
        history[0].properties.get("state").unwrap().as_str(),
        Some("discovered")
    );

    // Write 2: the transition event carries from/to/actor/reason.
    let ev = s.get_event(&event_id).unwrap().expect("event not appended");
    assert_eq!(ev.subject, id);
    assert!(matches!(&ev.kind,
        ekos_kir::EventKind::Custom(k) if k == kinds::TRANSITION_EVENT));
    assert_eq!(ev.payload["from"], "discovered");
    assert_eq!(ev.payload["to"], "profiled");
    assert_eq!(ev.payload["actor"], "human:tester");
    assert_eq!(ev.payload["reason"], "profiled it");
}

#[test]
fn an_illegal_transition_is_refused() {
    let (s, _d, id) = seeded_unit();
    let err = project::transition(
        &s,
        &id,
        UnitState::SignedOff,
        "human:tester",
        "skip ahead",
        "run-2",
    )
    .unwrap_err();
    assert!(matches!(
        err,
        ekos_migrate::Error::IllegalTransition {
            from: UnitState::Discovered,
            to: UnitState::SignedOff
        }
    ));
    assert_eq!(
        Unit::state_of(&s, &id).unwrap(),
        Some(UnitState::Discovered)
    );
}

#[test]
fn a_transition_to_the_same_state_is_a_no_op() {
    let (s, _d, id) = seeded_unit();
    let ev = project::transition(
        &s,
        &id,
        UnitState::Discovered,
        "human:tester",
        "again",
        "run-2",
    )
    .unwrap();
    assert!(ev.is_none(), "a no-op transition must not append an event");
    assert_eq!(s.object_history(&id).unwrap().len(), 1);
}

#[test]
fn terminal_states_have_no_exit() {
    assert!(UnitState::SignedOff.is_terminal());
    assert!(UnitState::Abandoned.is_terminal());
    for s in ekos_migrate::ALL_STATES {
        if !matches!(s, UnitState::SignedOff | UnitState::Abandoned) {
            assert!(!s.is_terminal(), "{s} should not be terminal");
        }
    }
}

/// Every non-terminal state can be abandoned: deciding not to migrate something is legitimate at
/// any point, and the completeness check (RFC 0158) needs it to be a *classification* rather than
/// a gap.
#[test]
fn every_non_terminal_state_can_be_abandoned() {
    for s in ekos_migrate::ALL_STATES {
        if s.is_terminal() {
            continue;
        }
        assert!(
            s.can_move_to(UnitState::Abandoned),
            "{s} cannot be abandoned"
        );
    }
}

#[test]
fn state_strings_round_trip() {
    for s in ekos_migrate::ALL_STATES {
        assert_eq!(s.as_str().parse::<UnitState>().unwrap(), s);
    }
    assert!("nonsense".parse::<UnitState>().is_err());
}

// ── lifecycle: human-only, append-only ───────────────────────────────────────

#[test]
fn abandon_requires_a_reason() {
    let (s, _d, id) = seeded_unit();
    assert!(matches!(
        lifecycle::abandon(&s, &id, lifecycle::Actor::Human, "tester", "   ", "run-2"),
        Err(ekos_migrate::Error::ReasonRequired)
    ));
    lifecycle::abandon(
        &s,
        &id,
        lifecycle::Actor::Human,
        "tester",
        "out of scope",
        "run-2",
    )
    .unwrap();
    assert_eq!(Unit::state_of(&s, &id).unwrap(), Some(UnitState::Abandoned));
}

#[test]
fn supersede_never_deletes() {
    let (s, _d) = store();
    project().create(&s, "run-1").unwrap();
    let old = Unit {
        project: "ledgersmb".into(),
        key: "public.orders".into(),
        state: UnitState::Discovered,
        wave: None,
    }
    .create(&s, "run-1")
    .unwrap();
    let new = Unit {
        project: "ledgersmb".into(),
        key: "public.orders_v2".into(),
        state: UnitState::Discovered,
        wave: None,
    }
    .create(&s, "run-1")
    .unwrap();

    lifecycle::supersede(
        &s,
        &old,
        &new,
        lifecycle::Actor::Human,
        "tester",
        "regrouped",
        "run-2",
    )
    .unwrap();

    let superseded = s.get_object(&old).unwrap().unwrap();
    assert_eq!(
        superseded.properties.get("status").unwrap().as_str(),
        Some("superseded")
    );
    assert_eq!(
        superseded.properties.get("superseded_by").unwrap().as_str(),
        Some(new.to_string().as_str())
    );
    assert!(
        s.get_object(&old).unwrap().is_some(),
        "nothing is ever deleted"
    );
    let edges = s.relationships_for(&new).unwrap();
    assert!(
        edges.iter().any(|r| matches!(&r.kind,
            ekos_kir::RelationshipKind::Custom(k) if k == kinds::SUPERSEDES)),
        "a Supersedes edge must be recorded"
    );
}

// ── provenance ───────────────────────────────────────────────────────────────

/// RFC 0135 Part B: every migration write carries a `migrate:<verb>` stage, so `ekos ledger audit`
/// explains a migration fact without this crate doing anything further.
#[test]
fn writes_carry_a_migrate_stage() {
    let (s, _d, id) = seeded_unit();
    project::transition(&s, &id, UnitState::Profiled, "human:tester", "r", "run-7").unwrap();
    let trail = s.audit_trail(&id).unwrap();
    assert!(!trail.is_empty(), "no audit trail recorded");
    assert!(
        trail.iter().all(|r| r
            .stage
            .as_deref()
            .is_some_and(|s| s.starts_with("migrate:"))),
        "every write must be stamped with a migrate:* stage, got: {:?}",
        trail.iter().map(|r| r.stage.clone()).collect::<Vec<_>>()
    );
    assert!(trail.iter().any(|r| r.run_id.as_deref() == Some("run-7")));
}

// ── environments ─────────────────────────────────────────────────────────────

#[test]
fn only_sandbox_writes_are_ungated() {
    assert!(!Environment::Sandbox.requires_approval_to_write());
    assert!(Environment::Staging.requires_approval_to_write());
    assert!(Environment::Production.requires_approval_to_write());
    assert_eq!(
        "prod".parse::<Environment>().unwrap(),
        Environment::Production
    );
    assert!("nowhere".parse::<Environment>().is_err());
    assert_eq!(EngineKind::Postgres.to_string(), "postgres");
}
