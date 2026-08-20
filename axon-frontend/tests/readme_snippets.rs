//! v2.81.0 — **the README is a build input**. Every ` ```axon ` block in
//! the published README is compiled by the real `axon check`, and the set of
//! blocks that do NOT compile is a checked-in ledger that may only SHRINK —
//! `the design plan`, axon-enterprise repo.
//!
//! **What was wrong.** Nothing compiled the README. Measured 2026-08-03 against
//! the published 2.80.0 binary: **52 of 56 blocks failed**, most on hard PARSE
//! errors — the published grammar was syntax the compiler rejects. Several
//! document primitives that sit in [`axon_frontend::advertised::KNOWN_DEBT`]
//! (`lambda`, `mandate`, `ots`), so they CANNOT be made to compile until those
//! primitives are real; that is a legitimate gap, and v2.67.0's doctrine is that a
//! legitimate gap must be WRITTEN DOWN rather than left to be discovered by an
//! adopter.
//!
//! **The ratchet.** [`KNOWN_UNCOMPILABLE`] keys each failing block by a content
//! digest, so:
//!   - a NEW failing block breaks the build (it is not in the ledger);
//!   - EDITING a failing block breaks the build (its digest changes) — you must
//!     look at it again, which is the point;
//!   - FIXING a block breaks the build until its stale row is deleted, so the
//!     ledger cannot silently over-state the debt.
//!
//! The list may only ever shrink. Adding a row means knowingly publishing
//! grammar that does not compile, and it costs an edit to this file — a new
//! unkept promise cannot land quietly. This is `feedback_published_grammar_must_compile`
//! turned from discipline into a gate.

use std::path::{Path, PathBuf};

