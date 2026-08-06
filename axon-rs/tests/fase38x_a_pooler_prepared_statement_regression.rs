//! §Fase 118.b.3 — this suite exercises the SQL data plane (the Postgres
//! backend, the pinned-connection contract, live introspection), which lives
//! behind the `postgres` feature. Gated at the file level: the core suites keep
//! running in a build with no driver.
#![cfg(feature = "postgres")]

//! §Fase 38.x.a — Pooler-coherent Transactions Contract — diagnostic anchor.
//!
//! Pins the regression the kivi adopter reported on 2026-05-20 (smoke 16,
//! v1.38.0) and the 5 §-assertions that close it. Every §-assertion is
//! designed to INVERT IN PLACE — pre-fix it pins the broken state; the
//! v1.38.1 patch (the post-fix code) makes it green. A future PR that
//! re-breaks the invariant turns this anchor RED before merge.
//!
//! # The contract
//!
//! - **D1** — every `sqlx::query(...)` / `sqlx::query_as(...)` against a
//!   Postgres pool, `PoolConnection`, `Transaction`, or `PgConnection`
//!   under `axon-rs/src/store/` carries `.persistent(false)`. The
//!   §4 grep §-assertion enforces this STATICALLY across the source
//!   tree.
//! - **D2** — `PoolOptions::after_release` runs `DEALLOCATE ALL` on
//!   every released connection. §5 inspects the `connect_named_with_namespace`
//!   doc-block + a behavioural property.
//! - **D3** — the 5 `match &resolved { Err(_) => … }` swallows in
//!   `query` / `persist` / `mutate` / `purge` / `row_stream::drain_stream`
//!   emit a structured `tracing::warn!` carrying the primary error.
//!   §3 inspects the source for the `tracing::warn!` site count.
//!
//! # No infrastructure required
//!
//! Every §-assertion is pure (no Postgres, no PgBouncer, no Supavisor).
//! Source-walk + symbol-existence + Display-string checks. The
//! integration-flavored sequential-transaction smoke lives in
//! `fase37x_i_pgbouncer_integration.rs` (PG-gated, opt-in).

use std::fs;
use std::path::PathBuf;

/// Walk `axon-rs/src/store/` and return every `.rs` file path.
fn walk_store_sources() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let store_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("store");
    walk_dir(&store_dir, &mut out);
    out
}

