//! Composable, read-only Ratatui presentation over Daimon's public client.
#![forbid(unsafe_code)]

mod agent_set;
mod control;
mod permission_composer;
mod prompt_composer;
pub use agent_set::{AgentSetComponent, AgentSetInput};
pub use control::{
    AgentControlBatchComponent, AgentControlChildDisposition, AgentControlComponent,
    AgentControlDisposition, AgentControlInput, AgentControlRequest, AgentPermissionControlRequest,
};
pub use permission_composer::{
    AgentPermissionChallenge, AgentPermissionComposer, AgentPermissionInput,
    AgentPermissionOutcome, AgentPermissionSelection,
};
pub use prompt_composer::{AgentPromptComposer, AgentPromptInput, AgentPromptOutcome};

use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, sync_channel};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use daimon_client::{AgentReadOnlyClient, AgentServicePort, ReadOnlyClient, ServicePort};
use daimon_protocol::{
    AgentSessionIdV1, AgentTurnIdV1, IdempotencyKeyV1, OutcomeV1, PresenceEntryV1, ReplyV1,
    SessionHandleV1,
    agent::{
        AgentEventPageV1, AgentEventV1, AgentOutcomeV1, AgentReplyV1, AgentSessionPhaseV1,
        AgentSessionViewV1,
    },
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    text::Line,
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

const PRESENCE_LIMIT: u8 = 64;
const AGENT_EVENT_LIMIT: u64 = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapabilityManifest {
    pub schema: u8,
    pub peer: &'static str,
    pub interactive: &'static [&'static str],
    pub headless: &'static [&'static str],
}

