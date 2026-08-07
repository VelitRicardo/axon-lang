//! §Fase 33.y.i — PIX variants (paper §6 hidden-state primitives).
//!
//! Three variants graduated in 33.y.i:
//!
//! - **`Hibernate`** (Fase 11.e + Fase 16 supervisor) — wait for
//!   a named event with a timeout. Sync runner uses CPS-style
//!   continuation passing: the flow suspends, the supervisor
//!   resumes it on event arrival. OSS reference impl emits the
//!   canonical `step_type: "hibernate"` wire shape + binds
//!   `__hibernating_<event>` marker; enterprise R&D
//!   (axon_enterprise.cognitive_states + .supervisor) wires the
//!   real continuation-passing semantics.
//!
//! - **`Drill`** (Fase 11.e PIX) — drill into a PIX subtree at
//!   `subtree_path` to answer `query`. OSS reads from
//!   `__pix_<pix_ref>_<subtree_path>` namespaced let_bindings;
//!   binds result under `output_name`.
//!
//! - **`Trail`** (Fase 11.e PIX) — walk the breadcrumb of a prior
//!   navigation (`navigate_ref`). OSS reads from
//!   `__navigate_<ref>_trail`; binds result under canonical key.
//!
//! # Paper §6 semantic contract
//!
//! PIX (Procedural Index of eXcurrences) is the paper §6 hidden-
//! state primitive used by Fase 11 neuro-symbolic flows. The sync
//! runner's CPS dispatcher is the canonical reference; 33.y.i ships
//! the async port with D10 byte-identical algebraic-semantics
//! parity for the OSS reference cases (placeholder bindings —
//! adopter flows using stub backend see consistent output between
//! sync + async paths).
//!
//! # D-letter anchors
//!
//! - **D1** — every variant has a NAMED async handler; exhaustive
//!   match in `dispatch_node`.
//! - **D3** — cancel checked at every `.await` boundary. Hibernate
//!   ESPECIALLY honors cancel (the flow suspended on Hibernate
//!   needs to wake on cancel within the standard p95 budget).
//! - **D7** — every error case routes through `DispatchError`; OSS
//!   helpers cannot fail (placeholder semantics); enterprise
//!   overrides surface `DispatchError::BackendError` for real
//!   supervisor/PIX runtime errors.
//! - **D10** — sync-runner parity: handler outputs deterministic
//!   placeholders for stub-backend OSS path; enterprise integration
//!   preserves the SAME wire envelope (only inner content
//!   differs).

use crate::flow_dispatcher::{DispatchCtx, DispatchError, NodeOutcome};
use crate::flow_execution_event::{now_ms, FlowExecutionEvent};
use crate::ir_nodes::{IRDrillStep, IRHibernateStep, IRTrailStep};

// ────────────────────────────────────────────────────────────────────
//  Public helpers (enterprise hooks override these)
// ────────────────────────────────────────────────────────────────────

// §Fase 119.d — `await_event_with_timeout` was DELETED here. It inserted a
// marker binding and returned "(hibernating <event> timeout=<t>)" while the
// flow KEPT WALKING — a hibernation that kept spending (§111 F20). The
// suspension is real now: NodeOutcome::Hibernated + the walk loop's park
// + halt, with resume riding `emit` (crate::hibernation).


/// Drill into a PIX subtree to answer a query. OSS default: looks
/// up `__pix_<pix_ref>_<subtree_path>` in let_bindings (binding
/// the raw stored value); falls back to a canonical
/// `"(drilled <pix_ref> path=<subtree_path> query=<query>)"`
/// placeholder when the subtree isn't pre-seeded. Enterprise
/// overrides wire the real PIX state machine (paper §6).
pub fn drill_pix_subtree(
    pix_ref: &str,
    subtree_path: &str,
    query: &str,
    ctx: &DispatchCtx,
) -> String {
    let key = format!("__pix_{pix_ref}_{subtree_path}");
    if let Some(value) = ctx.let_bindings.get(&key) {
        return value.clone();
    }
    format!("(drilled {pix_ref} path={subtree_path} query={query})")
}

