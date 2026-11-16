use std::{thread, time::Duration};

use daimon_client::{AgentServicePort, ServicePort};
use daimon_protocol::{
    AgentSessionIdV1, CommandV1, ContentDigestV1, OutcomeV1, PresenceEntryV1, ReplyV1,
    SessionHandleV1,
    agent::{
        AgentCapabilitiesV1, AgentCommandV1, AgentEventPageV1, AgentEventRecordV1, AgentEventV1,
        AgentOutcomeV1, AgentReplyV1, AgentSessionPhaseV1, AgentSessionViewV1,
    },
};
use ratatui::{Terminal, backend::TestBackend};

struct Port;
struct AgentPort;

impl ServicePort for Port {
    type Error = ();

    fn execute(
        &mut self,
        session: SessionHandleV1,
        command: CommandV1,
    ) -> Result<ReplyV1, Self::Error> {
        assert_eq!(session, SessionHandleV1(4));
        assert!(matches!(command, CommandV1::ReadPresence { limit: 64 }));
        Ok(ReplyV1::Accepted(OutcomeV1::Presence {
            entries: vec![PresenceEntryV1 {
                agent: [9; 16],
                age_millis: 12,
                stale: false,
            }],
            truncated: false,
        }))
    }
}

impl AgentServicePort for AgentPort {
    type Error = ();

    fn execute_agent(
        &mut self,
        service_session: SessionHandleV1,
        command: AgentCommandV1,
    ) -> Result<AgentReplyV1, Self::Error> {
        assert_eq!(service_session, SessionHandleV1(5));
        let session = AgentSessionIdV1::new([7; 16]);
        let outcome = match command {
            AgentCommandV1::Inspect { session: actual } if actual == session => {
                AgentOutcomeV1::Session(AgentSessionViewV1 {
                    session,
                    revision: 8,
                    phase: AgentSessionPhaseV1::ReattachRequired,
                    profile_document_digest: ContentDigestV1::new([3; 32]),
                    active_turn: None,
                    last_event_cursor: Some(2),
                    capabilities: AgentCapabilitiesV1 {
                        load_session: true,
                        resume_session: true,
                        permissions: true,
                        filesystem_read: true,
                        filesystem_write: false,
                        terminal: true,
                        session_modes: true,
                        session_models: true,
                    },
                })
            }
            AgentCommandV1::Events {
                session: actual,
                after: None,
                limit: 64,
            } if actual == session => AgentOutcomeV1::Events(AgentEventPageV1 {
                session,
                entries: vec![
                    AgentEventRecordV1 {
                        cursor: 1,
                        event: AgentEventV1::AssistantMessage {
                            bytes: b"hello\nworld".to_vec(),
                        },
                    },
                    AgentEventRecordV1 {
                        cursor: 2,
                        event: AgentEventV1::Usage {
                            input_tokens: 11,
                            output_tokens: 13,
                        },
                    },
                ],
                next_after: None,
            }),
            _ => panic!("unexpected agent observation command"),
        };
        Ok(AgentReplyV1::Accepted(outcome))
    }
}

#[test]
fn component_renders_redacted_public_presence() {
    let mut component = daimon_tui::Component::new(SessionHandleV1(4), Port);
    for _ in 0..100 {
        component.poll();
        let mut terminal = Terminal::new(TestBackend::new(80, 8)).unwrap();
        terminal
            .draw(|frame| component.draw_in(frame, frame.area(), true))
            .unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        if rendered.contains("age=12ms") {
            assert!(rendered.contains("agent current"));
            assert!(!rendered.contains("090909"));
            return;
        }
        thread::sleep(Duration::from_millis(1));
    }
    panic!("Daimon presence did not render");
}

#[test]
fn agent_component_renders_exact_configured_session_evidence() {
    let session = AgentSessionIdV1::new([7; 16]);
    let mut component = daimon_tui::AgentComponent::new(SessionHandleV1(5), AgentPort, session);
    for _ in 0..100 {
        component.poll();
        let mut terminal = Terminal::new(TestBackend::new(140, 14)).unwrap();
        terminal
            .draw(|frame| component.draw_in(frame, frame.area(), true))
            .unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        if rendered.contains("ReattachRequired/8") {
            assert!(rendered.contains("load=true resume=true permissions=true"));
            assert!(rendered.contains("assistant hello\\nworld"));
            assert!(rendered.contains("usage input=11 output=13"));
            assert!(!rendered.contains("070707"));
            return;
        }
        thread::sleep(Duration::from_millis(1));
    }
    panic!("Daimon agent session did not render");
}