pub const CAPABILITIES: CapabilityManifest = CapabilityManifest {
    schema: 1,
    peer: "daimon",
    interactive: &[
        "agents.presence.v1",
        "agents.session.inspect.v1",
        "agents.session-set.inspect.v1",
        "agents.session-set.control.v1",
        "agents.session-set.prompt.v1",
        "agents.session-set.permission.v1",
        "agents.session-set.reattach.v1",
    ],
    headless: &["agents.presence.v1", "agents.session.inspect.v1"],
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Input {
    Up,
    Down,
    Refresh,
    Close,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentObservation {
    pub view: AgentSessionViewV1,
    pub events: AgentEventPageV1,
}

/// Operator-selected projection over the owner-published event window.
///
/// Filters never affect observation, authority, or retention. They only reduce
/// the already-bounded event page shown in the terminal.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AgentEventFilter {
    #[default]
    All,
    Conversation,
    Actions,
    Decisions,
    Usage,
}

impl AgentEventFilter {
    pub const fn next(self) -> Self {
        match self {
            Self::All => Self::Conversation,
            Self::Conversation => Self::Actions,
            Self::Actions => Self::Decisions,
            Self::Decisions => Self::Usage,
            Self::Usage => Self::All,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Conversation => "conversation",
            Self::Actions => "actions",
            Self::Decisions => "decisions",
            Self::Usage => "usage",
        }
    }

    pub const fn includes(self, event: &AgentEventV1) -> bool {
        match self {
            Self::All => true,
            Self::Conversation => matches!(
                event,
                AgentEventV1::AssistantMessage { .. }
                    | AgentEventV1::Thought { .. }
                    | AgentEventV1::Plan { .. }
            ),
            Self::Actions => matches!(
                event,
                AgentEventV1::ToolCall { .. }
                    | AgentEventV1::Terminal { .. }
                    | AgentEventV1::FileChange { .. }
                    | AgentEventV1::FileRead { .. }
            ),
            Self::Decisions => matches!(
                event,
                AgentEventV1::Permission(_) | AgentEventV1::TurnFinished { .. }
            ),
            Self::Usage => matches!(event, AgentEventV1::Usage { .. }),
        }
    }
}

struct AgentWorkerReply(Result<AgentObservation, String>);

/// Read-only presentation for one explicitly configured agent session.
///
/// The exact opaque session identifier remains deployment input. The component
/// exposes no session catalogue and owns no open, prompt, permission, advance,
/// cancel, close, provider or reattach authority.
pub struct AgentComponent {
    session: AgentSessionIdV1,
    refresh: SyncSender<()>,
    replies: Receiver<AgentWorkerReply>,
    pending: bool,
    observation: Option<AgentObservation>,
    selected: usize,
    notice: String,
}

impl AgentComponent {
    pub fn new<P>(service_session: SessionHandleV1, port: P, session: AgentSessionIdV1) -> Self
    where
        P: AgentServicePort + Send + 'static,
        P::Error: Send + 'static,
    {
        let (request_tx, request_rx) = sync_channel::<()>(1);
        let (reply_tx, reply_rx) = sync_channel::<AgentWorkerReply>(1);
        std::thread::spawn(move || {
            let mut client = AgentReadOnlyClient::new(service_session, port);
            while request_rx.recv().is_ok() {
                let reply = observe_agent(&mut client, session);
                if reply_tx.send(AgentWorkerReply(reply)).is_err() {
                    break;
                }
            }
        });
        let mut component = Self {
            session,
            refresh: request_tx,
            replies: reply_rx,
            pending: false,
            observation: None,
            selected: 0,
            notice: "Daimon agent session ready; refresh queued".into(),
        };
        component.offer();
        component
    }

    pub fn poll(&mut self) {
        let _ = self.poll_settlement();
    }

    pub fn poll_settlement(&mut self) -> Option<Result<(), ()>> {
        let mut settlement = None;
        loop {
            match self.replies.try_recv() {
                Ok(reply) => {
                    settlement = Some(if self.admit(reply) { Ok(()) } else { Err(()) });
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if self.pending {
                        settlement = Some(Err(()));
                    }
                    self.pending = false;
                    self.notice = "Daimon agent worker unavailable".into();
                    break;
                }
            }
        }
        settlement
    }

    pub const fn request_pending(&self) -> bool {
        self.pending
    }

    pub fn update(&mut self, input: Input, focused: bool) -> Lifecycle {
        if !focused {
            return Lifecycle::Running;
        }
        match input {
            Input::Close => return Lifecycle::CloseRequested,
            Input::Refresh => self.offer(),
            Input::Up => self.selected = self.selected.saturating_sub(1),
            Input::Down => {
                let length = self
                    .observation
                    .as_ref()
                    .map_or(0, |observation| observation.events.entries.len());
                if length != 0 {
                    self.selected = (self.selected + 1).min(length - 1);
                }
            }
        }
        Lifecycle::Running
    }

    pub fn draw_in(&self, frame: &mut Frame<'_>, area: Rect, focused: bool) {
        let [body, footer] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
        let title = if focused {
            " Daimon agent • focused "
        } else {
            " Daimon agent "
        };
        let lines = self.observation.as_ref().map_or_else(
            || vec![Line::from("No current agent-session observation")],
            |observation| agent_lines(observation, self.selected),
        );
        frame.render_widget(
            Paragraph::new(lines).block(Block::default().title(title).borders(Borders::ALL)),
            body,
        );
        frame.render_widget(Paragraph::new(self.notice.as_str()), footer);
    }

    pub fn observation(&self) -> Option<&AgentObservation> {
        self.observation.as_ref()
    }

    fn offer(&mut self) {
        if self.pending {
            self.notice = "Daimon agent refresh already pending".into();
        } else if self.refresh.try_send(()).is_ok() {
            self.pending = true;
            self.notice = "Daimon agent refresh pending".into();
        } else {
            self.notice = "Daimon agent worker unavailable".into();
        }
    }

    fn admit(&mut self, AgentWorkerReply(reply): AgentWorkerReply) -> bool {
        self.pending = false;
        match reply {
            Ok(observation) if observation.view.session == self.session => {
                self.selected = self
                    .selected
                    .min(observation.events.entries.len().saturating_sub(1));
                self.notice = if observation.events.next_after.is_some() {
                    "Bounded agent event page; more events available".into()
                } else {
                    format!("{} agent event(s)", observation.events.entries.len())
                };
                self.observation = Some(observation);
                true
            }
            Ok(_) => {
                self.notice = "Invalid Daimon agent session reply".into();
                false
            }
            Err(error) => {
                self.notice = error;
                false
            }
        }
    }
}

pub fn observe_agent<P: AgentServicePort>(
    client: &mut AgentReadOnlyClient<P>,
    session: AgentSessionIdV1,
) -> Result<AgentObservation, String> {
    let before = inspect_agent(client, session)?;
    let after = before
        .last_event_cursor
        .filter(|cursor| *cursor >= AGENT_EVENT_LIMIT)
        .map(|cursor| cursor - AGENT_EVENT_LIMIT);
    let events = match client
        .events(session, after, AGENT_EVENT_LIMIT)
        .map_err(|_| "Daimon agent events unavailable; outcome not inferred")?
    {
        AgentReplyV1::Accepted(AgentOutcomeV1::Events(page)) if page.session == session => page,
        AgentReplyV1::Accepted(_) => return Err("Invalid Daimon agent events reply".into()),
        AgentReplyV1::Refused(refusal) => {
            return Err(format!("Daimon agent events refused: {refusal:?}"));
        }
    };
    let after = inspect_agent(client, session)?;
    if before != after {
        return Err("Daimon agent session changed during observation".into());
    }
    if events
        .entries
        .windows(2)
        .any(|pair| pair[1].cursor != pair[0].cursor.saturating_add(1))
    {
        return Err("Daimon agent event page is noncontiguous".into());
    }
    if events
        .entries
        .last()
        .is_some_and(|event| Some(event.cursor) > before.last_event_cursor)
    {
        return Err("Daimon agent event cursor exceeds stable session view".into());
    }
    Ok(AgentObservation {
        view: before,
        events,
    })
}

fn inspect_agent<P: AgentServicePort>(
    client: &mut AgentReadOnlyClient<P>,
    session: AgentSessionIdV1,
) -> Result<AgentSessionViewV1, String> {
    match client
        .inspect(session)
        .map_err(|_| "Daimon agent session unavailable; outcome not inferred")?
    {
        AgentReplyV1::Accepted(AgentOutcomeV1::Session(view)) if view.session == session => {
            Ok(view)
        }
        AgentReplyV1::Accepted(_) => Err("Invalid Daimon agent session reply".into()),
        AgentReplyV1::Refused(refusal) => Err(format!("Daimon agent session refused: {refusal:?}")),
    }
}

pub(crate) fn agent_lines(observation: &AgentObservation, selected: usize) -> Vec<Line<'static>> {
    agent_lines_with_filter(observation, selected, AgentEventFilter::All)
}

