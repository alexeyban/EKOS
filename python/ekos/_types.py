"""The results of reads, as frozen dataclasses. These are the package's public contract."""

from __future__ import annotations

import dataclasses
from dataclasses import dataclass, field
from typing import Any

JsonDict = dict[str, Any]


class _Record:
    """Shared by every result type."""

    def to_dict(self) -> JsonDict:
        """This record, and the records inside it, as plain dicts and lists."""
        assert dataclasses.is_dataclass(self)
        return dataclasses.asdict(self)


@dataclass(frozen=True)
class Status(_Record):
    """How much the ledger holds and where it is."""

    entries: int
    objects: int
    relationships: int
    ledger_path: str


@dataclass(frozen=True)
class Hit(_Record):
    """One search result. ``Knowledge.get(hit.id)`` gives the object and its evidence."""

    id: str
    name: str


@dataclass(frozen=True)
class Evidence(_Record):
    """Where a conclusion came from: a place in a source, and the text found there."""

    id: str
    path: str | None
    line: int | None
    column: int | None
    fragment: str
    confidence: float
    created_at: str

    @classmethod
    def from_json(cls, raw: JsonDict) -> Evidence:
        location = raw.get("location") or {}
        return cls(
            id=raw["id"],
            path=location.get("path"),
            line=location.get("line"),
            column=location.get("column"),
            fragment=raw.get("fragment", ""),
            confidence=raw.get("confidence", 0.0),
            created_at=raw.get("created_at", ""),
        )


@dataclass(frozen=True)
class Object(_Record):
    """One compiled object. ``evidence_ids`` name the evidence it was derived from."""

    id: str
    name: str
    kind: str
    properties: JsonDict
    evidence_ids: list[str]
    created_at: str

    @classmethod
    def from_json(cls, raw: JsonDict) -> Object:
        kind = raw["kind"]
        return cls(
            id=raw["id"],
            name=raw["name"],
            # A custom kind is serialised as {"Custom": "<name>"}.
            kind=kind if isinstance(kind, str) else str(next(iter(kind.values()))),
            properties=raw.get("properties") or {},
            evidence_ids=list(raw.get("evidence") or []),
            created_at=raw.get("created_at", ""),
        )


@dataclass(frozen=True)
class Relationship(_Record):
    """A directed, typed edge between two objects."""

    id: str
    kind: str
    from_id: str
    to_id: str
    properties: JsonDict
    evidence_ids: list[str]
    created_at: str
    valid_from: str | None = None
    valid_until: str | None = None

    @classmethod
    def from_json(cls, raw: JsonDict) -> Relationship:
        kind = raw["kind"]
        return cls(
            id=raw["id"],
            kind=kind if isinstance(kind, str) else str(next(iter(kind.values()))),
            from_id=raw["from"],
            to_id=raw["to"],
            properties=raw.get("properties") or {},
            evidence_ids=list(raw.get("evidence") or []),
            created_at=raw.get("created_at", ""),
            valid_from=raw.get("valid_from"),
            valid_until=raw.get("valid_until"),
        )


@dataclass(frozen=True)
class ObjectState(_Record):
    """An object with its direct relationships and the evidence behind it.

    ``evidence`` is always a list. An empty one means the ledger holds no evidence for the
    object, which is worth knowing, not a detail to skip. ``open_conflicts`` lists the places
    where sources disagree about this object and nobody has decided.
    """

    object: Object
    relationships: list[Relationship]
    evidence: list[Evidence]
    open_conflicts: list[JsonDict] = field(default_factory=list)

    @classmethod
    def from_json(cls, raw: JsonDict) -> ObjectState:
        return cls(
            object=Object.from_json(raw["object"]),
            relationships=[Relationship.from_json(r) for r in raw.get("relationships") or []],
            evidence=[Evidence.from_json(e) for e in raw.get("evidence") or []],
            open_conflicts=list(raw.get("open_conflicts") or []),
        )


@dataclass(frozen=True)
class Fact(_Record):
    """One attribute of an object and its compiled value."""

    attribute: str
    value: Any


