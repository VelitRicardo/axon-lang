---
name: ingest
summary: "Governed ingestion into a dataspace — bounds-BEFORE-parse, sha256 provenance, born-Untrusted taint (v2.63.0; the pre-v2.63.0 placeholder hallucinated success)."
category: data_plane
top_level: false
since: "pre-v2.63.0; made real v2.63.0"
grammar: |
  ingest <source> into <Dataspace>
---

# `ingest`

`ingest` loads external data into a declared `dataspace`.

## What the runtime actually does (v2.63.0)

- **Bounds BEFORE parse** — size/row ceilings are checked before any
  byte is interpreted (the v2.54.0 discipline).
- **sha256 provenance** — the artifact records what was ingested.
- **Born Untrusted** — ingested data carries the Untrusted taint
  (axon-T908); cognition must launder it through the declared gates.

The pre-v2.63.0 placeholder *hallucinated success* — it reported ingestion
that never happened. That finding is the mother of v2.67.0.

## Proof

`cognitive::run_ingest` (v2.63.0) + `dataspace_deploy.rs`.

## See also

- `axon://primitives/dataspace` — the destination.
- `axon://primitives/focus` · `associate` · `aggregate` · `explore`.
