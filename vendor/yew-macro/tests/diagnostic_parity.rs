#[test]
fn upstream_successful_macro_contracts() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/derive_props/pass.rs");
    for directory in [
        "classes_macro",
        "function_component_attr",
        "hook_attr",
        "hook_macro",
        "html_macro",
        "props_macro",
    ] {
        cases.pass(format!("tests/{directory}/*-pass.rs"));
    }
}

#[rustversion::since(1.94)]
#[test]
fn upstream_rust194_diagnostic_contracts() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/derive_props/fail.rs");
    for directory in [
        "classes_macro",
        "function_component_attr",
        "hook_attr",
        "hook_macro",
        "html_macro",
        "props_macro",
    ] {
        cases.compile_fail(format!("tests/{directory}/*-fail.rs"));
    }
}
