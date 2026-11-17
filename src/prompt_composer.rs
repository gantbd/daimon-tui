//! Content-safe multiline composition before an exact Daimon prompt run.

use daimon_agent_command::MAX_PROMPT_BYTES;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Field {
    Prompt,
    Review,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentPromptInput {
    Character(char),
    Backspace,
    Newline,
    NextField,
    Arm,
    Confirm,
    Back,
    Close,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgentPromptOutcome {
    None,
    Submit(Vec<u8>),
    CloseRequested,
}

/// A bounded prompt editor with a separate review/arm phase.
///
/// It owns no execution policy or mutation authority. The host turns the
/// returned bytes into exact per-session owner requests and must retain those
/// requests until every submitted child has a terminal disposition.
pub struct AgentPromptComposer {
    prompt: String,
    sessions: usize,
    field: Field,
    armed: bool,
    notice: String,
}

impl AgentPromptComposer {
    pub fn new(sessions: usize) -> Result<Self, &'static str> {
        if sessions == 0 || sessions > 16 {
            return Err("Daimon prompt composer requires 1–16 selected sessions");
        }
        Ok(Self {
            prompt: String::new(),
            sessions,
            field: Field::Prompt,
            armed: false,
            notice: "Compose bounded prompt text; Tab reviews the selected session set".into(),
        })
    }

    pub fn insert_text(&mut self, text: &str) {
        if self.field != Field::Prompt || self.armed {
            return;
        }
        for character in text.chars() {
            push_bounded(&mut self.prompt, character);
        }
        self.notice = format!("{} prompt byte(s) retained locally", self.prompt.len());
    }

    pub fn update(&mut self, input: AgentPromptInput, focused: bool) -> AgentPromptOutcome {
        if !focused {
            return AgentPromptOutcome::None;
        }
        match input {
            AgentPromptInput::Close if self.armed => {
                self.armed = false;
                self.notice = "Daimon prompt disarmed; no request was created".into();
            }
            AgentPromptInput::Close => return AgentPromptOutcome::CloseRequested,
            AgentPromptInput::NextField if !self.armed => {
                self.field = match self.field {
                    Field::Prompt => Field::Review,
                    Field::Review => Field::Prompt,
                };
                self.notice = match self.field {
                    Field::Prompt => "Editing prompt text".into(),
                    Field::Review => format!(
                        "Reviewing {} selected session(s); c arms the exact batch",
                        self.sessions
                    ),
                };
            }
            AgentPromptInput::Character(character)
                if self.field == Field::Prompt && !self.armed =>
            {
                push_bounded(&mut self.prompt, character);
            }
            AgentPromptInput::Backspace if self.field == Field::Prompt && !self.armed => {
                self.prompt.pop();
            }
            AgentPromptInput::Newline if self.field == Field::Prompt && !self.armed => {
                push_bounded(&mut self.prompt, '\n');
            }
            AgentPromptInput::Arm if self.field == Field::Review && !self.prompt.is_empty() => {
                self.armed = true;
                self.notice = format!(
                    "Daimon prompt armed for {} session(s); Enter creates the exact child requests",
                    self.sessions
                );
            }
            AgentPromptInput::Arm if self.field == Field::Review => {
                self.notice = "Daimon prompt cannot be empty".into();
            }
            AgentPromptInput::Confirm if self.field == Field::Review && self.armed => {
                self.armed = false;
                self.notice = "Daimon prompt handed to the guarded session-set runner".into();
                return AgentPromptOutcome::Submit(self.prompt.as_bytes().to_vec());
            }
            AgentPromptInput::Back if self.armed => {
                self.armed = false;
                self.notice = "Daimon prompt disarmed".into();
            }
            AgentPromptInput::Character(_)
            | AgentPromptInput::Backspace
            | AgentPromptInput::Newline
            | AgentPromptInput::NextField
            | AgentPromptInput::Arm
            | AgentPromptInput::Confirm
            | AgentPromptInput::Back => {}
        }
        AgentPromptOutcome::None
    }

    pub fn draw_in(&self, frame: &mut Frame<'_>, area: Rect, focused: bool) {
        let [body, footer] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
        let selected = |field| {
            if self.field == field {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            }
        };
        let mut lines = vec![Line::from(vec![
            Span::styled("prompt  ", selected(Field::Prompt)),
            Span::raw(format!("{} / {MAX_PROMPT_BYTES} bytes", self.prompt.len())),
        ])];
        if self.prompt.is_empty() {
            lines.push(Line::styled(
                "  (empty)",
                Style::default().fg(Color::DarkGray),
            ));
        } else {
            for line in self.prompt.lines().take(12) {
                lines.push(Line::from(format!("  {line}")));
            }
        }
        lines.extend([
            Line::from(""),
            Line::from(vec![
                Span::styled("review  ", selected(Field::Review)),
                Span::raw(format!(
                    "{} selected session(s) · independent execution envelopes and receipts",
                    self.sessions
                )),
            ]),
            Line::from(""),
            Line::styled(
                if self.armed {
                    "ARMED — Enter creates the exact batch; Esc disarms"
                } else {
                    "Enter newline · Tab edit/review · c arm · Enter confirm · Esc close"
                },
                Style::default().fg(if self.armed {
                    Color::Yellow
                } else {
                    Color::DarkGray
                }),
            ),
        ]);
        let title = if focused {
            " Compose Daimon prompt • focused "
        } else {
            " Compose Daimon prompt "
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

    pub fn reviewing(&self) -> bool {
        self.field == Field::Review
    }
}

fn push_bounded(target: &mut String, character: char) {
    if (!character.is_control() || character == '\n')
        && target.len() + character.len_utf8() <= MAX_PROMPT_BYTES
    {
        target.push(character);
    }
}
