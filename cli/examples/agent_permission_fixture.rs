//! Real bounded Daimon agent Unix fixture for guarded permission qualification.

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
    AgentPermissionOptionIdV1, AgentPermissionRequestIdV1, AgentSessionIdV1, AgentTurnIdV1,
    ContentDigestV1, IdempotencyKeyV1, SessionHandleV1,
    agent::{
        AgentCapabilitiesV1, AgentCommandV1, AgentEventPageV1, AgentEventRecordV1, AgentEventV1,
        AgentOutcomeV1, AgentPermissionOptionKindV1, AgentPermissionOptionV1,
        AgentPermissionRequestV1, AgentRefusalV1, AgentReplyV1, AgentSessionPhaseV1,
        AgentSessionViewV1,
    },
};
use daimon_wire::{LimitsV1, agent::AgentWireLimitsV1};

const SERVICE_SESSION: SessionHandleV1 = SessionHandleV1(41);
const AGENT_SESSION: AgentSessionIdV1 = AgentSessionIdV1::new([7; 16]);
const TURN: AgentTurnIdV1 = AgentTurnIdV1::new([8; 16]);
const KEY: IdempotencyKeyV1 = IdempotencyKeyV1::new([9; 16]);
const REQUEST: AgentPermissionRequestIdV1 = AgentPermissionRequestIdV1::new([21; 16]);
const OPTION: AgentPermissionOptionIdV1 = AgentPermissionOptionIdV1::new([22; 16]);

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
                    AgentSessionPhaseV1::AwaitingPermission,
                )))
            }
            AgentCommandV1::Events {
                session,
                after: Some(4),
                limit: 1,
            } if session == AGENT_SESSION => {
                AgentReplyV1::Accepted(AgentOutcomeV1::Events(AgentEventPageV1 {
                    session,
                    entries: vec![AgentEventRecordV1 {
                        cursor: 5,
                        event: AgentEventV1::Permission(challenge()),
                    }],
                    next_after: None,
                }))
            }
            AgentCommandV1::DecidePermission {
                session,
                expected_revision: 12,
                execution,
                decision,
                now: 34,
            } if session == AGENT_SESSION
                && execution.idempotency_key == KEY
                && execution.principal.into_bytes() == [12; 16]
                && decision.request == REQUEST
                && decision.option == OPTION =>
            {
                let mut marker = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&self.marker)
                    .expect("permission must be decided once");
                marker.write_all(b"decided\n").expect("marker write");
                AgentReplyV1::Accepted(AgentOutcomeV1::Session(view(
                    13,
                    AgentSessionPhaseV1::TurnActive,
                )))
            }
            AgentCommandV1::Inspect { .. } => {
                AgentReplyV1::Refused(AgentRefusalV1::UnknownAgentSession)
            }
            _ => panic!("unexpected guarded permission command"),
        };
        Ok(reply)
    }
}

fn challenge() -> AgentPermissionRequestV1 {
    AgentPermissionRequestV1 {
        request: REQUEST,
        turn: TURN,
        capability: b"terminal.execute".to_vec(),
        description: b"run the selected formatter".to_vec(),
        options: vec![
            AgentPermissionOptionV1 {
                id: OPTION,
                kind: AgentPermissionOptionKindV1::AllowOnce,
                label: b"Allow once".to_vec(),
            },
            AgentPermissionOptionV1 {
                id: AgentPermissionOptionIdV1::new([23; 16]),
                kind: AgentPermissionOptionKindV1::RejectOnce,
                label: b"Reject".to_vec(),
            },
        ],
    }
}

fn view(revision: u64, phase: AgentSessionPhaseV1) -> AgentSessionViewV1 {
    AgentSessionViewV1 {
        session: AGENT_SESSION,
        revision,
        phase,
        profile_document_digest: ContentDigestV1::new([3; 32]),
        active_turn: Some(TURN),
        last_event_cursor: Some(5),
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
