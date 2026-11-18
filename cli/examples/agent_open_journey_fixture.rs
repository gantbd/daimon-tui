//! Stateful Unix fixture for Pantheon's fresh-session work-item journey.

use std::{
    convert::Infallible, fs::OpenOptions, io::Write, os::unix::net::UnixStream, path::PathBuf,
    time::Duration,
};

use daimon_host::AuthenticatedSessionV1;
use daimon_local_daemon::{
    AgentServicePort,
    unix::{AgentDaemon, AgentSessionPort, Authenticator},
};
use daimon_protocol::{
    AgentSessionIdV1, AgentTurnIdV1, ContentDigestV1, SessionHandleV1,
    agent::{
        AgentCapabilitiesV1, AgentCommandV1, AgentEventPageV1, AgentOutcomeV1, AgentRefusalV1,
        AgentReplyV1, AgentSessionPhaseV1, AgentSessionViewV1,
    },
};
use daimon_wire::{LimitsV1, agent::AgentWireLimitsV1};

const SERVICE_SESSION: SessionHandleV1 = SessionHandleV1(51);

struct Authentication;

impl Authenticator for Authentication {
    type Error = Infallible;

    fn authenticate(&mut self, _: &UnixStream) -> Result<AuthenticatedSessionV1, Self::Error> {
        Ok(AuthenticatedSessionV1 {
            principal: [4; 16],
            authority_context: [9; 16],
        })
    }
}

struct Port {
    connected: bool,
    session: Option<AgentSessionIdV1>,
    turn: Option<AgentTurnIdV1>,
    marker: PathBuf,
}

impl AgentSessionPort for Port {
    fn open_agent_service_session(
        &mut self,
        _: AuthenticatedSessionV1,
    ) -> Result<SessionHandleV1, AgentRefusalV1> {
        self.connected = true;
        Ok(SERVICE_SESSION)
    }

    fn close_agent_service_session(&mut self, session: SessionHandleV1) -> bool {
        self.connected = false;
        session == SERVICE_SESSION
    }
}

impl AgentServicePort for Port {
    type Error = Infallible;

    fn execute_agent(
        &mut self,
        service_session: SessionHandleV1,
        command: AgentCommandV1,
    ) -> Result<AgentReplyV1, Self::Error> {
        assert!(self.connected);
        assert_eq!(service_session, SERVICE_SESSION);
        let reply = match command {
            AgentCommandV1::Open { request, now }
                if self.session.is_none()
                    && request.workspace == [0x15; 16]
                    && request.execution.task_attempt.task_key == [0x14; 16]
                    && request.execution.task_attempt.task_incarnation == 1
                    && request.execution.task_attempt.attempt_number == 1
                    && request.budget.max_turns == 8
                    && now > 0 =>
            {
                self.session = Some(request.session);
                self.append("opened");
                AgentReplyV1::Accepted(AgentOutcomeV1::Session(self.view()))
            }
            AgentCommandV1::Inspect { session } if self.session == Some(session) => {
                AgentReplyV1::Accepted(AgentOutcomeV1::Session(self.view()))
            }
            AgentCommandV1::Events {
                session,
                after: None,
                limit: 64,
            } if self.session == Some(session) => {
                AgentReplyV1::Accepted(AgentOutcomeV1::Events(AgentEventPageV1 {
                    session,
                    entries: Vec::new(),
                    next_after: None,
                }))
            }
            AgentCommandV1::Prompt { request, now }
                if self.session == Some(request.session)
                    && self.turn.is_none()
                    && request.expected_revision == 1
                    && request.execution.task_attempt.task_key == [0x14; 16]
                    && now > 0 =>
            {
                let prompt = std::str::from_utf8(&request.prompt)
                    .expect("work-item prompt is bounded UTF-8 text");
                assert!(prompt.contains("product-owner collaboration lens"));
                assert!(prompt.contains("not a human identity"));
                assert!(prompt.ends_with("Work item:\nlocal-agent-task"));
                self.turn = Some(request.turn);
                self.append("prompted");
                AgentReplyV1::Accepted(AgentOutcomeV1::Session(self.view()))
            }
            other => panic!("unexpected fresh-session journey command: {other:?}"),
        };
        Ok(reply)
    }
}

impl Port {
    fn append(&self, action: &str) {
        let mut marker = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.marker)
            .expect("append journey receipt");
        writeln!(marker, "{action}").expect("write journey receipt");
    }

    fn view(&self) -> AgentSessionViewV1 {
        AgentSessionViewV1 {
            session: self.session.expect("session opened before projection"),
            revision: if self.turn.is_some() { 2 } else { 1 },
            phase: if self.turn.is_some() {
                AgentSessionPhaseV1::TurnActive
            } else {
                AgentSessionPhaseV1::Ready
            },
            profile_document_digest: ContentDigestV1::new([3; 32]),
            active_turn: self.turn,
            last_event_cursor: None,
            last_context: None,
            capabilities: AgentCapabilitiesV1 {
                load_session: true,
                resume_session: true,
                permissions: true,
                filesystem_read: false,
                filesystem_write: false,
                terminal: false,
                session_modes: false,
                session_models: false,
            },
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let socket = PathBuf::from(args.next().ok_or("missing socket")?);
    let marker = PathBuf::from(args.next().ok_or("missing marker")?);
    if args.next().is_some() {
        return Err("unexpected argument".into());
    }
    let mut daemon = AgentDaemon::bind_new(
        socket,
        Port {
            connected: false,
            session: None,
            turn: None,
            marker,
        },
        Authentication,
        LimitsV1::default(),
        AgentWireLimitsV1::default(),
        Duration::from_secs(5),
        32,
    )?;
    loop {
        daemon
            .serve_next()
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    }
}
