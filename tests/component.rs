use std::{thread, time::Duration};

use daimon_client::ServicePort;
use daimon_protocol::{CommandV1, OutcomeV1, PresenceEntryV1, ReplyV1, SessionHandleV1};
use ratatui::{Terminal, backend::TestBackend};

struct Port;

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
