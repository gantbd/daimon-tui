#[test]
fn reusable_component_has_no_application_or_execution_authority() {
    let source = include_str!("../src/lib.rs");
    assert!(!source.contains("std::env::args"));
    assert!(!source.contains("ratatui::run"));
    assert!(!source.contains("ExecutionClient"));
    assert!(!source.contains("ModelExecutionClient"));
}

#[test]
fn headless_path_uses_public_read_only_client_without_component_state() {
    let source = include_str!("../cli/src/main.rs");
    let headless = source.split("fn run_headless").nth(1).unwrap();
    assert!(headless.contains("ReadOnlyClient"));
    assert!(!headless.contains("Component::new"));
}