/// Content digests of the ` ```axon ` blocks that are known NOT to compile.
///
/// Format: `(digest, "why")`. Every row is a promise the README makes that the
/// compiler cannot keep. **This list may only shrink.**
///
/// The v2.81.0 inventory: 52 of 56 blocks. The bulk document primitives whose
/// grammar drifted (`pix`, `corpus`, `psyche`) or that are in `KNOWN_DEBT` and
/// have no implementation to compile against (`ots`, `mandate`, `lambda`).
/// Rather than delete a third of the language documentation in a packaging
/// cycle, v2.81.0 makes the debt VISIBLE and hands each family to the cycle that
/// owns it. Run the test to get a paste-ready ledger.
const KNOWN_UNCOMPILABLE: &[(u64, &str)] = &[
    // ── Family A — step/flow BODY grammar drift ────────────────────────────
    // The README documents step-body and flow-body forms the parser rejects:
    // `for`, `know`, `use_tool`, `mandate`, a bare agent name, a leading `.`.
    // Observed: "Unexpected token in step body: 'for' — expected given, ask,
    // probe, reason, weave, stream, output, confidence". Either the grammar
    // regressed or the docs were written against a design that never landed;
    // deciding which is language work, not packaging work. Owner: v2.81.0.
    // v2.83.0 — blocks 3 and 4 LEFT this ledger on 2026-08-08, and their
    // recorded reasons were BOTH WRONG. They were filed as "`know` block form"
    // and "`for` in step body"; the actual first error in each was
    // `weave [A, B] into Report { format: T }` at FLOW level, a form the parser
    // never took (it accepted only the braced `weave { sources: … }`, which no
    // published block writes). The reasons in this table are v2.81.0's, and the
    // compiler has moved six times since — re-diagnose before trusting a row.
    // v2.83.0 — block 50 (`daemon PriceMonitor`) LEFT this ledger on
    // 2026-08-10, and its recorded reason — "leading `.` in flow body" — named
    // nothing that was ever wrong with it. The actual blocker was
    // `Undefined compute 'CalculateDeviation'`: the block applied a compute it
    // never declared, and declaring it required the published
    // `input:/output:/logic { let … return }` form, which did not parse. A
    // FIFTH row whose reason was a fossil.
    (0xcd74a5c09700b22c, "block 52: daemon TransactionIngester — flow-body drift"),

    // ── Family B — declaration FIELD grammar drift ─────────────────────────
    // Block-level fields whose declared shape no longer parses. Observed:
    // "Expected Colon, found Identifier(...)", "Expected StringLit, found
    // Identifier('env')", "Expected identifier or keyword value, found ','".
    // Owner: v2.81.0, per primitive.
    // v2.83.0 — FIVE rows left this ledger on 2026-08-08 (blocks 7, 9,
    // 11, 15 and the old block 55/56 pair), on the step that gave
    // `<Agent>(args)` a step-body position and v2.83.0's executor something
    // to be reached by. Not one of their recorded reasons named the real
    // blocker; each fell after two or three walls:
    //
    // * the bare agent call itself (v2.83.0's grammar);
    //   * `run <Flow>(…) with <Persona>` — README's spelling of the persona
    //     binder on EVERY run it publishes, where the parser took only `as`;
    //   * tools the examples used and never declared (the block-42 shape from
    // v2.83.0: an incomplete example, not a grammar gap);
    //   * providers outside T948's closed catalog (`serper` → `http`,
    // `internal`/`stdlib` → `native` — v2.83.0's precedent, where
    //     inventing the slug would fabricate a provider nothing dispatches);
    //   * `tone: professional`, outside the closed tone catalog — `formal` IS
    //     that tone, and inventing `professional` would fabricate one nothing
    // distinguishes (v2.83.0 made the same call on `warm`);
    //   * `allow:`/`deny:` on a shield, which axon-W010 says the runtime
    //     IGNORES — a published example carrying a line with NO effect is the
    // v2.67.0 shape in documentation. Now `allow_tools:`/`deny_tools:`.
    (0xa635a0be5887ff2b, "block 38: tool MarketData — field grammar drift"),
    (0xbbe28e149cb95e78, "block 51: shield ClinicalShield — field grammar drift"),
    // v2.83.0 — DIAGNOSED 2026-08-08, and the reason above ("`,` in field
    // value") was only the FIRST token that failed. The real blocker is
    // deeper and worth naming precisely, because it is a language decision
    // rather than a parser patch:
    //
    //   compute CalculatePremium {
    //       input: base_rate (Float), risk_factor (Float), years (Float)
    //       output: PremiveResult
    //       logic { let annual = base_rate * risk_factor
    //               let total  = annual * years
    //               return total - total * 0.05 }
    //   }
    //
    // v2.83.0 — blocks 47, 48, 49 and 50 ALL LEFT this ledger on
    // 2026-08-10. The analysis above was RIGHT, which is worth recording
    // because most of the reasons in this table have not been: `logic { … }`
    // really was a let-chain, `Expr` really had no `Let`, and inlining really
    // would have duplicated evaluation (`let total = annual * years` is named
    // three times in block 47 alone).
    //
    // v2.83.0 added `Expr::Let` / `IRExpr::Let` and a lexical scope stack in
    // `eval_expr`, so the chain evaluates once per binding and shadowing falls
    // out of the nesting. `input:`/`output:` became real parameter/return
    // spellings rather than reaching `skip_value()`.
    //
    // What the analysis did NOT predict: with the let-chain fixed, three of the
    // four still failed — on tools the examples used and never declared, and on
    // a compute block 50 applied without declaring. Those were completed in the
    // README (the v2.83.0 incomplete-example repair), not worked around.
    // v2.83.0 — the `axonstore Users` block LEFT this ledger on
    // 2026-08-08. Its recorded reason ("`env` unquoted in field") was fixed
    // back in v2.83.0 and the row survived on three LATER walls, none of them
    // named here — a third demonstration that a row's reason is a fossil:
    //   1. `retrieve from Users where "…"` had no step-body position, and the
    //      flow-level parser demanded the braced `retrieve X { where: … }`
    // form the README never writes (v2.83.0 gave it both);
    // 2. `transact Users { … }`, which v2.67.0 RETRACTED (`axon-T938`: it never
    //      opened a transaction, took no lock, rolled nothing back). Removed
    // from the README with the scope note v2.83.0 set the precedent for
    //      on block 54 — the construct is GONE, not "on the roadmap";
    //   3. `backend: sqlite`, which is not in the closed backend catalog
    //      (`in_memory` | `postgresql` | `secrets`). Inventing a sqlite backend
    //      to satisfy a doc comment would fabricate an engine; `in_memory` is
    //      what the catalog offers for the local-dev case the line describes.
    // v2.83.0 — this block was REWRITTEN (tools declared, `transact`
    // removed per v2.67.0's axon-T938 retraction and v2.83.0's precedent), so it
    // carries a new digest. What remains is NOT grammar: it is the SAME
    // `axon-T802` engine defect block 54 records — a bare identifier in a
    // `persist into … { }` field block is classed as a TEXT LITERAL instead of
    // a reference, so `confidence: DiscoveredFacts.confidence` is rejected
    // against a `Float` column. v2.10.0's literal/reference classification never
    // reached field blocks. Owner: the store type-checker.
    (0x2355c31bc9610888, "block 56: axonstore KnowledgeBase — axon-T802 field-block literal-vs-reference (same engine defect as block 54); `confidence: DiscoveredFacts.confidence` classed as a Text literal against a Float column"),

    // v2.83.0 — the pix/corpus family (10 rows) LEFT this ledger on
    // 2026-08-08. v2.81.0 predicted it: "ONE header-grammar decision for
    // pix/corpus clears 15". The decision turned out to be the the design decision shape
    // again — the PIX verbs (`navigate` / `drill` / `trail` / `validate`) are
    // step-body STATEMENTS with a braceless field list, and each is an
    // ELEVATION whose `as:` binding the step's own `ask:` interpolates. Plus
    // three value forms the README always wrote and the parser never took:
    // `query: <binding>` (not only a literal), `trail: enabled`, and
    // `into <dotted.path>`. The new `depth:` override is CONSUMED
    // (NavConfig.d_max) — a field decided by nothing would be the v2.67.0
    // defect this cycle exists to end.

    // v2.83.0 — six SEMANTIC rows left this ledger on 2026-08-08. They
    // were not grammar gaps: they were incomplete examples (the block-42
    // shape from v2.83.0). Blocks 8/13 named tools nobody declared; block 10
    // used a `tone:` outside the closed catalog (inventing `warm` would
    // fabricate a tone nothing distinguishes — `empathetic` IS that tone);
    // block 14 named a provider outside T948's catalog (omitting `provider:`
    // is the documented LLM-routed form); block 37 still carried `taint:`,
    // a field v2.67.0 explicitly RETRACTED — the one case here where the README
    // was stale and the compiler was right. Block 30 was the compiler moving:
    // `safety:` is what README psyche publishes and the parser only took
    // `safety_constraints:` (the epsilon/tolerance resolution of v2.83.0).

    // v2.83.0 — an epistemic block nested INSIDE a flow body now parses
    // (README scopes helper flows that way). Its children hoist to program
    // level, which is exactly what a TOP-LEVEL `know { … }` already does —
    // so the nested spelling costs nothing in the checker, the IR generator
    // or the runtime. A new FlowStep variant carrying declarations would
    // have forked all three.

    // v2.83.0 — block 54 (the financial-ledger example) was REWRITTEN,
    // not merely re-digested: it published `transact { … }`, which v2.67.0
    // RETRACTED (axon-T938 — it never opened a transaction, took no lock and
    // rolled nothing back). The prose claimed "both writes roll back
    // atomically", which was never true. The example now uses the idempotent
    // form T938's own diagnostic prescribes (both legs keyed by the same
    // `entry_ref`, so a retry converges instead of double-posting), and the
    // scope note says the construct is gone rather than "on the roadmap".
    //
    // It still does not compile, for a DIFFERENT and real reason — a
    // checker gap, kept in the ledger so it is not lost:
    (0x2c4bd66c6bf06a37, "block 54: axonstore Ledger — axon-T802 classes a bare identifier in a `persist into … { }` field block as a TEXT LITERAL instead of a reference to the flow parameter, so `amount: amount` (param `Real`) is rejected against a `Float` column. v2.10.0's literal/reference classification exists for `use`/`let` argument values and never reached field blocks. Owner: the store type-checker, not the README"),

    // ── Family C — `pix` ───────────────────────────────────────────────────
    // Seven blocks, all "Expected Colon, found Identifier(<name>)": the
    // documented `pix <Name> { … }` header does not match the parser. Owner:
    // the cycle that owns `pix` (v2.12.0 lineage).

    // ── Family D — `corpus` ────────────────────────────────────────────────
    // Same shape as `pix`: "Expected Colon, found Identifier(<name>)".
    (0x7815bf9da91face3, "block 28: corpus CaseLawGraph — header grammar drift"),
    (0x4f6fce64b79cc42d, "block 29: corpus FinancialNetwork — header grammar drift"),
    (0x196753b0511e697e, "block 39: corpus LegalCorpus — header grammar drift"),

    // ── Family E — `psyche` ────────────────────────────────────────────────
    // Type errors rather than parse errors, e.g. "Psyche 'TherapeuticProfile'
    // requires at least one safety_constraint" — the README's own examples
    // violate the rule the checker enforces.
    (0xd3b41e172d2a5c47, "block 31: psyche AffectTrajectory — checker rule violation"),
    (0x4568161d810e4a44, "block 32: psyche TeamCognition — checker rule violation"),
    (0xe5863dbe8efaab0e, "block 33: psyche StudentEpistemics — checker rule violation"),

    // ── Family F — primitives in KNOWN_DEBT ────────────────────────────────
    // `ots`, `mandate` and `lambda` are listed in
    // `axon_frontend::advertised::KNOWN_DEBT` as NOT implemented. These blocks
    // CANNOT be made to compile — there is nothing to compile them against.
    // They are the README half of a debt v2.67.0 already recorded on the compiler
    // side, and they leave this ledger the day their primitive leaves that one.
    // v2.83.0 — the three ots blocks (34-36) LEFT this ledger on
    // 2026-08-07: `loss_function:` accepts the identifier form the README
    // always wrote, and the README's homotopy_search values were bent to the
    // CLOSED catalog (A_Star/GradientDescent named algorithms nothing
    // dispatches on — widening the catalog would have fabricated it).
    // v2.83.0 — the three mandate blocks (40–42) LEFT this ledger
    // on 2026-08-06: `mandate X on Y` (and `shield X on Y -> b`) became a
    // step-body statement, which was the real reason all three failed — not
    // `pid`, and not mandate being unimplemented. Block 42 additionally
    // declared two tools it referenced. First rows ever retired from this
    // ratchet.
    // v2.83.0 — the four lambda blocks (43-46) LEFT this ledger on
    // 2026-08-07: `[T]` in flow signatures is List<T> sugar, and
    // `lambda X on y -> b` is a step-body statement (the the design decision position,
    // extended to the fourth member of the apply family).

    // ── Family G — undefined references ────────────────────────────────────
];

