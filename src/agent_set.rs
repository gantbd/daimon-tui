//! Bounded local session-set presentation over Daimon's exact-session client.

use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, sync_channel};

use daimon_client::{AgentReadOnlyClient, AgentServicePort};
use daimon_protocol::{AgentSessionIdV1, SessionHandleV1};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    text::Line,
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

use crate::{
    AgentEventFilter, AgentObservation, Lifecycle, agent_lines_with_filter, observe_agent,
    phase_actions,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentSetInput {
    PreviousSession,
    NextSession,
    ToggleSelection,
    EventUp,
    EventDown,
    CycleEventFilter,
    RefreshSelected,
    RefreshAll,
    Close,
}

struct Entry {
    session: AgentSessionIdV1,
    observation: Option<AgentObservation>,
    event: usize,
    selected: bool,
    error: Option<String>,
}

struct SetWorkerReply(Vec<(AgentSessionIdV1, Result<AgentObservation, String>)>);

/// An explicit bounded local workset, not a catalogue or fleet abstraction.
///
/// A refresh uses one connection for all requested sessions and then releases
/// it, leaving the serial local owner available for guarded control.
pub struct AgentSetComponent {
    refresh: SyncSender<Vec<AgentSessionIdV1>>,
    replies: Receiver<SetWorkerReply>,
    entries: Vec<Entry>,
    current: usize,
    event_filter: AgentEventFilter,
    pending: bool,
    notice: String,
}

impl AgentSetComponent {
    pub const MAX_SESSIONS: usize = 16;

    /// Uses an already connected port. Prefer `new_with_connector` in a
    /// long-lived host so a completed refresh releases its connection.
    pub fn new<P>(
        service_session: SessionHandleV1,
        port: P,
        sessions: Vec<AgentSessionIdV1>,
    ) -> Result<Self, &'static str>
    where
        P: AgentServicePort + Send + 'static,
        P::Error: Send + 'static,
    {
        Self::validate(&sessions)?;
        let (request_tx, request_rx) = sync_channel::<Vec<AgentSessionIdV1>>(1);
        let (reply_tx, reply_rx) = sync_channel(1);
        std::thread::spawn(move || {
            let mut client = AgentReadOnlyClient::new(service_session, port);
            while let Ok(requested) = request_rx.recv() {
                let replies = requested
                    .into_iter()
                    .map(|session| (session, observe_agent(&mut client, session)))
                    .collect();
                if reply_tx.send(SetWorkerReply(replies)).is_err() {
                    break;
                }
            }
        });
        Ok(Self::from_channels(request_tx, reply_rx, sessions))
    }

    /// Connects once per bounded refresh and drops that connection before the
    /// result is admitted by the render/update thread.
    pub fn new_with_connector<P, F, E>(
        connector: F,
        sessions: Vec<AgentSessionIdV1>,
    ) -> Result<Self, &'static str>
    where
        P: AgentServicePort + Send + 'static,
        P::Error: Send + 'static,
        F: Fn() -> Result<(SessionHandleV1, P), E> + Send + 'static,
        E: Send + 'static,
    {
        Self::validate(&sessions)?;
        let (request_tx, request_rx) = sync_channel::<Vec<AgentSessionIdV1>>(1);
        let (reply_tx, reply_rx) = sync_channel(1);
        std::thread::spawn(move || {
            while let Ok(requested) = request_rx.recv() {
                let replies = match connector() {
                    Ok((service_session, port)) => {
                        let mut client = AgentReadOnlyClient::new(service_session, port);
                        requested
                            .into_iter()
                            .map(|session| (session, observe_agent(&mut client, session)))
                            .collect()
                    }
                    Err(_) => requested
                        .into_iter()
                        .map(|session| {
                            (
                                session,
                                Err("Daimon session-set connection unavailable".into()),
                            )
                        })
                        .collect(),
                };
                if reply_tx.send(SetWorkerReply(replies)).is_err() {
                    break;
                }
            }
        });
        Ok(Self::from_channels(request_tx, reply_rx, sessions))
    }

    fn validate(sessions: &[AgentSessionIdV1]) -> Result<(), &'static str> {
        if sessions.is_empty() {
            return Err("Daimon session set requires at least one configured session");
        }
        if sessions.len() > Self::MAX_SESSIONS {
            return Err("Daimon session set exceeds the 16-session bound");
        }
        if sessions
            .iter()
            .enumerate()
            .any(|(index, session)| sessions[..index].contains(session))
        {
            return Err("Daimon session set requires unique exact sessions");
        }
        Ok(())
    }

    fn from_channels(
        refresh: SyncSender<Vec<AgentSessionIdV1>>,
        replies: Receiver<SetWorkerReply>,
        sessions: Vec<AgentSessionIdV1>,
    ) -> Self {
        let mut component = Self {
            refresh,
            replies,
            entries: sessions
                .into_iter()
                .map(|session| Entry {
                    session,
                    observation: None,
                    event: 0,
                    selected: false,
                    error: None,
                })
                .collect(),
            current: 0,
            event_filter: AgentEventFilter::All,
            pending: false,
            notice: "Daimon configured session set ready; refresh queued".into(),
        };
        component.offer_all();
        component
    }

    pub fn poll(&mut self) {
        let _ = self.poll_settlement();
    }

    pub fn poll_settlement(&mut self) -> Option<Result<(), ()>> {
        let mut settlement = None;
        loop {
            match self.replies.try_recv() {
                Ok(SetWorkerReply(replies)) => {
                    self.pending = false;
                    let mut accepted = 0;
                    let mut failed = 0;
                    let event_filter = self.event_filter;
                    for (session, result) in replies {
                        let Some(entry) = self
                            .entries
                            .iter_mut()
                            .find(|entry| entry.session == session)
                        else {
                            failed += 1;
                            continue;
                        };
                        match result {
                            Ok(observation) if observation.view.session == session => {
                                let visible = observation
                                    .events
                                    .entries
                                    .iter()
                                    .filter(|record| event_filter.includes(&record.event))
                                    .count();
                                entry.event = entry.event.min(visible.saturating_sub(1));
                                entry.observation = Some(observation);
                                entry.error = None;
                                accepted += 1;
                            }
                            Ok(_) => {
                                entry.error = Some("invalid session reply".into());
                                failed += 1;
                            }
                            Err(error) => {
                                entry.error = Some(error);
                                failed += 1;
                            }
                        }
                    }
                    self.notice = format!(
                        "Daimon session-set refresh settled: {accepted} current, {failed} unavailable"
                    );
                    settlement = Some(if failed == 0 { Ok(()) } else { Err(()) });
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if self.pending {
                        settlement = Some(Err(()));
                    }
                    self.pending = false;
                    self.notice = "Daimon session-set worker unavailable".into();
                    break;
                }
            }
        }
        settlement
    }

    pub fn update(&mut self, input: AgentSetInput, focused: bool) -> Lifecycle {
        if !focused {
            return Lifecycle::Running;
        }
        match input {
            AgentSetInput::Close => return Lifecycle::CloseRequested,
            AgentSetInput::PreviousSession => self.current = self.current.saturating_sub(1),
            AgentSetInput::NextSession => {
                self.current = (self.current + 1).min(self.entries.len() - 1);
            }
            AgentSetInput::ToggleSelection => {
                let entry = &mut self.entries[self.current];
                entry.selected = !entry.selected;
                self.notice = format!(
                    "{} session(s) selected for bounded work",
                    self.selected_count()
                );
            }
            AgentSetInput::EventUp => {
                self.entries[self.current].event =
                    self.entries[self.current].event.saturating_sub(1);
            }
            AgentSetInput::EventDown => {
                let event_filter = self.event_filter;
                let entry = &mut self.entries[self.current];
                let length = entry.observation.as_ref().map_or(0, |value| {
                    value
                        .events
                        .entries
                        .iter()
                        .filter(|record| event_filter.includes(&record.event))
                        .count()
                });
                if length != 0 {
                    entry.event = (entry.event + 1).min(length - 1);
                }
            }
            AgentSetInput::CycleEventFilter => {
                self.event_filter = self.event_filter.next();
                for entry in &mut self.entries {
                    entry.event = 0;
                }
                self.notice = format!(
                    "Showing {} events from each bounded owner window",
                    self.event_filter.label()
                );
            }
            AgentSetInput::RefreshSelected => self.offer(vec![self.entries[self.current].session]),
            AgentSetInput::RefreshAll => self.offer_all(),
        }
        Lifecycle::Running
    }

    pub fn draw_in(&self, frame: &mut Frame<'_>, area: Rect, focused: bool) {
        let [sessions, detail] =
            Layout::horizontal([Constraint::Length(43), Constraint::Min(32)]).areas(area);
        let items = self
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let cursor = if index == self.current { ">" } else { " " };
                let mark = if entry.selected { "x" } else { " " };
                let state = entry.observation.as_ref().map_or_else(
                    || entry.error.as_deref().unwrap_or("loading").to_owned(),
                    |value| format!("{:?} · {}", value.view.phase, phase_actions(&value.view)),
                );
                ListItem::new(format!(
                    "{cursor}[{mark}] session {:>2}  {state}",
                    index + 1
                ))
            })
            .collect::<Vec<_>>();
        let title = if focused {
            " Configured sessions • focused "
        } else {
            " Configured sessions "
        };
        frame.render_widget(
            List::new(items).block(Block::default().borders(Borders::ALL).title(title)),
            sessions,
        );
        let entry = &self.entries[self.current];
        let [body, footer] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(3)]).areas(detail);
        let lines = entry.observation.as_ref().map_or_else(
            || {
                vec![Line::from(
                    entry
                        .error
                        .as_deref()
                        .unwrap_or("No current session observation"),
                )]
            },
            |observation| agent_lines_with_filter(observation, entry.event, self.event_filter),
        );
        frame.render_widget(
            Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Daimon session evidence "),
            ),
            body,
        );
        let (set_summary, evidence_summary) = self.workset_summary();
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(set_summary),
                Line::from(evidence_summary),
                Line::from(format!(
                    "filter {} · f cycle · {}",
                    self.event_filter.label(),
                    self.notice
                )),
            ]),
            footer,
        );
    }

    pub const fn request_pending(&self) -> bool {
        self.pending
    }

    pub fn current_observation(&self) -> Option<&AgentObservation> {
        self.entries[self.current].observation.as_ref()
    }

    /// Sparse selection with one-item fallback keeps one and N on one path.
    pub fn selected_observations(&self) -> Vec<&AgentObservation> {
        let selected = self
            .entries
            .iter()
            .filter(|entry| entry.selected)
            .filter_map(|entry| entry.observation.as_ref())
            .collect::<Vec<_>>();
        if selected.is_empty() {
            self.current_observation().into_iter().collect()
        } else {
            selected
        }
    }

    pub fn selected_count(&self) -> usize {
        let count = self.entries.iter().filter(|entry| entry.selected).count();
        if count == 0 { 1 } else { count }
    }

    pub const fn event_filter(&self) -> AgentEventFilter {
        self.event_filter
    }

    fn workset_summary(&self) -> (String, String) {
        let current = self
            .entries
            .iter()
            .filter(|entry| entry.observation.is_some())
            .count();
        let unavailable = self
            .entries
            .iter()
            .filter(|entry| entry.error.is_some())
            .count();
        let marked = self.entries.iter().filter(|entry| entry.selected).count();
        let mut events = 0_usize;
        let mut visible = 0_usize;
        let mut input_tokens = 0_u64;
        let mut output_tokens = 0_u64;
        for observation in self
            .entries
            .iter()
            .filter_map(|entry| entry.observation.as_ref())
        {
            events = events.saturating_add(observation.events.entries.len());
            for record in &observation.events.entries {
                if self.event_filter.includes(&record.event) {
                    visible = visible.saturating_add(1);
                }
                if let daimon_protocol::agent::AgentEventV1::Usage {
                    input_tokens: input,
                    output_tokens: output,
                } = &record.event
                {
                    input_tokens = input_tokens.saturating_add(*input);
                    output_tokens = output_tokens.saturating_add(*output);
                }
            }
        }
        (
            format!(
                "set {} configured · {current} current · {unavailable} unavailable · {marked} marked (work target {})",
                self.entries.len(),
                self.selected_count()
            ),
            format!(
                "observed windows {visible}/{events} events visible · usage input={input_tokens} output={output_tokens}"
            ),
        )
    }

    /// Queues one bounded observation pass for an exact recovery subset.
    /// Returns false without changing state when the request is invalid or a
    /// refresh is already pending.
    pub fn refresh_sessions(&mut self, sessions: &[AgentSessionIdV1]) -> bool {
        if sessions.is_empty()
            || sessions.len() > Self::MAX_SESSIONS
            || sessions
                .iter()
                .enumerate()
                .any(|(index, session)| sessions[..index].contains(session))
            || sessions
                .iter()
                .any(|session| !self.entries.iter().any(|entry| entry.session == *session))
            || self.pending
        {
            return false;
        }
        self.offer(sessions.to_vec());
        self.pending
    }

    fn offer_all(&mut self) {
        self.offer(self.entries.iter().map(|entry| entry.session).collect());
    }

    fn offer(&mut self, sessions: Vec<AgentSessionIdV1>) {
        if self.pending {
            self.notice = "Daimon session-set refresh already pending".into();
        } else if self.refresh.try_send(sessions).is_ok() {
            self.pending = true;
            self.notice = "Daimon bounded session-set refresh pending".into();
        } else {
            self.notice = "Daimon session-set worker unavailable".into();
        }
    }
}
