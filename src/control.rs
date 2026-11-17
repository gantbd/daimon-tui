//! Closed two-step controls over Daimon's exact-state owner guards.

use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, sync_channel};

use daimon_agent_command::{
    AdvanceError, AdvanceRequest, CancelError, CancelRequest, CloseError, CloseRequest,
    PermissionError, PermissionRequest, PromptError, PromptRequest, ReattachError, ReattachRequest,
};
use daimon_client::AgentServicePort;
use daimon_protocol::{
    SessionHandleV1,
    agent::{AgentOutcomeV1, AgentPermissionRequestV1, AgentSessionViewV1},
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    text::Line,
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::Lifecycle;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgentControlRequest {
    Prompt(PromptRequest),
    Permission(AgentPermissionControlRequest),
    Reattach(ReattachRequest),
    Advance(AdvanceRequest),
    Cancel(CancelRequest),
    Close(CloseRequest),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentPermissionControlRequest {
    pub request: PermissionRequest,
    pub challenge: AgentPermissionRequestV1,
}

impl AgentControlRequest {
    pub const fn session(&self) -> daimon_protocol::AgentSessionIdV1 {
        match self {
            Self::Prompt(request) => request.session,
            Self::Permission(request) => request.request.session,
            Self::Reattach(request) => request.session,
            Self::Advance(request) => request.session,
            Self::Cancel(request) => request.session,
            Self::Close(request) => request.session,
        }
    }

    pub fn confirmation(&self) -> String {
        match self {
            Self::Prompt(request) => request.confirmation(),
            Self::Permission(request) => {
                daimon_agent_command::permission_confirmation(&request.request, &request.challenge)
            }
            Self::Reattach(request) => request.confirmation(),
            Self::Advance(request) => request.confirmation(),
            Self::Cancel(request) => request.confirmation(),
            Self::Close(request) => request.confirmation(),
        }
    }

    const fn label(&self) -> &'static str {
        match self {
            Self::Prompt(_) => "prompt ready session",
            Self::Permission(_) => "decide permission challenge",
            Self::Reattach(_) => "reattach detached session",
            Self::Advance(_) => "advance active turn",
            Self::Cancel(_) => "cancel active turn",
            Self::Close(_) => "close ready session",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentControlInput {
    Arm,
    Confirm,
    Back,
    Close,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentControlDisposition {
    Proposed,
    Pending,
    Applied,
    Refused,
    Failed,
    OutcomeUnknown,
}

pub(crate) struct WorkerReply {
    disposition: AgentControlDisposition,
    notice: &'static str,
    view: Option<AgentSessionViewV1>,
}

pub struct AgentControlComponent {
    submit: SyncSender<()>,
    replies: Receiver<WorkerReply>,
    request: AgentControlRequest,
    armed: bool,
    disposition: AgentControlDisposition,
    notice: String,
}

impl AgentControlComponent {
    pub fn new<P>(service_session: SessionHandleV1, port: P, request: AgentControlRequest) -> Self
    where
        P: AgentServicePort + Send + 'static,
        P::Error: Send + 'static,
    {
        let worker_request = request.clone();
        let (submit_tx, submit_rx) = sync_channel(1);
        let (reply_tx, reply_rx) = sync_channel(1);
        std::thread::spawn(move || {
            if submit_rx.recv().is_ok() {
                let reply = execute(service_session, port, worker_request);
                let _ = reply_tx.send(reply);
            }
        });
        Self::from_channels(submit_tx, reply_rx, request)
    }

    /// Defers transport connection to the one-shot worker after confirmation,
    /// so opening the proposal never blocks the host's render/update thread.
    pub fn new_with_connector<P, F, E>(connector: F, request: AgentControlRequest) -> Self
    where
        P: AgentServicePort + Send + 'static,
        P::Error: Send + 'static,
        F: FnOnce() -> Result<(SessionHandleV1, P), E> + Send + 'static,
        E: Send + 'static,
    {
        let worker_request = request.clone();
        let (submit_tx, submit_rx) = sync_channel(1);
        let (reply_tx, reply_rx) = sync_channel(1);
        std::thread::spawn(move || {
            if submit_rx.recv().is_ok() {
                let reply = match connector() {
                    Ok((service_session, port)) => execute(service_session, port, worker_request),
                    Err(_) => WorkerReply {
                        disposition: AgentControlDisposition::Failed,
                        notice: "Daimon control connection refused before submission",
                        view: None,
                    },
                };
                let _ = reply_tx.send(reply);
            }
        });
        Self::from_channels(submit_tx, reply_rx, request)
    }

    fn from_channels(
        submit_tx: SyncSender<()>,
        reply_rx: Receiver<WorkerReply>,
        request: AgentControlRequest,
    ) -> Self {
        Self {
            submit: submit_tx,
            replies: reply_rx,
            request,
            armed: false,
            disposition: AgentControlDisposition::Proposed,
            notice: "Review the exact proposal; c arms, Enter submits once".into(),
        }
    }

    pub fn poll(&mut self) {
        match self.replies.try_recv() {
            Ok(reply) => {
                self.disposition = reply.disposition;
                self.notice = reply.notice.into();
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected)
                if self.disposition == AgentControlDisposition::Pending =>
            {
                self.disposition = AgentControlDisposition::OutcomeUnknown;
                self.notice =
                    "Daimon control worker disconnected after submission; inspect before recovery"
                        .into();
            }
            Err(TryRecvError::Disconnected) => {}
        }
    }

    pub fn update(&mut self, input: AgentControlInput, focused: bool) -> Lifecycle {
        if !focused {
            return Lifecycle::Running;
        }
        match input {
            AgentControlInput::Close
                if matches!(
                    self.disposition,
                    AgentControlDisposition::Pending | AgentControlDisposition::OutcomeUnknown
                ) =>
            {
                self.notice =
                    "Daimon control is unresolved; wait or inspect exact session state first"
                        .into();
            }
            AgentControlInput::Close => return Lifecycle::CloseRequested,
            AgentControlInput::Arm if self.disposition == AgentControlDisposition::Proposed => {
                self.armed = true;
                self.notice = "Daimon control armed; Enter submits once, Esc disarms".into();
            }
            AgentControlInput::Confirm
                if self.armed && self.disposition == AgentControlDisposition::Proposed =>
            {
                self.armed = false;
                if self.submit.try_send(()).is_ok() {
                    self.disposition = AgentControlDisposition::Pending;
                    self.notice = "Daimon guarded request pending; no retry is permitted".into();
                } else {
                    self.disposition = AgentControlDisposition::Failed;
                    self.notice = "Daimon control worker unavailable before submission".into();
                }
            }
            AgentControlInput::Back if self.disposition == AgentControlDisposition::Proposed => {
                self.armed = false;
                self.notice = "Daimon control disarmed".into();
            }
            AgentControlInput::Arm | AgentControlInput::Confirm | AgentControlInput::Back => {}
        }
        Lifecycle::Running
    }

    pub fn draw_in(&self, frame: &mut Frame<'_>, area: Rect, focused: bool) {
        let [body, footer] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
        let title = if focused {
            " Daimon guarded control • focused "
        } else {
            " Daimon guarded control "
        };
        let lines = vec![
            Line::from(format!("action: {}", self.request.label())),
            Line::from(format!("state: {:?}", self.disposition)),
            Line::from(""),
            Line::from("Exact proposal"),
            Line::from(self.request.confirmation()),
            Line::from(""),
            Line::from(if self.armed {
                "ARMED — Enter submits exactly once; Esc cancels"
            } else {
                "c arm · Enter confirm · Esc close/disarm"
            }),
        ];
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .block(Block::default().borders(Borders::ALL).title(title)),
            body,
        );
        frame.render_widget(Paragraph::new(self.notice.as_str()), footer);
    }

    pub const fn disposition(&self) -> AgentControlDisposition {
        self.disposition
    }

    pub fn notice(&self) -> &str {
        &self.notice
    }
}

/// The per-session truth retained by a bounded guarded-control run.
///
/// A batch is deliberately not described as atomic: every child has its own
/// owner decision and durable session projection, and later children remain
/// `NotSent` after an uncertain outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentControlChildDisposition {
    NotSent,
    Pending,
    Applied,
    Refused,
    Failed,
    OutcomeUnknown,
}

struct BatchChild {
    request: AgentControlRequest,
    disposition: AgentControlChildDisposition,
    view: Option<AgentSessionViewV1>,
}

enum BatchReply {
    Started(usize),
    Finished(usize, WorkerReply),
    Done,
}

/// Sequential, bounded control over explicitly configured local sessions.
///
/// Each child uses a fresh owner connection. Known refusals do not hide other
/// independent children; failures and unknown outcomes stop the run because a
/// subsequent action would make operator reasoning about the boundary harder.
pub struct AgentControlBatchComponent {
    submit: SyncSender<()>,
    replies: Receiver<BatchReply>,
    children: Vec<BatchChild>,
    armed: bool,
    pending: bool,
    selected: usize,
    recovery_inspected: bool,
    notice: String,
}

impl AgentControlBatchComponent {
    pub const MAX_CHILDREN: usize = 16;

    pub fn new_with_connector<P, F, E>(
        connector: F,
        requests: Vec<AgentControlRequest>,
    ) -> Result<Self, &'static str>
    where
        P: AgentServicePort + Send + 'static,
        P::Error: Send + 'static,
        F: Fn() -> Result<(SessionHandleV1, P), E> + Send + 'static,
        E: Send + 'static,
    {
        if requests.is_empty() {
            return Err("Daimon control batch requires at least one exact request");
        }
        if requests.len() > Self::MAX_CHILDREN {
            return Err("Daimon control batch exceeds the 16-session bound");
        }
        let worker_requests = requests.clone();
        let (submit_tx, submit_rx) = sync_channel(1);
        let (reply_tx, reply_rx) = sync_channel(1);
        std::thread::spawn(move || {
            if submit_rx.recv().is_err() {
                return;
            }
            for (index, request) in worker_requests.into_iter().enumerate() {
                if reply_tx.send(BatchReply::Started(index)).is_err() {
                    return;
                }
                let reply = match connector() {
                    Ok((service_session, port)) => execute(service_session, port, request),
                    Err(_) => WorkerReply {
                        disposition: AgentControlDisposition::Failed,
                        notice: "Daimon child connection unavailable before submission",
                        view: None,
                    },
                };
                let stop = matches!(
                    reply.disposition,
                    AgentControlDisposition::Failed | AgentControlDisposition::OutcomeUnknown
                );
                if reply_tx.send(BatchReply::Finished(index, reply)).is_err() {
                    return;
                }
                if stop {
                    break;
                }
            }
            let _ = reply_tx.send(BatchReply::Done);
        });
        Ok(Self {
            submit: submit_tx,
            replies: reply_rx,
            children: requests
                .into_iter()
                .map(|request| BatchChild {
                    request,
                    disposition: AgentControlChildDisposition::NotSent,
                    view: None,
                })
                .collect(),
            armed: false,
            pending: false,
            selected: 0,
            recovery_inspected: false,
            notice: "Review every exact proposal; c arms, Enter starts the bounded run".into(),
        })
    }

    pub fn poll(&mut self) {
        loop {
            match self.replies.try_recv() {
                Ok(BatchReply::Started(index)) => {
                    self.selected = index;
                    self.children[index].disposition = AgentControlChildDisposition::Pending;
                    self.notice = format!(
                        "Daimon child {}/{} pending; it will be submitted at most once",
                        index + 1,
                        self.children.len()
                    );
                }
                Ok(BatchReply::Finished(index, reply)) => {
                    self.children[index].disposition = match reply.disposition {
                        AgentControlDisposition::Applied => AgentControlChildDisposition::Applied,
                        AgentControlDisposition::Refused => AgentControlChildDisposition::Refused,
                        AgentControlDisposition::Failed => AgentControlChildDisposition::Failed,
                        AgentControlDisposition::OutcomeUnknown => {
                            AgentControlChildDisposition::OutcomeUnknown
                        }
                        AgentControlDisposition::Proposed | AgentControlDisposition::Pending => {
                            AgentControlChildDisposition::Failed
                        }
                    };
                    self.children[index].view = reply.view;
                    self.notice = reply.notice.into();
                }
                Ok(BatchReply::Done) => {
                    self.pending = false;
                    let applied = self.applied_count();
                    let refused = self
                        .children
                        .iter()
                        .filter(|child| child.disposition == AgentControlChildDisposition::Refused)
                        .count();
                    let failed = self
                        .children
                        .iter()
                        .filter(|child| child.disposition == AgentControlChildDisposition::Failed)
                        .count();
                    let unknown = self
                        .children
                        .iter()
                        .filter(|child| {
                            child.disposition == AgentControlChildDisposition::OutcomeUnknown
                        })
                        .count();
                    let unsent = self
                        .children
                        .iter()
                        .filter(|child| child.disposition == AgentControlChildDisposition::NotSent)
                        .count();
                    self.notice = format!(
                        "Daimon run settled: {applied} applied, {refused} refused, {failed} failed, {unknown} unknown, {unsent} not sent"
                    );
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if self.pending {
                        if let Some(child) = self.children.iter_mut().find(|child| {
                            child.disposition == AgentControlChildDisposition::Pending
                        }) {
                            child.disposition = AgentControlChildDisposition::OutcomeUnknown;
                        }
                        self.pending = false;
                        self.notice = "Daimon batch worker disconnected with a child pending; outcome unknown and later children were not sent".into();
                    }
                    break;
                }
            }
        }
    }

    pub fn update(&mut self, input: AgentControlInput, focused: bool) -> Lifecycle {
        if !focused {
            return Lifecycle::Running;
        }
        match input {
            AgentControlInput::Close if self.pending || self.recovery_required() => {
                self.notice = "Daimon run is unresolved; inspect exact session state before leaving this evidence".into();
            }
            AgentControlInput::Close => return Lifecycle::CloseRequested,
            AgentControlInput::Arm if !self.pending && self.all_not_sent() => {
                self.armed = true;
                self.notice = "Daimon batch armed; Enter submits each child at most once".into();
            }
            AgentControlInput::Confirm if self.armed && self.all_not_sent() => {
                self.armed = false;
                if self.submit.try_send(()).is_ok() {
                    self.pending = true;
                    self.notice = "Daimon bounded control run started".into();
                } else {
                    self.notice = "Daimon batch worker unavailable before submission".into();
                }
            }
            AgentControlInput::Back if self.all_not_sent() => {
                self.armed = false;
                self.notice = "Daimon batch disarmed".into();
            }
            AgentControlInput::Arm | AgentControlInput::Confirm | AgentControlInput::Back => {}
        }
        Lifecycle::Running
    }

    pub fn draw_in(&self, frame: &mut Frame<'_>, area: Rect, focused: bool) {
        let [body, footer] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
        let title = if focused {
            " Daimon guarded session set • focused "
        } else {
            " Daimon guarded session set "
        };
        let mut lines = vec![
            Line::from(format!(
                "{} exact session controls • {} applied",
                self.children.len(),
                self.applied_count()
            )),
            Line::from("This run is sequential and non-atomic; every child keeps its own outcome."),
            Line::from(""),
        ];
        for (index, child) in self.children.iter().enumerate() {
            let marker = if index == self.selected { ">" } else { " " };
            let receipt = child.view.as_ref().map_or(String::new(), |view| {
                format!(" → {:?} r{}", view.phase, view.revision)
            });
            lines.push(Line::from(format!(
                "{marker} {:>2}. {:<14} {}{receipt}",
                index + 1,
                format!("{:?}", child.disposition),
                child.request.label(),
            )));
        }
        lines.extend([
            Line::from(""),
            Line::from(if self.armed {
                "ARMED — Enter starts once; Esc disarms"
            } else {
                "c arm · Enter confirm · Esc close/disarm"
            }),
        ]);
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .block(Block::default().borders(Borders::ALL).title(title)),
            body,
        );
        frame.render_widget(Paragraph::new(self.notice.as_str()), footer);
    }

    pub const fn request_pending(&self) -> bool {
        self.pending
    }

    pub fn notice(&self) -> &str {
        &self.notice
    }

    /// Aggregate severity for hosts; child dispositions remain the source of
    /// truth and are always available separately.
    pub fn disposition(&self) -> AgentControlDisposition {
        if self.pending {
            AgentControlDisposition::Pending
        } else if self.has_unknown() {
            AgentControlDisposition::OutcomeUnknown
        } else if self
            .children
            .iter()
            .any(|child| child.disposition == AgentControlChildDisposition::Failed)
        {
            AgentControlDisposition::Failed
        } else if self
            .children
            .iter()
            .any(|child| child.disposition == AgentControlChildDisposition::Refused)
        {
            AgentControlDisposition::Refused
        } else if self.all_not_sent() {
            AgentControlDisposition::Proposed
        } else {
            AgentControlDisposition::Applied
        }
    }

    pub fn child_dispositions(&self) -> Vec<AgentControlChildDisposition> {
        self.children
            .iter()
            .map(|child| child.disposition)
            .collect()
    }

    pub fn applied_views(&self) -> Vec<&AgentSessionViewV1> {
        self.children
            .iter()
            .filter_map(|child| child.view.as_ref())
            .collect()
    }

    pub fn applied_count(&self) -> usize {
        self.children
            .iter()
            .filter(|child| child.disposition == AgentControlChildDisposition::Applied)
            .count()
    }

    pub fn has_unknown(&self) -> bool {
        self.children
            .iter()
            .any(|child| child.disposition == AgentControlChildDisposition::OutcomeUnknown)
    }

    /// Exact sent sessions whose projections must be reconciled before an
    /// outcome-unknown evidence sheet can be dismissed. Applied siblings are
    /// included so the underlying workset cannot retain a mixed stale view.
    pub fn recovery_sessions(&self) -> Vec<daimon_protocol::AgentSessionIdV1> {
        self.children
            .iter()
            .filter(|child| {
                matches!(
                    child.disposition,
                    AgentControlChildDisposition::Applied
                        | AgentControlChildDisposition::Refused
                        | AgentControlChildDisposition::OutcomeUnknown
                )
            })
            .map(|child| child.request.session())
            .collect()
    }

    pub fn recovery_required(&self) -> bool {
        self.has_unknown() && !self.recovery_inspected
    }

    pub fn recovery_inspection_started(&mut self) {
        self.notice =
            "Exact outcome-unknown session inspection pending; no mutation will be retried".into();
    }

    pub fn complete_recovery_inspection(&mut self, available: bool) {
        self.recovery_inspected = true;
        self.notice = if available {
            "Recovery inspection complete; mutation outcome remains unknown, evidence retained; Esc closes"
        } else {
            "Recovery inspection unavailable; mutation outcome remains unknown, evidence retained; Esc closes"
        }
        .into();
    }

    fn all_not_sent(&self) -> bool {
        self.children
            .iter()
            .all(|child| child.disposition == AgentControlChildDisposition::NotSent)
    }
}

fn outcome(value: AgentOutcomeV1) -> WorkerReply {
    match value {
        AgentOutcomeV1::Session(view) => WorkerReply {
            disposition: AgentControlDisposition::Applied,
            notice: "Daimon durably accepted the guarded request; refresh the session",
            view: Some(view),
        },
        AgentOutcomeV1::OutcomeUnknown { .. } => WorkerReply {
            disposition: AgentControlDisposition::OutcomeUnknown,
            notice: "Daimon outcome unknown; inspect the exact session before any recovery",
            view: None,
        },
        _ => WorkerReply {
            disposition: AgentControlDisposition::Failed,
            notice: "Daimon returned an invalid guarded-control outcome",
            view: None,
        },
    }
}

pub(crate) fn execute<P: AgentServicePort>(
    service_session: SessionHandleV1,
    port: P,
    request: AgentControlRequest,
) -> WorkerReply {
    let confirmation = request.confirmation();
    match request {
        AgentControlRequest::Prompt(request) => {
            match daimon_agent_command::prompt(service_session, port, request, &confirmation) {
                Ok(value) => outcome(value),
                Err(PromptError::MutationTransport(_)) => unknown(),
                Err(
                    PromptError::InvalidPrompt
                    | PromptError::ObservationRefused(_)
                    | PromptError::SessionChanged { .. }
                    | PromptError::MutationRefused(_)
                    | PromptError::ConfirmationMismatch { .. },
                ) => refused(),
                Err(
                    PromptError::ObservationTransport(_)
                    | PromptError::InvalidObservation
                    | PromptError::InvalidMutationReply,
                ) => failed(),
            }
        }
        AgentControlRequest::Permission(request) => {
            match daimon_agent_command::decide_permission(
                service_session,
                port,
                request.request,
                &confirmation,
            ) {
                Ok(value) => outcome(value),
                Err(PermissionError::MutationTransport(_)) => unknown(),
                Err(
                    PermissionError::ObservationRefused(_)
                    | PermissionError::SessionChanged { .. }
                    | PermissionError::ChallengeChanged
                    | PermissionError::MutationRefused(_)
                    | PermissionError::ConfirmationMismatch { .. },
                ) => refused(),
                Err(
                    PermissionError::ObservationTransport(_)
                    | PermissionError::InvalidObservation
                    | PermissionError::InvalidMutationReply,
                ) => failed(),
            }
        }
        AgentControlRequest::Reattach(request) => {
            match daimon_agent_command::reattach(service_session, port, request, &confirmation) {
                Ok(value) => outcome(value),
                Err(ReattachError::MutationTransport(_)) => unknown(),
                Err(
                    ReattachError::ObservationRefused(_)
                    | ReattachError::SessionChanged { .. }
                    | ReattachError::MutationRefused(_)
                    | ReattachError::ConfirmationMismatch { .. },
                ) => refused(),
                Err(
                    ReattachError::ObservationTransport(_)
                    | ReattachError::InvalidObservation
                    | ReattachError::InvalidMutationReply,
                ) => failed(),
            }
        }
        AgentControlRequest::Advance(request) => {
            match daimon_agent_command::advance(service_session, port, request, &confirmation) {
                Ok(value) => outcome(value),
                Err(AdvanceError::MutationTransport(_)) => unknown(),
                Err(
                    AdvanceError::ObservationRefused(_)
                    | AdvanceError::SessionChanged { .. }
                    | AdvanceError::MutationRefused(_)
                    | AdvanceError::ConfirmationMismatch { .. },
                ) => refused(),
                Err(
                    AdvanceError::ObservationTransport(_)
                    | AdvanceError::InvalidObservation
                    | AdvanceError::InvalidMutationReply,
                ) => failed(),
            }
        }
        AgentControlRequest::Cancel(request) => {
            match daimon_agent_command::cancel(service_session, port, request, &confirmation) {
                Ok(value) => outcome(value),
                Err(CancelError::MutationTransport(_)) => unknown(),
                Err(
                    CancelError::ObservationRefused(_)
                    | CancelError::SessionChanged { .. }
                    | CancelError::MutationRefused(_)
                    | CancelError::ConfirmationMismatch { .. },
                ) => refused(),
                Err(
                    CancelError::ObservationTransport(_)
                    | CancelError::InvalidObservation
                    | CancelError::InvalidMutationReply,
                ) => failed(),
            }
        }
        AgentControlRequest::Close(request) => {
            match daimon_agent_command::close(service_session, port, request, &confirmation) {
                Ok(value) => outcome(value),
                Err(CloseError::MutationTransport(_)) => unknown(),
                Err(
                    CloseError::ObservationRefused(_)
                    | CloseError::SessionChanged { .. }
                    | CloseError::MutationRefused(_)
                    | CloseError::ConfirmationMismatch { .. },
                ) => refused(),
                Err(
                    CloseError::ObservationTransport(_)
                    | CloseError::InvalidObservation
                    | CloseError::InvalidMutationReply,
                ) => failed(),
            }
        }
    }
}

const fn unknown() -> WorkerReply {
    WorkerReply {
        disposition: AgentControlDisposition::OutcomeUnknown,
        notice: "Daimon mutation delivery is uncertain; inspect before any recovery",
        view: None,
    }
}

const fn refused() -> WorkerReply {
    WorkerReply {
        disposition: AgentControlDisposition::Refused,
        notice: "Daimon refused the exact guarded request; refresh current session state",
        view: None,
    }
}

const fn failed() -> WorkerReply {
    WorkerReply {
        disposition: AgentControlDisposition::Failed,
        notice: "Daimon control failed before a confirmed mutation outcome",
        view: None,
    }
}