pub(crate) fn agent_lines_with_filter(
    observation: &AgentObservation,
    selected: usize,
    filter: AgentEventFilter,
) -> Vec<Line<'static>> {
    let view = &observation.view;
    let capabilities = view.capabilities;
    let visible = observation
        .events
        .entries
        .iter()
        .filter(|record| filter.includes(&record.event))
        .count();
    let mut lines = vec![
        Line::from(format!(
            "phase/revision: {:?}/{}",
            view.phase, view.revision
        )),
        Line::from(format!(
            "owner-state candidates (authority still required): {}",
            phase_actions(view)
        )),
        Line::from(format!("active turn: {:?}", view.active_turn)),
        Line::from(format!("last event cursor: {:?}", view.last_event_cursor)),
        Line::from(format!(
            "capabilities: load={} resume={} permissions={} fs-read={} fs-write={} terminal={} modes={} models={}",
            capabilities.load_session,
            capabilities.resume_session,
            capabilities.permissions,
            capabilities.filesystem_read,
            capabilities.filesystem_write,
            capabilities.terminal,
            capabilities.session_modes,
            capabilities.session_models,
        )),
        Line::from(format!(
            "profile document digest: {}",
            hex(&view.profile_document_digest.into_bytes())
        )),
        Line::from(view.last_context.as_ref().map_or_else(
            || "last context: not yet admitted".into(),
            |receipt| {
                format!(
                    "last context: execution {} · manifest {} · {} item(s)",
                    hex(&receipt.execution.into_bytes()),
                    hex(&receipt.manifest_digest.into_bytes()),
                    receipt.entries.len()
                )
            },
        )),
        Line::from(format!(
            "events: {} visible / {} observed{} · filter {}",
            visible,
            observation.events.entries.len(),
            if observation.events.next_after.is_some() {
                "+"
            } else {
                ""
            },
            filter.label(),
        )),
    ];
    if let Some(receipt) = &view.last_context {
        lines.extend(receipt.entries.iter().map(|entry| {
            Line::from(format!(
                "  context {} · {:?} · {} · {} bytes{}",
                hex(&entry.reference.into_bytes()),
                entry.kind,
                hex(&entry.source_digest.into_bytes()),
                entry.content_bytes,
                if entry.grounding.is_some() {
                    " · grounded"
                } else {
                    ""
                }
            ))
        }));
    }
    for (index, record) in observation
        .events
        .entries
        .iter()
        .filter(|record| filter.includes(&record.event))
        .enumerate()
    {
        let marker = if index == selected { ">" } else { " " };
        lines.push(Line::from(format!(
            "{marker} {}  {}",
            record.cursor,
            event_summary(&record.event)
        )));
    }
    lines
}

