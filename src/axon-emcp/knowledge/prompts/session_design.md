---
name: session_design
title: Design an AXON session
summary: Guided walkthrough — turn a dialogue intent (chat, RPC, multiparty coordination) into a v2.3.0 duality-correct `session` + `socket` pair, honouring linearity, credit-refined backpressure, and multiparty projection where relevant.
arguments:
  - name: intent
    description: One-sentence description of the dialogue (e.g. "turn-taking chat with cancellation", "request/response RPC with streaming reply", "client/server/auditor 3-party coordination").
    required: true
  - name: parties
    description: Number of parties — `2` for client/server dialogue (default), `3+` for multiparty (triggers the v2.3.0 projection rules).
    required: false
  - name: backpressure
    description: "Credit window for the streaming side. Recommended values: `1` (turn-taking), `4`–`8` (typical SSE-like streaming), `≥ 16` (bulk transfer). Omit for unbounded fragment."
    required: false
  - name: reconnect
    description: "Set to `yes` to enable typed reconnection via `cognitive_state` (v2.3.0). The session-type cursor + credit window will be sealed into an AAD-bound snapshot on disconnect."
    required: false
---

You are about to design an AXON **session** for the following dialogue:

> {{intent}}

Parties: **{{parties}}**. Credit window: **{{backpressure}}**.
Reconnect-on-disconnect: **{{reconnect}}**.

A `session` declares the *type of the connection*. A `socket` binds
that type to a WebSocket transport. The four v2.3.0 pillars apply
unconditionally:

1. **Duality** (v2.3.0) — the server role must be `client⊥`.
2. **Linearity** — every channel is used exactly once on every path.
3. **Credit-refined backpressure** (v2.3.0) — Presburger-decidable.
4. **Multiparty projection** (v2.3.0) — only for `parties ≥ 3`.

Follow this loop.

### 1. Read the references — they encode the math

Before writing one byte:

- `axon.primitive_doc("session")` — grammar + cursor semantics.
- `axon.primitive_doc("socket")` — carrier binding + closure codes.
- Read the resource `axon://logic/session_duality` — the algebra
  rules + practical agent recipes. Quote-back the four pillars to
  the user so they know the discipline you are applying.

### 2. Write the client role end-to-end

Always write the **client** side first; mechanically dualise to
get the server side. The compiler will catch any deviation.

Use the closed action vocabulary: `send T`, `receive T`,
`select { label: [...] }`, `branch { label: [...] }`, `loop`, `end`,
recursion variables (`μX.S`).

Examples by intent shape:

- **Turn-taking chat** (your `parties: 2` typical case):

  ```
  client: [ loop, select { ask: [send Utterance, branch {
                                   token: [receive Token, loop],
                                   done:  [end] }],
                            cancel: [end] } ]
  server: [ loop, branch { ask: [receive Utterance, select {
                                   token: [send Token, loop],
                                   done:  [end] }],
                            cancel: [end] } ]
  ```

- **Single request/response with streaming reply**:

  ```
  client: [ send Request, loop,
            branch { token: [receive Token, loop],
                     done:  [end] } ]
  server: [ receive Request, loop,
            select { token: [send Token, loop],
                     done:  [end] } ]
  ```

- **3-party coordination** (parties = 3) — declare the global type
  inside a multiparty `session` and let the v2.3.0 projector emit
  each role's local view automatically. See the
  `axon-frontend::multiparty::project_all` reference.

### 3. Pick the credit window

The value of **{{backpressure}}** governs the v2.3.0 discipline:

- `1` → strict turn-taking, ack-per-message. Safest.
- `4`–`8` → typical SSE-like LLM-streaming. Buffers bursts.
- `≥ 16` → bulk-transfer / pipelined patterns. Verify the
  recursive body's Δ = `#send − #recv ≤ 0` with the type
  checker (it discharges Presburger automatically).

A value of `0` is rejected by the type checker — there is no
typing rule for a send at zero credit.

### 4. Declare the socket

Bind the session via:

```axon
socket <Name>WS {
    protocol: <SessionName>
    backpressure: credit({{backpressure}})
    reconnect: cognitive_state    // include only if {{reconnect}} == yes
    legal_basis: <basis>          // include if the dialogue is regulated
}
```

If **{{reconnect}}** is `yes`, the runtime seals the residual
session-type cursor + live credit window into an AAD-bound
`cognitive_state` snapshot on disconnect. Resume restores both
pillars 1 + 3 at the exact same point.

If the bound session's server role is **single-polarity** (only
`send`/`select`/`loop`/`end`), the same socket ALSO speaks W3C
Server-Sent Events on the same path with `Accept:
text/event-stream` — the v2.3.0 SSE-as-fragment unification fires
automatically.

### 5. Validate

Call `axon.check({ source: "<your draft>" })`. The type-checker
discharges all four pillars; the diagnostics tell you which pillar
failed if any did. Common failure modes:

- "Session 'X' duality violation: …" — pillar 1; re-dualise.
- "BurstOverflow: send-burst of N but credit(n) cannot absorb" —
  pillar 3; raise `n` or restructure the body.
- "LoopUnsustainable: Δ = … > 0" — pillar 3; rebalance
  send/receive in the recursive body.
- "multiparty_projection_failed at role 'X' …" — pillar 4; the
  global type does not project cleanly onto role X.

Never declare the design finished until `axon.check` returns
`ok: true`.

### 6. Surface the deliverable

Quote the final `session` + `socket` pair as a fenced ```axon
block. Quote the `axon.check` verdict. State which v2.3.0 pillars
the design exercises (duality always; backpressure if a credit
window was declared; multiparty projection if `parties ≥ 3`;
reconnection if `cognitive_state` is wired). Point the user at
`axon://logic/session_duality` for the proofs.
