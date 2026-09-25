//! RFC 0159 — PostgreSQL → ClickHouse type mapping and target design.
//!
//! The registry maps types with a **lossiness class** attached, the designer chooses engine,
//! `ORDER BY`, partitioning and codecs from measured evidence, and the emitter renders DDL that
//! carries its own reasoning in comments.
//!
//! Three rules run through all of it:
//!
//! 1. **A `NarrowingSafe` mapping cites the profile that proves it.** It is a claim about data at a
//!    point in time, not about the schema.
//! 2. **A growing column is never narrowed on measured data.** The profile describes the past; an
//!    identity column's future is larger by construction.
//! 3. **No evidence is stated as no evidence.** A design with no observed query shapes proposes the
//!    primary key *and says that is what it is doing*, rather than presenting a guess as a
//!    derivation.

pub mod ddl;
pub mod design;
pub mod typemap;

pub use ddl::{DdlError, create_table, rationale_comment};
pub use design::{
    DesignColumn, Engine, MAX_PARTITIONS, MIN_ROWS_TO_PARTITION, TableEvidence, TargetDesign,
    design,
};
pub use typemap::{ColumnEvidence, Lossiness, Mapping, map_column};