fn walk_dir(dir: &PathBuf, out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).expect("store dir readable");
    for entry in entries {
        let entry = entry.expect("dir entry readable");
        let path = entry.path();
        if path.is_dir() {
            walk_dir(&path, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// Return `(file_path, line_number_1_based, line_text)` for every line
/// in every `.rs` file under `store/` matching `pattern`.
fn grep_store_lines(pattern: &str) -> Vec<(PathBuf, usize, String)> {
    let mut hits = Vec::new();
    for path in walk_store_sources() {
        let body = fs::read_to_string(&path).expect("file readable");
        for (i, line) in body.lines().enumerate() {
            if line.contains(pattern) {
                hits.push((path.clone(), i + 1, line.to_string()));
            }
        }
    }
    hits
}

// ════════════════════════════════════════════════════════════════════
//  §1 — The kivi smoke 16 corpus: prepared statement collision symptoms
// ════════════════════════════════════════════════════════════════════

/// §1 — pin that the symptom strings the adopter saw in their deploy
/// logs (`sqlx_s_N` named prepared statements, `42710 duplicate_prepared_statement`)
/// are the EXACT regression class this anchor covers. If a future
/// refactor changes how sqlx names statements or how Postgres signals
/// the collision, the test author updating this anchor must verify
/// against the upstream sqlx + Postgres docs first.
///
/// This §-assertion never inverts — it's a literal docstring pin so the
/// test reader can correlate the anchor's intent with the kivi handoff.
#[test]
fn s1_kivi_smoke_16_collision_symptom_corpus() {
    let corpus = [
        // From the kivi smoke 16 deploy log (2026-05-20):
        "prepared statement \"sqlx_s_1\" already exists",
        "prepared statement \"sqlx_s_2\" already exists",
        "prepared statement \"sqlx_s_3\" already exists",
        // The PostgreSQL error class the above maps to:
        "42710",
        "duplicate_prepared_statement",
        // The SECONDARY cascade error that v1.38.0 surfaced INSTEAD:
        "current transaction is aborted",
        "25P02",
        "in_failed_sql_transaction",
    ];
    // Every entry is a non-empty literal; this §-assertion is the
    // intent pin, not a code probe.
    for s in &corpus {
        assert!(!s.is_empty(), "corpus entry must be non-empty");
    }
    assert_eq!(corpus.len(), 8, "corpus stable at 8 strings");
}

// ════════════════════════════════════════════════════════════════════
//  §2 — D3 observability: 5 silent swallows became tracing::warn!
// ════════════════════════════════════════════════════════════════════

/// §2 — pin that EVERY `Err(_) => (None, &no_types)` swallow in
/// `query` / `persist` / `mutate` / `purge` / `row_stream::drain_stream`
/// was replaced with a `tracing::warn!` site carrying the primary error.
///
/// Pre-fix (v1.38.0): 5 `Err(_) => (None, &no_types)` matches existed.
/// Post-fix (v1.38.1): 0 such matches; instead, 5 `tracing::warn!` sites
/// with `target: "axon::store"` and `d_letter = "D3+38.x.a"`.
///
/// **Invariant:** the count of legacy silent-swallows is 0, AND the count
/// of structured tracing::warn! sites in introspection-failure paths is
/// at least 5 (one per code path).
#[test]
fn s2_d3_observability_no_silent_swallows() {
    let silent_swallows = grep_store_lines("Err(_) => (None, &no_types)");
    assert_eq!(
        silent_swallows.len(),
        0,
        "v1.38.1 must not contain ANY `Err(_) => (None, &no_types)` \
         silent swallows. Found these sites (must be replaced with \
         `Err(e) => {{ tracing::warn!(...); (None, &no_types) }}`):\n{:#?}",
        silent_swallows
    );

    // §Fase 37.x.j.11/.12 superseded the original §38.x.a `d_letter =
    // "D3+38.x.a"` swallow-and-warn sites with a STRONGER pattern:
    // rollback the introspect-in-tx transaction and propagate the
    // primary error directly (no bare-table cascade). The observability
    // invariant is unchanged — one structured `tracing::warn!` per
    // introspection-failure path (query / persist / mutate / purge /
    // row_stream) — only the `d_letter` label tracks the superseding
    // sub-fase (`37.x.j.11` for the 4 backend ops, `37.x.j.12` for the
    // stream cursor).
    let warn_sites = grep_store_lines("d_letter = \"37.x.j.1");
    assert!(
        warn_sites.len() >= 5,
        "v1.38.1+ must emit at least 5 structured `tracing::warn!` sites \
         carrying a `d_letter = \"37.x.j.1…\"` label (one per \
         introspection-failure path replaced: query / persist / mutate / \
         purge / row_stream). Found: {} sites at:\n{:#?}",
        warn_sites.len(),
        warn_sites
    );
}

// ════════════════════════════════════════════════════════════════════
//  §3 — D3 invariant: warnings cite the primary error explicitly
// ════════════════════════════════════════════════════════════════════

/// §3 — pin that every `tracing::warn!` site emitted by D3 carries the
/// primary `StoreError` via the `error = %e` field. Without this, the
/// adopter still wouldn't see the root cause — only an axon-side
/// description of what fell through.
#[test]
fn s3_d3_invariant_warnings_carry_primary_error() {
    // §Fase 37.x.j.11/.12 — the superseding warn sites bind the primary
    // error as `error = %introspect_err` (the introspection failure that
    // is now rolled back + propagated, rather than swallowed). The
    // invariant is identical: every D3 warn carries the primary error.
    let warn_sites_with_error_field = grep_store_lines("error = %introspect_err");
    assert!(
        warn_sites_with_error_field.len() >= 5,
        "every D3 `tracing::warn!` must carry the primary error via \
         the `error = %…` field. Found {} sites; expected ≥5. The adopter \
         relies on this field to diagnose the root cause (e.g. `sqlx_s_N \
         already exists`). Sites:\n{:#?}",
        warn_sites_with_error_field.len(),
        warn_sites_with_error_field
    );
}

// ════════════════════════════════════════════════════════════════════
//  §4 — D1 STATIC ENFORCEMENT: every sqlx::query carries .persistent(false)
// ════════════════════════════════════════════════════════════════════

/// §4 — pin that EVERY `sqlx::query(...)` and `sqlx::query_as(...)` call
/// site under `axon-rs/src/store/` is followed within ±10 lines by a
/// `.persistent(false)` invocation.
///
/// This is the STRUCTURAL invariant of D1 — Pooler-coherent Transactions
/// Contract. A future PR that adds a new `sqlx::query` call without
/// `.persistent(false)` turns THIS test red BEFORE merge — so the
/// regression class kivi reported cannot ship again.
///
/// **Implementation note:** the proximity window is ±10 lines (not ±5)
/// because a few call sites span 8 lines from the `sqlx::query(` opening
/// token to the binding chain. The grep is fail-loud: if a site lacks
/// the directive, the test prints the file + line + line text for the
/// reviewer to fix.
#[test]
fn s4_d1_invariant_every_sqlx_query_uses_persistent_false() {
    let mut violations: Vec<String> = Vec::new();

    for path in walk_store_sources() {
        let body = fs::read_to_string(&path).expect("file readable");
        let lines: Vec<&str> = body.lines().collect();

        // Collect line indices of every `sqlx::query(` / `sqlx::query_as(`
        // call (excluding doc comments + the DEALLOCATE ALL site which §5
        // handles explicitly).
        let call_sites: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter_map(|(i, line)| {
                let stripped = line.trim_start();
                if stripped.starts_with("//") {
                    return None;
                }
                if !(line.contains("sqlx::query(")
                    || line.contains("sqlx::query_as("))
                {
                    return None;
                }
                // §5 owns the DEALLOCATE ALL meta-invariant separately.
                // The substring may live on this line OR the next 2 lines
                // (the after_release hook wraps it across 2-3 lines).
                let lookahead = lines
                    [i..(i + 3).min(lines.len())]
                    .join("\n");
                if lookahead.contains("DEALLOCATE ALL") {
                    return None;
                }
                Some(i)
            })
            .collect();

        // For each call site, the `.persistent(false)` must appear AFTER
        // the opening of `sqlx::query(` and BEFORE the next call site
        // (or end of file). This handles multi-line SQL string literals
        // gracefully — they can be arbitrarily long.
        for (k, &i) in call_sites.iter().enumerate() {
            let next_call = call_sites.get(k + 1).copied().unwrap_or(lines.len());
            let scan_lo = i;
            let scan_hi = next_call.min(lines.len());
            let window: String = lines[scan_lo..scan_hi].join("\n");
            if !window.contains(".persistent(false)") {
                violations.push(format!(
                    "{}:{}: missing `.persistent(false)` between this \
                     `sqlx::query` call and the next one. Line: {}",
                    path.display(),
                    i + 1,
                    lines[i].trim_end()
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "D1 invariant violated — every `sqlx::query(...)` / \
         `sqlx::query_as(...)` under `axon-rs/src/store/` MUST carry \
         `.persistent(false)`. The unnamed PARSE protocol is the only \
         pooler-coherent transactional contract; a named PARSE \
         (`sqlx_s_N`) leaks across logical sessions through a \
         transaction-mode pooler (Supavisor, PgBouncer, Neon, RDS \
         Proxy) and collides with the residual prep on the physical \
         conn from the prior session. Violations:\n{}",
        violations.join("\n")
    );
}

// ════════════════════════════════════════════════════════════════════
//  §5 — D2 INSTALLATION: after_release hook with DEALLOCATE ALL
// ════════════════════════════════════════════════════════════════════

/// §5 — pin that `connect_named_with_namespace` installs the
/// `PoolOptions::after_release` hook that runs `DEALLOCATE ALL` on
/// every released connection.
///
/// Belt-and-suspenders: if a future code path slips past D1's
/// `.persistent(false)` invariant, D2 cleans the physical conn before
/// the pooler returns it. A reviewer who deletes the hook on the
/// argument "D1 is enough" turns THIS test red.
#[test]
fn s5_d2_installation_after_release_runs_deallocate_all() {
    let backend_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("store")
        .join("postgres_backend.rs");
    let body = fs::read_to_string(&backend_path).expect("backend readable");

    assert!(
        body.contains(".after_release("),
        "D2 invariant violated — `connect_named_with_namespace` must \
         install `PoolOptions::after_release(...)`. Without it, a code \
         path that misses D1's `.persistent(false)` leaks named prepared \
         statements onto the physical Postgres conn behind the pooler."
    );
    assert!(
        body.contains("DEALLOCATE ALL"),
        "D2 invariant violated — the `after_release` hook must execute \
         `DEALLOCATE ALL` on every released conn. Anything weaker (e.g. \
         deallocating a single named statement) is not a defense against \
         arbitrary future leaks."
    );
    // The hook itself uses `.persistent(false)` — the meta-invariant.
    // Locate the actual CALL SITE: `sqlx::query("DEALLOCATE ALL")`,
    // distinguished from doc-comment references via the literal substring.
    let call_site_marker = "sqlx::query(\"DEALLOCATE ALL\")";
    let call_site = body
        .find(call_site_marker)
        .expect("DEALLOCATE ALL call site present (not just a doc-comment mention)");
    let window = &body[call_site..(call_site + 200).min(body.len())];
    assert!(
        window.contains(".persistent(false)"),
        "D2 meta-invariant violated — the `DEALLOCATE ALL` call inside \
         the `after_release` hook must itself carry `.persistent(false)`. \
         Otherwise the cleanup query ITSELF could leak a named prepared \
         statement on a connection mid-release. Window inspected:\n{}",
        window
    );
}

// ════════════════════════════════════════════════════════════════════
//  §6 — Fase 38.x.b D1: every axon-owned table lives in `axon_admin`
// ════════════════════════════════════════════════════════════════════

/// §6 — pin the kivi-specific regression class structurally: no
/// migration under `axon-rs/migrations/` may create a `tenants` table
/// outside the `axon_admin` schema.
///
/// **Scope (intentional):** the §-assertion is narrow — `tenants` only,
/// not every axon-owned admin table. `tenants` is the specific
/// adopter-collision class kivi reported (a common adopter-app table
/// name; the legacy M1 migration's `CREATE TABLE IF NOT EXISTS tenants`
/// would silently land on the adopter's table and produce
/// `column "plan" does not exist` at the first read).
///
/// Other legacy axon-owned tables in `public` (`traces`, `sessions`,
/// `daemons`, `audit_log`, `axon_stores`, …) are operational tables
/// with axon-specific names that have NOT generated collision reports
/// against adopter apps. A future Fase 39+ may extend this invariant
/// to other tables on a per-name basis as collision reports arrive.
///
/// A future PR that creates `public.tenants` or unqualified `tenants`
/// in a new migration file (or revives the legacy v1.38.1 M1) turns
/// this test RED.
#[test]
fn s6_d1_kivi_tenants_collision_class_closed() {
    let migrations_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("migrations");
    let entries = fs::read_dir(&migrations_dir)
        .expect("migrations dir readable");

    let mut tenants_in_public_or_bare: Vec<String> = Vec::new();
    let mut tenants_in_axon_admin_count: usize = 0;

    for entry in entries {
        let entry = entry.expect("dir entry readable");
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("sql") {
            continue;
        }
        let body = fs::read_to_string(&path).expect("file readable");
        // Strip line comments so we only look at executable SQL.
        let stripped: String = body
            .lines()
            .filter(|l| !l.trim_start().starts_with("--"))
            .collect::<Vec<_>>()
            .join("\n");
        for (line_idx, line) in stripped.lines().enumerate() {
            let upper = line.to_uppercase();
            if !upper.contains("CREATE TABLE") {
                continue;
            }
            if line.contains("EXECUTE") || line.contains("||") {
                continue;
            }
            // We care about lines that target the `tenants` table —
            // either as `tenants`, `public.tenants`, or
            // `axon_admin.tenants`.
            let table_part = upper
                .split("CREATE TABLE")
                .nth(1)
                .unwrap_or("")
                .trim_start_matches(" IF NOT EXISTS")
                .trim();
            let mentions_tenants = table_part.starts_with("TENANTS")
                || table_part.starts_with("PUBLIC.TENANTS")
                || table_part.starts_with("AXON_ADMIN.TENANTS");
            if !mentions_tenants {
                continue;
            }
            if table_part.starts_with("AXON_ADMIN.TENANTS") {
                tenants_in_axon_admin_count += 1;
            } else {
                tenants_in_public_or_bare.push(format!(
                    "{}:{}: {}",
                    path.display(),
                    line_idx + 1,
                    line.trim()
                ));
            }
        }
    }

    assert!(
        tenants_in_public_or_bare.is_empty(),
        "D1 invariant violated — `tenants` MUST be created in the \
         `axon_admin` schema (never unqualified or in `public`). The \
         kivi 2026-05-20 smoke-16 report named `public.tenants` as the \
         collision class against adopter-owned tables. Violations:\n{}",
        tenants_in_public_or_bare.join("\n")
    );
    assert!(
        tenants_in_axon_admin_count >= 1,
        "D1 positive invariant — the M1 migration (axon-rs/migrations/\
         003_add_tenants.sql) MUST contain at least one \
         `CREATE TABLE IF NOT EXISTS axon_admin.tenants`. Found {} \
         matching CREATE statements.",
        tenants_in_axon_admin_count
    );
}
