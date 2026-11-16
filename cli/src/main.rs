//! Standalone interactive and headless Daimon terminal application.
#![forbid(unsafe_code)]

#[cfg(unix)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::{path::PathBuf, time::Duration};

    let mut args = std::env::args_os().skip(1);
    let Some(mode) = args.next() else {
        return Err(usage().into());
    };
    if mode == "--help" || mode == "-h" {
        println!("{}", usage());
        return Ok(());
    }
    if mode == "capabilities" {
        if args.next().is_some() {
            return Err(usage().into());
        }
        let manifest = daimon_tui::CAPABILITIES;
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema": manifest.schema,
                "peer": manifest.peer,
                "interactive": manifest.interactive,
                "headless": manifest.headless,
            }))?
        );
        return Ok(());
    }
    if mode == "agent-advance" {
        return agent_advance(args);
    }
    if mode == "agent-cancel" {
        return agent_cancel(args);
    }
    if mode == "agent-close" {
        return agent_close(args);
    }
    if mode == "agent-prompt" {
        return agent_prompt(args);
    }
    if mode == "agent-permission" {
        return agent_permission(args);
    }
    if mode == "agent-inspect" || mode == "agent-interactive" {
        let socket = args.next().map(PathBuf::from).ok_or_else(usage)?;
        let session = args
            .next()
            .and_then(|value| daimon_tui::parse_agent_session(&value.to_string_lossy()))
            .ok_or("agent session must be 32 hexadecimal digits")?;
        if args.next().is_some() {
            return Err(usage().into());
        }
        let (service_session, port) = daimon_client::unix::connect_agent(
            socket,
            daimon_wire::LimitsV1::default(),
            daimon_wire::agent::AgentWireLimitsV1::default(),
            Duration::from_secs(5),
        )
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
        return if mode == "agent-interactive" {
            run_agent_interactive(daimon_tui::AgentComponent::new(
                service_session,
                port,
                session,
            ))
        } else {
            run_agent_headless(service_session, port, session)
        };
    }
    if mode != "interactive" && mode != "headless" {
        return Err(usage().into());
    }
    let socket = match args.next().map(PathBuf::from) {
        Some(socket) if args.next().is_none() => socket,
        _ if mode == "headless" => return headless_failure("configuration_refused", usage()),
        _ => return Err(usage().into()),
    };
    let connection = daimon_client::unix::connect(
        socket,
        daimon_wire::LimitsV1::default(),
        Duration::from_secs(5),
    );
    let (session, port) = match connection {
        Ok(connection) => connection,
        Err(error) if mode == "headless" => {
            return headless_failure("connect_refused", &format!("{error:?}"));
        }
        Err(error) => return Err(std::io::Error::other(format!("{error:?}")).into()),
    };
    if mode == "interactive" {
        run_interactive(daimon_tui::Component::new(session, port))
    } else {
        run_headless(session, port)
    }
}

#[cfg(unix)]
fn run_agent_interactive(
    mut component: daimon_tui::AgentComponent,
) -> Result<(), Box<dyn std::error::Error>> {
    use crossterm::event::{self, Event};
    use std::time::Duration;

    ratatui::run(|terminal| -> Result<(), Box<dyn std::error::Error>> {
        loop {
            component.poll();
            terminal.draw(|frame| component.draw_in(frame, frame.area(), true))?;
            if !event::poll(Duration::from_millis(50))? {
                continue;
            }
            match event::read()? {
                Event::Resize(..) => {}
                Event::Key(key) => {
                    let Some(input) = daimon_tui::translate_key(key) else {
                        continue;
                    };
                    if component.update(input, true) == daimon_tui::Lifecycle::CloseRequested {
                        break;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    })
}

#[cfg(unix)]
fn run_agent_headless(
    service_session: daimon_protocol::SessionHandleV1,
    port: daimon_client::unix::AgentUnixPort,
    session: daimon_protocol::AgentSessionIdV1,
) -> Result<(), Box<dyn std::error::Error>> {
    use daimon_client::AgentReadOnlyClient;

    let mut client = AgentReadOnlyClient::new(service_session, port);
    match daimon_tui::observe_agent(&mut client, session) {
        Ok(observation) => {
            let view = observation.view;
            let capabilities = view.capabilities;
            let events = observation
                .events
                .entries
                .iter()
                .map(|record| {
                    serde_json::json!({
                        "cursor": record.cursor,
                        "kind": agent_event_kind(&record.event),
                    })
                })
                .collect::<Vec<_>>();
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "version": 1,
                    "mode": "headless",
                    "complete": true,
                    "peer": "daimon",
                    "capability": "agents.session.inspect.v1",
                    "status": "current",
                    "revision": view.revision,
                    "phase": format!("{:?}", view.phase),
                    "active_turn": view.active_turn.map(|_| "present"),
                    "last_event_cursor": view.last_event_cursor,
                    "profile_document_digest": hex(&view.profile_document_digest.into_bytes()),
                    "capabilities": {
                        "load_session": capabilities.load_session,
                        "resume_session": capabilities.resume_session,
                        "permissions": capabilities.permissions,
                        "filesystem_read": capabilities.filesystem_read,
                        "filesystem_write": capabilities.filesystem_write,
                        "terminal": capabilities.terminal,
                        "session_modes": capabilities.session_modes,
                        "session_models": capabilities.session_models,
                    },
                    "events": events,
                    "truncated": observation.events.next_after.is_some(),
                }))?
            );
            Ok(())
        }
        Err(error) => headless_failure_for("agents.session.inspect.v1", "unavailable", &error),
    }
}

