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

#[test]
fn guarded_advance_reuses_the_owner_command_and_attempts_once() {
    let cli = include_str!("../cli/src/main.rs");
    assert!(cli.contains("daimon_agent_command::advance"));
    assert!(cli.contains("agent-advance"));
    assert!(!include_str!("../src/lib.rs").contains("AgentSessionClient"));

    let gate = include_str!("live/agent_advance.sh");
    for required in [
        "agent_advance_fixture",
        "confirmation_refused",
        "agents.session.advance.v1",
        "ADVANCE DAIMON SESSION",
        "advanced",
    ] {
        assert!(gate.contains(required), "advance gate omits {required}");
    }
}

#[test]
fn guarded_cancel_reuses_the_owner_command_and_attempts_once() {
    let cli = include_str!("../cli/src/main.rs");
    assert!(cli.contains("daimon_agent_command::cancel"));
    assert!(cli.contains("agent-cancel"));
    assert!(!include_str!("../src/lib.rs").contains("AgentSessionClient"));

    let gate = include_str!("live/agent_cancel.sh");
    for required in [
        "agent_cancel_fixture",
        "confirmation_refused",
        "agents.session.cancel.v1",
        "CANCEL DAIMON SESSION",
        "cancelled",
    ] {
        assert!(gate.contains(required), "cancel gate omits {required}");
    }
}

#[test]
fn guarded_prompt_reuses_the_owner_command_and_keeps_content_out_of_argv() {
    let cli = include_str!("../cli/src/main.rs");
    assert!(cli.contains("daimon_agent_command::prompt"));
    assert!(cli.contains("parse_prompt_document"));
    assert!(cli.contains("agent-prompt"));
    assert!(!include_str!("../src/lib.rs").contains("AgentSessionClient"));

    let gate = include_str!("live/agent_prompt.sh");
    for required in [
        "agent_prompt_fixture",
        "confirmation_refused",
        "agents.session.prompt.v1",
        "PROMPT DAIMON SESSION",
        "ENVELOPE",
        "prompt.txt",
        "prompted",
    ] {
        assert!(gate.contains(required), "prompt gate omits {required}");
    }
    assert!(!gate.contains("agent-prompt $tmp/success.sock review the boundary"));
}

#[test]
fn guarded_permission_binds_the_exact_current_challenge() {
    let cli = include_str!("../cli/src/main.rs");
    assert!(cli.contains("daimon_agent_command::decide_permission"));
    assert!(cli.contains("parse_permission_document"));
    assert!(cli.contains("agent-permission"));
    assert!(!include_str!("../src/lib.rs").contains("AgentSessionClient"));

    let gate = include_str!("live/agent_permission.sh");
    for required in [
        "agent_permission_fixture",
        "confirmation_refused",
        "agents.session.permission.v1",
        "DECIDE DAIMON PERMISSION",
        "CHALLENGE",
        "ENVELOPE",
        "decided",
    ] {
        assert!(gate.contains(required), "permission gate omits {required}");
    }
}
