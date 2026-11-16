//! Real bounded Daimon agent Unix fixture for guarded prompt qualification.

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
    AgentSessionIdV1, AgentTurnIdV1, ContentDigestV1, IdempotencyKeyV1, SessionHandleV1,
    agent::{
        AgentCapabilitiesV1, AgentCommandV1, AgentOutcomeV1, AgentRefusalV1, AgentReplyV1,
        AgentSessionPhaseV1, AgentSessionViewV1,
    },
};
use daimon_wire::{LimitsV1, agent::AgentWireLimitsV1};

const SERVICE_SESSION: SessionHandleV1 = SessionHandleV1(41);
const AGENT_SESSION: AgentSessionIdV1 = AgentSessionIdV1::new([7; 16]);
const TURN: AgentTurnIdV1 = AgentTurnIdV1::new([8; 16]);
const KEY: IdempotencyKeyV1 = IdempotencyKeyV1::new([9; 16]);
const PROMPT_DIGEST: [u8; 32] = [
    0xdd, 0x18, 0x84, 0x14, 0x48, 0x85, 0x5d, 0x3b, 0x88, 0x58, 0xc9, 0x36, 0x54, 0xe5, 0xd0, 0x2b,
    0x48, 0x84, 0x9a, 0x1b, 0xe8, 0xb5, 0x39, 0x8b, 0x76, 0x6e, 0x79, 0x36, 0xae, 0x2f, 0xde, 0x65,
];

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
        let reply = match command {
            AgentCommandV1::Inspect { session } if session == AGENT_SESSION => {
                AgentReplyV1::Accepted(AgentOutcomeV1::Session(view(
                    12,
                    AgentSessionPhaseV1::Ready,
                    None,
                )))
            }
            AgentCommandV1::Prompt { request, now }
                if request.session == AGENT_SESSION
                    && request.turn == TURN
                    && request.expected_revision == 12
                    && request.execution.idempotency_key == KEY
                    && request.execution.principal.into_bytes() == [12; 16]
                    && request.prompt == b"review the boundary"
                    && request.prompt_digest.into_bytes() == PROMPT_DIGEST
                    && now == 34 =>
            {
                let mut marker = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&self.marker)
                    .expect("prompt must be dispatched once");
                marker.write_all(b"prompted\n").expect("marker write");
                AgentReplyV1::Accepted(AgentOutcomeV1::Session(view(
                    13,
                    AgentSessionPhaseV1::TurnActive,
                    Some(TURN),
                )))
            }
            AgentCommandV1::Inspect { .. } => {
                AgentReplyV1::Refused(AgentRefusalV1::UnknownAgentSession)
            }
            _ => panic!("unexpected guarded prompt command"),
        };
        Ok(reply)
    }
}

fn view(
    revision: u64,
    phase: AgentSessionPhaseV1,
    active_turn: Option<AgentTurnIdV1>,
) -> AgentSessionViewV1 {
    AgentSessionViewV1 {
        session: AGENT_SESSION,
        revision,
        phase,
        profile_document_digest: ContentDigestV1::new([3; 32]),
        active_turn,
        last_event_cursor: Some(2),
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
