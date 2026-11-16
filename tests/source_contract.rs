#[test]
fn reusable_component_has_no_application_or_execution_authority() {
    let source = include_str!("../src/lib.rs");
    assert!(!source.contains("std::env::args"));
    assert!(!source.contains("ratatui::run"));
    assert!(!source.contains("ExecutionClient"));
    assert!(!source.contains("ModelExecutionClient"));
    assert!(!source.contains("AgentSessionClient"));
    assert!(source.contains("AgentReadOnlyClient"));
}

#[test]
fn headless_path_uses_public_read_only_client_without_component_state() {
    let source = include_str!("../cli/src/main.rs");
    let headless = source.split("fn run_headless").nth(1).unwrap();
    assert!(headless.contains("ReadOnlyClient"));
    assert!(!headless.contains("Component::new"));
    let agent = source.split("fn run_agent_headless").nth(1).unwrap();
    assert!(agent.contains("AgentReadOnlyClient"));
    assert!(agent.contains("observe_agent"));
    assert!(!agent.contains("AgentComponent::new"));
}

#[test]
fn real_gate_uses_the_canonical_agent_socket_in_both_modes() {
    let gate = include_str!("live/real_daemon.sh");
    for required in [
        "agent_daemon_fixture",
        "agent-inspect",
        "agent-interactive",
        "agents.session.inspect.v1",
        "ReattachRequired",
    ] {
        assert!(gate.contains(required), "real gate omits {required}");
    }
}
