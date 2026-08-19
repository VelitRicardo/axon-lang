//! v1.32.0 — Introspection Error Propagation Contract.
//!
//! These -assertions anchor the v1.40.2 + v1.40.3 hotfix cycle. The
//! invariant is structural: NO site under `axon-rs/src/store/` may
//! `match`-fall-through an `introspect_conn` failure into a bare-table
//! SQL cascade. Every introspect failure MUST roll back the transaction
//! explicitly and propagate the primary error to the caller.
//!
//! The grep gate (S) is the structural enforcement: any future PR that
//! reintroduces the `Err(_) => (None, &no_types)` pattern (or any
//! semantic equivalent) inside a production code path turns this test
//! RED before merge.

use std::fs;

const STORE_FILES: &[&str] = &[
    "src/store/postgres_backend.rs",
    "src/store/row_stream.rs",
];

fn read(path: &str) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("v1.32.0+12 — failed to read {path}: {e}"))
}

fn strip_comments_and_tests(src: &str) -> String {
    // Block out the `#[cfg(test)]` module + all line comments so the
    // S grep targets PRODUCTION code only — comments mentioning the
    // old pattern (e.g. "pre-hotfix the code fell through to ... (None,
    // &no_types)") and unit tests are not false positives.
    let mut out = String::with_capacity(src.len());
    let mut in_test_mod = false;
    let mut brace_depth = 0_i32;
    for line in src.lines() {
        let trimmed = line.trim_start();
        if !in_test_mod && trimmed.starts_with("#[cfg(test)]") {
            in_test_mod = true;
            brace_depth = 0;
            continue;
        }
        if in_test_mod {
            for ch in line.chars() {
                if ch == '{' {
                    brace_depth += 1;
                }
                if ch == '}' {
                    brace_depth -= 1;
                    if brace_depth == 0 {
                        in_test_mod = false;
                    }
                }
            }
            continue;
        }
        // Drop line comments — both `//` and `///`.
        let code_only = match line.find("//") {
            Some(idx) => &line[..idx],
            None => line,
        };
        out.push_str(code_only);
        out.push('\n');
    }
    out
}

#[test]
fn s_no_fall_through_to_bare_no_types_in_store_crate() {
    // S — STATIC grep gate. PRODUCTION code under `axon-rs/src/store/`
    // MUST NOT contain the pre-hotfix masking pattern.
    let forbidden_substrings: &[&str] = &[
        "(None, &no_types)",
        "Err(_) => (None,",
    ];
    for path in STORE_FILES {
        let src = read(path);
        let code = strip_comments_and_tests(&src);
        for needle in forbidden_substrings {
            assert!(
                !code.contains(needle),
                "v1.32.0+12 S — PRODUCTION code under `{path}` still \
                 contains the pre-hotfix introspect-masking pattern \
                 `{needle}`. Replace with the explicit ROLLBACK + \
                 propagate primary error pattern (see `postgres_backend.rs::query` \
                 lines 1429-1455 for the canonical shape)."
            );
        }
    }
}

#[test]
fn s_every_introspect_conn_site_rollbacks_on_error() {
    // S — STATIC grep gate. Every site that calls
    // `introspect_conn(&mut tx, …)` MUST be paired with a
    // `tx.rollback().await` on the error arm. We approximate the
    // pair-check by counting `introspect_conn(&mut tx,` calls and
    // matching them against `tx.rollback().await` occurrences in
    // production code.
    //
    // The shape today: 4 sites in postgres_backend.rs + 1 in
    // row_stream.rs = 5 calls. The cache-hit warm path in
    // row_stream.rs also has a rollback for the staleness retry, so
    // we lower-bound the assertion: `rollback().await` count MUST be
    // at least the `introspect_conn(&mut tx,` count.
    let mut introspect_in_tx = 0usize;
    let mut rollback_calls = 0usize;
    for path in STORE_FILES {
        let src = read(path);
        let code = strip_comments_and_tests(&src);
        introspect_in_tx += code.matches("introspect_conn(&mut tx,").count();
        rollback_calls += code.matches("tx.rollback().await").count();
    }
    assert!(
        rollback_calls >= introspect_in_tx,
        "v1.32.0+12 S — production code has {introspect_in_tx} \
         `introspect_conn(&mut tx, …)` call(s) but only {rollback_calls} \
         `tx.rollback().await` occurrence(s). Every introspect-in-tx \
         site MUST roll back on its error arm so the transaction is \
         not poisoned for the caller."
    );
}

#[test]
fn s_row_stream_site_carries_37xj12_d_letter() {
    // S — STATIC grep gate. The row_stream introspect site MUST
    // carry the `d_letter = "37.x.j.12"` tracing field so log readers
    // can correlate the warn back to this anchor.
    let src = read("src/store/row_stream.rs");
    assert!(
        src.contains("d_letter = \"37.x.j.12\""),
        "v1.32.0 S — `row_stream.rs` introspect warn site MUST \
         carry `d_letter = \"37.x.j.12\"` so adopters can grep their \
         logs back to the anchor."
    );
    assert!(
        src.contains("rolling back and propagating the primary"),
        "v1.32.0 S — `row_stream.rs` introspect warn site MUST \
         carry the canonical message `rolling back and propagating \
         the primary error to the caller`. Drift breaks the adopter \
         log grep contract."
    );
}

#[test]
fn s_postgres_backend_carries_37xj11_d_letter() {
    // S — STATIC grep gate. The 4 postgres_backend introspect sites
    // MUST carry the `d_letter = "37.x.j.11"` tracing field on their
    // error arm.
    let src = read("src/store/postgres_backend.rs");
    let count = src.matches("d_letter = \"37.x.j.11\"").count();
    assert!(
        count >= 4,
        "v1.32.0 S — `postgres_backend.rs` MUST have at least 4 \
         `d_letter = \"37.x.j.11\"` occurrences (one per CRUD site: \
         query / insert / mutate / purge); found {count}. Any drift \
         loses the adopter log correlation contract."
    );
}
