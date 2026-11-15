# Daimon terminal roadmap

## Current slice

- Reusable read-only Ratatui component over Daimon's public client.
- Separate standalone interactive/headless CLI.
- Peer-owned `agents.presence.v1` capability manifest.
- Redacted bounded presence with explicit refusal and transport uncertainty.
- No execution, resume, provider, tool, or supervisor authority in presentation.

## Delivery order

1. Qualify standalone modes against Daimon's real private daemon and roster.
2. Compose the peer into Workbench interactive and headless attachment.
3. Add exact-ID execution inspection and provider evidence without creating a
   global execution catalogue that Daimon does not expose.
4. Add dispatch/resume only with confirmation, current authority, stable
   idempotency identity, durable reconciliation, and no-redispatch evidence.
5. Add native Workbench hosting only after deployment-owned provider and
   credential composition is explicit.
