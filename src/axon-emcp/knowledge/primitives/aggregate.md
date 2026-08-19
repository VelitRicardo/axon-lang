---
name: aggregate
summary: "The dataspace verb (γ) over the first-party columnar engine (v2.63.0)."
category: data_plane
top_level: false
since: v2.63.0
grammar: |
  aggregate <Dataspace> ...
---

# `aggregate`

`aggregate` is one of the four **relational query verbs** over a declared
`dataspace` (v2.63.0), executed by the first-party columnar engine — no
LLM in the loop, deterministic by construction.

## What the runtime actually does

γ: grouped aggregation over declared columns.

## Proof

`dataspace_engine::aggregate_query` — the v2.67.0 audit verdict: Real.

## See also

- `axon://primitives/dataspace` — the container + its typed schema.
- `axon://primitives/ingest` — how data (governedly) gets in.