#[cfg(unix)]
fn agent_event_kind(event: &daimon_protocol::agent::AgentEventV1) -> &'static str {
    use daimon_protocol::agent::AgentEventV1;
    match event {
        AgentEventV1::AssistantMessage { .. } => "assistant_message",
        AgentEventV1::Thought { .. } => "thought",
        AgentEventV1::Plan { .. } => "plan",
        AgentEventV1::ToolCall { .. } => "tool_call",
        AgentEventV1::Terminal { .. } => "terminal",
        AgentEventV1::Permission(_) => "permission",
        AgentEventV1::FileChange { .. } => "file_change",
        AgentEventV1::FileRead { .. } => "file_read",
        AgentEventV1::Usage { .. } => "usage",
        AgentEventV1::TurnFinished { .. } => "turn_finished",
    }
}

#[cfg(unix)]
fn agent_advance(
    mut args: impl Iterator<Item = std::ffi::OsString>,
) -> Result<(), Box<dyn std::error::Error>> {
    use daimon_agent_command::{AdvanceError, AdvanceRequest};
    use daimon_client::unix::AgentClientErrorV1;
    use daimon_protocol::agent::AgentOutcomeV1;
    use std::{path::PathBuf, time::Duration};

    let socket = args.next().map(PathBuf::from).ok_or_else(usage)?;
    let session = args
        .next()
        .and_then(|value| daimon_tui::parse_agent_session(&value.to_string_lossy()))
        .ok_or("agent session must be 32 hexadecimal digits")?;
    let turn = args
        .next()
        .and_then(|value| daimon_tui::parse_agent_turn(&value.to_string_lossy()))
        .ok_or("agent turn must be 32 hexadecimal digits")?;
    let expected_revision = parse_u64(args.next())?;
    let idempotency_key = args
        .next()
        .and_then(|value| daimon_tui::parse_idempotency_key(&value.to_string_lossy()))
        .ok_or("idempotency key must be 32 hexadecimal digits")?;
    let now = parse_u64(args.next())?;
    let confirmation = args
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or_else(usage)?;
    if args.next().is_some() {
        return Err(usage().into());
    }
    let (service_session, port) = match daimon_client::unix::connect_agent(
        socket,
        daimon_wire::LimitsV1::default(),
        daimon_wire::agent::AgentWireLimitsV1::default(),
        Duration::from_secs(5),
    ) {
        Ok(connection) => connection,
        Err(error) => return agent_advance_failure("not_sent", &format!("{error:?}"), None, None),
    };
    let request = AdvanceRequest {
        session,
        turn,
        expected_revision,
        idempotency_key,
        now,
    };
    match daimon_agent_command::advance(service_session, port, request, &confirmation) {
        Ok(AgentOutcomeV1::Session(view)) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "version": 1,
                    "peer": "daimon",
                    "capability": daimon_agent_command::CAPABILITY,
                    "complete": true,
                    "status": "advanced",
                    "revision": view.revision,
                    "phase": format!("{:?}", view.phase),
                    "active_turn": view.active_turn.map(|_| "present"),
                    "last_event_cursor": view.last_event_cursor,
                }))?
            );
            Ok(())
        }
        Ok(AgentOutcomeV1::OutcomeUnknown { .. }) => agent_advance_failure(
            "outcome_unknown",
            "Daimon retained an indeterminate advance outcome; inspect before any further action",
            None,
            None,
        ),
        Ok(AgentOutcomeV1::Events(_)) => agent_advance_failure(
            "invalid_reply",
            "unexpected Daimon advance reply",
            None,
            None,
        ),
        Err(AdvanceError::ConfirmationMismatch { required }) => agent_advance_failure(
            "confirmation_refused",
            "exact Daimon advance confirmation required",
            Some(&required),
            None,
        ),
        Err(AdvanceError::SessionChanged { actual }) => agent_advance_failure(
            "stale_session",
            "Daimon session revision, phase, or active turn changed",
            None,
            Some(&actual),
        ),
        Err(AdvanceError::MutationTransport(AgentClientErrorV1::OutcomeUnknown(error))) => {
            agent_advance_failure("outcome_unknown", &format!("{error:?}"), None, None)
        }
        Err(AdvanceError::ObservationTransport(AgentClientErrorV1::NotSent(error)))
        | Err(AdvanceError::MutationTransport(AgentClientErrorV1::NotSent(error))) => {
            agent_advance_failure("not_sent", &format!("{error:?}"), None, None)
        }
        Err(AdvanceError::ObservationRefused(refusal))
        | Err(AdvanceError::MutationRefused(refusal)) => {
            agent_advance_failure("refused", &format!("{refusal:?}"), None, None)
        }
        Err(error) => agent_advance_failure("unavailable", &format!("{error:?}"), None, None),
    }
}

