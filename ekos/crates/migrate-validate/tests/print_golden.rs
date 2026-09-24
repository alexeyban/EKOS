//! `cargo test -p ekos-migrate-validate --test print_golden -- --ignored --nocapture`
//! regenerates the literal table in `golden.rs` after a deliberate rule change. Ignored by
//! default: it prints, it asserts nothing.
#[test]
#[ignore]
fn print_literals() {
    for (name, v, spec) in ekos_migrate_validate::fixtures::cases() {
        let c = ekos_migrate_validate::canon(&v, &spec).unwrap();
        println!(
            "    (\"{name}\", {c:?}, \"{}\"),",
            ekos_migrate_validate::row_hash(&c)
        );
    }
}
