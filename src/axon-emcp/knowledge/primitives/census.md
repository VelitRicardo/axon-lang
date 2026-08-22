---
name: census
summary: The manifest names the census a three-digit postal code relied on. The compiler checks the source was named, never that it is true.
category: cognitive_io
top_level: false
since: v4.5.0
grammar: |
  manifest <Name> {
      region:     "<region>"
      compliance: [<Class>, ...]
      census:     us_2020_decennial | us_2010_decennial | hhs_published_list
  }
---

# `census`

Names the census a deployment relied on to reduce a postal code to three digits.

```axon
manifest Production {
    region:     "us-east-1"
    compliance: [HIPAA]
    census:     us_2020_decennial
}
```

## The condition a compiler cannot check

Safe Harbor does not permit three postal digits unconditionally. 164.514(b)(2)(i)(B)
permits them **where the resulting unit holds twenty thousand people or more**,
and requires `000` where it does not.

Which units those are is a fact about the world on a date. It moves between
censuses, it is published by the census bureau and by HHS, and it appears nowhere
in the program. The compiler cannot decide it — but it can refuse to let the
assumption stay invisible.

So the deployment **names the source**. The compiler checks that it was named,
never that it is true; its truth belongs to the census, and the citation travels
in the evidence where an auditor can go and check it.

## The obligation is created by the operation

`axon-T1235` fires when a shield declares `generalise: [geography]` and no
manifest names a census. A program that never emits a three-digit postal code
owes nothing — charging every HIPAA deployment an ambient declaration is the kind
of cost that teaches adopters to route around the feature.

A named source outside the catalogue is also `axon-T1235`: a citation nobody can
look up is not a citation.

## Why it lives on the `manifest`

The manifest is already the deployment declaration — it decides region and
regulatory class, and the boundary laws read it. The census condition is the same
kind of fact: **per deployment**, not per program. Two deployments of the same
source may rely on different censuses, and each one's evidence should say which.

Keeping it here also keeps the compiler isolated from the volatility of
environment data. Nothing in the language has to be republished when the next
decennial census lands.

## What this primitive is NOT

- **Not a population check.** The compiler holds no census data and performs no
  lookup. It records which source you relied on.
- **Not a substitute for the `000` rule.** If your deployment emits a restricted
  prefix, handling it is your obligation; naming the census is what makes that
  obligation auditable.
- **Not required by the regime.** It is required by the **operation**. No
  `generalise: [geography]`, no declaration.

## See also

- [`manifest`](manifest) — the deployment declaration this extends
- [`declassify`](declassify) — where the reduction happens
- [`identifier`](identifier) — the `geography` kind this condition applies to
- [`attest`](attest) — the other fact about the world a deployment records
