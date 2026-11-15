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
    let manifest = daimon_tui::CAPABILITIES;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "version": 1,
            "mode": "headless",
            "complete": false,
            "peer": manifest.peer,
            "capability": manifest.headless[0],
            "status": status,
        }))?
    );
    Err(std::io::Error::other(message).into())
}

fn usage() -> &'static str {
    "usage: daimon-tui capabilities | daimon-tui <interactive|headless> SOCKET\nheadless emits one bounded versioned agents.presence.v1 observation without constructing TUI state"
}

#[cfg(not(unix))]
fn main() {
    eprintln!("daimon-tui's selected standalone client requires Unix");
    std::process::exit(1);
}
