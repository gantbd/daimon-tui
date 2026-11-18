//! Real bounded Daimon agent Unix fixture for guarded session-open qualification.

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
    AgentSessionIdV1, ContentDigestV1, IdempotencyKeyV1, SessionHandleV1,
    agent::{
        AgentCapabilitiesV1, AgentCommandV1, AgentOutcomeV1, AgentRefusalV1, AgentReplyV1,
        AgentSessionPhaseV1, AgentSessionViewV1,
    },
};
use daimon_wire::{LimitsV1, agent::AgentWireLimitsV1};

const SERVICE_SESSION: SessionHandleV1 = SessionHandleV1(41);
const AGENT_SESSION: AgentSessionIdV1 = AgentSessionIdV1::new([7; 16]);
const KEY: IdempotencyKeyV1 = IdempotencyKeyV1::new([9; 16]);

struct Authentication;

impl Authenticator for Authentication {
    type Error = Infallible;

    fn authenticate(&mut self, _: &UnixStream) -> Result<AuthenticatedSessionV1, Self::Error> {
        Ok(AuthenticatedSessionV1 {
            principal: [12; 16],
            authority_context: [9; 16],
        })
    }
}

struct Port {
    opened: bool,
    marker: PathBuf,
}

impl AgentSessionPort for Port {
    fn open_agent_service_session(
        &mut self,
        _: AuthenticatedSessionV1,
    ) -> Result<SessionHandleV1, AgentRefusalV1> {
        self.opened = true;
        Ok(SERVICE_SESSION)
    }

    fn close_agent_service_session(&mut self, session: SessionHandleV1) -> bool {
        self.opened = false;
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
        assert!(self.opened);
        assert_eq!(service_session, SERVICE_SESSION);
        let AgentCommandV1::Open { request, now } = command else {
            panic!("unexpected guarded session-open command");
        };
        assert_eq!(request.session, AGENT_SESSION);
        assert_eq!(request.workspace, [16; 16]);
        assert_eq!(request.execution.idempotency_key, KEY);
        assert_eq!(request.execution.principal.into_bytes(), [12; 16]);
        assert_eq!(request.budget.max_turns, 8);
        assert_eq!(request.budget.max_events, 128);
        assert_eq!(now, 34);
        let mut marker = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.marker)
            .expect("session open must be dispatched once");
        marker.write_all(b"opened\n").expect("marker write");
        Ok(AgentReplyV1::Accepted(AgentOutcomeV1::Session(view())))
    }
}

fn view() -> AgentSessionViewV1 {
    AgentSessionViewV1 {
        session: AGENT_SESSION,
        revision: 1,
        phase: AgentSessionPhaseV1::Ready,
        profile_document_digest: ContentDigestV1::new([3; 32]),
        active_turn: None,
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
            opened: false,
            marker,
        },
        Authentication,
        LimitsV1::default(),
        AgentWireLimitsV1::default(),
        Duration::from_secs(5),
        4,
    )?;
    daemon
        .serve_next()
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    Ok(())
}
