# Daimon TUI

Composable, read-only terminal presentation for Daimon's authenticated public
client. The reusable library owns bounded agent-presence projection, local
selection, input, and lifecycle output. It owns no provider dispatch, execution
authority, terminal lifecycle, CLI parsing, daemon startup, or process exit.

The separate standalone CLI supports:

```text
daimon-tui capabilities
daimon-tui interactive /path/to/daimon.sock
daimon-tui headless /path/to/daimon.sock
```

Headless mode uses `daimon_client::ReadOnlyClient` directly and creates no TUI
state. Agent identifiers remain undisclosed; the selected presence projection
contains only age and stale/current classification.