/// FNV-1a over the block's normalized text. Not a security primitive — a
/// content key. Implemented inline because `axon-frontend` advertises **zero
/// runtime dependencies** and this gate will not be the thing that adds one.
fn digest(block: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in normalize(block).as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    h
}

/// Trailing whitespace and surrounding blank lines are not content — normalizing
/// them keeps the digest stable against invisible edits.
fn normalize(block: &str) -> String {
    block
        .lines()
        .map(|l| l.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
        .trim_matches('\n')
        .to_string()
}

fn readme_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("axon-frontend always has a parent directory")
        .join("README.md")
}

/// Every fenced ` ```axon ` block, in document order.
fn axon_blocks(markdown: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current: Option<Vec<&str>> = None;
    for line in markdown.lines() {
        match &mut current {
            None => {
                if line.trim_end() == "```axon" {
                    current = Some(Vec::new());
                }
            }
            Some(buf) => {
                if line.starts_with("```") {
                    blocks.push(buf.join("\n"));
                    current = None;
                } else {
                    buf.push(line);
                }
            }
        }
    }
    blocks
}

/// Write `src` to a scratch `.axon` file and run the REAL `axon check` entry
/// point over it (the same `checker::run_check` the CLI dispatches to).
fn check_source(src: &str, tag: &str) -> i32 {
    let dir = std::env::temp_dir().join("axon_fase117a_readme");
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let path = dir.join(format!("{tag}.axon"));
    std::fs::write(&path, src).expect("write scratch snippet");
    axon_frontend::checker::run_check(&path.to_string_lossy(), true, false, None)
}