/// Trail the breadcrumb of a prior navigation. OSS default: looks
/// up `__navigate_<ref>_trail`; falls back to canonical
/// `"(trail of <ref>)"` placeholder. Enterprise overrides walk
/// the real PIX trail state.
pub fn trail_navigation(navigate_ref: &str, ctx: &DispatchCtx) -> String {
    let key = format!("__navigate_{navigate_ref}_trail");
    if let Some(value) = ctx.let_bindings.get(&key) {
        return value.clone();
    }
    format!("(trail of {navigate_ref})")
}

// ────────────────────────────────────────────────────────────────────
//  Hibernate (Fase 11.e — event-await with timeout)
// ────────────────────────────────────────────────────────────────────

/// §Fase 119.d — `hibernate <event> [timeout]`: observe the suspension point.
///
/// §111 F20 found this handler returning "(hibernating …)" SYNCHRONOUSLY —
/// the flow kept walking, kept spending, while claiming to sleep. It now
/// returns [`NodeOutcome::Hibernated`]; the WALK LOOP (which alone knows the
/// node's position in the body) parks the continuation and HALTS the run.
/// The task dies: no compute, no tokens — the primitive's one guarantee.
///
/// Inside a nested branch (par / for-in), suspension has no defined
/// continuation shape yet, so it REFUSES rather than half-suspends.
pub async fn run_hibernate(
    node: &IRHibernateStep,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }
    let step_index = ctx.step_counter;
    ctx.step_counter += 1;

    if !ctx.branch_path.is_empty() {
        return Err(DispatchError::BackendError {
            name: "hibernate".to_string(),
            message: format!(
                "hibernate '{}' appears inside a nested branch ({}); suspending \
                 a branch has no defined continuation shape yet, so it is \
                 refused rather than half-suspended. Place hibernate at \
                 top-level flow position.",
                node.event_name,
                ctx.branch_path_string()
            ),
        });
    }

    let step_name = if node.event_name.is_empty() {
        "Hibernate".to_string()
    } else {
        node.event_name.clone()
    };
    emit_step_start(ctx, &step_name, step_index, "hibernate")?;

    Ok(NodeOutcome::Hibernated {
        event_name: node.event_name.clone(),
        timeout: node.timeout.clone(),
        step_index,
    })
}


// ────────────────────────────────────────────────────────────────────
//  Drill (Fase 11.e PIX — drill into hidden-state subtree)
// ────────────────────────────────────────────────────────────────────

/// Drill handler. Wire shape: `step_type: "drill"`.
///
/// §Fase 62.A.3: when the source document is in scope, this runs REAL subtree
/// navigation — index the source, locate the named subtree by its dotted
/// title-path (`subtree_path`), then navigate WITHIN that subtree
/// ([`crate::pix_navigator::pix_drill`]), embeddings-free. Falls back to the
/// pre-seeded `__pix_<ref>_<path>` lookup ([`drill_pix_subtree`]) when no
/// indexable source is present (e.g. drilling keys a prior `navigate` seeded).
pub async fn run_drill(
    node: &IRDrillStep,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }
    let step_index = ctx.step_counter;
    ctx.step_counter += 1;

    let step_name = if node.output_name.is_empty() {
        "Drill".to_string()
    } else {
        node.output_name.clone()
    };
    emit_step_start(ctx, &step_name, step_index, "drill")?;

    let query = crate::exec_context::interpolate_vars(&node.query, &ctx.let_bindings);

    // Real subtree navigation when an indexable source is in scope.
    let real = crate::flow_dispatcher::cognitive::resolve_pix_source("", &node.pix_ref, ctx)
        .and_then(|source| crate::pix_navigator::index_markdown(&source).ok())
        .and_then(|tree| {
            let titles: Vec<&str> = node.subtree_path.split('.').collect();
            let subtree_root = crate::pix_navigator::find_by_title_path(&tree, &titles)?;
            let cfg = crate::pix_navigator::NavConfig::default();
            let scorer = crate::pix_navigator::LexicalScorer::default();
            let r = crate::pix_navigator::pix_drill(&tree, subtree_root, &query, &cfg, &scorer)?;
            Some(
                r.leaves
                    .iter()
                    .map(|l| l.content.as_str())
                    .collect::<Vec<_>>()
                    .join("\n\n---\n\n"),
            )
        });

    // Fallback (D5): the pre-seeded subtree binding (e.g. seeded by `navigate`).
    let result =
        real.unwrap_or_else(|| drill_pix_subtree(&node.pix_ref, &node.subtree_path, &query, ctx));

    if !node.output_name.is_empty() {
        ctx.let_bindings.insert(node.output_name.clone(), result.clone());
    }

    emit_step_complete(ctx, &step_name, step_index, &result, 0)?;

    Ok(NodeOutcome::Completed {
        output: result,
        tokens_emitted: 0,
        step_index,
    })
}

