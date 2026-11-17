# Daimon TUI

Status: published through the qualified GitHub source channel at the exact
revision and CI run recorded in
[`release/github-source.json`](release/github-source.json).

Composable terminal presentation and closed guarded controls for Daimon's
authenticated public clients. The reusable library owns bounded agent-presence
projection, exact configured agent-session/event observation, local selection,
input, lifecycle output, and exact prompt/permission/reattach/advance/cancel/close proposal sheets.
The session-set component accepts 1–16 unique exact session identities, provides
sparse selection with focused-item fallback, and observes a requested set over
one bounded connection that is released before control. The batch component
settles every child independently, retains each owner-returned session
projection, and stops with later children visibly NotSent after an unknown or
failed outcome. It owns
no provider policy, execution authority, terminal lifecycle, CLI parsing,
daemon startup, or process exit.

Each session row and evidence pane derives phase-aware work candidates from the
owner projection: prompt/close, advance/cancel, permission decision/cancel,
reattach, inspect, or no further work. The UI labels these as candidates with
authority still required; it never treats a phase hint as permission to act.

Outcome-unknown batches retain the exact sent-session set. A host can request
one bounded read-only recovery pass for those sessions, mark the evidence as
inspected, and then permit dismissal without retrying or restyling the original
mutation as successful.

The reusable guarded-control component accepts caller-generated execution,
operation identity, and time, displays the complete revision-bound proposal,
requires a distinct `c` arm step followed by Enter, and invokes Daimon's existing owner
guard exactly once. A mutation transport failure or outcome-unknown response
permanently fences the sheet; it never retries. The host must explicitly return
to the selected session observation for recovery.

Prompt runs use that same bounded 1–16 child ledger. Prompt bytes stay inside
the owner request, while the sheet presents only their digest and the digest of
the full credential-free execution envelope. Each selected session has an
independent turn, execution, and idempotency identity; a lost mutation reply
stops the run with later children visibly NotSent.

Permission composition is challenge-native rather than a generic approval
toggle. It presents each selected session's escaped capability, description,
and owner-published option list, defaults to `RejectOnce` when available, and
retains a distinct choice per session. The final exact sheet binds the current
revision, event cursor, request, option, full challenge digest, fresh execution
envelope, and idempotency key before the owner guard re-observes and submits.

Reattachment uses the same bounded child ledger. Every selected detached
session receives a fresh caller-created execution envelope, an exact revision
guard, and an independent owner receipt; failed or outcome-unknown children
leave later requests visibly NotSent rather than guessing or retrying.

The production host should use the connector-based session-set constructor.
Instrumentation tests prove a 16-session refresh has at most one simultaneous
owner connection and releases it between refreshes. This matters because the
canonical local daemon services bounded connections serially; the N-session UI
therefore does not consume O(N) connections or starve guarded controls.

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
the recent bounded contiguous 64-event tail and reject a mixed snapshot. The projection
includes phase, revision, provider-profile document digest, complete capability
bits, cursor evidence, and escaped event summaries. The opaque session remains
configuration input: no catalogue is exposed. The read-only agent component
itself cannot mutate a session; guarded actions live in the separate
exact-proposal component.

The bounded session-set component can cycle the already-observed event windows
through all, conversation, action, decision, and usage projections. It reports
exact configured/current/unavailable/marked cardinality and totals input/output
usage across the current owner-published windows. Filtering never changes what
is read, retained, or authorized and does not infer provider costs or policy.

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
