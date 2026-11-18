//! Real bounded Daimon Unix fixture for Pantheon session-set qualification.

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
    ContentDigestV1, SessionHandleV1,
    agent::{
        AgentCapabilitiesV1, AgentCommandV1, AgentEventPageV1, AgentEventRecordV1, AgentEventV1,
        AgentOutcomeV1, AgentPermissionOptionKindV1, AgentPermissionOptionV1,
        AgentPermissionRequestV1, AgentRefusalV1, AgentReplyV1, AgentSessionPhaseV1,
        AgentSessionViewV1,
    },
};
use daimon_wire::{LimitsV1, agent::AgentWireLimitsV1};

const SERVICE_SESSION: SessionHandleV1 = SessionHandleV1(41);

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
    opened: bool,
    completed: [bool; 3],
    turns: [Option<AgentTurnIdV1>; 3],
    mode: Mode,
    marker: PathBuf,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Mode {
    Close,
    CloseUnknown,
    Prompt,
    PromptLens,
    PromptLensTask,
    Permission,
    Reattach,
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
            AgentCommandV1::Inspect { session } => self.session_index(session).map_or_else(
                || AgentReplyV1::Refused(AgentRefusalV1::UnknownAgentSession),
                |index| AgentReplyV1::Accepted(AgentOutcomeV1::Session(self.view(index))),
            ),
            AgentCommandV1::Events {
                session,
                after: None,
                limit: 64,
            } if self.session_index(session).is_some() => {
                let entries = self.session_index(session).map_or_else(Vec::new, |index| {
                    if self.mode == Mode::Permission {
                        vec![AgentEventRecordV1 {
                            cursor: 5,
                            event: AgentEventV1::Permission(self.challenge(index)),
                        }]
                    } else {
                        Vec::new()
                    }
                });
                AgentReplyV1::Accepted(AgentOutcomeV1::Events(AgentEventPageV1 {
                    session,
                    entries,
                    next_after: None,
                }))
            }
            AgentCommandV1::Events {
                session,
                after: Some(4),
                limit: 1,
            } if self.mode == Mode::Permission => self.session_index(session).map_or_else(
                || AgentReplyV1::Refused(AgentRefusalV1::UnknownAgentSession),
                |index| {
                    AgentReplyV1::Accepted(AgentOutcomeV1::Events(AgentEventPageV1 {
                        session,
                        entries: vec![AgentEventRecordV1 {
                            cursor: 5,
                            event: AgentEventV1::Permission(self.challenge(index)),
                        }],
                        next_after: None,
                    }))
                },
            ),
            AgentCommandV1::Close {
                session,
                expected_revision: 12,
                idempotency_key,
                ..
            } if matches!(self.mode, Mode::Close | Mode::CloseUnknown) => {
                self.session_index(session).map_or_else(
                    || AgentReplyV1::Refused(AgentRefusalV1::UnknownAgentSession),
                    |index| {
                        assert!(
                            !self.completed[index],
                            "session close attempted more than once"
                        );
                        self.completed[index] = true;
                        let mut marker = OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(&self.marker)
                            .expect("append close receipt");
                        writeln!(marker, "session {} closed", index + 1)
                            .expect("write close receipt");
                        if self.mode == Mode::CloseUnknown && index == 1 {
                            AgentReplyV1::Accepted(AgentOutcomeV1::OutcomeUnknown {
                                session,
                                idempotency_key,
                            })
                        } else {
                            AgentReplyV1::Accepted(AgentOutcomeV1::Session(self.view(index)))
                        }
                    },
                )
            }
            AgentCommandV1::Prompt { request, .. }
                if request.expected_revision == 12
                    && matches!(
                        self.mode,
                        Mode::Prompt | Mode::PromptLens | Mode::PromptLensTask
                    ) =>
            {
                assert_task_attempt(&request.execution.task_attempt);
                self.session_index(request.session).map_or_else(
                    || AgentReplyV1::Refused(AgentRefusalV1::UnknownAgentSession),
                    |index| {
                        assert!(
                            !self.completed[index],
                            "session prompt attempted more than once"
                        );
                        if matches!(self.mode, Mode::PromptLens | Mode::PromptLensTask) {
                            let prompt = std::str::from_utf8(&request.prompt)
                                .expect("lens prompt is bounded UTF-8 text");
                            assert!(prompt.contains("product-owner collaboration lens"));
                            assert!(prompt.contains("not a human identity"));
                            assert!(
                                prompt.contains("not a human identity")
                                    && prompt.contains("authority")
                            );
                            if self.mode == Mode::PromptLensTask {
                                assert!(prompt.ends_with("Work item:\nlocal-agent-task"));
                            } else {
                                assert!(prompt.ends_with("Work item:\ninspect\nreport drift"));
                            }
                        } else {
                            assert_eq!(request.prompt, b"inspect\nreport drift");
                        }
                        self.completed[index] = true;
                        self.turns[index] = Some(request.turn);
                        let mut marker = OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(&self.marker)
                            .expect("append prompt receipt");
                        writeln!(marker, "session {} prompted", index + 1)
                            .expect("write prompt receipt");
                        AgentReplyV1::Accepted(AgentOutcomeV1::Session(self.view(index)))
                    },
                )
            }
            AgentCommandV1::DecidePermission {
                session,
                expected_revision: 12,
                execution,
                decision,
                ..
            } if self.mode == Mode::Permission => {
                assert_task_attempt(&execution.task_attempt);
                self.session_index(session).map_or_else(
                    || AgentReplyV1::Refused(AgentRefusalV1::UnknownAgentSession),
                    |index| {
                        assert!(
                            !self.completed[index],
                            "permission attempted more than once"
                        );
                        let challenge = self.challenge(index);
                        assert_eq!(decision.request, challenge.request);
                        let option = challenge
                            .options
                            .iter()
                            .find(|option| option.id == decision.option)
                            .expect("owner-published permission option");
                        self.completed[index] = true;
                        let mut marker = OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(&self.marker)
                            .expect("append permission receipt");
                        writeln!(marker, "session {} permission {:?}", index + 1, option.kind)
                            .expect("write permission receipt");
                        AgentReplyV1::Accepted(AgentOutcomeV1::Session(self.view(index)))
                    },
                )
            }
            AgentCommandV1::Reattach {
                session,
                expected_revision: 12,
                execution,
                ..
            } if self.mode == Mode::Reattach => {
                assert_task_attempt(&execution.task_attempt);
                self.session_index(session).map_or_else(
                    || AgentReplyV1::Refused(AgentRefusalV1::UnknownAgentSession),
                    |index| {
                        assert!(
                            !self.completed[index],
                            "session reattach attempted more than once"
                        );
                        self.completed[index] = true;
                        let mut marker = OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(&self.marker)
                            .expect("append reattach receipt");
                        writeln!(marker, "session {} reattached", index + 1)
                            .expect("write reattach receipt");
                        AgentReplyV1::Accepted(AgentOutcomeV1::Session(self.view(index)))
                    },
                )
            }
            other => panic!("unexpected session-set command: {other:?}"),
        };
        Ok(reply)
    }
}

