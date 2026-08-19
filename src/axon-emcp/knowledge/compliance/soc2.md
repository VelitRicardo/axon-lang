---
name: soc2
title: SOC 2 — Service Organization Control 2 (AICPA TSC 2017)
summary: AXON annotations covering AICPA's five SOC 2 Trust Services Criteria (Security, Availability, Processing Integrity, Confidentiality, Privacy) — what is statically proven vs. runtime-attested vs. auditor-judged.
---

# SOC 2 — Service Organization Control 2

**Scope:** service organizations that hold or process customer
data. Issued by AICPA; the criteria are the Trust Services Criteria
(TSC) 2017 with 2022 Points of Focus update.

SOC 2 is the most common AXON compliance tag — most adopter
deployments carry `compliance: [SOC2]` as a baseline, layered with
HIPAA / PCI_DSS / SOX / GDPR for the regulated subsystems.

## The five TSC

| TSC | Code | Mandatory? | Scope |
|---|---|:---:|---|
| Security | CC | ✓ | All engagements — Common Criteria CC1–CC9 |
| Availability | A | optional | Uptime, capacity, resilience |
| Processing Integrity | PI | optional | Data is complete + valid + accurate |
| Confidentiality | C | optional | Information designated confidential is protected |
| Privacy | P | optional | Personal information per AICPA privacy principles |

The Common Criteria (CC) are **always** included. An engagement
opts into A / PI / C / P based on what the service offers.

## Declaring SOC2

```axon
type CustomerProfile compliance [SOC2] {
    email: String,
    company: String,
    plan_tier: PlanTier
}

axonstore Customers
    compliance: [SOC2]
    backend: postgresql
    isolation: serializable
    encryption: at_rest
    retention: 7y

axonendpoint UpdateCustomer {
    flow: UpdateCustomerFlow
    method: PUT
    route: "/v1/customers/{id}"
    compliance: [SOC2]
}
```

## What the compiler enforces statically

| TSC | AXON enforcement |
|---|---|
| CC2.1 — communication of policies | Out of scope (process). |
| CC5.1–5.3 — control activities | A SOC2-tagged `axonendpoint` requires an `auth:` field; the type checker rejects SOC2-tagged endpoints with `auth: anonymous`. |
| CC6.1 — logical access | A SOC2-tagged `axonstore` requires an `isolation:` setting (`read_committed` / `repeatable_read` / `serializable`). |
| CC6.6 — transmission security | A SOC2-tagged `socket` rejects `ws://`; only `wss://` is accepted. |
| CC6.7 — cryptographic protection | A SOC2-tagged `axonstore` requires `encryption: at_rest`. |
| CC7.2 — system monitoring | A SOC2-tagged deployment requires the v2.0.0 observability layer to be enabled (verified at deploy, not at parse). |
| PI1.4 — input validation | A SOC2-PI-tagged `axonendpoint` whose body type has `where` refinements gets the v2.0.0 D4 body schema validation; non-PI SOC2 endpoints get it too if they declare it. |
| C1.1 — confidentiality | A SOC2-C-tagged `type` cannot leak through an `axonendpoint` whose `compliance:` list omits SOC2-C, unless passed through a shield with `strategy: pattern` over confidentiality markers. |

## What the runtime enforces

| TSC | AXON runtime enforcement |
|---|---|
| CC4.1 — monitoring | Every emission to a SOC2-tagged endpoint emits an audit row; uptime + error-rate metrics flow to the v2.0.0 observability layer. |
| CC6.2 — user authentication | The v2.0.0 auth layer enforces token signature + expiry + audience; SOC2 endpoints log every auth outcome. |
| CC7.1 — security events | The audit chain records every authentication failure, every authorisation denial, every shield breach with `category: security_event`. |
| CC7.4 — incident response | Structured incident records flow to the compliance event store; the IR playbook consumes them. |
| A1.1 — availability commitments | The runtime exposes `axon_uptime_seconds` + `axon_inflight_requests`; SLO breach triggers structured alerts. |
| PI1.3 — processing integrity | The v2.0.0 D4 body validation + D5 output validation + audit chain together provide the integrity evidence. |
| C1.2 — disposal of confidential information | Purge operations on SOC2-tagged stores emit `session:disposal_evidence` rows. |

## What you still attest manually

- **Type I vs. Type II report** — Type I is design at a point in
  time; Type II is operational effectiveness over 6+ months.
- **Auditor selection + scope** of the engagement.
- **Trust Service criteria selection** (CC always; A/PI/C/P
  optional).
- **Vendor due diligence** (CC9.2) for every subprocessor.
- **Risk assessment** (CC3.1–3.4).
- **Security policy** documentation.
- **Disaster recovery + business continuity** plans.
- **Background checks + access reviews** (CC1.4, CC6.3).

## Common patterns

### Pattern 1 — Baseline SOC2-CC

The most common annotation: every customer-facing API + every
internal data store carries `compliance: [SOC2]`. This activates
the audit chain, auth requirements, isolation, and encryption gates
without specialising for A/PI/C/P.

### Pattern 2 — Adding Availability (A) criteria

```axon
axonendpoint ServeContent {
    flow: ContentFlow
    method: GET
    route: "/v1/content/{id}"
    compliance: [SOC2_A]            # availability TSC
    sla: { p99: 200ms, uptime: 99.9% }
}
```

The `sla:` field (an enterprise extension) declares the
Availability commitment. The v2.0.0 observability layer compares
live measurements + emits structured breach events to the audit
chain when the commitment is missed.

### Pattern 3 — Adding Privacy (P) criteria

`compliance: [SOC2_P]` layers AICPA's privacy principles on top of
the baseline. It is **not** the same as GDPR — SOC2_P is the AICPA
framework; for EU data subjects use both SOC2_P and GDPR.

```axon
type CustomerProfile compliance [SOC2_P, GDPR] {
    email: String,
    consent_record: ConsentRef
}
```

## When NOT to use SOC2

- **Public-facing static content** with no customer data.
- **Internal-only tooling** that never touches customer data.
- **One-off scripts** outside the audited service.

For most production AXON programs, `compliance: [SOC2]` should
be present as a baseline alongside any framework-specific tag
(HIPAA, PCI_DSS, GDPR, SOX, …). The SOC2 audit consumes the
evidence packager output.
