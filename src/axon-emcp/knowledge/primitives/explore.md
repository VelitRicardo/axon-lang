---
name: explore
summary: "The dataspace profiling verb — zone-map statistics over declared columns (v2.63.0)."
category: data_plane
top_level: false
since: v2.63.0
grammar: |
  explore <Dataspace> ...
---

# `explore`

`explore` is one of the four **relational query verbs** over a declared
`dataspace` (v2.63.0), executed by the first-party columnar engine — no
LLM in the loop, deterministic by construction.

## What the runtime actually does

zone-map statistics: profiling: per-column stats from the columnar engine's zone maps.

## Proof

`dataspace_engine::explore_profile` — the v2.67.0 audit verdict: Real.

## See also

- `axon://primitives/dataspace` — the container + its typed schema.
- `axon://primitives/ingest` — how data (governedly) gets in.
