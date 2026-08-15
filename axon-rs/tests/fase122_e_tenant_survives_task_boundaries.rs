//! §Fase 122.e — **the ambient tenant, and the boundaries that drop it.**
//!
//! # The defect class
//!
//! `CURRENT_TENANT_ID` is a `tokio::task_local!`. A task-local crosses **no**
//! task boundary — not `tokio::spawn`, not `spawn_blocking`, and certainly not
//! a fresh OS thread. `current_tenant_id()` does not fail when it is gone; it
//! returns `"default"`. Measured, not assumed:
//!
//! ```text
//! same task      = acme
//! spawn_blocking = default
//! tokio::spawn   = default
//! block_on_store = default
//! ```
//!
//! That fallback is the whole problem. Nothing errors, nothing panics, and
//! every downstream reader carries on under an identity that is *plausible*.
//! §95.f threaded the tenant to the executor explicitly and still shipped two
//! doors that lost it, because the bridge was written on the wrong side of a
//! boundary and no test compared the two sides. §122.d.1 fixed those two, and
//! reintroduced the same defect one level deeper in the same commit, under a
//! comment asserting the opposite.
//!
//! Three attempts, three comments that were wrong. Hence a gate.
//!
//! # What this file does
//!
//! 1. Pins the BEHAVIOUR of each boundary primitive, so a future refactor that
//!    silently stops re-binding fails here.
//! 2. Ratchets the POPULATION of ambient reads. A new `current_tenant_id()`
//!    call anywhere in `src/` is a deliberate act that must be recorded — the
//!    §120.g dark-targets shape, applied to a different silent set.
//!
//! # What it deliberately does not claim
//!
//! It does not prove any particular ambient read is *safe*. Whether a read is
//! correct depends on whether its caller is scoped, which is not decidable from
//! the read's own text. What the ratchet buys is that **adding one is visible**,
//! which is exactly what was missing while this class accumulated across §95.f,
//! §118.b.2 and §122.d.1.

use axon::tenant_context::{current_tenant_id, scope_tenant, scope_tenant_blocking};

// ── 1. The boundaries, and the primitives that survive them ─────────────────

/// The measurement that motivated the whole sub-fase, kept as a test.
///
/// If a future tokio ever propagated task-locals across these boundaries, this
/// would fail — and the several parameters §122.d.1/§122.e added would become
/// belt-and-braces rather than load-bearing. That is worth knowing either way,
/// which is why the fact is pinned rather than written in a comment.
#[tokio::test(flavor = "multi_thread")]
async fn a_task_local_does_not_survive_any_task_boundary() {
    let same_task = scope_tenant("acme".into(), async { current_tenant_id() }).await;
    assert_eq!(same_task, "acme", "the baseline: scoping works at all");

    let across_spawn_blocking = scope_tenant("acme".into(), async {
        tokio::task::spawn_blocking(current_tenant_id).await.unwrap()
    })
    .await;
    assert_eq!(
        across_spawn_blocking, "default",
        "if this ever becomes 'acme', task-locals now cross the blocking pool and the \
         explicit tenant parameters added in §122.d.1 are no longer load-bearing"
    );

    let across_tokio_spawn = scope_tenant("acme".into(), async {
        tokio::spawn(async { current_tenant_id() }).await.unwrap()
    })
    .await;
    assert_eq!(
        across_tokio_spawn, "default",
        "this is the one that caught §122.d.1's second attempt: a spawned TASK inherits \
         no more than a spawned THREAD does"
    );
}

/// 🎯 `scope_tenant_blocking` re-binds across `spawn_blocking`.
///
/// This is the primitive `/v1/execute` uses to make its whole blocking task —
/// executor, fallback key resolution, every `storage_postgres` RLS read —
/// observe the request's tenant rather than the fallback.
#[tokio::test(flavor = "multi_thread")]
async fn scope_tenant_blocking_rebinds_across_the_blocking_pool() {
    let seen = scope_tenant("acme".into(), async {
        let captured = current_tenant_id(); // read on the ASYNC side, as callers must
        tokio::task::spawn_blocking(move || {
            scope_tenant_blocking(captured, current_tenant_id)
        })
        .await
        .unwrap()
    })
    .await;

    assert_eq!(
        seen, "acme",
        "the blocking task must observe the request's tenant; without the re-bind it sees \
         `default` and every ambient reader inside it — including the 30 `SET LOCAL \
         axon.current_tenant` sites in `storage_postgres.rs` — is scoped to the wrong tenant"
    );
}

/// The same, for the async boundary the SSE door crosses.
#[tokio::test(flavor = "multi_thread")]
async fn scope_tenant_rebinds_across_tokio_spawn() {
    let seen = scope_tenant("acme".into(), async {
        let captured = current_tenant_id();
        tokio::spawn(scope_tenant(captured, async { current_tenant_id() }))
            .await
            .unwrap()
    })
    .await;
    assert_eq!(seen, "acme");
}

/// **Capture on the WRONG side and the re-bind is worthless.**
///
/// This is the failure mode by itself, isolated. §122.d.1 shipped it: the
/// capture was moved inside the `tokio::spawn` and the value re-bound was
/// already `"default"`. The re-bind looked right in review — it *is* the right
/// call, on the wrong value.
#[tokio::test(flavor = "multi_thread")]
async fn capturing_inside_the_boundary_rebinds_the_fallback() {
    let seen = scope_tenant("acme".into(), async {
        tokio::task::spawn_blocking(|| {
            // The bug: read AFTER crossing. `current_tenant_id()` is already
            // `"default"` here, so the scope faithfully binds the wrong answer.
            let too_late = current_tenant_id();
            scope_tenant_blocking(too_late, current_tenant_id)
        })
        .await
        .unwrap()
    })
    .await;

    assert_eq!(
        seen, "default",
        "capturing after the boundary must NOT accidentally work — if it did, the ordering \
         these fixes depend on would not matter and the gates above would prove nothing"
    );
}

