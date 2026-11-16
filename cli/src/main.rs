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
    "usage: daimon-tui capabilities | daimon-tui <interactive|headless> SOCKET | daimon-tui <agent-inspect|agent-interactive> SOCKET AGENT_SESSION_HEX\nheadless commands emit bounded versioned observations without constructing TUI state"
}

#[cfg(not(unix))]
fn main() {
    eprintln!("daimon-tui's selected standalone client requires Unix");
    std::process::exit(1);
}
