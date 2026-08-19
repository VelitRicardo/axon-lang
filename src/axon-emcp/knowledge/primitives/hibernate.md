---
name: hibernate
summary: "Suspend-until — ADVERTISED BUT NOT IMPLEMENTED (v2.67.0 F20, KNOWN_DEBT): returns a placeholder synchronously; no CPS suspend, no resume, timeout not honored."
category: cognition
top_level: false
since: pre-v2.67.0 (introduction unrecorded)
grammar: |
  hibernate <duration>
---

# `hibernate`

`hibernate` is the suspend-until verb — **and it is NOT implemented**.

## The honest state (v2.67.0 F20 — KNOWN_DEBT)

The runtime returns `"(hibernating ...)"` **synchronously**: no CPS
suspension happens, nothing resumes, the timeout is not honored. This
is a ledger entry in `KNOWN_DEBT` (the compiler-held ratchet): the
promise is either implemented or retracted in a future cycle — it cannot
rot silently, and this doc will not pretend otherwise.

Do not build on `hibernate` today.

## See also

- `axon://primitives/daemon` — for real long-lived processes.
- `axon://primitives/window` — for real temporal gating.