#[cfg(unix)]
fn agent_cancel(
    mut args: impl Iterator<Item = std::ffi::OsString>,
) -> Result<(), Box<dyn std::error::Error>> {
    use daimon_agent_command::{CancelError, CancelRequest};
    use daimon_client::unix::AgentClientErrorV1;
    use daimon_protocol::agent::AgentOutcomeV1;
    use std::{path::PathBuf, time::Duration};

    let socket = args.next().map(PathBuf::from).ok_or_else(usage)?;
    let session = args
        .next()
        .and_then(|value| daimon_tui::parse_agent_session(&value.to_string_lossy()))
        .ok_or("agent session must be 32 hexadecimal digits")?;
    let turn = args
        .next()
        .and_then(|value| daimon_tui::parse_agent_turn(&value.to_string_lossy()))
        .ok_or("agent turn must be 32 hexadecimal digits")?;
    let expected_revision = parse_u64(args.next())?;
    let idempotency_key = args
        .next()
        .and_then(|value| daimon_tui::parse_idempotency_key(&value.to_string_lossy()))
        .ok_or("idempotency key must be 32 hexadecimal digits")?;
    let now = parse_u64(args.next())?;
    let confirmation = args
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or_else(usage)?;
    if args.next().is_some() {
        return Err(usage().into());
    }
    let (service_session, port) = match daimon_client::unix::connect_agent(
        socket,
        daimon_wire::LimitsV1::default(),
        daimon_wire::agent::AgentWireLimitsV1::default(),
        Duration::from_secs(5),
    ) {
        Ok(connection) => connection,
        Err(error) => return agent_cancel_failure("not_sent", &format!("{error:?}"), None, None),
    };
    let request = CancelRequest {
        session,
        turn,
        expected_revision,
        idempotency_key,
        now,
    };
    match daimon_agent_command::cancel(service_session, port, request, &confirmation) {
        Ok(AgentOutcomeV1::Session(view)) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "version": 1,
                    "peer": "daimon",
                    "capability": daimon_agent_command::CANCEL_CAPABILITY,
                    "complete": true,
                    "status": "cancelled",
                    "revision": view.revision,
                    "phase": format!("{:?}", view.phase),
                    "active_turn": view.active_turn.map(|_| "present"),
                    "last_event_cursor": view.last_event_cursor,
                }))?
            );
            Ok(())
        }
        Ok(AgentOutcomeV1::OutcomeUnknown { .. }) => agent_cancel_failure(
            "outcome_unknown",
            "Daimon retained an indeterminate cancellation outcome; inspect before any further action",
            None,
            None,
        ),
        Ok(AgentOutcomeV1::Events(_)) => agent_cancel_failure(
            "invalid_reply",
            "unexpected Daimon cancellation reply",
            None,
            None,
        ),
        Err(CancelError::ConfirmationMismatch { required }) => agent_cancel_failure(
            "confirmation_refused",
            "exact Daimon cancellation confirmation required",
            Some(&required),
            None,
        ),
        Err(CancelError::SessionChanged { actual }) => agent_cancel_failure(
            "stale_session",
            "Daimon session revision, phase, or active turn changed",
            None,
            Some(&actual),
        ),
        Err(CancelError::MutationTransport(AgentClientErrorV1::OutcomeUnknown(error))) => {
            agent_cancel_failure("outcome_unknown", &format!("{error:?}"), None, None)
        }
        Err(CancelError::ObservationTransport(AgentClientErrorV1::NotSent(error)))
        | Err(CancelError::MutationTransport(AgentClientErrorV1::NotSent(error))) => {
            agent_cancel_failure("not_sent", &format!("{error:?}"), None, None)
        }
        Err(CancelError::ObservationRefused(refusal))
        | Err(CancelError::MutationRefused(refusal)) => {
            agent_cancel_failure("refused", &format!("{refusal:?}"), None, None)
        }
        Err(error) => agent_cancel_failure("unavailable", &format!("{error:?}"), None, None),
    }
}

