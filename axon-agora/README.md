# axon-agora

**The first official library of [axon-lang](https://github.com/VelitRicardo/axon-lang).** Governed
native connectors for **LinkedIn**, **Facebook Pages**, **Instagram**, and **TikTok**, so a
cognitive agent can act directly inside those networks — read comments and metrics, moderate and
reply, edit and publish — with zero human input at execution time, as one step inside a larger
multi-tool task.

> The zero-input problem is not technical — it is **governance**, and no existing ecosystem
> solves it at the language level. Every operation `axon-agora` targets already exists in the
> platforms' official APIs; what the platforms condition is **autonomy**. `axon-agora` does not
> invent governance — it inherits Axon's (secret custody, linear budgets, born-Untrusted reads,
> governed egress) and encodes each platform's autonomy conditions in the type system, so the
> compiler enforces them before the agent runs.

The name is the Greek **agora** — the public square where a citizen *speaks in public*. Publishing
is a delegated **performative speech act**; the agent speaks in the agora on its principal's
behalf.

## What this crate is

This crate is the **OSS protocol layer** (§Fase 116, decision D116.5): the single source of truth
that both the `agora.*` module surface and the axon-frontend governance laws
(`axon-T956`/`T957`/`T958`/`W018`) consume. It contains **no network I/O and no credentials** —
per-tenant token custody, the refresh daemon, webhook ingress, and audit sinks live in the
enterprise layer.

| Module | Governs | Backs |
|---|---|---|
| `scope` | which OAuth scope each operation requires | `axon-T956` (scope coverage) |
| `protocol` | the session-typed publish flows (order enforced by types) | `axon-T957` (protocol typestate) |
| `posture` | the owned-only refusals (what each platform forbids) | `axon-T958` (posture refusal) |
| `quota` | the consumable posting budgets | `axon-W018` (quota pressure) |
| `connector` | the uniform `SocialConnector` seam | the native cores (§116.c–f) |

Every platform fact encoded here is sourced to the research paper `paper_axon_agora.md` §II,
confirmed against primary platform documentation (2026-07).

## Status

🔬 §Fase 116, sub-fase **§116.a** (substrate). See `docs/fase/fase_116_axon_agora.md` for the
build order.

## License

AGPL-3.0-or-later.
