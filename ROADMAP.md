# Daimon terminal roadmap

## Current slice

- Reusable read-only Ratatui component over Daimon's public client.
- Separate standalone interactive/headless CLI.
- Peer-owned `agents.presence.v1` capability manifest.
- Redacted bounded presence with explicit refusal and transport uncertainty.
- Exact configured `agents.session.inspect.v1` session/event observation over
  the public agent client, without a session catalogue.
- No execution, resume, provider, tool, permission, reattach, or supervisor
  authority in presentation.

## Delivery order

1. Qualify standalone modes against Daimon's real private daemon and roster.
2. Compose the peer into Workbench interactive and headless attachment.
3. Exact configured agent-session inspection and provider evidence are now
   qualified standalone without creating a global session catalogue. Compose
   the same component into Workbench next; legacy/model exact execution status
   remains a separate selected-ID slice.
4. Add dispatch/resume only with confirmation, current authority, stable
   idempotency identity, durable reconciliation, and no-redispatch evidence.
5. Add native Workbench hosting only after deployment-owned provider and
   credential composition is explicit.
