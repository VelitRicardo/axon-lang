# Migrating to AXON 4.3

One rule, on the primitive that had none: **a `tool` is an exit.**

4.2 made a compliance class survive being wrapped. 4.3 applies the coverage rule
at the boundary an AXON program crosses most often — the call to something
outside itself.

---

## Will this break my program?

Only if a `tool` takes or returns a type carrying a `compliance:` class and no
shield covers it. Every artifact in this repository still compiles unchanged.

---

## A tool call leaves the program

### What changed

`tool` is how AXON talks to any third party: an API, a scraper, a model, a
shell. Until 4.3 it was the one boundary with no regulatory rule at all, so this
compiled clean — the whole regulated record, out over the network:

```axon
type PatientRecord compliance [HIPAA] { ssn: String, diagnosis: String }

tool SendOut {
    provider:   scrape_enrich
    parameters: { payload: PatientRecord }
    output_type: Json
    effects:    <network, web>
}
```

From 4.3, `axon-T1221` requires a covering shield, exactly as `axon-T957` does
at an `axonendpoint`.

### The diagnostic

```text
axon-T1221 tool 'SendOut' carries regulated data (kappa = {HIPAA}) across the
process boundary but declares no `shield:`. A tool call leaves this program —
the ESK coverage rule applies here exactly as it does to an axonendpoint
(axon-T957). Declare `shield: <Name>` on the tool, where that shield lists at
least [HIPAA] in its `compliance:`. Neither `requires:` nor `secret:` covers a
class: they govern WHO may call and WITH WHAT credential, not what happens when
regulated data breaches.
```

### The fix

`tool` gains a `shield:` field:

```axon
shield ToolGuard {
    scan:       [data_exfil]
    on_breach:  raise
    compliance: [HIPAA]
}

tool SendOut {
    provider:    scrape_enrich
    parameters:  { payload: PatientRecord }
    output_type: Json
    effects:     <network, web>
    shield:      ToolGuard
}
```

### Two things that are not coverage

- **`requires:`** governs *who* may call the tool.
- **`secret:`** governs *with what credential* it calls.

Neither can act on a breach of a regulatory class, and that is what coverage
means here.

### The rule does not depend on `effects:`

`effects:` is optional. Gating the guarantee on it would mean a program could
switch the check off by leaving a row out, which is the failure mode this rule
exists to remove. A `tool` has a `provider:` — it is an external call by
construction, effect row or not.

### Both directions count

`parameters:` is data leaving and `output_type:` is data arriving. A regulated
boundary is a boundary either way: an external answer typed as a regulated class
is data your program must be able to act on.

---

## The exit catalogue is now closed

4.3 also publishes the list of ways data can leave an AXON program, and gates it.
Every entry says how that exit is covered — **or why it cannot be**:

| exit | κ coverage |
| --- | --- |
| `axonendpoint` | `axon-T957` — κ from `body:` / `output:` |
| `channel` | `axon-T1215` — κ from `message:` |
| `tool` | `axon-T1221` — κ from `parameters:` / `output_type:` |
| `document` | none — governed by `axon-T916` on the provenance axis |
| `deliver` | none — governed by `axon-T920` on the provenance axis |
| `notify` | none — governed by `axon-T934` on the provenance axis |
| `axonstore` | none |

The last four have no static κ, and the reason is structural rather than an
oversight:

- `document`, `deliver` and `notify` bind **bare value references**. There is no
  typed binding site in the grammar to read a class from — which is also
  precisely why their provenance barriers work: *"is this value attributed?"* is
  answerable about a bare reference, and *"what classes does it carry?"* is not.
- An `axonstore` schema is a closed catalogue of primitive SQL column types. A
  regulated value is decomposed into columns before it lands, and a class does
  not survive that decomposition.

Those limits are published rather than omitted, because an exit missing from a
list is indistinguishable from an exit nobody examined.

If you carry regulated data to one of those four, the control has to be the
shield on the boundary the data crosses **before** it gets there — the endpoint,
channel or tool that admitted it.

---

## Checking your codebase

```bash
axon check path/to/program.axon
```

Compile-time, with the exact classes and the exact missing control named.