pub(crate) fn phase_actions(view: &AgentSessionViewV1) -> &'static str {
    match view.phase {
        AgentSessionPhaseV1::Opening => "wait for readiness",
        AgentSessionPhaseV1::Ready if view.active_turn.is_none() => "prompt · close",
        AgentSessionPhaseV1::Ready => "inspect inconsistent active-turn evidence",
        AgentSessionPhaseV1::ReattachRequired if view.capabilities.resume_session => "reattach",
        AgentSessionPhaseV1::ReattachRequired => "inspect; resume unsupported",
        AgentSessionPhaseV1::TurnActive if view.active_turn.is_some() => "advance · cancel",
        AgentSessionPhaseV1::TurnActive => "inspect missing active-turn evidence",
        AgentSessionPhaseV1::AwaitingPermission
            if view.active_turn.is_some() && view.capabilities.permissions =>
        {
            "decide permission · cancel"
        }
        AgentSessionPhaseV1::AwaitingPermission => "inspect permission evidence",
        AgentSessionPhaseV1::OutcomeUnknown => "inspect; never retry implicitly",
        AgentSessionPhaseV1::Closed => "none; session closed",
    }
}

fn event_summary(event: &AgentEventV1) -> String {
    match event {
        AgentEventV1::AssistantMessage { bytes } => format!("assistant {}", escaped(bytes)),
        AgentEventV1::Thought { bytes } => format!("thought {}", escaped(bytes)),
        AgentEventV1::Plan { bytes } => format!("plan {}", escaped(bytes)),
        AgentEventV1::ToolCall { title, detail } => {
            format!("tool {} — {}", escaped(title), escaped(detail))
        }
        AgentEventV1::Terminal { stream, bytes } => {
            format!("terminal {} {}", escaped(stream), escaped(bytes))
        }
        AgentEventV1::Permission(request) => format!(
            "permission turn={:?} capability={} options={}",
            request.turn,
            escaped(&request.capability),
            request.options.len()
        ),
        AgentEventV1::FileChange {
            path,
            content_digest,
        } => format!(
            "file-change {} digest={}",
            escaped(path),
            hex(&content_digest.into_bytes())
        ),
        AgentEventV1::FileRead {
            path,
            content_digest,
            content_bytes,
        } => format!(
            "file-read {} bytes={} digest={}",
            escaped(path),
            content_bytes,
            hex(&content_digest.into_bytes())
        ),
        AgentEventV1::Usage {
            input_tokens,
            output_tokens,
        } => format!("usage input={input_tokens} output={output_tokens}"),
        AgentEventV1::TurnFinished {
            turn,
            disposition,
            stop_reason,
            output_digest,
        } => format!(
            "turn-finished {:?} {:?}/{:?} digest={}",
            turn,
            disposition,
            stop_reason,
            output_digest.map_or_else(|| "none".into(), |digest| hex(&digest.into_bytes()))
        ),
    }
}

pub(crate) fn escaped(bytes: &[u8]) -> String {
    const LIMIT: usize = 96;
    let mut value = String::new();
    for byte in bytes.iter().take(LIMIT) {
        for character in std::ascii::escape_default(*byte) {
            value.push(char::from(character));
        }
    }
    if bytes.len() > LIMIT {
        value.push('…');
    }
    value
}

fn hex(bytes: &[u8]) -> String {
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut value, "{byte:02x}").expect("writing to String cannot fail");
    }
    value
}

pub fn parse_agent_session(value: &str) -> Option<AgentSessionIdV1> {
    parse_opaque_id(value).map(AgentSessionIdV1::new)
}

pub fn parse_agent_turn(value: &str) -> Option<AgentTurnIdV1> {
    parse_opaque_id(value).map(AgentTurnIdV1::new)
}

pub fn parse_idempotency_key(value: &str) -> Option<IdempotencyKeyV1> {
    parse_opaque_id(value).map(IdempotencyKeyV1::new)
}

fn parse_opaque_id(value: &str) -> Option<[u8; 16]> {
    if value.len() != 32 {
        return None;
    }
    let mut bytes = [0_u8; 16];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(bytes)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Lifecycle {
    Running,
    CloseRequested,
}

pub fn translate_key(key: KeyEvent) -> Option<Input> {
    if key.kind != KeyEventKind::Press || key.modifiers != KeyModifiers::NONE {
        return None;
    }
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => Some(Input::Up),
        KeyCode::Down | KeyCode::Char('j') => Some(Input::Down),
        KeyCode::Char('r') => Some(Input::Refresh),
        KeyCode::Char('q') => Some(Input::Close),
        _ => None,
    }
}