// ────────────────────────────────────────────────────────────────────
//  Trail (Fase 11.e PIX — breadcrumb walk)
// ────────────────────────────────────────────────────────────────────

/// Trail handler. Wire shape: `step_type: "trail"`. Resolves the
/// trail via [`trail_navigation`] + binds result under
/// `<navigate_ref>_trail_walked` canonical key.
pub async fn run_trail(
    node: &IRTrailStep,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }
    let step_index = ctx.step_counter;
    ctx.step_counter += 1;

    let step_name = if node.navigate_ref.is_empty() {
        "Trail".to_string()
    } else {
        node.navigate_ref.clone()
    };
    emit_step_start(ctx, &step_name, step_index, "trail")?;

    let result = trail_navigation(&node.navigate_ref, ctx);
    if !node.navigate_ref.is_empty() {
        ctx.let_bindings
            .insert(format!("{}_trail_walked", node.navigate_ref), result.clone());
    }

    emit_step_complete(ctx, &step_name, step_index, &result, 0)?;

    Ok(NodeOutcome::Completed {
        output: result,
        tokens_emitted: 0,
        step_index,
    })
}

// ────────────────────────────────────────────────────────────────────
//  Wire-event helpers (shared)
// ────────────────────────────────────────────────────────────────────

fn emit_step_start(
    ctx: &mut DispatchCtx,
    step_name: &str,
    step_index: usize,
    step_type: &str,
) -> Result<(), DispatchError> {
    ctx.tx
        .send(FlowExecutionEvent::StepStart {
            step_name: step_name.to_string(),
            step_index,
            step_type: step_type.to_string(),
                branch_path: ctx.branch_path_string(),
            timestamp_ms: now_ms(),
        })
        .map_err(|_| DispatchError::ChannelClosed)
}

fn emit_step_complete(
    ctx: &mut DispatchCtx,
    step_name: &str,
    step_index: usize,
    full_output: &str,
    tokens_output: u64,
) -> Result<(), DispatchError> {
    ctx.tx
        .send(FlowExecutionEvent::StepComplete {
            step_name: step_name.to_string(),
            step_index,
            success: true,
            full_output: full_output.to_string(),
            tokens_input: 0,
            tokens_output,
                branch_path: ctx.branch_path_string(),
            timestamp_ms: now_ms(),
        })
        .map_err(|_| DispatchError::ChannelClosed)
}

