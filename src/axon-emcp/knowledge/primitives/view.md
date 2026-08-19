---
name: view
summary: "A UI view declaration — referential integrity is checked; route dispatch and session-typed reactivity are deferred (v2.67.0)."
category: wire
top_level: true
since: v1.3.1
grammar: |
  view <Name> {
      # UI view declaration; references are integrity-checked
  }
---

# `view`

`view` declares a **UI view**.

## What the runtime actually does — and does not (v2.67.0, honest scope)

- **Enforced**: referential integrity — the names a view references
  must resolve to declared entities.
- **Deferred**: no `route` check, no session-typed-reactivity check,
  and it **renders nothing** (no renderer exists in the runtime).

The v2.67.0 classification is **Partial**. A view is today a checked
declaration awaiting its runtime — declared scope, not a hidden gap.

## See also

- `axon://primitives/component` — the sibling declaration.
- `axon://primitives/axonendpoint` — the wire surface that IS real.