#[cfg(unix)]
fn agent_cancel_failure(
    status: &str,
    message: &str,
    required_confirmation: Option<&str>,
    actual: Option<&daimon_protocol::agent::AgentSessionViewV1>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "version": 1,
            "peer": "daimon",
            "capability": daimon_agent_command::CANCEL_CAPABILITY,
            "complete": false,
            "status": status,
            "required_confirmation": required_confirmation,
            "actual_revision": actual.map(|view| view.revision),
            "actual_phase": actual.map(|view| format!("{:?}", view.phase)),
            "actual_active_turn": actual.and_then(|view| view.active_turn).map(|_| "present"),
        }))?
    );
    Err(std::io::Error::other(message).into())
}

#[cfg(unix)]
fn agent_close(
    mut args: impl Iterator<Item = std::ffi::OsString>,
) -> Result<(), Box<dyn std::error::Error>> {
    use daimon_agent_command::{CloseError, CloseRequest};
    use daimon_client::unix::AgentClientErrorV1;
    use daimon_protocol::agent::AgentOutcomeV1;
    use std::{path::PathBuf, time::Duration};

    let socket = args.next().map(PathBuf::from).ok_or_else(usage)?;
    let session = args
        .next()
        .and_then(|value| daimon_tui::parse_agent_session(&value.to_string_lossy()))
        .ok_or("agent session must be 32 hexadecimal digits")?;
    let expected_revision = parse_u64(args.next())?;
    let idempotency_key = args
        .next()
        .and_then(|value| daimon_tui::parse_idempotency_key(&value.to_string_lossy()))
        .ok_or("idempotency key must be 32 hexadecimal digits")?;
    let now = parse_u64(args.next())?;
    let confirmation = args
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or_else(usage)?;
    if args.next().is_some() {
        return Err(usage().into());
    }
    let (service_session, port) = match daimon_client::unix::connect_agent(
        socket,
        daimon_wire::LimitsV1::default(),
        daimon_wire::agent::AgentWireLimitsV1::default(),
        Duration::from_secs(5),
    ) {
        Ok(connection) => connection,
        Err(error) => return agent_close_failure("not_sent", &format!("{error:?}"), None, None),
    };
    let request = CloseRequest {
        session,
        expected_revision,
        idempotency_key,
        now,
    };
    match daimon_agent_command::close(service_session, port, request, &confirmation) {
        Ok(AgentOutcomeV1::Session(view)) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "version": 1,
                    "peer": "daimon",
                    "capability": daimon_agent_command::CLOSE_CAPABILITY,
                    "complete": true,
                    "status": "closed",
                    "revision": view.revision,
                    "phase": format!("{:?}", view.phase),
                    "active_turn": view.active_turn.map(|_| "present"),
                    "last_event_cursor": view.last_event_cursor,
                }))?
            );
            Ok(())
        }
        Ok(AgentOutcomeV1::OutcomeUnknown { .. }) => agent_close_failure(
            "outcome_unknown",
            "Daimon retained an indeterminate close outcome; inspect before any further action",
            None,
            None,
        ),
        Ok(AgentOutcomeV1::Events(_)) => {
            agent_close_failure("invalid_reply", "unexpected Daimon close reply", None, None)
        }
        Err(CloseError::ConfirmationMismatch { required }) => agent_close_failure(
            "confirmation_refused",
            "exact Daimon close confirmation required",
            Some(&required),
            None,
        ),
        Err(CloseError::SessionChanged { actual }) => agent_close_failure(
            "stale_session",
            "Daimon session must remain at the exact ready revision with no active turn",
            None,
            Some(&actual),
        ),
        Err(CloseError::MutationTransport(AgentClientErrorV1::OutcomeUnknown(error))) => {
            agent_close_failure("outcome_unknown", &format!("{error:?}"), None, None)
        }
        Err(CloseError::ObservationTransport(AgentClientErrorV1::NotSent(error)))
        | Err(CloseError::MutationTransport(AgentClientErrorV1::NotSent(error))) => {
            agent_close_failure("not_sent", &format!("{error:?}"), None, None)
        }
        Err(CloseError::ObservationRefused(refusal))
        | Err(CloseError::MutationRefused(refusal)) => {
            agent_close_failure("refused", &format!("{refusal:?}"), None, None)
        }
        Err(error) => agent_close_failure("unavailable", &format!("{error:?}"), None, None),
    }
}

