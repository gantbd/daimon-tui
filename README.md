# Daimon TUI

Composable, read-only terminal presentation for Daimon's authenticated public
clients. The reusable library owns bounded agent-presence projection and exact
configured agent-session/event observation, local selection, input, and
lifecycle output. It owns no provider dispatch, execution authority, terminal
lifecycle, CLI parsing, daemon startup, or process exit.

The separate standalone CLI supports:

```text
daimon-tui capabilities
daimon-tui interactive /path/to/daimon.sock
daimon-tui headless /path/to/daimon.sock
daimon-tui agent-inspect /path/to/daimon-agent.sock AGENT_SESSION_HEX
daimon-tui agent-interactive /path/to/daimon-agent.sock AGENT_SESSION_HEX
daimon-tui agent-advance /path/to/daimon-agent.sock AGENT_SESSION_HEX TURN_HEX REVISION IDEMPOTENCY_KEY_HEX NOW 'ADVANCE DAIMON SESSION AGENT_SESSION_HEX TURN TURN_HEX REVISION REVISION KEY IDEMPOTENCY_KEY_HEX AT NOW'
daimon-tui agent-prompt /path/to/daimon-agent.sock request.json prompt.txt 'PROMPT DAIMON SESSION …'
daimon-tui agent-permission /path/to/daimon-agent.sock request.json 'DECIDE DAIMON PERMISSION …'
```

Headless mode uses `daimon_client::ReadOnlyClient` directly and creates no TUI
state. Agent identifiers remain undisclosed; the selected presence projection
contains only age and stale/current classification.

The exact agent-session commands use `AgentReadOnlyClient` over Daimon's
distinct canonical agent socket. They double-read the configured session around
one bounded contiguous event page and reject a mixed snapshot. The projection
includes phase, revision, provider-profile document digest, complete capability
bits, cursor evidence, and escaped event summaries. The opaque session remains
configuration input: no catalogue is exposed, and the component cannot open,
prompt, decide permission, advance, cancel, close, resume, or reattach a session.

The separate `agent-advance` application command is an explicit operator path,
not presentation authority. It reuses Daimon's owner-side guard to inspect the
exact configured session on the same authenticated connection, require the
current active turn and revision plus an exact confirmation, and attempt one
bounded provider slice with a stable idempotency key. It never retries an
uncertain delivery; the operator must inspect the retained session before any
further action. `tests/live/agent_advance.sh` proves a confirmation mismatch
causes no command and a real Unix-service success performs exactly one advance.

The separate `agent-prompt` application command reads a strict credential-free
execution document and bounded raw prompt file, then delegates to Daimon's
owner-side guard. The guard observes the exact ready session on the same
connection and attempts the prompt once. Its confirmation binds both the prompt
digest and a canonical digest of every execution-envelope field; prompt content
never enters argv or routine output. A lost reply is outcome-unknown and is
never redispatched. `tests/live/agent_prompt.sh` proves confirmation refusal
causes no provider dispatch and a real Unix-service success dispatches once.

The `agent-permission` command binds one exact cursor-identified provider
challenge, request, option, current revision, and separately authorized
execution envelope. Its confirmation includes digests of the complete challenge
and envelope without echoing provider text. A changed request, turn, option or
cursor cannot consume the retained challenge, and uncertain delivery is not
retried. `tests/live/agent_permission.sh` proves the real Unix path.
