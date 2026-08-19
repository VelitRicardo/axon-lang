---
name: drill
summary: "Subtree descent under a prior navigate — real navigation when a source is in scope; degrades to a placeholder otherwise (v2.67.0)."
category: data_plane
top_level: false
since: v2.12.0–v2.15.0 (PIX·MDN program)
grammar: |
  drill <target>
---

# `drill`

`drill` descends into a subtree surfaced by a prior `navigate`.

## Honest scope (v2.67.0)

Real subtree navigation **when a source is in scope**; degrades to a
placeholder string otherwise. Same discipline as `navigate`: the
deterministic engines are real, the sourceless path is not evidence.

## See also

- `axon://primitives/navigate` — the entry verb + the F11 warning.
