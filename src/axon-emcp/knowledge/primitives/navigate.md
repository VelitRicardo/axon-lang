---
name: navigate
summary: "Knowledge navigation — three REAL deterministic engines (MDN store-sourced, MDN in-memory, PIX); with no indexable source in scope it falls back to an LLM prompt (v2.67.0 F11, the named gap)."
category: data_plane
top_level: false
since: v2.12.0–v2.15.0 (PIX·MDN program)
grammar: |
  navigate <source> [where "<filter>"]
---

# `navigate`

`navigate` walks an indexable knowledge source deterministically.

## What the runtime actually does — and the named gap

THREE real deterministic engines: MDN store-sourced, MDN in-memory,
and PIX (conditional-mutual-information descent, embeddings-free).

⚠️ **v2.67.0 F11 (the live gap, named so it cannot rot):** with NO
indexable source in scope, `navigate` falls back to an LLM prompt that
*instructs the model to fabricate a provenance trail*. A trail from the
fallback is confabulation wearing an audit's clothes. Keep a `corpus`,
`pix` or MDN source in scope; treat sourceless navigation output as
LLM prose.

## See also

- `axon://primitives/drill` · `axon://primitives/trail` — the
  descent + breadcrumb verbs (they inherit F11).
- `axon://primitives/pix` · `axon://primitives/corpus` — real sources.
