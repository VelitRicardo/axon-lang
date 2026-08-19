---
name: fedramp
title: FedRAMP — Federal Risk and Authorization Management Program
summary: AXON annotations covering FedRAMP Low/Moderate/High baselines (NIST 800-53 control families) — what the compiler can attest, what the v2.0.0 runtime attests, what the 3PAO + JAB still need.
---

# FedRAMP — Federal Risk and Authorization Management Program

**Scope:** cloud services offered to US federal agencies. Operated
by GSA's PMO; rests on NIST SP 800-53 control catalogues.

The annotation is `compliance: [FedRAMP_Low]`,
`[FedRAMP_Moderate]`, or `[FedRAMP_High]`. AXON often combines
these with NIST_800_53 (the underlying catalogue) and FISMA (the
statutory basis):

```axon
compliance: [FedRAMP_Moderate, NIST_800_53, FISMA]
```

This page focuses on the **AXON-enforceable subset** of the
800-53 catalogue — AC (access control), AU (audit + accountability),
SC (system + communications protection), SI (system + information
integrity), and IR (incident response).

## Declaring FedRAMP

```axon
type CitizenRecord compliance [FedRAMP_Moderate, NIST_800_53] {
    case_id: String,
    citizen_id: String,
    benefit_class: BenefitClass
}

axonstore CaseFile
    compliance: [FedRAMP_Moderate, NIST_800_53]
    backend: postgresql
    isolation: serializable
    encryption: at_rest
    retention: 7y
    on_breach: raise

axonendpoint UpdateCase {
    flow: UpdateCaseFlow
    method: PUT
    route: "/v1/cases/{case_id}"
    compliance: [FedRAMP_Moderate]
}
```

## What the compiler enforces statically

| NIST 800-53 control | AXON enforcement |
|---|---|
| AC-3 — access enforcement | A FedRAMP-tagged `axonendpoint` requires `auth:` (the v2.0.0 OIDC layer); anonymous endpoints are rejected. |
| AC-4 — information flow enforcement | A FedRAMP_Moderate-tagged `flow` whose body emits to an endpoint with a *lower* baseline is rejected (no downgrade). |
| AC-6 — least privilege | A `mandate:` whose `requires:` capability list grants `*` (wildcard) is rejected on FedRAMP-tagged endpoints. |
| AU-2 — audit events | All FedRAMP-tagged emissions are routed through the audit chain (always-on; not opt-in). |
| AU-12 — audit generation | The audit row's `category:` is auto-set per the v2.0.0 catalogue (no free-form categories). |
| SC-7 — boundary protection | A FedRAMP-tagged `socket` requires `wss://`; `tool` calls with `network` effect to an external endpoint require an explicit `transfer_boundary:` annotation. |
| SC-12, SC-13 — cryptographic key management | The runtime selects FIPS 140-3 validated modules; the compiler verifies that the `encryption:` field is declared. |
| SI-4 — system monitoring | The v2.0.0 observability layer must be configured; deploy-time gate, not parse-time. |
| SI-10 — input validation | A FedRAMP-tagged `axonendpoint` whose body type carries `where` refinements activates the v2.0.0 D4 body schema validation. |

## What the runtime enforces

| Control family | AXON runtime enforcement |
|---|---|
| AC — access control | Every request is authenticated, authorised, and recorded with `(subject, role, resource, action, decision, timestamp)`. |
| AU — audit + accountability | The hash-linked + signed audit chain provides AU-9 (audit info protection) and AU-10 (non-repudiation). |
| SC — system + comms protection | Transport: wss:// only; storage: at-rest encryption; in-process: the v2.0.0 cross-tenant isolation gate. |
| SI — system + info integrity | The v2.0.0 D4 + D5 validation + the audit chain integrity (hash + signature) cover SI-7 (software/firmware/info integrity) and SI-10. |
| IR — incident response | Structured incident records flow to the compliance event store; the playbook consumes them for IR-4 (handling) and IR-6 (reporting). |
| CP — contingency planning | Backup integrity (CP-9) — the audit chain hash is a self-verifying snapshot of the data plane. |

## What you still attest manually

- **System Security Plan (SSP)** + **POA&M** (Plan of Action &
  Milestones).
- **3PAO assessment** + **Security Assessment Report (SAR)**.
- **Provisional ATO** (JAB) or **Agency ATO**.
- **Annual continuous monitoring** + monthly POA&M updates.
- **Physical + personnel controls** (PE, PS, AT families).
- **Configuration management** (CM family) — version-control
  evidence flows out via the deployment pipeline.
- **Contingency tests** — annual.
- **Supply chain risk management** (SR family).

## Baseline selection

| Baseline | When to choose | Indicative AXON pattern |
|---|---|---|
| **FedRAMP Low** | Public-facing, low-impact information (FIPS 199 = L) | Smaller agencies, public-data portals |
| **FedRAMP Moderate** | Most agency missions (FIPS 199 = M) | Default for citizen-services, case-management, benefits |
| **FedRAMP High** | National security, life-safety (FIPS 199 = H) | Healthcare for VA, law-enforcement, financial intelligence |

The compiler does **not** check FIPS 199 categorisation — that is
the operator's call documented in the SSP.

## Common patterns

### Pattern 1 — Agency benefits-eligibility endpoint

```axon
type BenefitsEligibility compliance [FedRAMP_Moderate, FISMA, NIST_800_53] {
    citizen_id: String,
    program: BenefitProgram,
    decision: EligibilityDecision,
    rationale: String
}

flow DetermineEligibility(req: EligibilityRequest) -> BenefitsEligibility { … }

axonendpoint EligibilityAPI {
    flow: DetermineEligibility
    method: POST
    route: "/v1/eligibility"
    compliance: [FedRAMP_Moderate, FISMA, NIST_800_53, SOC2]
    requires: [benefits.evaluate]
}
```

This matches the canonical example at
`examples/government_reference.axon`.

### Pattern 2 — High-baseline audit-log query

```axon
flow QueryAuditForAUDeep(window: TimeWindow) -> List<AuditRow> {
    step Query {
        given: window
        retrieve: AuditLog where tag = "FedRAMP_High" AND window
        output: List<AuditRow>
    }
    return Query.output
}
```

A High-baseline audit query is itself audited (every audit row
referencing AU-12 emits its own AU-2 entry, indirect access
tracking).

### Pattern 3 — Cross-agency data sharing

```axon
axonendpoint ShareToOtherAgency {
    flow: ShareCaseRoutine
    method: POST
    route: "https://partner.gov/share"
    compliance: [FedRAMP_Moderate, FISMA]
    transfer_boundary: ISA_2024_017    # closed catalogue of ISAs
}
```

The `transfer_boundary:` field references the Information Sharing
Agreement (ISA) between the source and destination agencies; the
runtime refuses to dispatch without a valid ISA reference.

## When NOT to use FedRAMP

- **Non-federal customers.** A SaaS serving only commercial
  customers does not need FedRAMP — pick SOC2, ISO 27001, HIPAA
  per the data type.
- **Tools used internally by federal staff** that don't store
  federal information. The boundary is "is federal data in
  scope?", not "is the user federal?"
- **SaaS already authorized through agency-specific programs**
  (StateRAMP, TX-RAMP, GovRAMP, …). The state programs reuse
  FedRAMP artifacts; AXON annotations can mirror.
