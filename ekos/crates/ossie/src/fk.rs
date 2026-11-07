//! The columns of a ledger `fk_desc`. DDL writes `from.col → to.col`; a live catalog writes
//! composite keys as `T.a, b → U.x, y`, and names may be schema-qualified and quoted.

use crate::build::name_parts;

fn side(s: &str) -> Option<Vec<String>> {
    let mut segments = s.split(',').map(str::trim);
    // The first segment is `table.col` (the table possibly qualified); the rest are bare columns.
    let first = name_parts(segments.next()?);
    let mut cols = vec![first.last()?.clone()];
    if first.len() < 2 {
        return None;
    }
    for c in segments {
        let p = name_parts(c);
        cols.push(p.last()?.clone());
    }
    Some(cols)
}

/// `(from columns, to columns)` of an `fk_desc`, equally long, or `None`.
pub fn fk_columns(desc: &str) -> Option<(Vec<String>, Vec<String>)> {
    let (from, to) = desc.split_once('→')?;
    let (a, b) = (side(from)?, side(to)?);
    (a.len() == b.len()).then_some((a, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_composite_and_quoted() {
        let v = |x: &[&str]| x.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            fk_columns("orders.customer_id → customers.id"),
            Some((v(&["customer_id"]), v(&["id"])))
        );
        assert_eq!(
            fk_columns("ORDER_LINE.ORDER_ID, LINE_NO → ORDERS.ID, NO"),
            Some((v(&["ORDER_ID", "LINE_NO"]), v(&["ID", "NO"])))
        );
        assert_eq!(
            fk_columns("\"public\".\"rulings\".\"card_id\" → \"cards\".\"id\""),
            Some((v(&["card_id"]), v(&["id"])))
        );
        assert_eq!(fk_columns("a.x, y → b.z"), None);
        assert_eq!(fk_columns("nonsense"), None);
    }
}
