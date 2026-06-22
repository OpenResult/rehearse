#[test]
fn operation_macro_ui() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/ui/operation/pass/*.rs");
    cases.compile_fail("tests/ui/operation/fail/*.rs");
}