#[cfg(unix)]
fn agent_close_failure(
    status: &str,
    message: &str,
    required_confirmation: Option<&str>,
    actual: Option<&daimon_protocol::agent::AgentSessionViewV1>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "version": 1,
            "peer": "daimon",
            "capability": daimon_agent_command::CLOSE_CAPABILITY,
            "complete": false,
            "status": status,
            "required_confirmation": required_confirmation,
            "actual_revision": actual.map(|view| view.revision),
            "actual_phase": actual.map(|view| format!("{:?}", view.phase)),
            "actual_active_turn": actual.and_then(|view| view.active_turn).map(|_| "present"),
        }))?
    );
    Err(std::io::Error::other(message).into())
}

#[cfg(unix)]
fn parse_u64(value: Option<std::ffi::OsString>) -> Result<u64, Box<dyn std::error::Error>> {
    value
        .and_then(|value| value.into_string().ok())
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| usage().into())
}

#[cfg(unix)]
fn agent_advance_failure(
    status: &str,
    message: &str,
    required_confirmation: Option<&str>,
    actual: Option<&daimon_protocol::agent::AgentSessionViewV1>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "version": 1,
            "peer": "daimon",
            "capability": daimon_agent_command::CAPABILITY,
            "complete": false,
            "status": status,
            "required_confirmation": required_confirmation,
            "actual_revision": actual.map(|view| view.revision),
            "actual_phase": actual.map(|view| format!("{:?}", view.phase)),
            "actual_active_turn": actual.and_then(|view| view.active_turn).map(|_| "present"),
        }))?
    );
    Err(std::io::Error::other(message).into())
}

