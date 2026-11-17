use daimon_protocol::{
    AgentPermissionOptionIdV1, AgentPermissionRequestIdV1, AgentSessionIdV1, AgentTurnIdV1,
    agent::{AgentPermissionOptionKindV1, AgentPermissionOptionV1, AgentPermissionRequestV1},
};
use daimon_tui::{
    AgentPermissionChallenge, AgentPermissionComposer, AgentPermissionInput, AgentPermissionOutcome,
};
use ratatui::{Terminal, backend::TestBackend};

fn challenge(value: u8) -> AgentPermissionChallenge {
    AgentPermissionChallenge {
        session: AgentSessionIdV1::new([value; 16]),
        expected_revision: 12,
        event_cursor: 5,
        challenge: AgentPermissionRequestV1 {
            request: AgentPermissionRequestIdV1::new([value + 10; 16]),
            turn: AgentTurnIdV1::new([value + 20; 16]),
            capability: b"terminal.execute".to_vec(),
            description: format!("run formatter {value}").into_bytes(),
            options: vec![
                AgentPermissionOptionV1 {
                    id: AgentPermissionOptionIdV1::new([value + 30; 16]),
                    kind: AgentPermissionOptionKindV1::AllowOnce,
                    label: b"Allow once".to_vec(),
                },
                AgentPermissionOptionV1 {
                    id: AgentPermissionOptionIdV1::new([value + 40; 16]),
                    kind: AgentPermissionOptionKindV1::RejectOnce,
                    label: b"Reject once".to_vec(),
                },
            ],
        },
    }
}

#[test]
fn permission_workset_defaults_each_session_to_reject_once() {
    let challenges = vec![challenge(1), challenge(2), challenge(3)];
    let expected = challenges
        .iter()
        .map(|challenge| challenge.challenge.options[1].id)
        .collect::<Vec<_>>();
    let mut composer = AgentPermissionComposer::new(challenges).unwrap();
    composer.update(AgentPermissionInput::Arm, true);
    let AgentPermissionOutcome::Submit(selections) =
        composer.update(AgentPermissionInput::Confirm, true)
    else {
        panic!("expected selected workset");
    };
    assert_eq!(
        selections
            .iter()
            .map(|selection| selection.option)
            .collect::<Vec<_>>(),
        expected
    );
}

#[test]
fn operator_can_choose_different_exact_options_per_session() {
    let mut composer = AgentPermissionComposer::new(vec![challenge(1), challenge(2)]).unwrap();
    composer.update(AgentPermissionInput::PreviousOption, true);
    composer.update(AgentPermissionInput::NextSession, true);
    composer.update(AgentPermissionInput::Arm, true);
    let AgentPermissionOutcome::Submit(selections) =
        composer.update(AgentPermissionInput::Confirm, true)
    else {
        panic!("expected selected workset");
    };
    assert_eq!(
        selections[0].challenge.session,
        AgentSessionIdV1::new([1; 16])
    );
    assert_eq!(
        selections[0].option,
        selections[0].challenge.challenge.options[0].id
    );
    assert_eq!(
        selections[1].option,
        selections[1].challenge.challenge.options[1].id
    );
}

#[test]
fn permission_sheet_renders_challenge_and_owner_options() {
    let composer = AgentPermissionComposer::new(vec![challenge(1)]).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(120, 18)).unwrap();
    terminal
        .draw(|frame| composer.draw_in(frame, frame.area(), true))
        .unwrap();
    let rendered = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("terminal.execute"));
    assert!(rendered.contains("run formatter 1"));
    assert!(rendered.contains("RejectOnce"));
    assert!(rendered.contains("> RejectOnce"));
}
