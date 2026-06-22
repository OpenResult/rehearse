#[test]
fn pipeline_macro_ui() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/ui/pipeline/pass/*.rs");
    cases.compile_fail("tests/ui/pipeline/fail/*.rs");
}
