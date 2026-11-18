use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use daimon_client::{AgentServicePort, ServicePort};
use daimon_protocol::{
    AgentSessionIdV1, CommandV1, ContentDigestV1, ContextKindV1, ContextReferenceIdV1,
    ExecutionIdV1, OutcomeV1, PresenceEntryV1, ReplyV1, SessionHandleV1,
    agent::{
        AgentCapabilitiesV1, AgentCommandV1, AgentContextManifestEntryV1, AgentContextReceiptV1,
        AgentEventPageV1, AgentEventRecordV1, AgentEventV1, AgentOutcomeV1, AgentReplyV1,
        AgentSessionPhaseV1, AgentSessionViewV1,
    },
};
use ratatui::{Terminal, backend::TestBackend};

struct Port;
struct AgentPort(u8);
struct SetPort;
struct TailPort;
struct CountingPort {
    active: Arc<AtomicUsize>,
}

impl Drop for CountingPort {
    fn drop(&mut self) {
        self.active.fetch_sub(1, Ordering::SeqCst);
    }
}

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
        let session = AgentSessionIdV1::new([self.0; 16]);
        let outcome = match command {
            AgentCommandV1::Inspect { session: actual } if actual == session => {
                AgentOutcomeV1::Session(AgentSessionViewV1 {
                    session,
                    revision: 8,
                    phase: AgentSessionPhaseV1::ReattachRequired,
                    profile_document_digest: ContentDigestV1::new([3; 32]),
                    active_turn: None,
                    last_event_cursor: Some(2),
                    last_context: Some(AgentContextReceiptV1 {
                        execution: ExecutionIdV1::new([4; 16]),
                        manifest_digest: ContentDigestV1::new([5; 32]),
                        entries: vec![AgentContextManifestEntryV1 {
                            reference: ContextReferenceIdV1::new([6; 16]),
                            kind: ContextKindV1::VellumDocument,
                            source_digest: ContentDigestV1::new([8; 32]),
                            content_bytes: 21,
                            grounding: None,
                        }],
                    }),
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

impl AgentServicePort for SetPort {
    type Error = ();

    fn execute_agent(
        &mut self,
        service_session: SessionHandleV1,
        command: AgentCommandV1,
    ) -> Result<AgentReplyV1, Self::Error> {
        assert_eq!(service_session, SessionHandleV1(5));
        let outcome = match command {
            AgentCommandV1::Inspect { session } => AgentOutcomeV1::Session(AgentSessionViewV1 {
                session,
                revision: 8,
                phase: AgentSessionPhaseV1::Ready,
                profile_document_digest: ContentDigestV1::new([3; 32]),
                active_turn: None,
                last_event_cursor: Some(1),
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
            }),
            AgentCommandV1::Events {
                session,
                after: None,
                limit: 64,
            } => AgentOutcomeV1::Events(AgentEventPageV1 {
                session,
                entries: vec![AgentEventRecordV1 {
                    cursor: 1,
                    event: AgentEventV1::Usage {
                        input_tokens: 5,
                        output_tokens: 7,
                    },
                }],
                next_after: None,
            }),
            _ => panic!("unexpected session-set observation command"),
        };
        Ok(AgentReplyV1::Accepted(outcome))
    }
}

impl AgentServicePort for TailPort {
    type Error = ();

    fn execute_agent(
        &mut self,
        _: SessionHandleV1,
        command: AgentCommandV1,
    ) -> Result<AgentReplyV1, Self::Error> {
        let session = AgentSessionIdV1::new([6; 16]);
        let outcome = match command {
            AgentCommandV1::Inspect { session: actual } if actual == session => {
                AgentOutcomeV1::Session(AgentSessionViewV1 {
                    session,
                    revision: 12,
                    phase: AgentSessionPhaseV1::AwaitingPermission,
                    profile_document_digest: ContentDigestV1::new([3; 32]),
                    active_turn: None,
                    last_event_cursor: Some(100),
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
                })
            }
            AgentCommandV1::Events {
                session: actual,
                after: Some(36),
                limit: 64,
            } if actual == session => AgentOutcomeV1::Events(AgentEventPageV1 {
                session,
                entries: vec![AgentEventRecordV1 {
                    cursor: 100,
                    event: AgentEventV1::Usage {
                        input_tokens: 1,
                        output_tokens: 2,
                    },
                }],
                next_after: None,
            }),
            other => panic!("unexpected tail observation command: {other:?}"),
        };
        Ok(AgentReplyV1::Accepted(outcome))
    }
}

impl AgentServicePort for CountingPort {
    type Error = ();

    fn execute_agent(
        &mut self,
        service_session: SessionHandleV1,
        command: AgentCommandV1,
    ) -> Result<AgentReplyV1, Self::Error> {
        SetPort.execute_agent(service_session, command)
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
fn exact_session_observation_reads_the_recent_bounded_event_tail() {
    let session = AgentSessionIdV1::new([6; 16]);
    let mut client = daimon_client::AgentReadOnlyClient::new(SessionHandleV1(5), TailPort);
    let observation = daimon_tui::observe_agent(&mut client, session).unwrap();
    assert_eq!(observation.events.entries[0].cursor, 100);
    assert_eq!(observation.view.last_event_cursor, Some(100));
}

#[test]
fn agent_component_renders_exact_configured_session_evidence() {
    let session = AgentSessionIdV1::new([7; 16]);
    let mut component = daimon_tui::AgentComponent::new(SessionHandleV1(5), AgentPort(7), session);
    for _ in 0..100 {
        component.poll();
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
        if rendered.contains("ReattachRequired/8") {
            assert!(rendered.contains("load=true resume=true permissions=true"));
            assert!(
                rendered.contains("owner-state candidates (authority still required): reattach")
            );
            assert!(rendered.contains("assistant hello\\nworld"));
            assert!(rendered.contains("usage input=11 output=13"));
            assert!(rendered.contains("last context: execution 04040404"));
            assert!(rendered.contains("VellumDocument"));
            assert!(rendered.contains("21 bytes"));
            assert!(!rendered.contains("070707"));
            return;
        }
        thread::sleep(Duration::from_millis(1));
    }
    panic!("Daimon agent session did not render");
}

#[test]
fn configured_session_set_supports_sparse_n_selection() {
    let configured = (1_u8..=3)
        .map(|value| AgentSessionIdV1::new([value; 16]))
        .collect();
    let mut component =
        daimon_tui::AgentSetComponent::new(SessionHandleV1(5), SetPort, configured).unwrap();
    for _ in 0..100 {
        component.poll();
        if component.current_observation().is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    component.update(daimon_tui::AgentSetInput::ToggleSelection, true);
    component.update(daimon_tui::AgentSetInput::NextSession, true);
    component.update(daimon_tui::AgentSetInput::NextSession, true);
    component.update(daimon_tui::AgentSetInput::ToggleSelection, true);
    assert_eq!(component.selected_count(), 2);
    for _ in 0..100 {
        component.poll();
        if component.selected_observations().len() == 2 {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    let selected = component.selected_observations();
    assert_eq!(selected.len(), 2);
    assert_eq!(selected[0].view.session, AgentSessionIdV1::new([1; 16]));
    assert_eq!(selected[1].view.session, AgentSessionIdV1::new([3; 16]));
    assert_eq!(
        component
            .observation(AgentSessionIdV1::new([2; 16]))
            .unwrap()
            .view
            .session,
        AgentSessionIdV1::new([2; 16])
    );
    assert!(
        component
            .observation(AgentSessionIdV1::new([9; 16]))
            .is_none()
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
    assert!(rendered.contains("[x] session  1"));
    assert!(rendered.contains(">[x] session  3"));
    assert!(rendered.contains("Ready · prompt · close"));
    assert!(rendered.contains("set 3 configured · 3 current · 0 unavailable · 2 marked"));
    assert!(rendered.contains("observed windows 3/3 events visible · usage input=15 output=21"));
    assert!(!rendered.contains("010101"));

    for _ in 0..4 {
        component.update(daimon_tui::AgentSetInput::CycleEventFilter, true);
    }
    assert_eq!(
        component.event_filter(),
        daimon_tui::AgentEventFilter::Usage
    );
    terminal
        .draw(|frame| component.draw_in(frame, frame.area(), true))
        .unwrap();
    let filtered = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(filtered.contains("filter usage"));
    assert!(filtered.contains("events: 1 visible / 1 observed"));
}

#[test]
fn owner_created_session_can_enter_the_bounded_presentation_workset() {
    let first = AgentSessionIdV1::new([1; 16]);
    let opened = AgentSessionIdV1::new([4; 16]);
    let mut component =
        daimon_tui::AgentSetComponent::new(SessionHandleV1(5), SetPort, vec![first]).unwrap();
    for _ in 0..100 {
        component.poll();
        if !component.request_pending() {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    component.admit_session(opened).unwrap();
    assert!(component.request_pending());
    assert_eq!(component.selected_count(), 1);
    for _ in 0..100 {
        component.poll();
        if component
            .current_observation()
            .is_some_and(|observation| observation.view.session == opened)
        {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        component.current_observation().unwrap().view.session,
        opened
    );
    assert!(component.admit_session(opened).is_err());
}

#[test]
fn empty_attached_workset_stays_dormant_until_owner_session_admission() {
    let connections = Arc::new(AtomicUsize::new(0));
    let observed_connections = Arc::clone(&connections);
    let mut component = daimon_tui::AgentSetComponent::new_empty_with_connector(move || {
        observed_connections.fetch_add(1, Ordering::SeqCst);
        Ok::<_, ()>((SessionHandleV1(5), SetPort))
    });
    assert!(!component.request_pending());
    assert_eq!(connections.load(Ordering::SeqCst), 0);
    assert!(component.current_observation().is_none());

    let opened = AgentSessionIdV1::new([4; 16]);
    component.admit_session(opened).unwrap();
    for _ in 0..100 {
        component.poll();
        if !component.request_pending() {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(connections.load(Ordering::SeqCst), 1);
    assert_eq!(
        component.current_observation().unwrap().view.session,
        opened
    );
}

#[test]
fn sixteen_session_filter_and_render_path_meets_local_budget() {
    let sessions = (1_u8..=16)
        .map(|value| AgentSessionIdV1::new([value; 16]))
        .collect();
    let mut component =
        daimon_tui::AgentSetComponent::new(SessionHandleV1(5), SetPort, sessions).unwrap();
    for _ in 0..100 {
        component.poll();
        if component.current_observation().is_some() && !component.request_pending() {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    assert!(!component.request_pending());
    let mut terminal = Terminal::new(TestBackend::new(180, 40)).unwrap();
    let started = Instant::now();
    for _ in 0..250 {
        component.update(daimon_tui::AgentSetInput::CycleEventFilter, true);
        terminal
            .draw(|frame| component.draw_in(frame, frame.area(), true))
            .unwrap();
    }
    let elapsed = started.elapsed();
    assert!(
        elapsed <= Duration::from_secs(2),
        "250 sixteen-session filter/render frames took {elapsed:?}"
    );
}

#[test]
fn session_set_refresh_is_o_one_connections_and_releases_between_work() {
    let active = Arc::new(AtomicUsize::new(0));
    let maximum = Arc::new(AtomicUsize::new(0));
    let connections = Arc::new(AtomicUsize::new(0));
    let connector_active = Arc::clone(&active);
    let connector_maximum = Arc::clone(&maximum);
    let connector_connections = Arc::clone(&connections);
    let sessions = (1_u8..=16)
        .map(|value| AgentSessionIdV1::new([value; 16]))
        .collect();
    let mut component = daimon_tui::AgentSetComponent::new_with_connector(
        move || {
            connector_connections.fetch_add(1, Ordering::SeqCst);
            let current = connector_active.fetch_add(1, Ordering::SeqCst) + 1;
            connector_maximum.fetch_max(current, Ordering::SeqCst);
            Ok::<_, ()>((
                SessionHandleV1(5),
                CountingPort {
                    active: Arc::clone(&connector_active),
                },
            ))
        },
        sessions,
    )
    .unwrap();
    for expected in 1..=2 {
        for _ in 0..100 {
            component.poll();
            if !component.request_pending() {
                break;
            }
            thread::sleep(Duration::from_millis(1));
        }
        assert!(!component.request_pending());
        assert_eq!(connections.load(Ordering::SeqCst), expected);
        assert_eq!(active.load(Ordering::SeqCst), 0);
        if expected == 1 {
            assert!(component.refresh_sessions(&[AgentSessionIdV1::new([8; 16])]));
            assert!(!component.refresh_sessions(&[AgentSessionIdV1::new([8; 16])]));
        }
    }
    assert_eq!(maximum.load(Ordering::SeqCst), 1);
    assert_eq!(component.selected_observations().len(), 1);
}
