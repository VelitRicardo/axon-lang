---
name: shield_design
title: Design an AXON shield
summary: Guided walkthrough — turn a shield purpose (PHI redaction, jailbreak defence, financial scrubbing) into a typed `shield` declaration with the right scan list, on_breach policy, and compliance tags.
arguments:
  - name: purpose
    description: What the shield protects against (e.g. "PHI exfiltration on healthcare endpoints", "prompt-injection on a public chat", "PAN leakage on a payment-decision endpoint").
    required: true
  - name: severity
    description: One of `low` | `medium` | `high` | `critical`. Drives the on_breach policy. Defaults to `high` when omitted.
    required: false
  - name: compliance
    description: Comma-separated compliance frameworks the shield must satisfy (HIPAA, GDPR, PCI_DSS, SOC2, GxP, FISMA, NIST_800_53).
    required: false
---

You are about to design an AXON **shield** for the following purpose:

> {{purpose}}

Severity target: **{{severity}}**. Compliance frameworks the shield
must satisfy: **{{compliance}}**.

Follow this loop. A shield is a *transform* (it mutates the
candidate); never confuse it with an `anchor` (which is a
*predicate* that decides whether the candidate is accepted).

### 1. Read the primitive reference

Call `axon.primitive_doc("shield")` to read the grammar + semantic
constraints. Note specifically:

- The closed `scan:` catalogue —
  `prompt_injection | pii_leak | data_exfil | model_theft |
   social_engineering | hallucination | toxicity | jailbreak |
   bias | code_injection | training_poisoning`.
- The closed `on_breach:` catalogue —
  `deflect | escalate | halt | quarantine | sanitize_and_retry`.
- The closed `severity:` catalogue — `low | medium | high | critical`.
- The closed `strategy:` catalogue —
  `canary | classifier | dual_llm | ensemble | pattern | perplexity`.

### 2. Match purpose → scan vocabulary

The purpose **{{purpose}}** maps to ≥ 1 entries from the `scan:`
catalogue. Be precise:

- "PHI exfiltration" → `[pii_leak, data_exfil]`.
- "Prompt-injection" → `[prompt_injection, jailbreak]`.
- "PAN leakage" → `[pii_leak, data_exfil]` + a `pattern` strategy
  over the PAN regex catalogue.
- "Hallucination on a Q&A endpoint" → `[hallucination]`.
- "Model theft / extraction" → `[model_theft, data_exfil]`.

Combine entries when the purpose has multiple facets. The shield
**aggregates** scans — every check runs in parallel.

### 3. Pick the right on_breach policy

The severity **{{severity}}** drives the choice:

| Severity | Recommended on_breach | When to deviate |
|---|---|---|
| `critical` | `halt` or `quarantine` | Use `sanitize_and_retry` only if you can prove the sanitisation is sound. |
| `high` | `quarantine` | `escalate` when you have a defined SOC role. |
| `medium` | `sanitize_and_retry` | `deflect` for stylistic / soft policies. |
| `low` | `deflect` | Rarely worth a shield at all. |

### 4. Declare the shield

Read a working canonical example by quoting one of the embedded
templates:

- `axon.compose({ intent: "PHI redaction", domain: "healthcare" })`
  shows a HIPAA-tagged shield wired to an endpoint.
- `axon.compose({ intent: "credit card payment scrubbing", domain: "banking" })`
  shows a PCI_DSS-tagged shield with the financial vocabulary.

Use these as priors. Adapt the `scan:`, `on_breach:`, and
`severity:` fields to your concrete need; layer the `compliance:`
tags from **{{compliance}}** plus the domain's defaults.

### 5. Tie the shield to a transport

A declared shield does nothing until a wire surface binds it. The
canonical binding sites:

- `axonendpoint { ... shield: <ShieldName> ... }` — HTTP gate.
- `socket { ... shield: <ShieldName> ... }` — WebSocket gate (v2.3.0).
- `axonstore { ... shield: <ShieldName> ... }` — data-plane gate.

Bind the shield to **every** surface that emits or receives the
data the shield is meant to protect. Forgetting a surface is the
most common shield-design bug.

### 6. Validate

Call `axon.check({ source: "<your current draft>" })`. The
type-checker enforces that:

- `scan:` and `on_breach:` values are in the closed catalogue.
- The shield is referenced from at least one transport (a
  declared-but-unused shield gets a `axon-W005` warning).
- Compliance tags on the shield align with the tags on every bound
  transport (a HIPAA-tagged endpoint with a non-HIPAA shield is
  rejected by the v2.0.0 cross-tag check).

Never declare the shield finished until the verdict is `ok: true`.

### 7. Surface the deliverable

Quote the shield declaration + its binding sites, plus the
`axon.check` verdict. For each framework in **{{compliance}}**,
quote one line from `axon://compliance/<framework>` explaining
which control(s) the shield + transport pair attests.
