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
daimon-tui agent-cancel /path/to/daimon-agent.sock AGENT_SESSION_HEX TURN_HEX REVISION IDEMPOTENCY_KEY_HEX NOW 'CANCEL DAIMON SESSION AGENT_SESSION_HEX TURN TURN_HEX REVISION REVISION KEY IDEMPOTENCY_KEY_HEX AT NOW'
daimon-tui agent-close /path/to/daimon-agent.sock AGENT_SESSION_HEX REVISION IDEMPOTENCY_KEY_HEX NOW 'CLOSE DAIMON SESSION AGENT_SESSION_HEX REVISION REVISION KEY IDEMPOTENCY_KEY_HEX AT NOW'
daimon-tui agent-reattach /path/to/daimon-agent.sock request.json 'REATTACH DAIMON SESSION …'
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

The separate `agent-cancel` application command uses the same owner-command
boundary for authority attenuation. It requires the exact active turn and
revision in either turn-active or permission-waiting state, exact confirmation,
and one stable idempotency key. It attempts cancellation once and preserves an
uncertain reply for later inspection. `tests/live/agent_cancel.sh` proves a
confirmation mismatch cannot cancel and the real Unix path mutates once.

The `agent-close` command separately fences an exact Ready revision with no
active turn, preserving Daimon's requirement to cancel active work first. Its
confirmation binds the session, revision, idempotency identity and time; the
close is attempted once and uncertain delivery requires later inspection.
`tests/live/agent_close.sh` proves confirmation refusal and one real mutation.

The `agent-reattach` command reads a strict credential-free execution document,
binds its complete envelope to an exact confirmation, and accepts only the
exact resume-capable `ReattachRequired` revision with no active turn. It makes
one public-client mutation without replaying history, submitting a prompt, or
retrying uncertain delivery. `tests/live/agent_reattach.sh` proves confirmation
refusal and one real reattach.

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