struct WorkerReply(Result<ReplyV1, ()>);

pub struct Component {
    refresh: SyncSender<()>,
    replies: Receiver<WorkerReply>,
    pending: bool,
    entries: Vec<PresenceEntryV1>,
    selected: usize,
    truncated: bool,
    notice: String,
}

impl Component {
    pub fn new<P>(session: SessionHandleV1, port: P) -> Self
    where
        P: ServicePort + Send + 'static,
        P::Error: Send + 'static,
    {
        let (request_tx, request_rx) = sync_channel::<()>(1);
        let (reply_tx, reply_rx) = sync_channel::<WorkerReply>(1);
        std::thread::spawn(move || {
            let mut client = ReadOnlyClient::new(session, port);
            while request_rx.recv().is_ok() {
                let reply = client.read_presence(PRESENCE_LIMIT).map_err(|_| ());
                if reply_tx.send(WorkerReply(reply)).is_err() {
                    break;
                }
            }
        });
        let mut component = Self {
            refresh: request_tx,
            replies: reply_rx,
            pending: false,
            entries: Vec::new(),
            selected: 0,
            truncated: false,
            notice: "Daimon ready; refresh queued".into(),
        };
        component.offer();
        component
    }

    pub fn poll(&mut self) {
        let _ = self.poll_settlement();
    }

    pub fn poll_settlement(&mut self) -> Option<Result<(), ()>> {
        let mut settlement = None;
        loop {
            match self.replies.try_recv() {
                Ok(reply) => {
                    settlement = Some(if self.admit(reply) { Ok(()) } else { Err(()) });
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if self.pending {
                        settlement = Some(Err(()));
                    }
                    self.pending = false;
                    self.notice = "Daimon worker unavailable".into();
                    break;
                }
            }
        }
        settlement
    }

    pub const fn request_pending(&self) -> bool {
        self.pending
    }

    pub fn update(&mut self, input: Input, focused: bool) -> Lifecycle {
        if !focused {
            return Lifecycle::Running;
        }
        match input {
            Input::Close => return Lifecycle::CloseRequested,
            Input::Refresh => self.offer(),
            Input::Up => self.selected = self.selected.saturating_sub(1),
            Input::Down if !self.entries.is_empty() => {
                self.selected = (self.selected + 1).min(self.entries.len() - 1);
            }
            Input::Down => {}
        }
        Lifecycle::Running
    }

    pub fn draw_in(&self, frame: &mut Frame<'_>, area: Rect, focused: bool) {
        let [body, footer] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
        let title = if focused {
            " Daimon • focused "
        } else {
            " Daimon "
        };
        let items = self
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let marker = if index == self.selected { ">" } else { " " };
                ListItem::new(format!(
                    "{marker} agent {}  age={}ms",
                    if entry.stale { "stale" } else { "current" },
                    entry.age_millis
                ))
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            List::new(items).block(Block::default().title(title).borders(Borders::ALL)),
            body,
        );
        frame.render_widget(Paragraph::new(self.notice.as_str()), footer);
    }

    fn offer(&mut self) {
        if self.pending {
            self.notice = "Daimon refresh already pending".into();
        } else if self.refresh.try_send(()).is_ok() {
            self.pending = true;
            self.notice = "Daimon refresh pending".into();
        } else {
            self.notice = "Daimon worker unavailable".into();
        }
    }

    fn admit(&mut self, WorkerReply(reply): WorkerReply) -> bool {
        self.pending = false;
        match reply {
            Ok(ReplyV1::Accepted(OutcomeV1::Presence { entries, truncated })) => {
                self.entries = entries;
                self.selected = self.selected.min(self.entries.len().saturating_sub(1));
                self.truncated = truncated;
                self.notice = if truncated {
                    "Bounded presence page; more agents exist".into()
                } else {
                    format!("{} agent(s)", self.entries.len())
                };
                true
            }
            Ok(ReplyV1::Refused(refusal)) => {
                self.notice = format!("Daimon refused: {refusal:?}");
                false
            }
            Ok(ReplyV1::Accepted(_)) => {
                self.notice = "Invalid Daimon reply".into();
                false
            }
            Err(()) => {
                self.notice = "Daimon unavailable; outcome not inferred".into();
                false
            }
        }
    }
}
