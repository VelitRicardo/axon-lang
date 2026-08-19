---
name: associate
summary: "The dataspace join verb (⋈) over the first-party columnar engine (v2.63.0)."
category: data_plane
top_level: false
since: v2.63.0
grammar: |
  associate <Dataspace> ...
---

# `associate`

`associate` is one of the four **relational query verbs** over a declared
`dataspace` (v2.63.0), executed by the first-party columnar engine — no
LLM in the loop, deterministic by construction.

## What the runtime actually does

⋈: a hash equi-join over two dataspaces — and it REFUSES a keyless join (a cross product nobody declared is a defect).

## Proof

`dataspace_engine::associate_query` — the v2.67.0 audit verdict: Real.

## See also

- `axon://primitives/dataspace` — the container + its typed schema.
- `axon://primitives/ingest` — how data (governedly) gets in.
