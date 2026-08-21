# Migrating to AXON 4.2

One change, in two halves. **A compliance class can no longer be laundered by
wrapping the value that carries it**, and the jurisdiction check reads the
region your deployment actually uses.

Nothing was added to the language. Two rules that already existed stopped
looking at the wrong thing.

---

## Will this break my program?

Only if it was not protected in the first place. Concretely: if a type carrying
a `compliance:` class reaches an `axonendpoint` or a `channel` **through a
wrapper**, and no shield covers that class, the program now fails to compile.

Every artifact in this repository — 34 domain scaffolds and 12 reference
programs — still compiles unchanged. Their shields already named the right
classes; nothing had verified that until now.

---

## 1. A wrapper does not launder a class

### What changed

An endpoint's κ used to be read from the declared type named in `body:` or
`output:`, after peeling `FlowEnvelope<>`, `List<>` and `Stream<>`. It is now
read from the **value**: every field of every type reachable from there.

The difference appears in the most ordinary shape in the language:

```axon
type PatientRecord compliance [HIPAA] { ssn: String, diagnosis: String }

// A request envelope over the domain type — the normal way to write this.
type PatientSummaryRequest { rec: PatientRecord }

axonendpoint PatientSummary {
    method:  post
    path:    "/v1/patients/summary"
    body:    PatientSummaryRequest
    execute: SummariseFlow
    output:  String
    shield:  ClinicalShield
}
```

Before 4.2, `PatientSummaryRequest` declared no class of its own, so the
endpoint's κ was empty and `ClinicalShield` covered nothing. Deleting the
`shield:` line compiled clean. From 4.2, the endpoint carries `{HIPAA}` and the
shield has to cover it.

### The diagnostic

```text
axon-T957 axonendpoint 'PatientSummary' carries regulated data (kappa = {HIPAA})
across a trust boundary but declares no `shield:`. Regulated boundaries require
a shield whose `compliance:` covers the type's kappa — the ESK coverage rule.
Declare `shield: <Name>` on the endpoint, where that shield lists at least
[HIPAA] in its `compliance:`. Declaring the classes on the endpoint's own
`compliance:` does NOT cover them: that list is a label, the shield is the
control that acts on a breach.
```

### The fix

Name a shield that covers the listed classes:

```axon
shield ClinicalShield {
    scan:       [pii_leak]
    on_breach:  raise
    compliance: [HIPAA]      // ← the class the boundary carries
}
```

If a shield is already named and the message says it *does not cover* some
classes, add exactly those to its `compliance:` list. The diagnostic prints the
set difference, so it is a work list, not a restatement.

### What also changed, and is worth knowing

- **`axon-T1215`** — the channel rule — reads the same κ. A channel whose
  `message:` type wraps a regulated type now carries that class.
- **Unknown generics no longer hide anything.** `Foo<PatientRecord>` carries
  HIPAA whether or not the compiler recognises `Foo`.
- **Cyclic types are fine.** `type Node { next: Node, p: PatientRecord }`
  resolves and reports `{HIPAA}`.

### What did NOT change

κ still does not survive **decomposing a value**. Reading `rec.ssn` into a
`String` and passing that on loses the class. That limit is unchanged in 4.2
and stated here so it is not discovered later.

---

## 2. The jurisdiction check reads the effective region

### What changed

`axon-E042` refuses a deployment whose compliance tag binds a jurisdiction the
substrate cannot satisfy. It used to read the **fabric's** `region:`. A
manifest's own `region:` is documented as an override of the fabric's, and it is
what the deployment uses — so the check now reads that when it is present.

```axon
fabric ClinicalCloud {
    provider: aws
    region:   "eu-west-1"
    zones:    3
}

manifest ProductionHealthcare {
    fabric:     ClinicalCloud
    region:     "us-east-1"     // ← the override the deployment uses
    compliance: [GDPR]
}
```

Before 4.2 this compiled clean: the fabric was in the EU, so the check passed,
while the deployment went to `us-east-1`. This is the direction that matters —
it failed **open**, and it looked green.

### The diagnostic

```text
axon-E042 compliance/jurisdiction: manifest 'ProductionHealthcare' declares
compliance 'gdpr', which binds data to the Eu jurisdiction, but provider 'aws'
region 'us-east-1' is NonEu. The deployment would move regulated data out of the
jurisdiction the tag promises to keep it in (the manifest's own `region:`
overrides fabric 'ClinicalCloud's `eu-west-1`)
```

The trailing clause is there because without it you would check the fabric, find
a compliant region, and have no way to tell where the offending one came from.

### The fix

Set the manifest's `region:` to one the tag permits, or drop the override and
inherit the fabric's.

The **other** direction was also wrong and is now correct: a manifest that
overrides a non-EU fabric **to** an EU region is compliant and no longer
refused.

---

## Checking your codebase

```bash
axon check path/to/program.axon
```

Both rules are compile-time and both name the exact classes and the exact
control that is missing. There is no runtime component to this change and no
configuration to set.
