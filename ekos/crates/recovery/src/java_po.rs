//! RFC 0212 §3.2 — `PO` deletes: `v.delete(…)` / `v.deleteEx(…)` on a model object, attributed to
//! the object's table.
//!
//! There is no call graph (RFC 0209 §2), so a receiver's type comes only from what the method
//! itself declares: a local (`MInvoiceLine line = …`, `MInvoiceLine[] lines`), an enhanced-`for`
//! variable, a parameter, `this`, or — inside a model class — an unqualified call. Scopes are not
//! tracked: a name declared twice keeps its last declaration before the call. A receiver whose
//! type the method does not declare (a field, `getLines()[i]`, a generic `PO`) is reported as
//! unresolved, never guessed.
//!
//! Only calls **with arguments** count: `PO.delete(boolean force[, String trxName])` and
//! `deleteEx(…)` always take one, while `File.delete()` takes none.

use crate::java_syntax::{Param, TokKind, Token};
use std::collections::HashMap;

/// One delete call on a model object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoDelete {
    pub line: u32,
    /// The receiver's declared type as written (`MInvoiceLine`), or `None` for `this` / an
    /// unqualified call, which mean the enclosing class.
    pub receiver_type: Option<String>,
}

/// What one method body holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PoCalls {
    pub resolved: Vec<PoDelete>,
    /// Delete calls with arguments whose receiver's type is not declared in the method.
    pub unresolved: Vec<u32>,
}

const DELETE_METHODS: &[&str] = &["delete", "deleteEx"];

/// The declared type of a written type: generics and array brackets dropped.
fn element_type(ty: &str) -> String {
    let t = ty.split('<').next().unwrap_or(ty);
    t.trim_end_matches("[]")
        .trim_end_matches("...")
        .trim()
        .to_string()
}

/// A type name starts upper case (`MInvoice`, `X_C_Order`, `org.x.MOrder`).
fn looks_like_type(name: &str) -> bool {
    name.rsplit('.')
        .next()
        .and_then(|s| s.chars().next())
        .is_some_and(char::is_uppercase)
}

/// Read a (possibly qualified, possibly generic) type starting at `i`; returns the type text and
/// the index after it, including any `[]` pairs.
fn read_type(tokens: &[Token], mut i: usize, end: usize) -> Option<(String, usize)> {
    if !tokens.get(i)?.is_ident() {
        return None;
    }
    let mut ty = tokens[i].text.clone();
    i += 1;
    while i + 1 < end && tokens[i].is(".") && tokens[i + 1].is_ident() {
        ty.push('.');
        ty.push_str(&tokens[i + 1].text);
        i += 2;
    }
    if i < end && tokens[i].is("<") {
        let mut depth = 0i32;
        while i < end {
            if tokens[i].is("<") {
                depth += 1;
            } else if tokens[i].is(">") {
                depth -= 1;
            } else if tokens[i].is(">>") {
                depth -= 2;
            }
            i += 1;
            if depth <= 0 {
                break;
            }
        }
    }
    while i + 1 < end && tokens[i].is("[") && tokens[i + 1].is("]") {
        i += 2;
    }
    Some((ty, i))
}

/// Find the delete calls in `tokens[from..to]` (a method body) and resolve their receivers.
pub fn delete_calls(tokens: &[Token], (from, to): (usize, usize), params: &[Param]) -> PoCalls {
    let to = to.min(tokens.len());
    let mut types: HashMap<String, String> = params
        .iter()
        .map(|p| (p.name.clone(), element_type(&p.ty)))
        .collect();
    let mut out = PoCalls::default();
    let mut i = from;
    while i < to {
        let t = &tokens[i];
        // A declaration: `Type name` followed by `=`, `;`, `:` or `,`.
        if t.is_ident()
            && looks_like_type(&t.text)
            && (i == from || !tokens[i - 1].is("."))
            && let Some((ty, after)) = read_type(tokens, i, to)
            && after + 1 < to
            && tokens[after].is_ident()
            && ["=", ";", ":", ","].iter().any(|p| tokens[after + 1].is(p))
        {
            types.insert(tokens[after].text.clone(), ty);
            i = after;
            continue;
        }
        if t.is_ident()
            && DELETE_METHODS.contains(&t.text.as_str())
            && i + 2 < to
            && tokens[i + 1].is("(")
            && !tokens[i + 2].is(")")
        {
            let qualified = i > from && tokens[i - 1].is(".");
            if !qualified {
                out.resolved.push(PoDelete {
                    line: t.line,
                    receiver_type: None,
                });
            } else {
                match receiver(tokens, i - 1, from) {
                    Some(Receiver::This) => out.resolved.push(PoDelete {
                        line: t.line,
                        receiver_type: None,
                    }),
                    Some(Receiver::Name(v)) => match types.get(&v) {
                        Some(ty) => out.resolved.push(PoDelete {
                            line: t.line,
                            receiver_type: Some(ty.clone()),
                        }),
                        None => out.unresolved.push(t.line),
                    },
                    None => out.unresolved.push(t.line),
                }
            }
        }
        i += 1;
    }
    out
}

