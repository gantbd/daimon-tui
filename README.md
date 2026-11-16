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
