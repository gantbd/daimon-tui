use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use daimon_agent_command::{CloseRequest, PromptRequest};
use daimon_client::AgentServicePort;
use daimon_protocol::{
    AgentSessionIdV1, AgentTurnIdV1, ContentDigestV1, ContextReferenceIdV1, ExecuteRequestV1,
    ExecutionBudgetV1, ExecutionIdV1, HarnessProfileIdV1, IdempotencyKeyV1, PrincipalIdV1,
    SessionHandleV1, TaskAttemptRefV1, ToolCapabilityIdV1,
    agent::{
        AgentCapabilitiesV1, AgentCommandV1, AgentOutcomeV1, AgentReplyV1, AgentSessionPhaseV1,
        AgentSessionViewV1,
    },
};
use daimon_tui::{
    AgentControlBatchComponent, AgentControlChildDisposition, AgentControlComponent,
    AgentControlDisposition, AgentControlInput, AgentControlRequest, Lifecycle,
};
use ratatui::{Terminal, backend::TestBackend};

struct Port {
    mutations: Arc<AtomicUsize>,
    unknown: bool,
}

struct BatchPort {
    mutations: Arc<AtomicUsize>,
}

struct PromptBatchPort {
    mutations: Arc<AtomicUsize>,
}

impl AgentServicePort for PromptBatchPort {
    type Error = ();

    fn execute_agent(
        &mut self,
        service_session: SessionHandleV1,
        command: AgentCommandV1,
    ) -> Result<AgentReplyV1, Self::Error> {
        assert_eq!(service_session, SessionHandleV1(5));
        match command {
            AgentCommandV1::Inspect { session } => Ok(AgentReplyV1::Accepted(
                AgentOutcomeV1::Session(view_for(session, AgentSessionPhaseV1::Ready, 8)),
            )),
            AgentCommandV1::Prompt { request, now: 42 } => {
                assert_eq!(request.expected_revision, 8);
                assert_eq!(request.prompt, b"inspect every selected workspace");
                self.mutations.fetch_add(1, Ordering::SeqCst);
                if request.session == AgentSessionIdV1::new([8; 16]) {
                    Ok(AgentReplyV1::Accepted(AgentOutcomeV1::OutcomeUnknown {
                        session: request.session,
                        idempotency_key: request.execution.idempotency_key,
                    }))
                } else {
                    let mut accepted =
                        view_for(request.session, AgentSessionPhaseV1::TurnActive, 9);
                    accepted.active_turn = Some(request.turn);
                    Ok(AgentReplyV1::Accepted(AgentOutcomeV1::Session(accepted)))
                }
            }
            other => panic!("unexpected prompt batch command: {other:?}"),
        }
    }
}

impl AgentServicePort for BatchPort {
    type Error = ();

    fn execute_agent(
        &mut self,
        service_session: SessionHandleV1,
        command: AgentCommandV1,
    ) -> Result<AgentReplyV1, Self::Error> {
        assert_eq!(service_session, SessionHandleV1(5));
        match command {
            AgentCommandV1::Inspect { session } => Ok(AgentReplyV1::Accepted(
                AgentOutcomeV1::Session(view_for(session, AgentSessionPhaseV1::Ready, 8)),
            )),
            AgentCommandV1::Close {
                session,
                expected_revision: 8,
                idempotency_key,
                now: 42,
            } => {
                self.mutations.fetch_add(1, Ordering::SeqCst);
                if session == AgentSessionIdV1::new([8; 16]) {
                    Ok(AgentReplyV1::Accepted(AgentOutcomeV1::OutcomeUnknown {
                        session,
                        idempotency_key,
                    }))
                } else {
                    Ok(AgentReplyV1::Accepted(AgentOutcomeV1::Session(view_for(
                        session,
                        AgentSessionPhaseV1::Closed,
                        9,
                    ))))
                }
            }
            other => panic!("unexpected batch command: {other:?}"),
        }
    }
}

fn view_for(
    session: AgentSessionIdV1,
    phase: AgentSessionPhaseV1,
    revision: u64,
) -> AgentSessionViewV1 {
    let mut value = view(phase, revision);
    value.session = session;
    value
}

impl AgentServicePort for Port {
    type Error = ();