#[cfg(unix)]
fn agent_prompt(
    mut args: impl Iterator<Item = std::ffi::OsString>,
) -> Result<(), Box<dyn std::error::Error>> {
    use daimon_agent_command::{PromptError, parse_prompt_document};
    use daimon_client::unix::AgentClientErrorV1;
    use daimon_protocol::agent::AgentOutcomeV1;
    use std::{path::PathBuf, time::Duration};

    let socket = args.next().map(PathBuf::from).ok_or_else(usage)?;
    let document_path = args.next().map(PathBuf::from).ok_or_else(usage)?;
    let prompt_path = args.next().map(PathBuf::from).ok_or_else(usage)?;
    let confirmation = args
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or_else(usage)?;
    if args.next().is_some() {
        return Err(usage().into());
    }
    let document = read_bounded(&document_path, 64 * 1024)?;
    let prompt_bytes = read_bounded(&prompt_path, daimon_agent_command::MAX_PROMPT_BYTES)?;
    let request = parse_prompt_document(&document, prompt_bytes)
        .map_err(|error| format!("Daimon prompt document refused: {error:?}"))?;
    let (service_session, port) = match daimon_client::unix::connect_agent(
        socket,
        daimon_wire::LimitsV1::default(),
        daimon_wire::agent::AgentWireLimitsV1::default(),
        Duration::from_secs(5),
    ) {
        Ok(connection) => connection,
        Err(error) => return agent_prompt_failure("not_sent", &format!("{error:?}"), None, None),
    };
    match daimon_agent_command::prompt(service_session, port, request, &confirmation) {
        Ok(AgentOutcomeV1::Session(view)) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "version": 1,
                    "peer": "daimon",
                    "capability": daimon_agent_command::PROMPT_CAPABILITY,
                    "complete": true,
                    "status": "dispatched",
                    "revision": view.revision,
                    "phase": format!("{:?}", view.phase),
                    "active_turn": view.active_turn.map(|_| "present"),
                    "last_event_cursor": view.last_event_cursor,
                }))?
            );
            Ok(())
        }
        Ok(AgentOutcomeV1::OutcomeUnknown { .. }) => agent_prompt_failure(
            "outcome_unknown",
            "Daimon retained an indeterminate prompt outcome; inspect before any further action",
            None,
            None,
        ),
        Ok(AgentOutcomeV1::Events(_)) => agent_prompt_failure(
            "invalid_reply",
            "unexpected Daimon prompt reply",
            None,
            None,
        ),
        Err(PromptError::ConfirmationMismatch { required }) => agent_prompt_failure(
            "confirmation_refused",
            "exact Daimon prompt confirmation required",
            Some(&required),
            None,
        ),
        Err(PromptError::SessionChanged { actual }) => agent_prompt_failure(
            "stale_session",
            "Daimon session revision, phase, or active turn changed",
            None,
            Some(&actual),
        ),
        Err(PromptError::MutationTransport(AgentClientErrorV1::OutcomeUnknown(error))) => {
            agent_prompt_failure("outcome_unknown", &format!("{error:?}"), None, None)
        }
        Err(PromptError::ObservationTransport(AgentClientErrorV1::NotSent(error)))
        | Err(PromptError::MutationTransport(AgentClientErrorV1::NotSent(error))) => {
            agent_prompt_failure("not_sent", &format!("{error:?}"), None, None)
        }
        Err(PromptError::ObservationRefused(refusal))
        | Err(PromptError::MutationRefused(refusal)) => {
            agent_prompt_failure("refused", &format!("{refusal:?}"), None, None)
        }
        Err(error) => agent_prompt_failure("unavailable", &format!("{error:?}"), None, None),
    }
}

#[cfg(unix)]
fn read_bounded(
    path: &std::path::Path,
    limit: usize,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    use std::io::Read;

    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(u64::try_from(limit)?.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(format!("{} exceeds {limit} bytes", path.display()).into());
    }
    Ok(bytes)
}

#[cfg(unix)]
fn agent_prompt_failure(
    status: &str,
    message: &str,
    required_confirmation: Option<&str>,
    actual: Option<&daimon_protocol::agent::AgentSessionViewV1>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "version": 1,
            "peer": "daimon",
            "capability": daimon_agent_command::PROMPT_CAPABILITY,
            "complete": false,
            "status": status,
            "required_confirmation": required_confirmation,
            "actual_revision": actual.map(|view| view.revision),
            "actual_phase": actual.map(|view| format!("{:?}", view.phase)),
            "actual_active_turn": actual.and_then(|view| view.active_turn).map(|_| "present"),
        }))?
    );
    Err(std::io::Error::other(message).into())
}