// ────────────────────────────────────────────────────────────────────
//  Unit tests
// ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cancel_token::CancellationFlag;
    
    use tokio::sync::mpsc;

    fn fresh_ctx() -> (
        DispatchCtx,
        mpsc::UnboundedReceiver<FlowExecutionEvent>,
    ) {
        let (tx, rx) = mpsc::unbounded_channel();
        let ctx = DispatchCtx::new(
            "TestFlow",
            "stub",
            "",
            CancellationFlag::new(),
            tx,
        );
        (ctx, rx)
    }

    // ── Public helpers ───────────────────────────────────────────────

    // §Fase 119.d — `await_event_sets_marker_and_returns_placeholder` was
    // DELETED here. It pinned the §111 F20 placeholder in green: a
    // "(hibernating …)" string returned synchronously while the flow kept
    // walking. The suspension is real now; its gates live in
    // `tests/fase119_d_hibernate.rs` (halt, park, emit-wake, lazy expiry).

    /// §Fase 119.d — the handler observes the suspension point; the walk
    /// loop parks and halts. Inside a nested branch it refuses.
    #[tokio::test]
    async fn run_hibernate_returns_the_suspension_outcome() {
        let (mut ctx, _rx) = fresh_ctx();
        let node = IRHibernateStep {
            node_type: "hibernate",
            source_line: 7,
            source_column: 3,
            event_name: "user_action".into(),
            timeout: "5m".into(),
        };
        match run_hibernate(&node, &mut ctx).await.unwrap() {
            NodeOutcome::Hibernated {
                event_name,
                timeout,
                ..
            } => {
                assert_eq!(event_name, "user_action");
                assert_eq!(timeout, "5m");
            }
            other => panic!("expected Hibernated, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn run_hibernate_refuses_inside_a_branch() {
        let (mut ctx, _rx) = fresh_ctx();
        ctx.branch_path.push("par[0]".into());
        let node = IRHibernateStep {
            node_type: "hibernate",
            source_line: 7,
            source_column: 3,
            event_name: "ev".into(),
            timeout: String::new(),
        };
        let err = run_hibernate(&node, &mut ctx).await.err().expect("refuses");
        match err {
            DispatchError::BackendError { message, .. } => {
                assert!(message.contains("no defined"), "{message}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn drill_returns_stored_value_when_present() {
        let (mut ctx, _rx) = fresh_ctx();
        ctx.let_bindings
            .insert("__pix_main_root.leaf".into(), "leaf-content".into());
        assert_eq!(
            drill_pix_subtree("main", "root.leaf", "q", &ctx),
            "leaf-content"
        );
    }

    #[test]
    fn drill_returns_placeholder_when_not_stored() {
        let (ctx, _rx) = fresh_ctx();
        assert_eq!(
            drill_pix_subtree("main", "root", "what", &ctx),
            "(drilled main path=root query=what)"
        );
    }

    #[test]
    fn trail_returns_stored_value_when_present() {
        let (mut ctx, _rx) = fresh_ctx();
        ctx.let_bindings
            .insert("__navigate_navA_trail".into(), "step1>step2>step3".into());
        assert_eq!(trail_navigation("navA", &ctx), "step1>step2>step3");
    }

    #[test]
    fn trail_returns_placeholder_when_not_stored() {
        let (ctx, _rx) = fresh_ctx();
        assert_eq!(trail_navigation("nav_missing", &ctx), "(trail of nav_missing)");
    }

    // ── Hibernate ────────────────────────────────────────────────────

    // §Fase 119.d — `run_hibernate_emits_wire_shape_and_marker` was DELETED
    // here: it pinned the F20 placeholder ("(hibernating …)" + marker binding
    // + Completed outcome) in green. The handler now returns the SUSPENSION
    // (`run_hibernate_returns_the_suspension_outcome` below) and the walk
    // loop parks + halts (tests/fase119_d_hibernate.rs).

    // ── Drill ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn run_drill_binds_result_under_output_name() {
        let (mut ctx, mut rx) = fresh_ctx();
        ctx.let_bindings
            .insert("__pix_law_corpus_civil.article23".into(), "Art. 23 text".into());
        let node = IRDrillStep {
            node_type: "drill",
            source_line: 0,
            source_column: 0,
            pix_ref: "law_corpus".into(),
            subtree_path: "civil.article23".into(),
            query: "interpret".into(),
            output_name: "article_text".into(),
        };
        run_drill(&node, &mut ctx).await.unwrap();
        assert_eq!(ctx.let_bindings.get("article_text").unwrap(), "Art. 23 text");
        let first = rx.try_recv().unwrap();
        match first {
            FlowExecutionEvent::StepStart { step_type, .. } => {
                assert_eq!(step_type, "drill");
            }
            e => panic!("expected StepStart, got {e:?}"),
        }
    }

    #[tokio::test]
    async fn run_drill_real_navigates_subtree_when_source_in_scope() {
        // §Fase 62.A.3 — with the source in scope, drill indexes it, locates the
        // named subtree by title-path, and navigates within it (no LLM, no
        // embeddings).
        let (mut ctx, _rx) = fresh_ctx();
        ctx.let_bindings.insert(
            "__pix_ContractIndex_source".into(),
            "# Liability\n## Limitation\nLiability is capped at the contract value.\n\
             # Termination\n## Notice\nThirty days notice."
                .into(),
        );
        let node = IRDrillStep {
            node_type: "drill",
            source_line: 0,
            source_column: 0,
            pix_ref: "ContractIndex".into(),
            subtree_path: "liability.limitation".into(),
            query: "cap on liability".into(),
            output_name: "clause".into(),
        };
        run_drill(&node, &mut ctx).await.unwrap();
        assert!(
            ctx.let_bindings.get("clause").unwrap().contains("capped at the contract value"),
            "drill should return the Limitation leaf content"
        );
    }

    #[tokio::test]
    async fn run_drill_placeholder_when_pix_not_seeded() {
        let (mut ctx, _rx) = fresh_ctx();
        let node = IRDrillStep {
            node_type: "drill",
            source_line: 0,
            source_column: 0,
            pix_ref: "unknown".into(),
            subtree_path: "root".into(),
            query: "q".into(),
            output_name: "result".into(),
        };
        run_drill(&node, &mut ctx).await.unwrap();
        assert_eq!(
            ctx.let_bindings.get("result").unwrap(),
            "(drilled unknown path=root query=q)"
        );
    }

    // ── Trail ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn run_trail_binds_under_canonical_key() {
        let (mut ctx, mut rx) = fresh_ctx();
        ctx.let_bindings
            .insert("__navigate_search1_trail".into(), "n1->n2->n3".into());
        let node = IRTrailStep {
            node_type: "trail",
            source_line: 0,
            source_column: 0,
            navigate_ref: "search1".into(),
        };
        run_trail(&node, &mut ctx).await.unwrap();
        assert_eq!(
            ctx.let_bindings.get("search1_trail_walked").unwrap(),
            "n1->n2->n3"
        );
        let first = rx.try_recv().unwrap();
        match first {
            FlowExecutionEvent::StepStart { step_type, .. } => {
                assert_eq!(step_type, "trail");
            }
            e => panic!("expected StepStart, got {e:?}"),
        }
    }

    // ── Cancel guards ────────────────────────────────────────────────

    #[tokio::test]
    async fn every_pix_handler_short_circuits_on_cancel() {
        let cancel = CancellationFlag::new();
        cancel.cancel();
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut ctx = DispatchCtx::new("F", "stub", "", cancel, tx);

        let h = IRHibernateStep {
            node_type: "hibernate",
            source_line: 0,
            source_column: 0,
            event_name: "e".into(),
            timeout: "1s".into(),
        };
        assert!(matches!(
            run_hibernate(&h, &mut ctx).await,
            Err(DispatchError::UpstreamCancelled)
        ));

        let d = IRDrillStep {
            node_type: "drill",
            source_line: 0,
            source_column: 0,
            pix_ref: "p".into(),
            subtree_path: "s".into(),
            query: "q".into(),
            output_name: "o".into(),
        };
        assert!(matches!(
            run_drill(&d, &mut ctx).await,
            Err(DispatchError::UpstreamCancelled)
        ));

        let t = IRTrailStep {
            node_type: "trail",
            source_line: 0,
            source_column: 0,
            navigate_ref: "n".into(),
        };
        assert!(matches!(
            run_trail(&t, &mut ctx).await,
            Err(DispatchError::UpstreamCancelled)
        ));
    }
}
