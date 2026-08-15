//! §Fase 122.d — **`axon-T1213`: a streaming tool cannot be memoised.**
//!
//! # Why this law exists
//!
//! §122.d cabled the `cache` primitive into the dispatch path. Measuring the
//! consumers first — rather than trusting §122.a's note that this was *"the
//! cheapest cable in the class"* — turned up three, not one:
//!
//! | consumer | dispatch site | does the cache seam fit? |
//! |---|---|---|
//! | `tool` via `use Tool(…)` | `lambda_tools::dispatch_use_tool_real` | yes — synchronous, one `Vec<u8>` |
//! | `tool` via `apply:` streaming | `pure_shape::run_step_streaming_tool` | **no** — the result is a chunk stream |
//! | `retrieve … cache:` | `wire_integrations::run_retrieve` | yes |
//!
//! The middle row is the one this file is about. `CacheOutcome` carries a
//! `Vec<u8>`; a streaming result is *these seventeen deltas, in this order, at
//! these boundaries*. Buffering it to memoise destroys the streaming the author
//! asked for — the first byte would arrive last. Replaying a hit as one fat
//! chunk is a different observable shape than the one recorded. And doing
//! neither, silently, is precisely the defect §122 exists to remove.
//!
//! So the combination is refused. This follows **D122.4** — the ruling that
//! `shield`'s refusal is breaking, correct, and BOUNDED BY THE DECLARATION.
//! The only programs affected are those that declared both halves, which is
//! exactly the population that was being told a memoisation was happening when
//! none was.
//!
//! # Why the law is in two halves
//!
//! `resolve_tool_cache` reaches a cache by exactly two routes: an explicit
//! `cache: <Name>` on the tool, or a `default: true` cache whose
//! `apply_to_effects:` covers the tool's effect bases. Half one closes the
//! first at the tool; half two closes the second at the cache. Neither
//! re-derives eligibility, which is the point — a second copy of
//! `resolve_tool_cache`'s precedence rules living in the checker is a law that
//! drifts from the law, the §120 defect this project has already paid for
//! twice.
//!
//! `no_program_lets_a_streaming_tool_reach_a_cache` is the test that the two
//! halves together are exhaustive rather than merely both present.

use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn parse(src: &str) -> axon_frontend::ast::Program {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    Parser::new(tokens).parse().expect("parse")
}

/// Errors through BOTH public doors of the checker.
///
/// §120.e found `check` and `check_with_warnings` silently divergent for entire
/// fases, with the CLI taking the one that had lost two laws. They are
/// projections of one walk now, and this asserts they still agree rather than
/// trusting that they do.
fn errors_through_both_doors(src: &str) -> Vec<String> {
    let prog = parse(src);
    let via_check: Vec<String> = TypeChecker::new(&prog)
        .check()
        .iter()
        .map(|e| e.message.clone())
        .collect();
    let (errs, _warns) = TypeChecker::new(&prog).check_with_warnings();
    let via_warnings_door: Vec<String> = errs.iter().map(|e| e.message.clone()).collect();
    assert_eq!(
        via_check, via_warnings_door,
        "the two checker entry points disagree about this program — §120.e's defect, returned"
    );
    via_check
}

const STREAMING_TOOL: &str = r#"
tool MarketFeed {
    provider: http
    effects: <stream:drop_oldest>
    output_type: Quote
    parameters: { symbol: String }
}
"#;

/// 🎯 Half one: a streaming tool naming a cache is refused.
#[test]
fn a_streaming_tool_may_not_name_a_cache() {
    let src = format!(
        "{STREAMING_TOOL}\ncache Fast {{ ttl: 5m }}\n\
         flow F() -> Unit {{ step S {{ ask: \"go\" }} }}\n"
    );
    // Sanity: this is the shape that compiled CLEAN before §122.d. `axon-T865`
    // only demands a finite `ttl:` for a non-pure tool, and `Fast` has one — so
    // the program type-checked, and then streamed past a cache that never saw it.
    let with_cache = src.replace("provider: http", "provider: http\n    cache: Fast");
    let errs = errors_through_both_doors(&with_cache);

    let t1213: Vec<&String> = errs.iter().filter(|e| e.contains("axon-T1213")).collect();
    assert_eq!(
        t1213.len(),
        1,
        "exactly one T1213 expected; got all errors: {errs:?}"
    );
    let msg = t1213[0];
    assert!(
        msg.contains("MarketFeed") && msg.contains("Fast"),
        "the diagnostic must name the TOOL and the CACHE the adopter wrote: {msg}"
    );
    assert!(
        msg.contains("chunk"),
        "and it must say why a stream is not memoisable, not merely that it is not: {msg}"
    );
}

