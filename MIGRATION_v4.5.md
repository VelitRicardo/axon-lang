# Migrating to AXON 4.5

Four additions, and together they close the shape 4.2–4.4 opened. Those releases
made every exit check that a regulated value crossing a boundary is covered by a
control. **This release gives a regulated value a legitimate way to stop being
one** — typed, checked, and signed.

There is also one **behaviour fix on a deployed path**, described in section 5.
It is not a grammar change and it needs no edit, but you should read it.

Nothing existing breaks. Every construct here is new, and the one mandatory rule
(`axon-T1234`) can only fire on a `declassify`, which did not exist before 4.5.

---

## Will this break my program?

No. Every artifact in this repository still compiles unchanged.

The four new constructs are opt-in, and the laws attach to them. A program that
writes no `declassify` never meets `axon-T1234`; a deployment that reduces no
postal code never meets `axon-T1235`.

---

## 1. `identifier` — a type can say what it is

Until now a type could say **which regimes govern it** (`compliance`) and not
**what it is**. Those are different questions, and de-identification needs the
second one: HIPAA Safe Harbor is a statement about *identifiers*, not about
regimes.

```axon
type SocialSecurity identifier ssn      { value: Text }
type BirthDate      identifier date     { value: Text }
type HomeZip        identifier geography { value: Text }
```

Eighteen kinds, matching 45 CFR 164.514(b)(2)'s enumerable classes. A kind
outside the catalogue is `axon-T1225`, with a suggestion — an unrecognised kind
is not a weaker claim about the data, it is a claim about *nothing*, which a
de-identification rule would then compare against and find satisfied.

The catalogue is **transversal**, not HIPAA-specific: an SSN is an identifier
under HIPAA and GDPR and Ley 1581, so the type describes the data and each
regime says what to do with it.

### The detail that decides whether any of this protects you

**A kind belongs to the TYPE, and a field inherits it by using that type.**

```axon
type PatientRecord compliance [HIPAA] {
    ssn: SocialSecurity   // ✅ carries the `ssn` kind
    dob: BirthDate        // ✅ carries `date`
    admitted: Text        // ⚠️ carries nothing
}
```

`admitted: Text` tells the compiler nothing, so no rule fires for it and **no
diagnostic appears**. That is deliberate — the compiler will not guess what a
string holds — but a `declassify` written against untyped fields suppresses and
reduces nothing. If you are de-identifying, give the identifier-bearing fields a
type that declares its kind.

---

## 2. `declassify` — the legitimate door

```axon
declassify HIPAA from record -> DeidentifiedNote via SafeHarborShield
```

Retires a regulatory class from a value, producing a **different type** — one
your program has declared no longer carries the class.

The reason this had to exist: closing a border without a legitimate door does
not remove the traffic. It pushes it into a side channel that is trivial to
write and invisible to the compiler — *copy the fields into an unlabelled type*.
The class does not follow intra-expression flow, so nothing sees it happen. This
verb is the version the compiler can check, and it is cheaper to write than the
wrong one.

The shield must declare the capability. Scanning is not declassifying:

```axon
shield SafeHarborShield {
    scan:         [pii_leak]
    compliance:   [HIPAA]
    declassifies: [HIPAA]      // ← new: the classes this control may RETIRE
    suppress:     [ssn]        // ← new: the identifier kinds it removes
    generalise:   [date]       // ← new: the kinds it reduces
    on_breach:    halt
}
```

`suppress:` and `generalise:` name **kinds**, not field names — a control welded
to one type's spelling is useless on the next.

### What the compiler refuses

| Code | Refused |
|---|---|
| `axon-T1226` | a class outside Κ |
| `axon-T1227` | a shield that does not declare `declassifies:` |
| `axon-T1228` | a destination type that STILL carries the class |
| `axon-T1229` | a kept identifier the regime allows only reduced, that the shield does not reduce |
| `axon-T1234` | no `attest` for the destination type |

### What happens to the data

The compiler resolves a plan at lowering and the runtime executes it.

**The projection is the suppression.** A field the destination type has no slot
for cannot survive into it — by construction, not by anyone remembering to name
it.

Then the kinds the regime allows only reduced are reduced: a date to its year, a
postal code to three digits, an age over 89 to one category. **Safe Harbor does
not ask you to delete the date; it asks for the year**, and keeping the year
keeps analysable data.

Every fork fails closed. One field that cannot be reduced honestly fails the
whole operation, because a record with four fields reduced and the fifth skipped
is emitted as de-identified and is not.

---

## 3. `attest` — where the compiler stops, a person signs

**Mandatory.** `axon-T1234`: a `declassify` whose destination type has no
attestation does not compile.

```axon
attest SafeHarborDetermination {
    for:      DeidentifiedNote
    basis:    safe_harbor
    residual: [other_unique_identifier, actual_knowledge]
    by:       "Compliance Officer, Acme Health"
    on:       "2026-08-21"
}
```