fn assert_task_attempt(attempt: &daimon_protocol::TaskAttemptRefV1) {
    assert_eq!(attempt.task_key, [0x14; 16]);
    assert_eq!(attempt.task_incarnation, 1);
    assert_eq!(attempt.attempt_number, 1);
}

impl Port {
    fn session_index(&self, session: AgentSessionIdV1) -> Option<usize> {
        (0..3).find(|index| session == AgentSessionIdV1::new([*index as u8 + 7; 16]))
    }

    fn view(&self, index: usize) -> AgentSessionViewV1 {
        AgentSessionViewV1 {
            session: AgentSessionIdV1::new([index as u8 + 7; 16]),
            revision: if self.completed[index] { 13 } else { 12 },
            phase: if self.completed[index] && matches!(self.mode, Mode::Close | Mode::CloseUnknown)
            {
                AgentSessionPhaseV1::Closed
            } else if self.completed[index] && self.mode == Mode::Reattach {
                AgentSessionPhaseV1::Ready
            } else if self.completed[index] {
                AgentSessionPhaseV1::TurnActive
            } else if self.mode == Mode::Permission {
                AgentSessionPhaseV1::AwaitingPermission
            } else if self.mode == Mode::Reattach {
                AgentSessionPhaseV1::ReattachRequired
            } else {
                AgentSessionPhaseV1::Ready
            },
            profile_document_digest: ContentDigestV1::new([3; 32]),
            active_turn: if self.mode == Mode::Permission {
                Some(AgentTurnIdV1::new([index as u8 + 20; 16]))
            } else {
                self.turns[index]
            },
            last_event_cursor: (self.mode == Mode::Permission).then_some(5),
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

    fn challenge(&self, index: usize) -> AgentPermissionRequestV1 {
        AgentPermissionRequestV1 {
            request: AgentPermissionRequestIdV1::new([index as u8 + 30; 16]),
            turn: AgentTurnIdV1::new([index as u8 + 20; 16]),
            capability: b"terminal.execute".to_vec(),
            description: format!("run formatter for session {}", index + 1).into_bytes(),
            options: vec![
                AgentPermissionOptionV1 {
                    id: AgentPermissionOptionIdV1::new([index as u8 + 40; 16]),
                    kind: AgentPermissionOptionKindV1::AllowOnce,
                    label: b"Allow once".to_vec(),
                },
                AgentPermissionOptionV1 {
                    id: AgentPermissionOptionIdV1::new([index as u8 + 50; 16]),
                    kind: AgentPermissionOptionKindV1::RejectOnce,
                    label: b"Reject once".to_vec(),
                },
            ],
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let socket = PathBuf::from(args.next().ok_or("missing socket")?);
    let marker = PathBuf::from(args.next().ok_or("missing marker")?);
    let mode = match args.next().as_deref() {
        None => Mode::Close,
        Some(value) if value == "close" => Mode::Close,
        Some(value) if value == "close-unknown" => Mode::CloseUnknown,
        Some(value) if value == "prompt" => Mode::Prompt,
        Some(value) if value == "prompt-lens" => Mode::PromptLens,
        Some(value) if value == "prompt-lens-task" => Mode::PromptLensTask,
        Some(value) if value == "permission" => Mode::Permission,
        Some(value) if value == "reattach" => Mode::Reattach,
        Some(_) => {
            return Err(
                "mode must be close, close-unknown, prompt, prompt-lens, prompt-lens-task, permission, or reattach"
                    .into(),
            );
        }
    };
    if args.next().is_some() {
        return Err("unexpected argument".into());
    }
    let mut daemon = AgentDaemon::bind_new(
        socket,
        Port {
            opened: false,
            completed: [false; 3],
            turns: [None; 3],
            mode,
            marker,
        },
        Authentication,
        LimitsV1::default(),
        AgentWireLimitsV1::default(),
        Duration::from_secs(5),
        32,
    )?;
    // One startup probe, one 3-session refresh, one connection per child, and
    // one post-receipt refresh that proves the accepted owner projections.
    let connections = if mode == Mode::CloseUnknown { 5 } else { 6 };
    for _ in 0..connections {
        daemon
            .serve_next()
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    }
    Ok(())
}