#[test]
fn readme_axon_blocks_match_the_ledger() {
    let markdown = std::fs::read_to_string(readme_path()).expect("README.md is readable");
    let blocks = axon_blocks(&markdown);

    // A parser that silently matches nothing would make this gate vacuous.
    assert!(
        blocks.len() >= 56,
        "expected at least the 56 ```axon blocks present at v2.81.0, found {}",
        blocks.len()
    );

    let ledger: std::collections::HashMap<u64, &str> =
        KNOWN_UNCOMPILABLE.iter().copied().collect();

    let mut failing: Vec<(usize, u64, String)> = Vec::new();
    for (i, block) in blocks.iter().enumerate() {
        if check_source(block, &format!("block_{i:02}")) != 0 {
            let first = normalize(block)
                .lines()
                .find(|l| !l.trim().is_empty() && !l.trim_start().starts_with("//"))
                .unwrap_or("(empty)")
                .trim()
                .chars()
                .take(60)
                .collect::<String>();
            failing.push((i, digest(block), first));
        }
    }

    let failing_digests: std::collections::HashSet<u64> =
        failing.iter().map(|(_, d, _)| *d).collect();

    // (1) Regression — a block that fails and is NOT in the ledger.
    let unrecorded: Vec<&(usize, u64, String)> = failing
        .iter()
        .filter(|(_, d, _)| !ledger.contains_key(d))
        .collect();

    // (2) Ratchet hygiene — a ledger row whose block now compiles, or whose
    //     text changed. Either way the row is stale and must go.
    let stale: Vec<u64> = ledger
        .keys()
        .copied()
        .filter(|d| !failing_digests.contains(d))
        .collect();

    if !unrecorded.is_empty() || !stale.is_empty() {
        let mut msg = String::new();
        if !unrecorded.is_empty() {
            msg.push_str(&format!(
                "\n{} ```axon block(s) in the README do NOT compile and are NOT in \
                 KNOWN_UNCOMPILABLE.\n\nFix the block, or — if you are knowingly \
                 publishing grammar the compiler cannot keep — add its row below \
                 WITH A REASON:\n\n",
                unrecorded.len()
            ));
            for (i, d, first) in &unrecorded {
                msg.push_str(&format!("    (0x{d:016x}, \"block {i}: {first} — WHY?\"),\n"));
            }
        }
        if !stale.is_empty() {
            msg.push_str(&format!(
                "\n{} KNOWN_UNCOMPILABLE row(s) are stale — the block now compiles, or its \
                 text changed. Delete them (the ledger may only shrink):\n\n",
                stale.len()
            ));
            for d in &stale {
                msg.push_str(&format!(
                    "    0x{d:016x}  // was: {}\n",
                    ledger.get(d).copied().unwrap_or("?")
                ));
            }
        }
        panic!("{msg}");
    }
}