enum Receiver {
    This,
    Name(String),
}

/// The receiver ending just before the `.` at `dot`: `v`, `this`, or `arr[…]` (an element of a
/// declared array, which has the array's element type).
fn receiver(tokens: &[Token], dot: usize, from: usize) -> Option<Receiver> {
    let prev = dot.checked_sub(1).filter(|&p| p >= from)?;
    let t = &tokens[prev];
    if t.is("this") {
        return Some(Receiver::This);
    }
    if t.kind == TokKind::Ident {
        // `a.b.delete(…)`: a field of something — not a declared local.
        if prev > from && tokens[prev - 1].is(".") {
            return None;
        }
        return Some(Receiver::Name(t.text.clone()));
    }
    if t.is("]") {
        let mut depth = 0i32;
        let mut j = prev;
        loop {
            if tokens[j].is("]") {
                depth += 1;
            } else if tokens[j].is("[") {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            if j == from {
                return None;
            }
            j -= 1;
        }
        let name = j.checked_sub(1).filter(|&k| k >= from)?;
        if tokens[name].is_ident() && !(name > from && tokens[name - 1].is(".")) {
            return Some(Receiver::Name(tokens[name].text.clone()));
        }
    }
    None
}

/// Whether a source path is test code, which never gives a table a delete class.
pub fn is_test_path(path: &str) -> bool {
    let p = path.replace('\\', "/");
    p.contains("/src/test/")
        || p.contains("/test/src/")
        || p.contains("org.idempiere.test")
        || p.ends_with("Test.java")
        || p.ends_with("Tests.java")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::java_syntax::parse;

    fn calls(src: &str) -> PoCalls {
        let f = parse(src);
        let m = &f.types[0].methods[0];
        delete_calls(&f.tokens, m.body.unwrap(), &m.params)
    }

    fn types(c: &PoCalls) -> Vec<Option<&str>> {
        c.resolved
            .iter()
            .map(|d| d.receiver_type.as_deref())
            .collect()
    }

    #[test]
    fn locals_params_loops_and_arrays_resolve() {
        let c = calls(
            "class A { void m(MOrder order, String trx) {\n\
             MInvoiceLine line = new MInvoiceLine(ctx, 1, trx);\n\
             line.deleteEx(true);\n\
             order.delete(true, trx);\n\
             for (MOrderLine ol : order.getLines()) ol.deleteEx(true);\n\
             MPayment[] pays = MPayment.get(ctx);\n\
             pays[0].delete(true);\n\
             java.util.List<MTax> taxes = null;\n\
             } }",
        );
        assert_eq!(
            types(&c),
            [
                Some("MInvoiceLine"),
                Some("MOrder"),
                Some("MOrderLine"),
                Some("MPayment")
            ]
        );
        assert!(c.unresolved.is_empty());
    }

    #[test]
    fn this_and_unqualified_calls_mean_the_enclosing_class() {
        let c = calls("class MOrder { void m() { delete(true); this.deleteEx(true); } }");
        assert_eq!(types(&c), [None, None]);
    }

    #[test]
    fn undeclared_receivers_are_unresolved_and_file_delete_is_not_a_call() {
        let c = calls(
            "class A { void m() {\n\
             getLines()[0].delete(true);\n\
             m_order.deleteEx(true);\n\
             x.y.delete(true);\n\
             File f = new File(\"a\");\n\
             f.delete();\n\
             } }",
        );
        assert!(c.resolved.is_empty(), "{c:?}");
        assert_eq!(c.unresolved.len(), 3);
    }

    #[test]
    fn test_paths_are_recognised() {
        assert!(is_test_path(
            "org.idempiere.test/src/org/idempiere/test/model/MOrderTest.java"
        ));
        assert!(is_test_path("x/src/test/java/Y.java"));
        assert!(!is_test_path(
            "org.adempiere.base/src/org/compiere/model/MOrder.java"
        ));
    }
}
