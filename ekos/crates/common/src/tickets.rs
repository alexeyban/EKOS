//! RFC 0219 — issue-tracker ticket ids in free text, by configured key.
//!
//! A ticket id is `KEY-<digits>` for a key the caller names, with no letter or digit right before
//! it. There is deliberately no generic `[A-Z]+-[0-9]+` mode: it matches `UTF-8`, `ISO-8859` and
//! `SHA-256` as readily as a ticket.

/// Every `KEY-<digits>` in `text` for one of `keys`, in order of appearance, without repeats.
pub fn find(text: &str, keys: &[impl AsRef<str>]) -> Vec<String> {
    let mut out: Vec<(usize, String)> = Vec::new();
    for key in keys {
        let needle = format!("{}-", key.as_ref());
        let mut from = 0;
        while let Some(pos) = text[from..].find(&needle) {
            let start = from + pos;
            let digits: String = text[start + needle.len()..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            let boundary_before = text[..start]
                .chars()
                .next_back()
                .is_none_or(|c| !c.is_ascii_alphanumeric());
            if !digits.is_empty() && boundary_before {
                let id = format!("{needle}{digits}");
                if !out.iter().any(|(_, t)| *t == id) {
                    out.push((start, id));
                }
            }
            from = start + needle.len();
        }
    }
    out.sort_by_key(|(at, _)| *at);
    out.into_iter().map(|(_, t)| t).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEYS: &[&str] = &["IDEMPIERE", "ADEMPIERE"];

    #[test]
    fn exact_keys_with_a_boundary_before_them() {
        assert_eq!(
            find("IDEMPIERE-6203 Back-date costing (IDEMPIERE-6204)", KEYS),
            ["IDEMPIERE-6203", "IDEMPIERE-6204"]
        );
        assert!(find("XIDEMPIERE-1", KEYS).is_empty());
        assert!(find("IDEMPIERE-", KEYS).is_empty());
        assert!(find("UTF-8 and SHA-256", KEYS).is_empty());
        assert_eq!(
            find("ADEMPIERE-12 then IDEMPIERE-3, IDEMPIERE-3", KEYS),
            ["ADEMPIERE-12", "IDEMPIERE-3"],
            "order of appearance, no repeats"
        );
        assert!(find("IDEMPIERE-1", &[] as &[&str]).is_empty());
    }
}