/// The flagship. The "Try it in 30 seconds" program does not live in a
/// ` ```axon ` fence — it is echoed from a ` ```bash ` block — so the ledger
/// above would never see it. It is the single most-read program in the project
/// and the one that was broken; it gets its own gate, in both directions.
#[test]
fn thirty_second_demo_compiles_and_its_negative_case_fires() {
    let markdown = std::fs::read_to_string(readme_path()).expect("README.md is readable");

    // Extract what the reader's shell actually writes to app.axon: the
    // single-quoted argument of `echo '…' > app.axon`.
    let start = markdown.find("echo 'type PatientRecord").expect(
        "the 30-second demo must still start with `echo 'type PatientRecord` — if it was \
         rewritten, update this gate deliberately",
    ) + "echo '".len();
    let rest = &markdown[start..];
    let end = rest
        .find("' > app.axon")
        .expect("the 30-second demo must still end with `' > app.axon`");
    let demo = &rest[..end];

    assert_eq!(
        check_source(demo, "demo_with_shield"),
        0,
        "the README's 30-second demo MUST compile clean — it is the first thing \
         every adopter runs.\n\n{demo}"
    );

    // The claim the README makes about this program: delete the shield
    // reference and it becomes a TYPE ERROR (v2.81.0 `axon-T957`). Before
    // v2.81.0 this produced byte-identical output to the covered case, which is
    // how a vacuous guarantee looks from the outside.
    let no_shield = demo.replace(" shield: PHIShield", "");
    assert_ne!(
        no_shield, demo,
        "the demo must still declare `shield: PHIShield` for the negative case to mean anything"
    );
    assert_ne!(
        check_source(&no_shield, "demo_no_shield"),
        0,
        "removing the shield MUST fail the check — this is the README's load-bearing claim"
    );
}