#[cfg(unix)]
fn agent_permission(
    mut args: impl Iterator<Item = std::ffi::OsString>,
) -> Result<(), Box<dyn std::error::Error>> {
    use daimon_agent_command::{PermissionError, parse_permission_document};
    use daimon_client::unix::AgentClientErrorV1;
    use daimon_protocol::agent::AgentOutcomeV1;
    use std::{path::PathBuf, time::Duration};

    let socket = args.next().map(PathBuf::from).ok_or_else(usage)?;
    let document_path = args.next().map(PathBuf::from).ok_or_else(usage)?;
    let confirmation = args
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or_else(usage)?;
    if args.next().is_some() {
        return Err(usage().into());
    }
    let document = read_bounded(&document_path, 64 * 1024)?;
    let request = parse_permission_document(&document)
        .map_err(|error| format!("Daimon permission document refused: {error:?}"))?;
    let (service_session, port) = match daimon_client::unix::connect_agent(
        socket,
        daimon_wire::LimitsV1::default(),
        daimon_wire::agent::AgentWireLimitsV1::default(),
        Duration::from_secs(5),
    ) {
        Ok(connection) => connection,
        Err(error) => {
            return agent_permission_failure("not_sent", &format!("{error:?}"), None, None);
        }
    };
    match daimon_agent_command::decide_permission(service_session, port, request, &confirmation) {
        Ok(AgentOutcomeV1::Session(view)) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "version": 1,
                    "peer": "daimon",
                    "capability": daimon_agent_command::PERMISSION_CAPABILITY,
                    "complete": true,
                    "status": "decided",
                    "revision": view.revision,
                    "phase": format!("{:?}", view.phase),
                    "active_turn": view.active_turn.map(|_| "present"),
                    "last_event_cursor": view.last_event_cursor,
                }))?
            );
            Ok(())
        }
        Ok(AgentOutcomeV1::OutcomeUnknown { .. }) => agent_permission_failure(
            "outcome_unknown",
            "Daimon retained an indeterminate permission outcome; inspect before any further action",
            None,
            None,
        ),
        Ok(AgentOutcomeV1::Events(_)) => agent_permission_failure(
            "invalid_reply",
            "unexpected Daimon permission reply",
            None,
            None,
        ),
        Err(PermissionError::ConfirmationMismatch { required }) => agent_permission_failure(
            "confirmation_refused",
            "exact Daimon permission confirmation required",
            Some(&required),
            None,
        ),
        Err(PermissionError::SessionChanged { actual }) => agent_permission_failure(
            "stale_session",
            "Daimon session revision, phase, or active turn changed",
            None,
            Some(&actual),
        ),
        Err(PermissionError::ChallengeChanged) => agent_permission_failure(
            "stale_challenge",
            "Daimon permission cursor, request, turn, or option changed",
            None,
            None,
        ),
        Err(PermissionError::MutationTransport(AgentClientErrorV1::OutcomeUnknown(error))) => {
            agent_permission_failure("outcome_unknown", &format!("{error:?}"), None, None)
        }
        Err(PermissionError::ObservationTransport(AgentClientErrorV1::NotSent(error)))
        | Err(PermissionError::MutationTransport(AgentClientErrorV1::NotSent(error))) => {
            agent_permission_failure("not_sent", &format!("{error:?}"), None, None)
        }
        Err(PermissionError::ObservationRefused(refusal))
        | Err(PermissionError::MutationRefused(refusal)) => {
            agent_permission_failure("refused", &format!("{refusal:?}"), None, None)
        }
        Err(error) => agent_permission_failure("unavailable", &format!("{error:?}"), None, None),
    }
}

#[cfg(unix)]
fn agent_permission_failure(
    status: &str,
    message: &str,
    required_confirmation: Option<&str>,
    actual: Option<&daimon_protocol::agent::AgentSessionViewV1>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "version": 1,
            "peer": "daimon",
            "capability": daimon_agent_command::PERMISSION_CAPABILITY,
            "complete": false,
            "status": status,
            "required_confirmation": required_confirmation,
            "actual_revision": actual.map(|view| view.revision),
            "actual_phase": actual.map(|view| format!("{:?}", view.phase)),
            "actual_active_turn": actual.and_then(|view| view.active_turn).map(|_| "present"),
        }))?
    );
    Err(std::io::Error::other(message).into())
}

#[cfg(unix)]
fn hex(bytes: &[u8]) -> String {
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut value, "{byte:02x}").expect("writing to String cannot fail");
    }
    value
}

