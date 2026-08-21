# Migrating to AXON 4.4

One addition, and it completes something: **the three remaining exits can now be
checked.** `document`, `deliver` and `notify` gain a typed input, and with it the
same compliance-coverage rule an `axonendpoint` has always had.

Nothing existing breaks. The new field is optional and the rule fires only where
a regulatory class would otherwise leave the program uncovered.

---

## Will this break my program?

Only if a `document`, `deliver` or `notify` names a `payload:` whose type
carries a `compliance:` class and no shield covers it — and that field did not
exist before 4.4, so no program written against 4.3 can be in that state.

Every artifact in this repository still compiles unchanged.

---

## 1. `payload:` — the typed input

Until 4.4 these three declarations bound **bare value references**, and that had
a consequence worth stating plainly: the compiler could not tell what regulatory
classes they carried. A class originates in a `type` declaration, so an exit
that never names a type has nothing to read one from. That is why 4.3 published
them in the exit catalogue as *not statically coverable* rather than pretending
otherwise.

`payload:` names a **declared type**:

```axon
type MedicalAlert compliance [HIPAA] {
    patient_id: String,
    summary:    String
}

shield EgressShield {
    scan:       [pii_leak]
    compliance: [HIPAA]
    on_breach:  halt
}

notify DoctorAlert {
    channel:  sms
    to:       secret(ops.doctor_phone)
    template: "clinical-alert"
    effects:  <web>
    window:   business_hours
    payload:  MedicalAlert      // ← the type the coverage rule reads
    shield:   EgressShield      // ← the control that covers its classes
}
```

The same two fields, spelled the same way, on all three:

| exit | rule |
| --- | --- |
| `document` | `axon-T1222` |
| `deliver` | `axon-T1223` |
| `notify` | `axon-T1224` |

### The diagnostic

```text
axon-T1224 notify 'DoctorAlert' carries regulated data (kappa = {HIPAA}) out of
the program but declares no `shield:`. An egress is a trust boundary — the ESK
coverage rule applies here exactly as it does to an axonendpoint (axon-T957).
Declare `shield: <Name>`, where that shield lists at least [HIPAA] in its
`compliance:`. The classes on the payload TYPE are the claim; the shield is the
control that acts on a breach.
```

If a shield is already named and the message says it *does not cover* some
classes, add exactly those. The message prints the set difference, so it is a
work list rather than a restatement.

### Two things that are not violations

- **No `payload:`.** A declaration that names no type carries no class to cover
  and compiles exactly as before. The rule refuses a class leaving *uncovered*,
  not a missing field.
- **A payload with no class.** An ordinary typed payload needs no shield. A rule
  that taxed every program would be noise, and noise is how a real rule gets
  switched off.

### A wrapper still does not launder a class

If `payload:` names a type whose field holds a regulated type, the class is
carried. These three read the same walk the endpoint, the channel and the tool
read — there is one definition of "what regulated data is in here", not five.

---

## 2. The exit catalogue

With this release, six of the seven ways data leaves an AXON program carry a
compliance-coverage rule:

| exit | κ read from | rule |
| --- | --- | --- |
| `axonendpoint` | `body:` / `output:` | `axon-T957` |
| `channel` | `message:` | `axon-T1215` |
| `tool` | `parameters:` / `output_type:` | `axon-T1221` |
| `document` | `payload:` | `axon-T1222` |
| `deliver` | `payload:` | `axon-T1223` |
| `notify` | `payload:` | `axon-T1224` |
| `axonstore` | — | none |

`axonstore` remains uncovered, and the reason is structural rather than an
omission: a store schema is a closed catalogue of primitive SQL column types, so
a regulated value is decomposed into columns before it lands, and a class does
not survive that decomposition.

---

## What this release does NOT give you

Two limits, stated here rather than left to be discovered.

**A class cannot be retired.** There is no declassification: once a type carries
`[HIPAA]`, every exit its values reach needs a covering shield, and there is no
way to say *"this has been de-identified, it is no longer PHI"* and have the
compiler know. HIPAA Safe Harbor is literally that operation.

**A class does not survive taking a value apart.** Reading a field into a
`String` and passing that on loses it.

Those two combine into a real incentive: the simplest way to escape a class you
have legitimately retired is to copy the fields into an unlabelled type — which
is exactly the decomposition the compiler cannot see. If you do that today, know
that you are outside what the coverage rule can check, and that a native
declassification verb is the next piece of work rather than a gap nobody
noticed.

---

## Checking your codebase

```bash
axon check path/to/program.axon
```

Compile-time, with the exact classes and the exact missing control named.
