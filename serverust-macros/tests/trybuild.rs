//! Harness trybuild: valida que os casos `pass_*` em `tests/ui/` compilam.

#[test]
fn ui() {
    let t = trybuild::TestCases::new();
    t.pass("tests/ui/pass_*.rs");
    t.compile_fail("tests/ui/fail_*.rs");
}
