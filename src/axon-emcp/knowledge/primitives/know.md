---
name: know
summary: "Epistemic scope — stamps its block's derivations with the `know` level of the uncertainty lattice."
category: cognition
top_level: true
since: v0.1.0 (epistemic lattice)
grammar: |
  know {
      # declarations / flow steps whose derivations carry the `know` level
  }
---

# `know`

`know` is one of the four **epistemic scopes** (`know` > `believe` >
`speculate` > `doubt`): a block form that stamps every derivation
inside it with its level of the uncertainty lattice.

## Semantics

the STRONGEST claim — reserved for verified, source-grounded derivations.

The lattice is load-bearing at the EGRESS boundaries: `document` (v2.53.0)
and `deliver` (v2.60.0) read the level to decide whether content may leave
as an assertion, must carry `attribute:`, or is refused — the
assertion-laundering barrier.

## See also

- `axon://primitives/document` — where the lattice gates egress.
- The other scopes: `know` · `believe` · `speculate` · `doubt`.