    fn execute_agent(
        &mut self,
        service_session: SessionHandleV1,
        command: AgentCommandV1,
    ) -> Result<AgentReplyV1, Self::Error> {
        assert_eq!(service_session, SessionHandleV1(5));
        let session = AgentSessionIdV1::new([7; 16]);
        match command {
            AgentCommandV1::Inspect { session: actual } => {
                assert_eq!(actual, session);
                Ok(AgentReplyV1::Accepted(AgentOutcomeV1::Session(view(
                    AgentSessionPhaseV1::Ready,
                    8,
                ))))
            }
            AgentCommandV1::Close {
                session: actual,
                expected_revision,
                idempotency_key,
                now,
            } => {
                assert_eq!(actual, session);
                assert_eq!(expected_revision, 8);
                assert_eq!(idempotency_key, IdempotencyKeyV1::new([9; 16]));
                assert_eq!(now, 42);
                self.mutations.fetch_add(1, Ordering::SeqCst);
                if self.unknown {
                    Ok(AgentReplyV1::Accepted(AgentOutcomeV1::OutcomeUnknown {
                        session,
                        idempotency_key,
                    }))
                } else {
                    Ok(AgentReplyV1::Accepted(AgentOutcomeV1::Session(view(
                        AgentSessionPhaseV1::Closed,
                        9,
                    ))))
                }
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }
}

fn view(phase: AgentSessionPhaseV1, revision: u64) -> AgentSessionViewV1 {
    AgentSessionViewV1 {
        session: AgentSessionIdV1::new([7; 16]),
        revision,
        phase,
        profile_document_digest: ContentDigestV1::new([3; 32]),
        active_turn: None,
        last_event_cursor: None,
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

fn component(unknown: bool, mutations: Arc<AtomicUsize>) -> AgentControlComponent {
    AgentControlComponent::new(
        SessionHandleV1(5),
        Port { mutations, unknown },
        AgentControlRequest::Close(CloseRequest {
            session: AgentSessionIdV1::new([7; 16]),
            expected_revision: 8,
            idempotency_key: IdempotencyKeyV1::new([9; 16]),
            now: 42,
        }),
    )
}

fn prompt_request(value: u8) -> AgentControlRequest {
    AgentControlRequest::Prompt(PromptRequest {
        session: AgentSessionIdV1::new([value; 16]),
        turn: AgentTurnIdV1::new([value + 10; 16]),
        expected_revision: 8,
        execution: ExecuteRequestV1 {
            execution_id: ExecutionIdV1::new([value + 20; 16]),
            task_attempt: TaskAttemptRefV1 {
                task_key: [value + 30; 16],
                task_incarnation: 1,
                attempt_number: 1,
            },
            principal: PrincipalIdV1::new([40; 16]),
            profile: HarnessProfileIdV1::new([41; 16]),
            idempotency_key: IdempotencyKeyV1::new([value + 42; 16]),
            context: vec![ContextReferenceIdV1::new([43; 16])],
            tools: vec![ToolCapabilityIdV1::new([44; 16])],
            budget: ExecutionBudgetV1 {
                max_elapsed_millis: 60_000,
                max_input_tokens: 4_096,
                max_output_tokens: 8_192,
                max_tool_calls: 32,
                max_context_bytes: 1_048_576,
                max_output_bytes: 1_048_576,
            },
        },
        prompt: b"inspect every selected workspace".to_vec(),
        now: 42,
    })
}

fn settle(component: &mut AgentControlComponent) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while component.disposition() == AgentControlDisposition::Pending {
        component.poll();
        assert!(Instant::now() < deadline, "Daimon control did not settle");
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn exact_control_requires_arm_and_submits_once() {
    let mutations = Arc::new(AtomicUsize::new(0));
    let mut component = component(false, Arc::clone(&mutations));
    component.update(AgentControlInput::Confirm, true);
    assert_eq!(component.disposition(), AgentControlDisposition::Proposed);
    component.update(AgentControlInput::Arm, true);
    component.update(AgentControlInput::Confirm, true);
    settle(&mut component);
    assert_eq!(component.disposition(), AgentControlDisposition::Applied);
    component.update(AgentControlInput::Confirm, true);
    assert_eq!(mutations.load(Ordering::SeqCst), 1);

    let mut terminal = Terminal::new(TestBackend::new(100, 12)).unwrap();
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
    assert!(rendered.contains("CLOSE DAIMON SESSION"));
    assert!(rendered.contains("Applied"));
}

#[test]
fn unknown_outcome_is_fenced_without_a_second_mutation() {
    let mutations = Arc::new(AtomicUsize::new(0));
    let mut component = component(true, Arc::clone(&mutations));
    component.update(AgentControlInput::Arm, true);
    component.update(AgentControlInput::Confirm, true);
    settle(&mut component);
    assert_eq!(
        component.disposition(),
        AgentControlDisposition::OutcomeUnknown
    );
    assert_eq!(
        component.update(AgentControlInput::Close, true),
        Lifecycle::Running
    );
    component.update(AgentControlInput::Arm, true);
    component.update(AgentControlInput::Confirm, true);
    assert_eq!(mutations.load(Ordering::SeqCst), 1);
    assert!(component.notice().contains("unresolved"));
}

#[test]
fn bounded_batch_retains_receipt_and_stops_after_unknown() {
    let mutations = Arc::new(AtomicUsize::new(0));
    let requests = (7_u8..=9)
        .map(|value| {
            AgentControlRequest::Close(CloseRequest {
                session: AgentSessionIdV1::new([value; 16]),
                expected_revision: 8,
                idempotency_key: IdempotencyKeyV1::new([value + 20; 16]),
                now: 42,
            })
        })
        .collect();
    let connector_mutations = Arc::clone(&mutations);
    let mut component = AgentControlBatchComponent::new_with_connector(
        move || {
            Ok::<_, ()>((
                SessionHandleV1(5),
                BatchPort {
                    mutations: Arc::clone(&connector_mutations),
                },
            ))
        },
        requests,
    )
    .unwrap();
    component.update(AgentControlInput::Arm, true);
    component.update(AgentControlInput::Confirm, true);
    let deadline = Instant::now() + Duration::from_secs(2);
    while component.request_pending() {
        component.poll();
        assert!(Instant::now() < deadline, "Daimon batch did not settle");
        thread::sleep(Duration::from_millis(1));
    }

    assert_eq!(mutations.load(Ordering::SeqCst), 2);
    assert_eq!(
        component.child_dispositions(),
        vec![
            AgentControlChildDisposition::Applied,
            AgentControlChildDisposition::OutcomeUnknown,
            AgentControlChildDisposition::NotSent,
        ]
    );
    assert_eq!(component.applied_views().len(), 1);
    assert_eq!(component.applied_views()[0].revision, 9);
    assert!(component.has_unknown());
    assert!(component.recovery_required());
    assert_eq!(
        component.recovery_sessions(),
        vec![
            AgentSessionIdV1::new([7; 16]),
            AgentSessionIdV1::new([8; 16]),
        ]
    );
    assert_eq!(
        component.update(AgentControlInput::Close, true),
        Lifecycle::Running
    );
    component.recovery_inspection_started();
    assert!(component.notice().contains("no mutation will be retried"));
    component.complete_recovery_inspection(true);
    assert!(!component.recovery_required());
    assert_eq!(
        component.update(AgentControlInput::Close, true),
        Lifecycle::CloseRequested
    );

    let mut terminal = Terminal::new(TestBackend::new(120, 16)).unwrap();
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
    assert!(rendered.contains("Applied"));
    assert!(rendered.contains("OutcomeUnknown"));
    assert!(rendered.contains("NotSent"));
    assert!(rendered.contains("Closed r9"));
}

#[test]
fn bounded_prompt_batch_keeps_content_private_and_stops_after_unknown() {
    let mutations = Arc::new(AtomicUsize::new(0));
    let connector_mutations = Arc::clone(&mutations);
    let mut component = AgentControlBatchComponent::new_with_connector(
        move || {
            Ok::<_, ()>((
                SessionHandleV1(5),
                PromptBatchPort {
                    mutations: Arc::clone(&connector_mutations),
                },
            ))
        },
        (7_u8..=9).map(prompt_request).collect(),
    )
    .unwrap();
    component.update(AgentControlInput::Arm, true);
    component.update(AgentControlInput::Confirm, true);
    let deadline = Instant::now() + Duration::from_secs(2);
    while component.request_pending() {
        component.poll();
        assert!(
            Instant::now() < deadline,
            "Daimon prompt batch did not settle"
        );
        thread::sleep(Duration::from_millis(1));
    }

    assert_eq!(mutations.load(Ordering::SeqCst), 2);
    assert_eq!(
        component.child_dispositions(),
        vec![
            AgentControlChildDisposition::Applied,
            AgentControlChildDisposition::OutcomeUnknown,
            AgentControlChildDisposition::NotSent,
        ]
    );
    assert_eq!(
        component.applied_views()[0].phase,
        AgentSessionPhaseV1::TurnActive
    );

    let mut terminal = Terminal::new(TestBackend::new(140, 16)).unwrap();
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
    assert!(rendered.contains("prompt ready session"));
    assert!(!rendered.contains("inspect every selected workspace"));
}
