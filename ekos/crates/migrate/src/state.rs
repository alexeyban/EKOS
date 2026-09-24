//! RFC 0154 — the migration unit state machine.
//!
//! A transition is two ledger writes, never an edit: the unit object is **re-appended** with its
//! new `state` (a new ledger version, the old one still readable through `ekos ledger audit`), and
//! a `MigrationTransition` event is appended recording `from`, `to`, actor and reason.
//!
//! Reading a unit's state is therefore a point lookup, never a fold over the event log. The event
//! log is the audit trail, not the source of truth — the same split that makes RFC 0151's session
//! claim status cheap to read and fully historical to audit.

use std::fmt;
use std::str::FromStr;

/// Where a migration unit is in its lifecycle. See RFC 0154's state table for the entry and exit
/// conditions behind each one.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum UnitState {
    Discovered,
    Profiled,
    Assessed,
    Planned,
    Mapped,
    Approved,
    Generated,
    DryRunPassed,
    Loaded,
    Validated,
    Diverged,
    Syncing,
    SignedOff,
    /// Deliberately removed from scope by a human. Terminal, and not a failure state.
    Abandoned,
}

impl UnitState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Discovered => "discovered",
            Self::Profiled => "profiled",
            Self::Assessed => "assessed",
            Self::Planned => "planned",
            Self::Mapped => "mapped",
            Self::Approved => "approved",
            Self::Generated => "generated",
            Self::DryRunPassed => "dry_run_passed",
            Self::Loaded => "loaded",
            Self::Validated => "validated",
            Self::Diverged => "diverged",
            Self::Syncing => "syncing",
            Self::SignedOff => "signed_off",
            Self::Abandoned => "abandoned",
        }
    }

    /// Every state a unit may move to from here.
    ///
    /// `Diverged` is reachable from any post-load state and returns to `Mapped` or `Generated`,
    /// because reconciliation may be a mapping problem or a generation problem and the validator
    /// cannot tell which. `Abandoned` is reachable from everywhere except the two terminal states:
    /// deciding not to migrate something is a legitimate outcome at any point.
    pub fn allowed_next(self) -> &'static [UnitState] {
        use UnitState::*;
        match self {
            Discovered => &[Profiled, Abandoned],
            Profiled => &[Assessed, Abandoned],
            Assessed => &[Planned, Abandoned],
            Planned => &[Mapped, Abandoned],
            Mapped => &[Approved, Planned, Abandoned],
            Approved => &[Generated, Mapped, Abandoned],
            Generated => &[DryRunPassed, Mapped, Abandoned],
            DryRunPassed => &[Loaded, Generated, Abandoned],
            Loaded => &[Validated, Diverged, Abandoned],
            Validated => &[Syncing, SignedOff, Diverged, Abandoned],
            Diverged => &[Mapped, Generated, Abandoned],
            Syncing => &[SignedOff, Diverged, Abandoned],
            SignedOff => &[],
            Abandoned => &[],
        }
    }

    pub fn can_move_to(self, to: UnitState) -> bool {
        self.allowed_next().contains(&to)
    }

    /// `true` for states no transition may leave. A terminal state is not a stuck state: it is a
    /// decision that was recorded, and undoing it means superseding the unit, not editing it.
    pub fn is_terminal(self) -> bool {
        self.allowed_next().is_empty()
    }

    /// The states that count as "this unit is done and may appear in a sign-off report".
    pub fn is_complete(self) -> bool {
        matches!(self, Self::Validated | Self::SignedOff)
    }
}

impl fmt::Display for UnitState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for UnitState {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use UnitState::*;
        Ok(match s {
            "discovered" => Discovered,
            "profiled" => Profiled,
            "assessed" => Assessed,
            "planned" => Planned,
            "mapped" => Mapped,
            "approved" => Approved,
            "generated" => Generated,
            "dry_run_passed" => DryRunPassed,
            "loaded" => Loaded,
            "validated" => Validated,
            "diverged" => Diverged,
            "syncing" => Syncing,
            "signed_off" => SignedOff,
            "abandoned" => Abandoned,
            other => return Err(format!("unknown migration unit state: {other}")),
        })
    }
}

/// Every state, in lifecycle order. Used by `ekos migrate status` so its output ordering is a
/// property of the state machine rather than of a hand-written list that drifts.
pub const ALL_STATES: [UnitState; 14] = [
    UnitState::Discovered,
    UnitState::Profiled,
    UnitState::Assessed,
    UnitState::Planned,
    UnitState::Mapped,
    UnitState::Approved,
    UnitState::Generated,
    UnitState::DryRunPassed,
    UnitState::Loaded,
    UnitState::Validated,
    UnitState::Diverged,
    UnitState::Syncing,
    UnitState::SignedOff,
    UnitState::Abandoned,
];
