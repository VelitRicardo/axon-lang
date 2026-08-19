---
name: delivery_is_assertion_egress
title: "Delivery is assertion egress — a value written into a system of record must carry its epistemic origin, or the author vouched (v2.60.0)"
summary: "The law governing `deliver` (v2.60.0), the egress-dual of acquisition (`scrape`, v2.52.0). Writing a record into a CRM is not I/O plumbing — it is PUBLISHING ASSERTIONS into a system of record that downstream humans (a sales rep, an auditor) will read AS FACT. A v2.58.0-enriched email is born `speculate`/`believe` + Untrusted — a vendor's probabilistic guess, never a verified truth. Letting it land in the CRM as a bare string LAUNDERS the guess into a fact — the v2.53.0 assertion-laundering sin (`document` T916), in egress form. `deliver` refuses the laundering BY CONSTRUCTION: with `provenance: attached` (the default) every delivered field lands beside its epistemic origin (level + confidence + source), so a guess arrives LABELED a guess; `provenance: cleared` (bare values) is a COMPILE-TIME refusal (axon-T920) unless the flow vouched the values through `shield` + `anchor` under an `epistemic { believe|know }` wrapper. The barrier is LABELING, not prohibition — the lead pipeline delivers guesses honestly instead of being blocked; only SILENT stripping is forbidden. Every operation carries an idempotency `key:` (axon-T926) so an at-least-once retry never double-creates. Enforced at compile (axon-T920 the barrier + T921–T926 structure), verify/deploy (`DeliveryProvenanceSoundness` re-derives T920 from the IR), and — in the enterprise runtime — a `crm:deliver` SoD gate + a per-tenant legal flag + a fail-closed, PII-free audit. What the law does NOT promise: that the CRM, downstream, keeps honoring the provenance — once the bytes leave axon, the label's survival is the adopter's system's responsibility (the same honest perimeter as v2.53.0's `document`)."
---

# Delivery is assertion egress

`deliver` is the point where a value **leaves the epistemic lattice into a
machine system of record** — a CRM whose rows downstream humans treat as
verified fact. It is the exact dual of `scrape` (v2.52.0): where a scraped
value ENTERS the program born adversarial and Untrusted, a delivered
value goes OUT into a system that confers an authority the value may
never have earned.

> **The law.** A value written into a system of record must carry its
> epistemic origin (`provenance: attached`, the default), OR the author
> must have vouched — under `epistemic { believe|know }`, after a
> `shield` + `anchor` cleared it — that it is a verified fact
> (`provenance: cleared`). Stripping the origin from an Untrusted /
> Inferred value at the delivery boundary is impossible: it is a
> compile-time refusal (`axon-T920`), not a runtime surprise. A vendor's
> guess arrives in the CRM *labeled a guess*, or it does not arrive bare.

## Why this is a law, not a lint

A v2.58.0-enriched email is a vendor model's probabilistic guess — born
`speculate` (a pattern heuristic) or `believe` (deliverability-verified),
**never `know`**, and epistemically Untrusted. The CRM does not know that.
A row in a system of record *reads as true* — the format confers
authority the value never earned. Booking a `speculate` email as a bare
`email` field is precisely the assertion-laundering v2.53.0 forbids for a
`document` (T916); `deliver` is that barrier in egress form (T920).

## The surface

```axon
deliver PushLead {
    target:     crm            # the system-of-record class (axon-T921)
    provenance: attached       # each field lands with its level/confidence/source
    secret: crm_api_key # v2.48.0 custody — the credential never enters cognition
    effects:    <web>          # a CRM write crosses the network trust boundary (axon-T924)
    upsert_contact {
        key:   lead_email      # idempotency key — a retry never double-creates (axon-T926)
        email: lead_email
        name:  lead_name
    }
}
```

`provenance: cleared` is legal ONLY when the flow vouches:

```axon
# axon-T920: launders a vendor guess into the CRM as a bare fact.
deliver bad { target: crm  secret: k  effects: <web>  provenance: cleared
    upsert_contact { key: guessed_email  email: guessed_email }
}

# OK: the author vouches the values are ≥ believe (after shield + anchor).
believe {
    deliver good { target: crm  secret: k  effects: <web>  provenance: cleared
        upsert_contact { key: verified_email  email: verified_email }
    }
}
```

## The three enforcement layers (all fail-closed)

1. **Compile** — `axon-T920` (the provenance-stripping barrier) + `T921–T926`
   (target catalog, provenance catalog, `secret:` required, `web` effect,
   operation catalog + non-empty, idempotency `key:`).
2. **Verify / deploy** — the PCC `DeliveryProvenanceSoundness` class
   re-derives T920 (+ T921/T924) from the compiled IR, so a hand-edited IR
   that flips `provenance` to `cleared` without the vouch is REFUTED before
   it deploys.
3. **Runtime (enterprise)** — a `crm:deliver` SoD gate (role-expanding) + a
   per-tenant legal flag (default OFF) + a fail-closed `crm:delivered` audit
   that witnesses the delivery's op kinds + field-count — **never the PII
   values** (a delivered email is itself a PII record).

## What the law does NOT promise

- **Not that the CRM keeps the label.** Once the bytes reach the vendor,
  whether the `axon_provenance` companion survives is the adopter's CRM's
  responsibility — the same honest perimeter as `document` (v2.53.0): axon
  guarantees the artifact leaves *labeled*, not that every downstream tool
  respects the label.
- **Not a lawful basis.** Delivering v2.58.0-enriched PII is *further
  processing*; the governance demonstrates diligence, it does not create a
  GDPR lawful basis (the tenant is the controller).

## See also

- `document` (v2.53.0) — the sibling egress primitive (into a human artifact);
  its T916 assertion-laundering barrier is the same law for a different sink.
- `scrape` (v2.52.0) — the acquisition dual; born-Untrusted ingress.
- The lead-gen vertical: acquire (v2.56.0) → enrich (v2.58.0) → **deliver** (v2.60.0).