@dataclass(frozen=True)
class Graph(_Record):
    """Objects and the relationships between them. ``truncated`` when a limit cut it short."""

    objects: list[Object]
    relationships: list[Relationship]
    truncated: bool

    @classmethod
    def from_json(cls, raw: JsonDict) -> Graph:
        return cls(
            objects=[Object.from_json(o) for o in raw.get("objects") or []],
            relationships=[Relationship.from_json(r) for r in raw.get("relationships") or []],
            truncated=bool(raw.get("truncated", False)),
        )


@dataclass(frozen=True)
class Neighbor(_Record):
    """An object one relationship away, with the relationship that links it."""

    id: str
    name: str | None
    kind: str | None
    relationship: str
    status: Any
    properties: JsonDict

    @classmethod
    def from_json(cls, raw: JsonDict) -> Neighbor:
        return cls(
            id=raw["id"],
            name=raw.get("name"),
            kind=raw.get("kind"),
            relationship=raw["relationship"],
            status=raw.get("status"),
            properties=raw.get("properties") or {},
        )


@dataclass(frozen=True)
class ImpactHop(_Record):
    """An object reached while tracing impact: how many hops away, and through what."""

    hop: int
    id: str
    name: str
    kind: str
    via: str


@dataclass(frozen=True)
class ClaimEvidence(_Record):
    """The evidence a claim rests on: the evidence record's id and where it points."""

    id: str
    location: str
    extracted_by: str


@dataclass(frozen=True)
class Claim(_Record):
    """One statement of a compiled answer.

    ``evidence`` is always a list; empty means the claim has no evidence record. ``disputed``
    means sources disagree about this attribute and nobody has decided: ``value`` is then only
    the side EKOS kept, and ``conflicts`` names the open conflicts. ``weak`` marks a partial
    match, a candidate rather than support for the question's premise.
    """

    text: str
    value: Any
    entity: str | None
    attribute: str | None
    confidence: float
    evidence: list[ClaimEvidence]
    disputed: bool
    conflicts: list[str]
    weak: bool

    @classmethod
    def from_json(cls, raw: JsonDict) -> Claim:
        source = raw.get("source")
        evidence = (
            [
                ClaimEvidence(
                    id=source,
                    location=raw.get("location", ""),
                    extracted_by=raw.get("extracted_by", ""),
                )
            ]
            if source
            else []
        )
        return cls(
            text=raw["claim"],
            value=raw.get("value"),
            entity=raw.get("entity"),
            attribute=raw.get("attr"),
            confidence=raw.get("confidence", 0.0),
            evidence=evidence,
            disputed=bool(raw.get("disputed", False)),
            conflicts=list(raw.get("conflicts") or []),
            weak=bool(raw.get("weak", False)),
        )


@dataclass(frozen=True)
class Answer(_Record):
    """A compiled answer: claims read from the ledger. No model is called to make it."""

    claims: list[Claim]
    diagnostics: list[JsonDict]

    @classmethod
    def from_json(cls, raw: JsonDict) -> Answer:
        return cls(
            claims=[Claim.from_json(i) for i in raw.get("items") or []],
            diagnostics=list(raw.get("diagnostics") or []),
        )


@dataclass(frozen=True)
class Change(_Record):
    """Something written to the ledger in a time window: an object or a relationship."""

    entity: str
    id: str
    name: str | None = None
    kind: str | None = None
    from_id: str | None = None
    to_id: str | None = None


@dataclass(frozen=True)
class Diff(_Record):
    """What changed between two times. ``changed`` lists at most 200 of ``changed_total``."""

    since: str
    until: str
    changed_total: int
    changed: list[Change]
    unchanged: int

    @classmethod
    def from_json(cls, raw: JsonDict) -> Diff:
        return cls(
            since=raw["from"],
            until=raw["to"],
            changed_total=raw["changed_total"],
            changed=[
                Change(
                    entity=c["entity"],
                    id=c["id"],
                    name=c.get("name"),
                    kind=c.get("kind"),
                    from_id=c.get("from"),
                    to_id=c.get("to"),
                )
                for c in raw.get("changed") or []
            ],
            unchanged=raw.get("unchanged", 0),
        )