// ── 1b. Source-drift gates for the two spawned key resolutions ──────────────

const AXON_SERVER_SRC: &str = include_str!("../src/axon_server.rs");

/// 🎯 The two `resolve_backend_key` calls that sit behind a task boundary must
/// pass a THREADED tenant, never an ambient read.
///
/// # Why a source gate rather than a behavioural one
///
/// This one is honest about its own strength. The behavioural assertion would
/// be "the SSE door resolves *acme's* key" — and the tier it selects is an
/// in-process AWS Secrets Manager cache with no public seam to seed, so proving
/// it end-to-end means adding test-only API to production code for one
/// assertion.
///
/// The mutation record is what decided it. Perturbing the SSE call site to pass
/// `"default"` **survived** every other test in this file: the parameter change
/// was pinned by its signature (`integration.rs`'s `_fn_ptr`) but nothing
/// checked that the caller passes the right thing. A fix whose regression
/// nobody would notice is the shape §122 exists to remove, so it gets the gate
/// this codebase already uses for exactly this — the `adapters.rs` §49.o
/// source-drift pattern.
#[test]
fn the_spawned_key_resolutions_pass_a_threaded_tenant() {
    // The SSE door: inside `execute_sse_handler_inner`'s `tokio::spawn`.
    assert!(
        AXON_SERVER_SRC.contains("resolve_backend_key(&s, &effective_backend, &tenant_id)"),
        "`server_execute_streaming` must pass its `tenant_id` PARAMETER to \
         resolve_backend_key. It is called from inside a `tokio::spawn`, where an ambient \
         `current_tenant_id()` is the `\"default\"` fallback and the per-tenant Secrets \
         Manager cache is consulted under the wrong name (§Fase 122.e)"
    );

    // The `/v1/execute` FALLBACK branch: inside `execute_handler`'s `spawn_blocking`.
    assert!(
        AXON_SERVER_SRC.contains("resolve_backend_key(&s, fallback_backend, tenant_id)"),
        "`execute_with_fallback`'s fallback branch must pass its `tenant_id` parameter. It \
         runs on the blocking pool, so an ambient read there resolves a DIFFERENT key than \
         the primary attempt used — precisely when the primary provider is already \
         failing (§Fase 122.e)"
    );
}

// ── 2. The ratchet on ambient reads ─────────────────────────────────────────

/// Non-comment occurrences of `current_tenant_id()` per file under `src/`.
fn ambient_reads_by_file() -> std::collections::BTreeMap<String, usize> {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = std::collections::BTreeMap::new();
    let mut stack = vec![src.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            // Comments are prose ABOUT the defect — this file is full of it, and
            // counting it would make the ratchet measure documentation.
            let n = text
                .lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .map(|l| l.matches("current_tenant_id()").count())
                .sum::<usize>();
            if n > 0 {
                let rel = path
                    .strip_prefix(&src)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                out.insert(rel, n);
            }
        }
    }
    out
}

/// 🎯 **The ratchet.** A new ambient tenant read is a deliberate act.
///
/// Regenerate only on purpose, and only with a reason in the commit message:
///
/// ```text
/// AXON_REGEN_AMBIENT_READS=1 cargo test --test fase122_e_tenant_survives_task_boundaries
/// ```
///
/// The same shape as `fase120_g_dark_targets.pinned`, and for the same reason:
/// §119.i.2's finding that *a gap written down is honoured as often as someone
/// rereads the note*. This class went unnoticed across three fases while every
/// suite stayed green, because nothing counted it.
///
/// **Adding a read is not automatically wrong.** An async handler under the
/// middleware's scope reads it correctly, and most of the pinned population is
/// exactly that. What the pin forces is that someone looked at which side of a
/// boundary the new one sits on.
#[test]
fn ambient_tenant_reads_are_pinned() {
    const PIN_FILE: &str = "tests/fase122_e_ambient_tenant_reads.pinned";
    let pin_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(PIN_FILE);
    let measured = ambient_reads_by_file();

    let rendered: String = measured
        .iter()
        .map(|(f, n)| format!("{f}\t{n}\n"))
        .collect();

    if std::env::var("AXON_REGEN_AMBIENT_READS").is_ok() {
        std::fs::write(&pin_path, &rendered).expect("write pin file");
        return;
    }

    let pinned = std::fs::read_to_string(&pin_path)
        .unwrap_or_else(|e| panic!("cannot read {PIN_FILE}: {e}"))
        .replace("\r\n", "\n");

    assert_eq!(
        rendered.replace("\r\n", "\n"),
        pinned,
        "\n\nThe population of AMBIENT `current_tenant_id()` reads under `src/` changed.\n\n\
         A task-local is `\"default\"` on the far side of `tokio::spawn`, `spawn_blocking`, \
         and any fresh thread — silently, because the fallback is plausible. Before \
         regenerating this pin, answer for each new read:\n\n\
         \x20 1. Which task is it on? An async handler under `tenant_extractor_middleware` \
         is fine. Anything reached from a spawn is not.\n\
         \x20 2. Could the caller pass the tenant instead? A parameter makes the boundary \
         visible in the signature; an ambient read hides it.\n\
         \x20 3. If many readers are downstream, re-bind at the BOUNDARY with \
         `scope_tenant` / `scope_tenant_blocking` — capturing on the ASYNC side.\n\n\
         Then: AXON_REGEN_AMBIENT_READS=1 cargo test --test \
         fase122_e_tenant_survives_task_boundaries\n"
    );
}
