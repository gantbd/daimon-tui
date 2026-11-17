use daimon_tui::{AgentPromptComposer, AgentPromptInput, AgentPromptOutcome};
use ratatui::{Terminal, backend::TestBackend};

#[test]
fn multiline_prompt_requires_review_and_distinct_arm() {
    let mut composer = AgentPromptComposer::new(3).unwrap();
    composer.insert_text("inspect all");
    assert_eq!(
        composer.update(AgentPromptInput::Newline, true),
        AgentPromptOutcome::None
    );
    composer.insert_text("report drift");
    assert_eq!(
        composer.update(AgentPromptInput::Confirm, true),
        AgentPromptOutcome::None
    );
    composer.update(AgentPromptInput::NextField, true);
    composer.update(AgentPromptInput::Arm, true);
    assert!(composer.armed());
    assert_eq!(
        composer.update(AgentPromptInput::Confirm, true),
        AgentPromptOutcome::Submit(b"inspect all\nreport drift".to_vec())
    );
}

#[test]
fn review_sheet_shows_cardinality_without_exposing_execution_authority() {
    let mut composer = AgentPromptComposer::new(16).unwrap();
    composer.insert_text("perform bounded work");
    composer.update(AgentPromptInput::NextField, true);

    let mut terminal = Terminal::new(TestBackend::new(120, 16)).unwrap();
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
    assert!(rendered.contains("16 selected session(s)"));
    assert!(rendered.contains("independent execution envelopes and receipts"));
}

#[test]
fn escape_disarms_before_closing() {
    let mut composer = AgentPromptComposer::new(1).unwrap();
    composer.insert_text("work");
    composer.update(AgentPromptInput::NextField, true);
    composer.update(AgentPromptInput::Arm, true);
    assert_eq!(
        composer.update(AgentPromptInput::Close, true),
        AgentPromptOutcome::None
    );
    assert!(!composer.armed());
    assert_eq!(
        composer.update(AgentPromptInput::Close, true),
        AgentPromptOutcome::CloseRequested
    );
}
