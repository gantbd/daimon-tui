# Daimon terminal roadmap

## Current slice

- Reusable read-only Ratatui component over Daimon's public client.
- Separate standalone interactive/headless CLI.
- Peer-owned `agents.presence.v1` capability manifest.
- Redacted bounded presence with explicit refusal and transport uncertainty.
- Exact configured `agents.session.inspect.v1` session/event observation over
  the public agent client, without a session catalogue.
- Separate exact-confirmed `agents.session.advance.v1` operator command over the
  same public client. It observes current state, attempts once and preserves
  outcome uncertainty; the reusable presentation remains read-only.
- Separate exact-confirmed `agents.session.cancel.v1` operator command fences
  the current active turn in both ordinary active and permission-waiting phases,
  attempts once, and preserves uncertain delivery without retry.
- Separate exact-confirmed `agents.session.prompt.v1` operator command over the
  same public client. It binds bounded prompt content plus the complete frozen
  execution envelope by digest, fences the ready revision and never redispatches
  transport uncertainty.
- Separate exact-confirmed `agents.session.permission.v1` command binds one
  cursor-identified provider challenge and the separately authorized execution
  envelope before attempting one decision.
- No execution, resume, provider, tool, permission, reattach, or supervisor
  authority in presentation.

## Delivery order

1. Qualify standalone modes against Daimon's real private daemon and roster.
2. Compose the peer into Workbench interactive and headless attachment.
3. Exact configured agent-session inspection and provider evidence are now
   qualified standalone without creating a global session catalogue. Compose
   the same component into Workbench next; legacy/model exact execution status
   remains a separate selected-ID slice.
4. Bounded active-turn advance, cancellation and prompt dispatch now have confirmation,
   current-state fencing, stable idempotency identity and no-retry evidence. Add
   close or reattach only with their own authority and durable
   uncertainty/reconciliation evidence.
5. Add native Workbench hosting only after deployment-owned provider and
   credential composition is explicit.
