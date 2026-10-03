//! Helpers shared by the SQL object analyzers — routines (RFC 0163), views (RFC 0169) and
//! triggers (RFC 0163 *Triggers*).

use ekos_kir::KirId;
use uuid::Uuid;

/// The id of the `File` object `build` writes for `file_key` (a project-qualified,
/// observe-path-relative path — RFC 0079). One definition, so the SQL analyzers' `Contains` edges
/// can never drift from the id `build` actually gives the file.
pub fn file_kir_id(file_key: &str) -> KirId {
    KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, file_key.as_bytes()))
}

/// At most `max` bytes of `s`, cut on a char boundary, with a marker when cut. Evidence fragments
/// are capped; the exact span is always recorded alongside.
pub fn clip(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut at = max;
    while !s.is_char_boundary(at) {
        at -= 1;
    }
    format!("{} …", &s[..at])
}
