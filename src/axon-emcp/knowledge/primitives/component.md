---
name: component
summary: "A compliance-bound UI component — the compile-time shield-coverage law over regulated κ IS enforced (a real set difference); rendering is deferred (v2.67.0)."
category: wire
top_level: true
since: v1.3.1
grammar: |
  component <Name> {
      # compliance-bound UI component; κ tags must be covered by its shield
  }
---

# `component`

`component` declares a **compliance-bound UI component**.

## What the runtime actually does — and does not (v2.67.0, honest scope)

- **Enforced**: the compile-time **shield-coverage law** over regulated
  compliance (κ) tags — a real set difference. A component carrying
  HIPAA-tagged data behind a shield that does not cover HIPAA is
  refused at compile time. This is the ONE genuine κ-coverage rule in
  the checker (v2.67.0 F16 verdict).
- **Deferred**: the component **renders nothing**. There is no UI
  renderer in the runtime; the README itself defers it.

The v2.67.0 classification is **Partial**, and this doc keeps that honest:
the compliance half is real, the rendering half does not exist yet.

## See also

- `axon://primitives/view` — the sibling declaration (same posture).
- `axon://primitives/shield` — what must cover the κ tags.
