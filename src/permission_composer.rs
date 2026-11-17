//! Challenge-native composition for bounded Daimon permission worksets.

use daimon_protocol::{
    AgentPermissionOptionIdV1, AgentSessionIdV1,
    agent::{AgentPermissionOptionKindV1, AgentPermissionRequestV1},
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::escaped;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentPermissionChallenge {
    pub session: AgentSessionIdV1,
    pub expected_revision: u64,
    pub event_cursor: u64,
    pub challenge: AgentPermissionRequestV1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentPermissionSelection {
    pub challenge: AgentPermissionChallenge,
    pub option: AgentPermissionOptionIdV1,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentPermissionInput {
    PreviousSession,
    NextSession,
    PreviousOption,
    NextOption,
    Arm,
    Confirm,
    Back,
    Close,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgentPermissionOutcome {
    None,
    Submit(Vec<AgentPermissionSelection>),
    CloseRequested,
}

struct Entry {
    challenge: AgentPermissionChallenge,
    option: usize,
}

/// Selects one exact owner-published option for every waiting session.
///
/// Each entry defaults to the least persistent reject option available. The
/// operator separately arms the completed workset before any execution
/// envelope is created.
pub struct AgentPermissionComposer {
    entries: Vec<Entry>,
    current: usize,
    armed: bool,
    notice: String,
}

impl AgentPermissionComposer {
    pub const MAX_SESSIONS: usize = 16;
    pub const MAX_OPTIONS: usize = 16;

    pub fn new(challenges: Vec<AgentPermissionChallenge>) -> Result<Self, &'static str> {
        if challenges.is_empty() || challenges.len() > Self::MAX_SESSIONS {
            return Err("Daimon permission composer requires 1–16 exact challenges");
        }
        if challenges.iter().enumerate().any(|(index, candidate)| {
            challenges[..index]
                .iter()
                .any(|prior| prior.session == candidate.session)
        }) {
            return Err("Daimon permission composer requires unique sessions");
        }
        if challenges.iter().any(|candidate| {
            candidate.challenge.options.is_empty()
                || candidate.challenge.options.len() > Self::MAX_OPTIONS
        }) {
            return Err("Daimon permission challenge requires 1–16 owner options");
        }
        let entries = challenges
            .into_iter()
            .map(|challenge| {
                let option = challenge
                    .challenge
                    .options
                    .iter()
                    .position(|option| option.kind == AgentPermissionOptionKindV1::RejectOnce)
                    .or_else(|| {
                        challenge.challenge.options.iter().position(|option| {
                            option.kind == AgentPermissionOptionKindV1::RejectAlways
                        })
                    })
                    .unwrap_or(0);
                Entry { challenge, option }
            })
            .collect();
        Ok(Self {
            entries,
            current: 0,
            armed: false,
            notice: "Review each exact challenge; reject-once is preferred as the initial choice"
                .into(),
        })
    }

    pub fn update(&mut self, input: AgentPermissionInput, focused: bool) -> AgentPermissionOutcome {
        if !focused {
            return AgentPermissionOutcome::None;
        }
        match input {
            AgentPermissionInput::Close if self.armed => {
                self.armed = false;
                self.notice = "Daimon permission workset disarmed".into();
            }
            AgentPermissionInput::Close => return AgentPermissionOutcome::CloseRequested,
            AgentPermissionInput::PreviousSession if !self.armed => {
                self.current = self.current.saturating_sub(1);
            }
            AgentPermissionInput::NextSession if !self.armed => {
                self.current = (self.current + 1).min(self.entries.len() - 1);
            }
            AgentPermissionInput::PreviousOption if !self.armed => {
                let entry = &mut self.entries[self.current];
                entry.option = entry.option.saturating_sub(1);
                self.notice = "Selected the previous owner-published option".into();
            }
            AgentPermissionInput::NextOption if !self.armed => {
                let entry = &mut self.entries[self.current];
                entry.option = (entry.option + 1).min(entry.challenge.challenge.options.len() - 1);
                self.notice = "Selected the next owner-published option".into();
            }
            AgentPermissionInput::Arm if !self.armed => {
                self.armed = true;
                self.notice = format!(
                    "Daimon permission workset armed for {} session(s); Enter creates exact proposals",
                    self.entries.len()
                );
            }
            AgentPermissionInput::Confirm if self.armed => {
                self.armed = false;
                return AgentPermissionOutcome::Submit(
                    self.entries
                        .iter()
                        .map(|entry| AgentPermissionSelection {
                            challenge: entry.challenge.clone(),
                            option: entry.challenge.challenge.options[entry.option].id,
                        })
                        .collect(),
                );
            }
            AgentPermissionInput::Back if self.armed => {
                self.armed = false;
                self.notice = "Daimon permission workset disarmed".into();
            }
            AgentPermissionInput::PreviousSession
            | AgentPermissionInput::NextSession
            | AgentPermissionInput::PreviousOption
            | AgentPermissionInput::NextOption
            | AgentPermissionInput::Arm
            | AgentPermissionInput::Confirm
            | AgentPermissionInput::Back => {}
        }
        AgentPermissionOutcome::None
    }

    pub fn draw_in(&self, frame: &mut Frame<'_>, area: Rect, focused: bool) {
        let [body, footer] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
        let entry = &self.entries[self.current];
        let mut lines = vec![
            Line::from(format!(
                "{} exact permission challenge(s) · session {}/{}",
                self.entries.len(),
                self.current + 1,
                self.entries.len()
            )),
            Line::from(format!(
                "revision {} · cursor {}",
                entry.challenge.expected_revision, entry.challenge.event_cursor
            )),
            Line::from(vec![
                Span::styled(
                    "capability  ",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(escaped(&entry.challenge.challenge.capability)),
            ]),
            Line::from(vec![
                Span::styled(
                    "request     ",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(escaped(&entry.challenge.challenge.description)),
            ]),
            Line::from(""),
            Line::from("Owner-published options"),
        ];
        for (index, option) in entry.challenge.challenge.options.iter().enumerate() {
            let marker = if index == entry.option { ">" } else { " " };
            let style = if index == entry.option {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            lines.push(Line::styled(
                format!("{marker} {:?} · {}", option.kind, escaped(&option.label)),
                style,
            ));
        }
        lines.extend([
            Line::from(""),
            Line::styled(
                if self.armed {
                    "ARMED — Enter creates every exact proposal; Esc disarms"
                } else {
                    "↑/↓ sessions · ←/→ option · c arm · Enter confirm · Esc close"
                },
                Style::default().fg(if self.armed {
                    Color::Yellow
                } else {
                    Color::DarkGray
                }),
            ),
        ]);
        let title = if focused {
            " Decide Daimon permissions • focused "
        } else {
            " Decide Daimon permissions "
        };
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .block(Block::default().borders(Borders::ALL).title(title)),
            body,
        );
        frame.render_widget(Paragraph::new(self.notice.as_str()), footer);
    }

    pub fn notice(&self) -> &str {
        &self.notice
    }

    pub const fn armed(&self) -> bool {
        self.armed
    }
}