Safe Harbor enumerates eighteen identifier classes and this compiler decides
**seventeen** of them from the type graph. The eighteenth — item (R), *"any
other unique identifying number, characteristic, or code"* — asks a question
about the world. And 164.514(b)(2)(ii) disqualifies the method outright if you
have actual knowledge the residual data could identify someone. Neither
quantifies over source text.

Those do not become weaker compiler claims. They become a signature with an
owner and a date, and an act of governance with no trace is indistinguishable
from a shortcut.

The attestation is matched **by subject** — it must be `for:` the type the step
produces. Matching loosely would let one attestation anywhere satisfy every
declassification in the program.

### The two bases differ, on purpose

| | `safe_harbor` | `expert_determination` |
|---|---|---|
| what the compiler proved | seventeen identifier classes | nothing about the content |
| `residual:` | **both judgements required** | not required |

Under Safe Harbor the compiler knows exactly what it left open, so it demands a
signature over precisely that remainder. Under expert determination a person
took on the whole analysis, so demanding they enumerate this compiler's
leftovers would be theatre.

**A partial Safe Harbor signature is worse than none** — it reads as diligence
while leaving a hole in a citable place.

`by:` is free prose; the compiler requires only that it not be empty. `on:` is
checked for **shape only** — whether a determination is stale depends on the
data, the population and the regulator, none of which are in your source, but
the field must be a date so the question can be asked of it.

### It ships in the evidence package

`attestations.json`, in a file of its own, because every other artifact there is
derived and mixing them would erase the distinction that makes the package worth
reading. The package README marks the row *"SIGNED, not derived"*. A program
that signs nothing ships no such file — absence is informative.

---

## 4. `census` — the world-fact a `zip3` leans on

```axon
manifest Production {
    region:     "us-east-1"
    compliance: [HIPAA]
    census:     us_2020_decennial
}
```

Safe Harbor (B) permits three postal digits only where the resulting unit holds
**twenty thousand people or more**. Which units those are changes between
censuses and appears nowhere in your program, so the compiler cannot decide it —
but it can refuse to let the assumption stay invisible.

`axon-T1235` fires when a shield declares `generalise: [geography]` and no
manifest names a census. **The obligation is created by the operation, not by
the regime**: a program that never emits a three-digit postal code owes nothing.

The compiler checks the source was *named*, never that it is true. Its truth
belongs to the census, and the citation travels in the evidence where an auditor
can check it.

---

## 5. A behaviour fix on the non-streaming execution path

**No edit required. Read it anyway.**

AXON has two execution paths: streaming (`transport: sse`, via the flow
dispatcher) and non-streaming (via the server runner). The non-streaming path
decided what to do with each kind of node using a list of the verbs that bridge
into the dispatcher — and **every verb not on that list fell through to the
language model**.

That default is the wrong direction to fail in. A verb that reaches a model gets
back a plausible sentence, and *a fabricated result is indistinguishable from a
real one at the call site*. Missing data is visible; a convincing paragraph is
not.

In 4.5 the routing decision is an exhaustive table with no default. `declassify`
is routed to its handler on both paths, and the table now states, per verb, why
it is where it is.

### If you run non-streaming endpoints

Some verbs still reach the model on that path — they have real handlers wired
for `transport: sse` and were never bridged to the runner. This is now a
declared, ratcheted list rather than an unstated default, and it will shrink.
If a flow of yours depends on one of these doing real work, prefer
`transport: sse` for it today:

`shield`-apply · `ots`-apply · `mandate`-apply · `compute`-apply · `warden` ·
`quant` · `emit` · `publish` · `discover` · `listen` · `transact` · `hibernate` ·
`par` · `stream` · `handle` · `perform` · `resume` · `abort` · `forward` ·
`yield` · `return` · `run` · daemon steps

---

## What is NOT in this release

Stated plainly, because a verb named `declassify` and a shield named
`SafeHarborShield` **sound** like a certification and are not:

- **No quantitative guarantees.** No k-anonymity, no l-diversity, no
  t-closeness, no differential privacy. Safe Harbor does not require them — it
  is an *enumerative* method, which is exactly why a compiler can accompany it.
  If you need an ε, you need it **in addition**.
- **No robust declassification.** An attacker controlling part of the program
  could provoke a declassification that would not otherwise occur. That needs an
  attacker model AXON does not have.
- **No intra-expression flow tracking.** The legitimate door now exists and is
  cheaper than the side channel; the side channel is still there.
- **`generalise:` has three operations.** Cell suppression, micro-aggregation
  and swapping are out of scope.
- **The compiler proves a NECESSARY condition, not a sufficient one.** That no
  field carrying the class survives is checkable. That the result carries no
  re-identification risk is a judgement — which is what `attest` is for.
