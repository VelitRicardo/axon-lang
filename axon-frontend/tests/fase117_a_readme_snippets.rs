//! §Fase 117.a — **the README is a build input**. Every ` ```axon ` block in
//! the published README is compiled by the real `axon check`, and the set of
//! blocks that do NOT compile is a checked-in ledger that may only SHRINK —
//! `docs/fase/fase_117_the_public_artifact.md`, axon-enterprise repo.
//!
//! **What was wrong.** Nothing compiled the README. Measured 2026-08-03 against
//! the published 2.80.0 binary: **52 of 56 blocks failed**, most on hard PARSE
//! errors — the published grammar was syntax the compiler rejects. Several
//! document primitives that sit in [`axon_frontend::advertised::KNOWN_DEBT`]
//! (`lambda`, `mandate`, `ots`), so they CANNOT be made to compile until those
//! primitives are real; that is a legitimate gap, and §111's doctrine is that a
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
/// The §117.a inventory: 52 of 56 blocks. The bulk document primitives whose
/// grammar drifted (`pix`, `corpus`, `psyche`) or that are in `KNOWN_DEBT` and
/// have no implementation to compile against (`ots`, `mandate`, `lambda`).
/// Rather than delete a third of the language documentation in a packaging
/// fase, §117.a makes the debt VISIBLE and hands each family to the fase that
/// owns it. Run the test to get a paste-ready ledger.
const KNOWN_UNCOMPILABLE: &[(u64, &str)] = &[
    // ── Family A — step/flow BODY grammar drift ────────────────────────────
    // The README documents step-body and flow-body forms the parser rejects:
    // `for`, `know`, `use_tool`, `mandate`, a bare agent name, a leading `.`.
    // Observed: "Unexpected token in step body: 'for' — expected given, ask,
    // probe, reason, weave, stream, output, confidence". Either the grammar
    // regressed or the docs were written against a design that never landed;
    // deciding which is language work, not packaging work. Owner: §118.
    (0xe5a730f995babb86, "block 1: flow AnalyzeContract — `for` in step body"),
    (0x3d0ab268c3d7c4f1, "block 3: know { } — `know` block form"),
    (0x53eaaaef6cb38ffb, "block 4: flow MarketIntelligence — `for` in step body"),
    (0xb8d965c219532f56, "block 16: flow ProcessOrder — `use_tool` in step body"),
    (0x6b75a9d52ad1965c, "block 50: daemon PriceMonitor — leading `.` in flow body"),
    (0xcd74a5c09700b22c, "block 52: daemon TransactionIngester — flow-body drift"),

    // ── Family B — declaration FIELD grammar drift ─────────────────────────
    // Block-level fields whose declared shape no longer parses. Observed:
    // "Expected Colon, found Identifier(...)", "Expected StringLit, found
    // Identifier('env')", "Expected identifier or keyword value, found ','".
    // Owner: §118, per primitive.
    (0xa9ba00796cdc4b40, "block 5: persona SupportAgent — field grammar drift"),
    (0xb291368d4a4f07b6, "block 7: persona ResearchAnalyst — field grammar drift"),
    (0xb1f41e01bf60999e, "block 10: persona OnboardingSpecialist — field grammar drift"),
    (0x4dac1337eb78dbea, "block 11: shield InputGuard — bare agent name in step body"),
    (0x12461c4565a69904, "block 13: shield ResearchShield — field grammar drift"),
    (0xcdf3c8f0279d30ee, "block 14: tool MarketFeed — field grammar drift"),
    (0x425cfaac8362e16a, "block 15: tool WebSearch — field grammar drift"),
    (0xa635a0be5887ff2b, "block 38: tool MarketData — field grammar drift"),
    (0xbbe28e149cb95e78, "block 51: shield ClinicalShield — field grammar drift"),
    (0x56a2865ca114d657, "block 47: compute CalculatePremium — `,` in field value"),
    (0xdb657847c1fa995a, "block 48: compute NormalizeMetrics — `,` in field value"),
    (0xc10bd6bb3dbd6226, "block 49: compute EligibilityScore — `,` in field value"),
    (0xbd4628145ab5b09f, "block 53: axonstore Ledger — `env` unquoted in field"),
    (0x8ec59ea78ba27d50, "block 54: axonstore Users — `env` unquoted in field"),
    (0xf8c2160c210922da, "block 55: axonstore KnowledgeBase — `env` unquoted in field"),

    // ── Family C — `pix` ───────────────────────────────────────────────────
    // Seven blocks, all "Expected Colon, found Identifier(<name>)": the
    // documented `pix <Name> { … }` header does not match the parser. Owner:
    // the fase that owns `pix` (§62 lineage).
    (0xf85f85b185d872bf, "block 17: pix ContractIndex — header grammar drift"),
    (0xb24b02521e08ce86, "block 18: pix ClinicalProtocol — header grammar drift"),
    (0xce95afa6ee05156e, "block 19: pix SDKDocs — header grammar drift"),
    (0x9c39389a82aaac2b, "block 20: pix GDPRRegulation — header grammar drift"),
    (0x05236cbea762876a, "block 21: pix BoardInspector — header grammar drift"),
    (0x720a281c04a63599, "block 22: pix TissueAnalyzer — header grammar drift"),
    (0x306145f5459ae86f, "block 23: pix SatelliteWatch — header grammar drift"),

    // ── Family D — `corpus` ────────────────────────────────────────────────
    // Same shape as `pix`: "Expected Colon, found Identifier(<name>)".
    (0xccbb7005cbe84a67, "block 24: corpus ClinicalKnowledge — header grammar drift"),
    (0xc2630fa205e5bc2e, "block 25: corpus CaseLawGraph — header grammar drift"),
    (0x28456c860b16677e, "block 26: corpus DueDiligence — header grammar drift"),
    (0xf92913227e84ad9b, "block 27: corpus ClinicalKnowledge — header grammar drift"),
    (0x7815bf9da91face3, "block 28: corpus CaseLawGraph — header grammar drift"),
    (0x4f6fce64b79cc42d, "block 29: corpus FinancialNetwork — header grammar drift"),
    (0xa5a68efabbe9e3fd, "block 37: corpus ClinicalDB from mcp(...) — header grammar drift"),
    (0x196753b0511e697e, "block 39: corpus LegalCorpus — header grammar drift"),

    // ── Family E — `psyche` ────────────────────────────────────────────────
    // Type errors rather than parse errors, e.g. "Psyche 'TherapeuticProfile'
    // requires at least one safety_constraint" — the README's own examples
    // violate the rule the checker enforces.
    (0xf88d4b467c1e92c8, "block 30: psyche TherapeuticProfile — missing safety_constraint"),
    (0xd3b41e172d2a5c47, "block 31: psyche AffectTrajectory — checker rule violation"),
    (0x4568161d810e4a44, "block 32: psyche TeamCognition — checker rule violation"),
    (0xe5863dbe8efaab0e, "block 33: psyche StudentEpistemics — checker rule violation"),

    // ── Family F — primitives in KNOWN_DEBT ────────────────────────────────
    // `ots`, `mandate` and `lambda` are listed in
    // `axon_frontend::advertised::KNOWN_DEBT` as NOT implemented. These blocks
    // CANNOT be made to compile — there is nothing to compile them against.
    // They are the README half of a debt §111 already recorded on the compiler
    // side, and they leave this ledger the day their primitive leaves that one.
    (0xd2d40bcdb71707d5, "block 34: ots ThreatPatcher — `ots` is in KNOWN_DEBT"),
    (0x74e1447e5fe9fd94, "block 35: ots ProtocolAdapter — `ots` is in KNOWN_DEBT"),
    (0x906572430bbc8bb7, "block 36: ots MathOperator — `ots` is in KNOWN_DEBT"),
    (0xd6a53497ef00289f, "block 40: mandate SECCompliance — `mandate` is in KNOWN_DEBT"),
    (0xdecbc77b99fbe0b5, "block 41: mandate ClinicalProtocol — `mandate` is in KNOWN_DEBT"),
    (0x0ee2f0a8f2c1fd89, "block 42: mandate LegalPrecision — `mandate` is in KNOWN_DEBT"),
    (0x8944f7fd287174f9, "block 43: lambda SensorReading — `lambda` is in KNOWN_DEBT"),
    (0xf4bf6a58604cdcbd, "block 44: lambda RawTemp — `lambda` is in KNOWN_DEBT"),
    (0xb8f872e365006817, "block 45: lambda RawQuote — `lambda` is in KNOWN_DEBT"),
    (0xf3306c058ceed04e, "block 46: lambda PatientObservation — `lambda` is in KNOWN_DEBT"),

    // ── Family G — undefined references ────────────────────────────────────
    (0x10f35cdaf52e1469, "block 8: agent CaseLawResearcher — references undeclared symbols"),
    (0x6261cff9f54df359, "block 9: agent DataGatherer — references undeclared symbols"),
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
fn fase117a_readme_axon_blocks_match_the_ledger() {
    let markdown = std::fs::read_to_string(readme_path()).expect("README.md is readable");
    let blocks = axon_blocks(&markdown);

    // A parser that silently matches nothing would make this gate vacuous.
    assert!(
        blocks.len() >= 56,
        "expected at least the 56 ```axon blocks present at §117.a, found {}",
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
fn fase117a_thirty_second_demo_compiles_and_its_negative_case_fires() {
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
    // reference and it becomes a TYPE ERROR (§117.a `axon-T957`). Before
    // §117.a this produced byte-identical output to the covered case, which is
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