#[cfg(unix)]
fn run_interactive(mut component: daimon_tui::Component) -> Result<(), Box<dyn std::error::Error>> {
    use crossterm::event::{self, Event};
    use std::time::Duration;

    ratatui::run(|terminal| -> Result<(), Box<dyn std::error::Error>> {
        loop {
            component.poll();
            terminal.draw(|frame| component.draw_in(frame, frame.area(), true))?;
            if !event::poll(Duration::from_millis(50))? {
                continue;
            }
            match event::read()? {
                Event::Resize(..) => {}
                Event::Key(key) => {
                    let Some(input) = daimon_tui::translate_key(key) else {
                        continue;
                    };
                    if component.update(input, true) == daimon_tui::Lifecycle::CloseRequested {
                        break;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    })
}

#[cfg(unix)]
fn run_headless(
    session: daimon_protocol::SessionHandleV1,
    port: daimon_client::unix::UnixPort,
) -> Result<(), Box<dyn std::error::Error>> {
    use daimon_client::ReadOnlyClient;
    use daimon_protocol::{OutcomeV1, ReplyV1};

    let mut client = ReadOnlyClient::new(session, port);
    match client.read_presence(64) {
        Ok(ReplyV1::Accepted(OutcomeV1::Presence { entries, truncated })) => {
            let manifest = daimon_tui::CAPABILITIES;
            let current = entries.iter().filter(|entry| !entry.stale).count();
            let stale = entries.len().saturating_sub(current);
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "version": 1,
                    "mode": "headless",
                    "complete": true,
                    "peer": manifest.peer,
                    "capability": manifest.headless[0],
                    "status": "current",
                    "agents": entries.len(),
                    "current": current,
                    "stale": stale,
                    "truncated": truncated,
                }))?
            );
            Ok(())
        }
        Ok(ReplyV1::Refused(refusal)) => {
            headless_failure("refused", &format!("Daimon refused: {refusal:?}"))
        }
        Ok(ReplyV1::Accepted(_)) => headless_failure("invalid_reply", "unexpected Daimon reply"),
        Err(error) => {
            use daimon_client::unix::ClientErrorV1;
            let status = match error {
                ClientErrorV1::NotSent(_) => "not_sent",
                ClientErrorV1::OutcomeUnknown(_) => "outcome_unknown",
                ClientErrorV1::Wire(_) | ClientErrorV1::SessionMismatch => "invalid_reply",
            };
            headless_failure(status, "Daimon presence read failed")
        }
    }
}

#[cfg(unix)]
fn headless_failure(status: &str, message: &str) -> Result<(), Box<dyn std::error::Error>> {
    headless_failure_for(daimon_tui::CAPABILITIES.headless[0], status, message)
}

#[cfg(unix)]
fn headless_failure_for(
    capability: &str,
    status: &str,
    message: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let manifest = daimon_tui::CAPABILITIES;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "version": 1,
            "mode": "headless",
            "complete": false,
            "peer": manifest.peer,
            "capability": capability,
            "status": status,
        }))?
    );
    Err(std::io::Error::other(message).into())
}

fn usage() -> &'static str {
    "usage: daimon-tui capabilities | daimon-tui <interactive|headless> SOCKET | daimon-tui <agent-inspect|agent-interactive> SOCKET AGENT_SESSION_HEX\ndaimon-tui agent-advance SOCKET AGENT_SESSION_HEX TURN_HEX EXPECTED_REVISION IDEMPOTENCY_KEY_HEX NOW 'ADVANCE DAIMON SESSION AGENT_SESSION_HEX TURN TURN_HEX REVISION EXPECTED_REVISION KEY IDEMPOTENCY_KEY_HEX AT NOW'\ndaimon-tui agent-cancel SOCKET AGENT_SESSION_HEX TURN_HEX EXPECTED_REVISION IDEMPOTENCY_KEY_HEX NOW 'CANCEL DAIMON SESSION AGENT_SESSION_HEX TURN TURN_HEX REVISION EXPECTED_REVISION KEY IDEMPOTENCY_KEY_HEX AT NOW'\ndaimon-tui agent-close SOCKET AGENT_SESSION_HEX EXPECTED_REVISION IDEMPOTENCY_KEY_HEX NOW 'CLOSE DAIMON SESSION AGENT_SESSION_HEX REVISION EXPECTED_REVISION KEY IDEMPOTENCY_KEY_HEX AT NOW'\ndaimon-tui agent-prompt SOCKET REQUEST_JSON PROMPT_FILE 'PROMPT DAIMON SESSION …'\ndaimon-tui agent-permission SOCKET REQUEST_JSON 'DECIDE DAIMON PERMISSION …'\nheadless commands emit bounded versioned observations without constructing TUI state; agent mutations observe exact current state and attempt once without retry"
}

#[cfg(not(unix))]
fn main() {
    eprintln!("daimon-tui's selected standalone client requires Unix");
    std::process::exit(1);
}
