---
name: discover
summary: The dual of publish — imports a previously published channel capability under a local alias.
category: wire
top_level: false
since: v1.6.0
grammar: |
  # Flow-body form (dual of Publish-Ext):
  discover <ChannelRef> as <alias>
---

# `discover`

`discover` is **the dual of `publish`**: it imports a previously
extruded channel capability, binding the discovered handle under
a local alias. Only shield-gated channels can be discovered — a
channel with no `shield:` is not publishable, so there is
nothing to import (D8: capability extrusion is shield-mediated).

## Surface

`discover` is **nested** — a flow-body / daemon-body statement.

```axon
flow Consume() -> Unit {
    discover SkillCompleted as live
}
```

The alias binds the shield reference of the publication in the
flow's context; an undiscovered capability resolves empty (the
flow observes the absence rather than crashing — totality).

## Typing

- The channel reference must be a declared `channel`.
- The channel's definition must declare a `shield:` (only
  shield-gated channels are publishable/discoverable).
- `as <alias>` is required — an unbound discovery is useless.

## What this primitive is NOT

- **Not a subscription.** Consuming events is `listen`'s job;
  discover imports the CAPABILITY (the right to interact with
  the gated channel).
- **Not cross-process.** Discovery resolves against the same
  runtime's capability context.

## See also

- `axon://primitives/publish` — the extrusion this imports.
- `axon://primitives/channel` — the conduit.
- `axon://primitives/listen` — the event consumer.
