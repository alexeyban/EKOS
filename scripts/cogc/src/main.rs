//! Estimates Sonar's cognitive complexity (rule S3776) for every Rust function in the given
//! files, so the limit can be checked locally without a SonarCloud scan.
//!
//! `cargo run --release --manifest-path scripts/cogc/Cargo.toml -- <min> <file.rs>...` prints
//! `score<TAB>file:line<TAB>function` for each function scoring at least `min`. Functions inside
//! a `mod tests` are skipped.
//!
//! Calibrated on 2026-10-12 against SonarCloud's own values for revision `c0723c4`: 31 of 35
//! flagged functions matched exactly and the other four were within 8, always low. Treat a score
//! within a few points of the limit (30) as over it.

use syn::visit::{self, Visit};
use syn::{BinOp, Expr};

struct Cx { score: u32, nest: u32, name: String }

impl Cx {
    fn structural(&mut self) { self.score += 1 + self.nest; }
    fn nested<F: FnOnce(&mut Self)>(&mut self, f: F) { self.nest += 1; f(self); self.nest -= 1; }
    fn visit_if(&mut self, i: &syn::ExprIf, is_else_if: bool) {
        if is_else_if { self.score += 1; } else { self.structural(); }
        self.visit_expr(&i.cond);
        self.nested(|s| s.visit_block(&i.then_branch));
        if let Some((_, e)) = &i.else_branch {
            match &**e {
                Expr::If(ei) => self.visit_if(ei, true),
                other => { self.score += 1; self.nested(|s| s.visit_expr(other)); }
            }
        }
    }
    fn bool_seq(&mut self, e: &Expr, parent: Option<bool>) {
        // one increment per run of the same operator
        if let Expr::Binary(b) = e {
            let kind = match b.op { BinOp::And(_) => Some(true), BinOp::Or(_) => Some(false), _ => None };
            if let Some(k) = kind {
                if parent != Some(k) { self.score += 1; }
                self.bool_seq(&b.left, Some(k));
                self.bool_seq(&b.right, Some(k));
                return;
            }
        }
        if let Expr::Paren(p) = e { self.bool_seq(&p.expr, None); return; }
        self.visit_expr(e);
    }
}

impl<'ast> Visit<'ast> for Cx {
    fn visit_expr(&mut self, e: &'ast Expr) {
        match e {
            Expr::If(i) => self.visit_if(i, false),
            Expr::Match(m) => {
                self.structural();
                self.visit_expr(&m.expr);
                self.nested(|s| for a in &m.arms {
                    if let Some((_, g)) = &a.guard { s.visit_expr(g); }
                    s.visit_expr(&a.body);
                });
            }
            Expr::ForLoop(f) => { self.structural(); self.visit_expr(&f.expr); self.nested(|s| s.visit_block(&f.body)); }
            Expr::While(w) => { self.structural(); self.visit_expr(&w.cond); self.nested(|s| s.visit_block(&w.body)); }
            Expr::Loop(l) => { self.structural(); self.nested(|s| s.visit_block(&l.body)); }
            Expr::Closure(c) => self.nested(|s| s.visit_expr(&c.body)),
            Expr::Binary(b) if matches!(b.op, BinOp::And(_) | BinOp::Or(_)) => self.bool_seq(e, None),
            Expr::Break(b) => { if b.label.is_some() { self.score += 1; } visit::visit_expr_break(self, b); }
            Expr::Continue(c) => { if c.label.is_some() { self.score += 1; } }
            Expr::Call(c) => {
                if let Expr::Path(p) = &*c.func { if p.path.segments.len() == 1 && p.path.segments[0].ident == self.name { self.score += 1; } }
                visit::visit_expr_call(self, c);
            }
            _ => visit::visit_expr(self, e),
        }
    }
    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) { self.nested(|s| s.visit_block(&f.block)); }
    fn visit_macro(&mut self, _m: &'ast syn::Macro) {}
}

struct Finder { out: Vec<(usize, String, u32)>, in_test: bool }
impl Finder {
    fn score(&mut self, name: &str, line: usize, block: &syn::Block) {
        let mut c = Cx { score: 0, nest: 0, name: name.to_string() };
        c.visit_block(block);
        self.out.push((line, name.to_string(), c.score));
    }
}
impl<'ast> Visit<'ast> for Finder {
    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        self.score(&f.sig.ident.to_string(), f.sig.ident.span().start().line, &f.block);
        visit::visit_item_fn(self, f);
    }
    fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
        self.score(&f.sig.ident.to_string(), f.sig.ident.span().start().line, &f.block);
        visit::visit_impl_item_fn(self, f);
    }
    fn visit_item_mod(&mut self, m: &'ast syn::ItemMod) {
        if !self.in_test && m.ident == "tests" { return; }
        visit::visit_item_mod(self, m);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let min: u32 = args[0].parse().unwrap();
    for path in &args[1..] {
        let Ok(src) = std::fs::read_to_string(path) else { continue };
        let Ok(file) = syn::parse_file(&src) else { eprintln!("parse fail {path}"); continue };
        let mut f = Finder { out: vec![], in_test: false };
        f.visit_file(&file);
        for (line, name, score) in f.out { if score >= min { println!("{score}\t{path}:{line}\t{name}"); } }
    }
}
