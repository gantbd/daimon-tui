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
    AgentSessionIdV1, AgentTurnIdV1, ContentDigestV1, ExecutionDispositionV1, SessionHandleV1,
    agent::{
        AgentCapabilitiesV1, AgentCommandV1, AgentEventPageV1, AgentEventRecordV1, AgentEventV1,
        AgentOutcomeV1, AgentRefusalV1, AgentReplyV1, AgentResultBindingV1, AgentSessionPhaseV1,
        AgentSessionViewV1, AgentTurnRequestV1, AgentTurnStopReasonV1,
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
    last_turn: Option<AgentTurnIdV1>,
    last_request: Option<AgentTurnRequestV1>,
    revision: u64,
    prompt_count: u32,
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
                // Model the local agent completing its accepted turn between
                // observations. Production readiness remains owner-derived;
                // Pantheon never clears or invents the active turn itself.
                self.turn = None;
                AgentReplyV1::Accepted(AgentOutcomeV1::Session(self.view()))
            }
            AgentCommandV1::Events {
                session,
                after,
                limit,
            } if self.session == Some(session) => {
                let entries = self
                    .result_events()
                    .into_iter()
                    .filter(|record| after.is_none_or(|cursor| record.cursor > cursor))
                    .take(usize::try_from(limit).unwrap_or(usize::MAX))
                    .collect();
                AgentReplyV1::Accepted(AgentOutcomeV1::Events(AgentEventPageV1 {
                    session,
                    entries,
                    next_after: None,
                }))
            }
            AgentCommandV1::Result { session, turn }
                if self.session == Some(session)
                    && self.last_request.as_ref().map(|request| request.turn) == Some(turn) =>
            {
                let request = self.last_request.as_ref().expect("matched retained prompt");
                let first = u64::from(self.prompt_count.saturating_sub(1)) * 2 + 1;
                let (_, digest) = self.result_content();
                AgentReplyV1::Accepted(AgentOutcomeV1::Result(AgentResultBindingV1 {
                    session,
                    turn,
                    execution: request.execution.execution_id,
                    task_attempt: request.execution.task_attempt,
                    principal: request.execution.principal,
                    profile: request.execution.profile,
                    operation: request.execution.idempotency_key,
                    authority_decision: [0x21; 16],
                    authority_expires_at: 100,
                    prompt_digest: request.prompt_digest,
                    context_manifest_digest: ContentDigestV1::new([0x23; 32]),
                    event_start_cursor: first,
                    terminal_cursor: first + 1,
                    terminal_revision: self.revision,
                    disposition: ExecutionDispositionV1::Completed,
                    stop_reason: AgentTurnStopReasonV1::EndTurn,
                    output_digest: Some(ContentDigestV1::new(digest)),
                }))
            }
            AgentCommandV1::Prompt { request, now }
                if self.session == Some(request.session)
                    && self.turn.is_none()
                    && request.expected_revision == self.revision
                    && request.execution.task_attempt.task_key == [0x14; 16]
                    && now > 0 =>
            {
                let prompt = std::str::from_utf8(&request.prompt)
                    .expect("work-item prompt is bounded UTF-8 text");
                match self.prompt_count {
                    0 => {
                        assert!(prompt.contains("product-owner collaboration lens"));
                        assert!(prompt.contains("not a human identity"));
                        assert!(prompt.ends_with("Work item:\nlocal-agent-task"));
                    }
                    1 => {
                        assert!(prompt.contains("ordinary room chatter is not a prompt"));
                        assert!(prompt.contains("Please inspect the bounded acceptance evidence"));
                        assert!(prompt.contains("Configured addressee: local-agent"));
                    }
                    count => panic!("unexpected journey prompt count: {count}"),
                }
                self.turn = Some(request.turn);
                self.last_turn = Some(request.turn);
                self.last_request = Some(request.clone());
                self.revision += 1;
                self.prompt_count += 1;
                self.append("prompted");
                AgentReplyV1::Accepted(AgentOutcomeV1::Session(self.view()))
            }
            other => panic!("unexpected fresh-session journey command: {other:?}"),
        };
        Ok(reply)
    }
}

impl Port {
    fn result_content(&self) -> (Vec<u8>, [u8; 32]) {
        if self.prompt_count == 1 {
            (
                b"The acceptance evidence is bounded and attributable.".to_vec(),
                [
                    0x56, 0xdc, 0xa1, 0x98, 0xaa, 0xaf, 0x18, 0x23, 0x9d, 0x3c, 0xd5, 0xb3, 0x6a,
                    0x4a, 0xe9, 0xbd, 0x48, 0x6e, 0x7e, 0x65, 0x24, 0xae, 0xdf, 0x8c, 0xa9, 0x4a,
                    0x17, 0x03, 0x3b, 0x29, 0x68, 0x70,
                ],
            )
        } else {
            (
                b"The addressed room evidence was inspected without ambient authority.".to_vec(),
                [
                    0x84, 0xdc, 0x53, 0x24, 0x9d, 0xb0, 0xa3, 0xc4, 0xda, 0x79, 0x8f, 0xb2, 0x06,
                    0x77, 0xe9, 0x41, 0x59, 0xfb, 0x66, 0x00, 0xd4, 0xcf, 0x1e, 0x10, 0xa6, 0x4e,
                    0x49, 0x84, 0xf5, 0xd6, 0x31, 0x3a,
                ],
            )
        }
    }

    fn result_events(&self) -> Vec<AgentEventRecordV1> {
        let Some(turn) = self.last_turn else {
            return Vec::new();
        };
        let (bytes, digest) = self.result_content();
        let first = u64::from(self.prompt_count.saturating_sub(1)) * 2 + 1;
        vec![
            AgentEventRecordV1 {
                cursor: first,
                event: AgentEventV1::AssistantMessage {
                    bytes: bytes.clone(),
                },
            },
            AgentEventRecordV1 {
                cursor: first + 1,
                event: AgentEventV1::TurnFinished {
                    turn,
                    disposition: ExecutionDispositionV1::Completed,
                    stop_reason: AgentTurnStopReasonV1::EndTurn,
                    output_digest: Some(ContentDigestV1::new(digest)),
                },
            },
        ]
    }

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
            revision: self.revision,
            phase: if self.turn.is_some() {
                AgentSessionPhaseV1::TurnActive
            } else {
                AgentSessionPhaseV1::Ready
            },
            profile_document_digest: ContentDigestV1::new([3; 32]),
            active_turn: self.turn,
            last_event_cursor: (self.prompt_count > 0).then(|| u64::from(self.prompt_count) * 2),
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
            last_turn: None,
            last_request: None,
            revision: 1,
            prompt_count: 0,
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
