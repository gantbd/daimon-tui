//! Real bounded Daimon agent Unix fixture for terminal qualification.

use std::{convert::Infallible, os::unix::net::UnixStream, path::PathBuf, time::Duration};

use daimon_host::AuthenticatedSessionV1;
use daimon_local_daemon::{
    AgentServicePort,
    unix::{AgentDaemon, AgentSessionPort, Authenticator},
};
use daimon_protocol::{
    AgentSessionIdV1, AgentTurnIdV1, ContentDigestV1, SessionHandleV1,
    agent::{
        AgentCapabilitiesV1, AgentCommandV1, AgentEventPageV1, AgentEventRecordV1, AgentEventV1,
        AgentOutcomeV1, AgentRefusalV1, AgentReplyV1, AgentSessionPhaseV1, AgentSessionViewV1,
    },
};
use daimon_wire::{LimitsV1, agent::AgentWireLimitsV1};

const SERVICE_SESSION: SessionHandleV1 = SessionHandleV1(41);
const AGENT_SESSION: AgentSessionIdV1 = AgentSessionIdV1::new([7; 16]);

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

#[derive(Default)]
struct Port {
    opened: bool,
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
                AgentReplyV1::Accepted(AgentOutcomeV1::Session(view()))
            }
            AgentCommandV1::Events {
                session,
                after: None,
                limit: 64,
            } if session == AGENT_SESSION => {
                AgentReplyV1::Accepted(AgentOutcomeV1::Events(AgentEventPageV1 {
                    session,
                    entries: vec![
                        AgentEventRecordV1 {
                            cursor: 1,
                            event: AgentEventV1::Plan {
                                bytes: b"inspect before resuming".to_vec(),
                            },
                        },
                        AgentEventRecordV1 {
                            cursor: 2,
                            event: AgentEventV1::Usage {
                                input_tokens: 21,
                                output_tokens: 34,
                            },
                        },
                    ],
                    next_after: None,
                }))
            }
            AgentCommandV1::Inspect { .. } | AgentCommandV1::Events { .. } => {
                AgentReplyV1::Refused(AgentRefusalV1::UnknownAgentSession)
            }
            _ => panic!("read-only fixture received a mutation"),
        };
        Ok(reply)
    }
}

fn view() -> AgentSessionViewV1 {
    AgentSessionViewV1 {
        session: AGENT_SESSION,
        revision: 12,
        phase: AgentSessionPhaseV1::ReattachRequired,
        profile_document_digest: ContentDigestV1::new([3; 32]),
        active_turn: Some(AgentTurnIdV1::new([8; 16])),
        last_event_cursor: Some(2),
        last_context: None,
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
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let socket = PathBuf::from(args.next().ok_or("missing socket")?);
    if args.next().is_some() {
        return Err("unexpected argument".into());
    }
    let mut daemon = AgentDaemon::bind_new(
        socket,
        Port::default(),
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