/// The same tool without the `cache:` reference still compiles. Without this,
/// the test above would pass for a law that refuses every streaming tool.
#[test]
fn a_streaming_tool_without_a_cache_reference_still_compiles() {
    let src = format!(
        "{STREAMING_TOOL}\ncache Fast {{ ttl: 5m }}\n\
         flow F() -> Unit {{ step S {{ ask: \"go\" }} }}\n"
    );
    let errs = errors_through_both_doors(&src);
    assert!(
        errs.is_empty(),
        "declaring a stream and declaring a cache are both fine; only pointing one at the \
         other is refused: {errs:?}"
    );
}

/// …and a NON-streaming tool may still name that same cache. The law is about
/// the stream, not about the cache.
#[test]
fn a_non_streaming_tool_may_still_name_the_cache() {
    let src = "\
tool Lookup {
    provider: http
    effects: <pure>
    cache: Fast
    output_type: Quote
    parameters: { symbol: String }
}
cache Fast { ttl: 5m }
flow F() -> Unit { step S { ask: \"go\" } }
";
    let errs = errors_through_both_doors(src);
    assert!(errs.is_empty(), "a pure tool is exactly what a cache is for: {errs:?}");
}

/// 🎯 Half two: the default-policy route, closed at the CACHE.
#[test]
fn a_cache_may_not_widen_to_cover_streams() {
    let src = "\
cache Everything {
    default: true
    ttl: 5m
    apply_to_effects: [pure, stream]
}
flow F() -> Unit { step S { ask: \"go\" } }
";
    let errs = errors_through_both_doors(src);
    let t1213: Vec<&String> = errs.iter().filter(|e| e.contains("axon-T1213")).collect();
    assert_eq!(t1213.len(), 1, "expected one T1213; all errors: {errs:?}");
    assert!(
        t1213[0].contains("Everything") && t1213[0].contains("stream"),
        "the diagnostic must name the cache and the offending effect: {}",
        t1213[0]
    );
}

/// Widening to a non-stream effect is still allowed — it is a `W013` warning,
/// not an error. This pins that §122.d narrowed the language by exactly one
/// shape and left D85.2's widening story intact.
#[test]
fn widening_to_a_non_stream_effect_is_still_only_a_warning() {
    let src = "\
cache Everything {
    default: true
    ttl: 5m
    apply_to_effects: [pure, network]
}
tool Fetch {
    provider: http
    effects: <network>
    output_type: Quote
    parameters: { symbol: String }
}
flow F() -> Unit { step S { ask: \"go\" } }
";
    let errs = errors_through_both_doors(src);
    assert!(
        errs.iter().all(|e| !e.contains("axon-T1213")),
        "§122.d must not have turned every widening into an error: {errs:?}"
    );
}

/// 🎯 The exhaustiveness claim, tested rather than asserted in a comment.
///
/// The two halves are only airtight if they cover every route
/// `resolve_tool_cache` can take to a cache. That function's precedence is:
/// explicit `none` → explicit name → the single `default: true` cache, if the
/// tool's effect bases are a subset of its `apply_to_effects` (implicitly
/// `[pure]`).
///
/// So for a streaming tool the routes are enumerable, and this walks all of
/// them. If someone later adds a third route to `resolve_tool_cache` without
/// extending the law, the arm added here to describe it is what fails.
#[test]
fn no_program_lets_a_streaming_tool_reach_a_cache() {
    // Route 1 — explicit reference. Refused by half one.
    let explicit = format!(
        "{}\ncache Fast {{ ttl: 5m }}\nflow F() -> Unit {{ step S {{ ask: \"go\" }} }}\n",
        STREAMING_TOOL.replace("provider: http", "provider: http\n    cache: Fast")
    );
    assert!(
        errors_through_both_doors(&explicit)
            .iter()
            .any(|e| e.contains("axon-T1213")),
        "route 1 (explicit `cache:`) must be refused"
    );

    // Route 2 — the default cache, widened to reach streams. Refused by half two.
    let widened = format!(
        "{STREAMING_TOOL}\ncache Fast {{ default: true ttl: 5m apply_to_effects: [pure, stream] }}\n\
         flow F() -> Unit {{ step S {{ ask: \"go\" }} }}\n"
    );
    assert!(
        errors_through_both_doors(&widened)
            .iter()
            .any(|e| e.contains("axon-T1213")),
        "route 2 (a default cache widened to streams) must be refused"
    );

    // Route 3 — the default cache NOT widened. Legal, and it must not reach the
    // streaming tool: `stream` ⊄ `[pure]`, so `resolve_tool_cache` returns None.
    // This is the arm that proves the law needs only two halves: the third route
    // is closed by the eligibility rule that was always there, so adding a third
    // diagnostic would have been noise.
    let narrow = format!(
        "{STREAMING_TOOL}\ncache Fast {{ default: true ttl: 5m }}\n\
         flow F() -> Unit {{ step S {{ ask: \"go\" }} }}\n"
    );
    let errs = errors_through_both_doors(&narrow);
    assert!(
        errs.iter().all(|e| !e.contains("axon-T1213")),
        "route 3 is closed by eligibility, not by a diagnostic — refusing it here would \
         make every program with a default cache and a streaming tool illegal: {errs:?}"
    );
}
