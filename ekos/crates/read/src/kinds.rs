//! Kind names as people type them.

use ekos_kir::ObjectKind;

/// Parses an [`ObjectKind`] name case-insensitively against the built-in variants, falling back to
/// [`ObjectKind::Custom`] for anything else — the same escape-hatch posture
/// `RelationshipKind::from_str` takes. Shared by `ekos graph export` and the `ekos_graph_export` tool.
pub fn parse_object_kind(s: &str) -> ObjectKind {
    macro_rules! ci {
        ($($variant:ident),+ $(,)?) => {
            $(if s.eq_ignore_ascii_case(stringify!($variant)) {
                return ObjectKind::$variant;
            })+
        };
    }
    ci!(
        File,
        Directory,
        Table,
        Entity,
        Service,
        Api,
        BusinessRule,
        BusinessConcept,
        Dataset,
        Column,
        Pipeline,
        Dashboard,
        Person,
        Model,
        Prompt,
        Agent,
        Unknown,
    );
    ObjectKind::Custom(s.to_string())
}
