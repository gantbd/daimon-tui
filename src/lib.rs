//! Composable, read-only Ratatui presentation over Daimon's public client.
#![forbid(unsafe_code)]

use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, sync_channel};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use daimon_client::{ReadOnlyClient, ServicePort};
use daimon_protocol::{OutcomeV1, PresenceEntryV1, ReplyV1, SessionHandleV1};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

const PRESENCE_LIMIT: u8 = 64;

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
    interactive: &["agents.presence.v1"],
    headless: &["agents.presence.v1"],
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Input {
    Up,
    Down,
    Refresh,
    Close,
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
        loop {
            match self.replies.try_recv() {
                Ok(reply) => self.admit(reply),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.pending = false;
                    self.notice = "Daimon worker unavailable".into();
                    break;
                }
            }
        }
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

    fn admit(&mut self, WorkerReply(reply): WorkerReply) {
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
            }
            Ok(ReplyV1::Refused(refusal)) => {
                self.notice = format!("Daimon refused: {refusal:?}");
            }
            Ok(ReplyV1::Accepted(_)) => self.notice = "Invalid Daimon reply".into(),
            Err(()) => self.notice = "Daimon unavailable; outcome not inferred".into(),
        }
    }
}
