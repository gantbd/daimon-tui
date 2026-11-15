#![cfg(unix)]
#![forbid(unsafe_code)]

use std::{
    convert::Infallible,
    os::unix::net::UnixStream,
    path::PathBuf,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

use daimon_harness::Limits;
use daimon_host::{
    AttemptRefusalV1, AttemptValidator, AuthenticatedSessionV1, AuthorityGrantV1,
    AuthorityRefusalV1, ExecutionAuthorizer, Provider, ProviderOutcomeV1,
    presence::{AgentId, Disclosure, Roster},
};
use daimon_local_daemon::unix::{Authenticator, Daemon};
use daimon_protocol::ExecuteRequestV1;
use daimon_service::{Service, ServiceLimitsV1};
use daimon_wire::LimitsV1;

struct Authentication;
impl Authenticator for Authentication {
    type Error = Infallible;

    fn authenticate(&mut self, _: &UnixStream) -> Result<AuthenticatedSessionV1, Self::Error> {
        Ok(AuthenticatedSessionV1 {
            principal: [4; 16],
            authority_context: [9; 16],
        })
    }
}

struct Authorizer;
impl ExecutionAuthorizer for Authorizer {
    fn authorize(
        &mut self,
        _: &AuthenticatedSessionV1,
        request: &ExecuteRequestV1,
        _: u64,
    ) -> Result<AuthorityGrantV1, AuthorityRefusalV1> {
        Ok(AuthorityGrantV1 {
            decision_id: [8; 16],
            expires_at: u64::MAX,
            tools: request.tools.clone(),
        })
    }
}

struct Attempts;
impl AttemptValidator for Attempts {
    fn validate(
        &mut self,
        _: &AuthenticatedSessionV1,
        _: &ExecuteRequestV1,
    ) -> Result<(), AttemptRefusalV1> {
        Ok(())
    }
}

struct UnusedProvider;
impl Provider for UnusedProvider {
    fn execute(&mut self, _: &ExecuteRequestV1) -> ProviderOutcomeV1 {
        panic!("read-only presence qualification must not dispatch a provider")
    }
}

struct AllowPresence;
impl Disclosure for AllowPresence {
    fn allows_roster(&mut self, _: &AuthenticatedSessionV1) -> bool {
        true
    }

    fn allows_agent(&mut self, _: &AuthenticatedSessionV1, _: AgentId) -> bool {
        true
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1).map(PathBuf::from);
    let store_path = args.next().ok_or("store path required")?;
    let socket_path = args.next().ok_or("socket path required")?;
    if args.next().is_some() {
        return Err("unexpected argument".into());
    }
    let store_limits = daimon_checkpoint::StoreLimitsV1 {
        max_bytes: 16_384,
        max_executions: 4,
        max_context_references: 4,
        max_tools: 4,
        max_artifacts: 4,
    };
    let store = daimon_local::create_new(store_path, store_limits)
        .map_err(|error| std::io::Error::other(format!("store failed: {error:?}")))?;
    let service = Service::open(
        Limits::default(),
        ServiceLimitsV1 {
            max_sessions: 4,
            max_executions: 4,
        },
        Authorizer,
        Attempts,
        UnusedProvider,
        store,
    )
    .map_err(|error| std::io::Error::other(format!("service failed: {error:?}")))?;
    let roster = Arc::new(RwLock::new(Roster::new(4, Duration::from_secs(30))));
    roster
        .write()
        .map_err(|_| "roster lock")?
        .replace([([7; 16], Instant::now())]);
    let mut daemon = Daemon::bind_new(
        socket_path,
        service,
        Authentication,
        LimitsV1::default(),
        Duration::from_secs(5),
        8,
    )?;
    daemon.bind_presence(roster, AllowPresence);
    daemon
        .serve_next()
        .map_err(|error| std::io::Error::other(format!("daemon failed: {error:?}")))?;
    Ok(())
}
