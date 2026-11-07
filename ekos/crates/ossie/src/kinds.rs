//! The kind names the Ossie integration reads and writes. `ekos-semantic` re-exports them
//! (`ossie_links`) and a test there holds `CONCEPT` / `ENUM_MEANING` equal to RFC 0170's own.

/// One Ossie semantic model ingested from a source file, keyed by (file, model name).
pub const OSSIE_MODEL: &str = "OssieModel";
/// One dataset of an ingested model, keyed by (file, model, dataset).
pub const OSSIE_DATASET: &str = "OssieDataset";
/// One metric of an ingested model, keyed by (file, model, scope, name).
pub const OSSIE_METRIC: &str = "OssieMetric";
/// The `link` property of the `DependsOn` edge from an `OssieDataset` to its table.
pub const LINK: &str = "ossie_source";

/// RFC 0170's business concept (the built-in `ObjectKind::BusinessConcept`).
pub const CONCEPT: &str = "BusinessConcept";
/// RFC 0170's code meaning, `Custom("EnumMeaning")`.
pub const ENUM_MEANING: &str = "EnumMeaning";

/// Whether `o` is an RFC 0170 business concept.
pub fn is_concept(o: &ekos_kir::KirObject) -> bool {
    match &o.kind {
        ekos_kir::ObjectKind::BusinessConcept => true,
        ekos_kir::ObjectKind::Custom(k) => k == CONCEPT,
        _ => false,
    }
}

/// Whether `o` is an RFC 0170 code meaning.
pub fn is_enum_meaning(o: &ekos_kir::KirObject) -> bool {
    matches!(&o.kind, ekos_kir::ObjectKind::Custom(k) if k == ENUM_MEANING)
}
