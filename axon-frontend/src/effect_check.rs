//! v2.87.0 — the static effect discipline: **D9 exhaustiveness**, the
//! resolution diagnostics of **the design decision**, and the clause-scope law.
//!
//! `the design plan` D9, verbatim:
//!
//! > Any `perform Effect.Op(...)` not discharged by an enclosing
//! > `handle Effect { ... }` before reaching the top-level run statement is a
//! > **compile error**. There is no runtime fallback.
//!
//! # Why this is INTERPROCEDURAL, and why that is not gold-plating
//!
//! The canonical program `the design plan` section 3.1 publishes performs in one place and
//! handles in another:
//!
//! ```text
//! flow generate(prompt: Text) -> Text {
//!     step Gen { ask: "…"  perform Emit(Gen.output) }      // knows nothing of SSE
//! }
//! flow stream_chat(user_input: Text) -> Text {
//!     handle SSE { Emit(t) -> { emit t on Wire  resume() } } in {
//!         run generate(prompt: user_input)                  // the discharge
//!     }
//! }
//! ```
//!
//! An intra-flow check would look correct and prove nothing: `generate`'s
//! `perform` has no enclosing `handle` *in its own body*, and refusing it would
//! make the composition the whole paradigm rests on illegal. Accepting it
//! blindly would let an adopter deploy `run generate(...)` with no handler
//! anywhere and hit `UnhandledEffect` at runtime — the fallback D9 exists to
//! forbid. **The only correct answer propagates the row through the call
//! graph**, which is what this module does.
//!
//! # The algorithm
//!
//! 1. Each flow gets an **effect row**: the effects it performs (directly, or
//!    through a flow it runs) that its own body does not handle. Computed as a
//!    monotone fixpoint over the call graph — rows only grow, the effect set is
//!    finite, so it terminates. A recursive call graph converges for the same
//!    reason.
//! 2. A row is discharged at an **entry point**. Every entry point is
//!    enumerated in `EffectChecker::check_entry_points`, and a leak at any of
//!    them is `axon-T966`. Missing one would be the hole that makes the whole
//!    check decorative, so the enumeration is exhaustive and named:
//!    a top-level `run`, an `axonendpoint`'s `execute:`, and a daemon
//!    `listen` body's `run`.
//! 3. The diagnostic names BOTH ends — the entry point that leaks and the
//!    `perform` that raised the effect — because "effect SSE is unhandled" at
//!    the run site is not something an author can act on.
//!
//! # What this module does NOT do
//!
//! **D11 (effect-row polymorphism) and D1 (operation polymorphism) are out of
//! scope for v2.87.0** and are recorded as such rather than half-built. Rows here
//! are CLOSED sets of concrete effect names — enough for exhaustiveness, which
//! is what makes `perform` safe. `ε`-variables over step signatures are a
//! separate piece of type-checker work.
//!
//! **D10 (linear resume: at most one `resume` per clause path) is enforced
//! here only in its STRUCTURAL form** — two `resume`s in one clause's
//! straight-line body (`EffectChecker::check_handle`). A `resume` on both
//! sides of an `if` is one per path and legal; two on a path that a loop
//! re-enters would need the control-flow graph D10 describes, and building half
//! of that would be worse than naming what is missing.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::ast::{
    Declaration, FlowStep, HandleBlock, ListenStep, Loc, PerformStep, Program, StepNode,
};
use crate::effect_catalog::{EffectCatalog, OpResolution};

/// One diagnostic: a message and the source position it belongs to.
pub struct EffectDiagnostic {
    pub message: String,
    pub loc: Loc,
}

/// Where an effect entered a flow's row, so a leak diagnostic can name it.
#[derive(Debug, Clone)]
struct RowOrigin {
    /// The `perform` that raised it (may be in a flow further down the graph).
    loc: Loc,
    /// A human description of where it came from — either "this flow" or the
    /// chain of flows it travelled.
    via: String,
}

/// effect name → where it came from. `BTreeMap` so diagnostics are emitted in a
/// DETERMINISTIC order; a compiler whose error list depends on hash iteration
/// cannot be asserted on by a gate.
type Row = BTreeMap<String, RowOrigin>;

/// The read-only context the fixpoint walker needs.
struct RowCtx<'c> {
    rows: &'c HashMap<String, Row>,
    catalog: &'c EffectCatalog,
}

impl RowCtx<'_> {
    /// Which effect does this site raise?
    ///
    /// **D9 must see through the BARE form.** `the design plan` section 3.1 publishes
    /// `perform Emit(response.token)`, and reading `PerformStep::effect_name`
    /// straight off the AST returns `None` for it — the resolution happens in
    /// the IR generator, downstream of here. Skipping unresolved sites would
    /// have made D9 fire only for the qualified spelling and stay silent for
    /// the published one: a check that is green exactly where it matters.
    fn effect_of(&self, explicit: Option<&str>, operation: &str) -> Option<String> {
        match self.catalog.resolve_site(explicit, operation) {
            OpResolution::Resolved(name) => Some(name),
            OpResolution::Ambiguous(_) | OpResolution::Undeclared => None,
        }
    }
}

pub struct EffectChecker<'a> {
    program: &'a Program,
    catalog: EffectCatalog,
    diagnostics: Vec<EffectDiagnostic>,
    /// Flow name → its effect row, at the current fixpoint iteration.
    rows: HashMap<String, Row>,
    /// Flow names declared in the program, for resolving `run` targets.
    flow_names: HashSet<String>,
}

impl<'a> EffectChecker<'a> {
    pub fn new(program: &'a Program) -> Self {
        let mut flow_names = HashSet::new();
        for decl in &program.declarations {
            if let Declaration::Flow(f) = decl {
                flow_names.insert(f.name.clone());
            }
        }
        Self {
            program,
            catalog: EffectCatalog::from_program(program),
            diagnostics: Vec::new(),
            rows: HashMap::new(),
            flow_names,
        }
    }

    /// Run every v2.87.0 static law and return the diagnostics, in source order.
    pub fn check(mut self) -> Vec<EffectDiagnostic> {
        // Laws that do not depend on the call graph: resolution, arity,
        // clause scope, handler well-formedness. Reported once, at their site.
        self.check_local_laws();

        // D9, in two stages: the fixpoint, then the entry points.
        self.compute_rows();
        self.check_entry_points();

        self.diagnostics
            .sort_by_key(|d| (d.loc.line, d.loc.column));
        self.diagnostics
    }

    // ════════════════════════════════════════════════════════════════
    //  Local laws — resolution, arity, scope
    // ════════════════════════════════════════════════════════════════

    fn check_local_laws(&mut self) {
        // Effect declarations themselves.
        for decl in &self.program.declarations {
            if let Declaration::Effect(eff) = decl {
                if eff.operations.is_empty() {
                    self.diagnostics.push(EffectDiagnostic {
                        message: format!(
                            "axon-T960 effect '{}' declares no operations. An effect with an \
                             empty operation set can never be performed, so a `handle {}` over \
                             it intercepts nothing — the frame would be dead code that reads \
                             like governance. Declare at least one operation.",
                            eff.name, eff.name
                        ),
                        loc: eff.loc.clone(),
                    });
                }
            }
        }

        // Flow bodies. `clause_ops` is the enclosing clause chain, used by the
        // scope law for `resume` / `abort` / `forward`.
        let mut bodies: Vec<(&[FlowStep], String)> = Vec::new();
        for decl in &self.program.declarations {
            match decl {
                Declaration::Flow(f) => bodies.push((&f.body, format!("flow '{}'", f.name))),
                Declaration::Daemon(d) => {
                    for l in &d.listeners {
                        bodies.push((&l.body, format!("daemon '{}' listen body", d.name)));
                    }
                }
                _ => {}
            }
        }
        for (body, _scope) in bodies {
            self.walk_local(body, &mut Vec::new(), 0);
        }
    }

    /// Walk a body applying the laws that need no call-graph knowledge.
    ///
    /// `frames` is the stack of enclosing `handle` frames (their effect-name
    /// sets); `clause_depth` counts enclosing handler CLAUSE bodies.
    fn walk_local(&mut self, body: &[FlowStep], frames: &mut Vec<BTreeSet<String>>, clause_depth: usize) {
        for step in body {
            match step {
                FlowStep::Handle(h) => self.check_handle(h, frames, clause_depth),
                FlowStep::Perform(p) => self.check_perform_site(p, "perform"),
                FlowStep::Forward(f) => {
                    let as_perform = PerformStep {
                        effect_name: f.effect_name.clone(),
                        operation_name: f.operation_name.clone(),
                        arguments: f.arguments.clone(),
                        loc: f.loc.clone(),
                    };
                    self.check_perform_site(&as_perform, "forward");
                    if clause_depth == 0 {
                        self.diagnostics.push(EffectDiagnostic {
                            message: format!(
                                "axon-T967 `forward {}` appears outside a handler clause. \
                                 `forward` propagates an operation to the NEXT OUTER frame, \
                                 which only means something inside a clause that is currently \
                                 intercepting it. Outside one there is no frame to bypass — \
                                 write `perform {}` instead.",
                                f.operation_name, f.operation_name
                            ),
                            loc: f.loc.clone(),
                        });
                    }
                }
                FlowStep::Resume(r) if clause_depth == 0 => {
                    self.diagnostics.push(EffectDiagnostic {
                        message: "axon-T967 `resume(…)` appears outside a handler clause. \
                                  `resume` invokes the continuation captured at a `perform` \
                                  site, and outside a clause there is no captured continuation \
                                  — the call would have nothing to return to."
                            .to_string(),
                        loc: r.loc.clone(),
                    });
                }
                FlowStep::Abort(a) if clause_depth == 0 => {
                    self.diagnostics.push(EffectDiagnostic {
                        message: "axon-T967 `abort(…)` appears outside a handler clause. \
                                  `abort` leaves the enclosing `handle` without resuming; \
                                  outside a clause there is no handle to leave."
                            .to_string(),
                        loc: a.loc.clone(),
                    });
                }
                FlowStep::Step(s) => self.check_step_local(s),
                FlowStep::If(c) => {
                    self.walk_local(&c.then_body, frames, clause_depth);
                    self.walk_local(&c.else_body, frames, clause_depth);
                }
                FlowStep::ForIn(f) => self.walk_local(&f.body, frames, clause_depth),
                FlowStep::Par(p) => {
                    for branch in &p.branches {
                        self.walk_local(branch, frames, clause_depth);
                    }
                }
                FlowStep::Listen(l) => self.walk_local(&l.body, frames, clause_depth),
                _ => {}
            }
        }
    }

    fn check_step_local(&mut self, step: &StepNode) {
        for p in &step.performs {
            self.check_perform_site(p, "perform");
        }
        for op in &step.pix_ops {
            let body = std::slice::from_ref(op);
            self.walk_local(body, &mut Vec::new(), 0);
        }
        if let Some(sb) = &step.stream {
            for arm in [&sb.on_chunk, &sb.on_complete, &sb.on_error].into_iter().flatten() {
                self.check_step_local(arm);
            }
            self.walk_local(&sb.body, &mut Vec::new(), 0);
        }
    }

    /// Handler well-formedness + **D10's structural half**.
    fn check_handle(
        &mut self,
        h: &HandleBlock,
        frames: &mut Vec<BTreeSet<String>>,
        clause_depth: usize,
    ) {
        for name in &h.effect_names {
            if !self.catalog.declares_effect(name) {
                let known = self.catalog.effect_names().join(", ");
                self.diagnostics.push(EffectDiagnostic {
                    message: format!(
                        "axon-T961 `handle {name}` names an effect that is not declared. \
                         A frame over an undeclared effect can never intercept anything — \
                         nothing could name an operation on it. Declared effects: [{}]",
                        if known.is_empty() { "none".to_string() } else { known }
                    ),
                    loc: h.loc.clone(),
                });
            }
        }

        for clause in &h.clauses {
            // The clause must handle an operation one of THIS frame's effects
            // declares. A clause for an operation the frame's effects do not
            // own is dead: no `perform` can ever route to it.
            let owner = h
                .effect_names
                .iter()
                .find(|e| self.catalog.declares_operation(e, &clause.operation_name));
            match owner {
                None => {
                    if h.effect_names.iter().all(|e| self.catalog.declares_effect(e)) {
                        self.diagnostics.push(EffectDiagnostic {
                            message: format!(
                                "axon-T962 clause `{}` handles an operation that none of the \
                                 effects this frame names ([{}]) declares. It is unreachable: \
                                 no `perform` can route to it, so the handler silently does \
                                 less than it reads as doing.",
                                clause.operation_name,
                                h.effect_names.join(", ")
                            ),
                            loc: clause.loc.clone(),
                        });
                    }
                }
                Some(effect) => {
                    // Arity. A clause binding fewer names than the operation
                    // declares parameters leaves an argument unbound at
                    // dispatch — the handler would read a name that resolves to
                    // nothing and put it on the wire verbatim.
                    if let Some(declared) = self.catalog.arity_of(effect, &clause.operation_name) {
                        if declared != clause.parameter_names.len() {
                            self.diagnostics.push(EffectDiagnostic {
                                message: format!(
                                    "axon-T963 clause `{}` binds {} parameter(s) but \
                                     `{effect}.{}` declares {declared}. A mismatch leaves an \
                                     argument unbound at dispatch, and an unbound name \
                                     resolves to itself — the handler would put the NAME on \
                                     the wire where the adopter expected the value.",
                                    clause.operation_name,
                                    clause.parameter_names.len(),
                                    clause.operation_name,
                                ),
                                loc: clause.loc.clone(),
                            });
                        }
                    }
                }
            }

            // D10, structurally: a clause body with two `resume`s on one
            // straight-line path would consume a one-shot continuation twice.
            // The runtime is allowed to ASSUME this cannot happen (D2 skips the
            // clone), so the compiler owes the guarantee.
            let straight_line_resumes = clause
                .body
                .iter()
                .filter(|s| matches!(s, FlowStep::Resume(_)))
                .count();
            if straight_line_resumes > 1 {
                let loc = clause
                    .body
                    .iter()
                    .filter_map(|s| match s {
                        FlowStep::Resume(r) => Some(r.loc.clone()),
                        _ => None,
                    })
                    .nth(1)
                    .unwrap_or_else(|| clause.loc.clone());
                self.diagnostics.push(EffectDiagnostic {
                    message: format!(
                        "axon-T968 clause `{}` invokes `resume` {straight_line_resumes} times \
                         on one path. Continuations are ONE-SHOT (the design plan D2): the runtime \
                         consumes the captured continuation on the first `resume` and does \
                         not clone it, so a second call has nothing to resume. Multi-shot \
                         resumption (backtracking, non-determinism) is deliberately not \
                         supported.",
                        clause.operation_name
                    ),
                    loc,
                });
            }

            self.walk_local(&clause.body, frames, clause_depth + 1);
        }

        let mut set: BTreeSet<String> = BTreeSet::new();
        set.extend(h.effect_names.iter().cloned());
        frames.push(set);
        self.walk_local(&h.body, frames, clause_depth);
        frames.pop();
    }

    /// the design decision — resolution and membership at one `perform` / `forward` site.
    fn check_perform_site(&mut self, p: &PerformStep, verb: &str) {
        match self.catalog.resolve_site(p.effect_name.as_deref(), &p.operation_name) {
            OpResolution::Ambiguous(owners) => {
                self.diagnostics.push(EffectDiagnostic {
                    message: format!(
                        "axon-T964 `{verb} {}` is AMBIGUOUS — {} effects declare an operation \
                         called `{}`: {}. Qualify it (`{verb} {}.{}(…)`) so the effect is the \
                         one you meant. Picking one for you is exactly the defect this refuses: \
                         a token routed to the logger instead of the wire is a bug that \
                         produces output, not an error.",
                        p.operation_name,
                        owners.len(),
                        p.operation_name,
                        owners.join(", "),
                        owners[0],
                        p.operation_name,
                    ),
                    loc: p.loc.clone(),
                });
            }
            OpResolution::Undeclared => {
                self.diagnostics.push(EffectDiagnostic {
                    message: format!(
                        "axon-T965 `{verb} {}` names an operation no declared effect owns. \
                         Declare it — `effect E {{ {}(…) -> T }}` — or correct the name. \
                         Declared effects: [{}]",
                        p.operation_name,
                        p.operation_name,
                        {
                            let names = self.catalog.effect_names().join(", ");
                            if names.is_empty() { "none".to_string() } else { names }
                        }
                    ),
                    loc: p.loc.clone(),
                });
            }
            OpResolution::Resolved(effect) => {
                // A QUALIFIED site is taken at its word by the catalog, so the
                // membership check happens here — and it must, or
                // `perform SSE.Emitt(x)` would lower with an effect name and an
                // operation the effect does not have, failing only at dispatch.
                if !self.catalog.declares_effect(&effect) {
                    self.diagnostics.push(EffectDiagnostic {
                        message: format!(
                            "axon-T965 `{verb} {effect}.{}` names an effect that is not \
                             declared. Declared effects: [{}]",
                            p.operation_name,
                            {
                                let names = self.catalog.effect_names().join(", ");
                                if names.is_empty() { "none".to_string() } else { names }
                            }
                        ),
                        loc: p.loc.clone(),
                    });
                } else if !self.catalog.declares_operation(&effect, &p.operation_name) {
                    self.diagnostics.push(EffectDiagnostic {
                        message: format!(
                            "axon-T965 effect `{effect}` declares no operation `{}`. \
                             Add it to the declaration or correct the name.",
                            p.operation_name
                        ),
                        loc: p.loc.clone(),
                    });
                } else if let Some(declared) =
                    self.catalog.arity_of(&effect, &p.operation_name)
                {
                    if declared != p.arguments.len() {
                        self.diagnostics.push(EffectDiagnostic {
                            message: format!(
                                "axon-T963 `{verb} {effect}.{}` passes {} argument(s) but the \
                                 operation declares {declared}. The handler binds parameters \
                                 positionally, so a mismatch leaves a binder unbound (or drops \
                                 a value on the floor) with no error at dispatch.",
                                p.operation_name,
                                p.arguments.len(),
                            ),
                            loc: p.loc.clone(),
                        });
                    }
                }
            }
        }
    }

    // ════════════════════════════════════════════════════════════════
    //  D9 — the interprocedural fixpoint
    // ════════════════════════════════════════════════════════════════

    fn compute_rows(&mut self) {
        for name in &self.flow_names.clone() {
            self.rows.insert(name.clone(), Row::new());
        }

        // Monotone: a row only ever gains entries, the effect set is finite, so
        // the loop terminates. The cap is a backstop against a bug in that
        // argument, not part of the argument — if it ever fires, the rows are
        // still SOUND (they are an under-approximation mid-iteration, and an
        // under-approximation of the leak set means a MISSED error, so the cap
        // is set far above any real program's call depth).
        let cap = self.flow_names.len().saturating_add(2) * 4 + 8;
        for _ in 0..cap {
            let mut changed = false;
            for decl in &self.program.declarations {
                let Declaration::Flow(f) = decl else { continue };
                let mut row = Row::new();
                let ctx = RowCtx {
                    rows: &self.rows,
                    catalog: &self.catalog,
                };
                Self::collect_row(&f.body, &mut Vec::new(), &ctx, &mut row);
                let prior = self.rows.get(&f.name);
                if prior.map(|p| p.len()) != Some(row.len())
                    || prior.is_some_and(|p| p.keys().ne(row.keys()))
                {
                    changed = true;
                }
                self.rows.insert(f.name.clone(), row);
            }
            if !changed {
                break;
            }
        }
    }

    /// Accumulate the effects `body` raises that `frames` does not discharge.
    ///
    /// An associated fn rather than a method so the fixpoint can read the
    /// previous iteration's rows while writing this one's.
    ///
    /// ⚠️ **`frames` holds only the ENCLOSING frames.** A `handle`'s own frame
    /// is pushed for its `in` body and NOT for its clause bodies, which is what
    /// makes a clause that performs the operation it handles escape to an outer
    /// frame instead of discharging itself — a self-discharge would be a
    /// compiler-blessed infinite loop. The same property is why `forward` needs
    /// no special stack surgery here: by construction its frame is already off.
    fn collect_row(
        body: &[FlowStep],
        frames: &mut Vec<BTreeSet<String>>,
        ctx: &RowCtx<'_>,
        out: &mut Row,
    ) {
        for step in body {
            match step {
                FlowStep::Perform(p) => {
                    Self::raise(
                        ctx.effect_of(p.effect_name.as_deref(), &p.operation_name),
                        &p.loc,
                        "a `perform` in this flow",
                        frames,
                        out,
                    );
                }
                FlowStep::Forward(f) => {
                    Self::raise(
                        ctx.effect_of(f.effect_name.as_deref(), &f.operation_name),
                        &f.loc,
                        "a `forward` to an outer frame",
                        frames,
                        out,
                    );
                }
                FlowStep::Handle(h) => {
                    for clause in &h.clauses {
                        Self::collect_row(&clause.body, frames, ctx, out);
                    }
                    let mut set: BTreeSet<String> = BTreeSet::new();
                    set.extend(h.effect_names.iter().cloned());
                    frames.push(set);
                    Self::collect_row(&h.body, frames, ctx, out);
                    frames.pop();
                }
                FlowStep::Run(r) => {
                    if let Some(callee) = ctx.rows.get(&r.flow_name) {
                        for (effect, origin) in callee {
                            Self::raise(
                                Some(effect.clone()),
                                &origin.loc,
                                &format!("`run {}`", r.flow_name),
                                frames,
                                out,
                            );
                        }
                    }
                }
                FlowStep::Step(s) => Self::collect_row_step(s, frames, ctx, out),
                FlowStep::If(c) => {
                    Self::collect_row(&c.then_body, frames, ctx, out);
                    Self::collect_row(&c.else_body, frames, ctx, out);
                }
                FlowStep::ForIn(f) => Self::collect_row(&f.body, frames, ctx, out),
                FlowStep::Par(p) => {
                    for branch in &p.branches {
                        Self::collect_row(branch, frames, ctx, out);
                    }
                }
                FlowStep::Listen(l) => Self::collect_row(&l.body, frames, ctx, out),
                _ => {}
            }
        }
    }

    fn collect_row_step(
        step: &StepNode,
        frames: &mut Vec<BTreeSet<String>>,
        ctx: &RowCtx<'_>,
        out: &mut Row,
    ) {
        for p in &step.performs {
            Self::raise(
                ctx.effect_of(p.effect_name.as_deref(), &p.operation_name),
                &p.loc,
                "a `perform` in a step body",
                frames,
                out,
            );
        }
        for op in &step.pix_ops {
            Self::collect_row(std::slice::from_ref(op), frames, ctx, out);
        }
        if let Some(sb) = &step.stream {
            for arm in [&sb.on_chunk, &sb.on_complete, &sb.on_error].into_iter().flatten() {
                Self::collect_row_step(arm, frames, ctx, out);
            }
            Self::collect_row(&sb.body, frames, ctx, out);
        }
    }

    /// Record `effect` as escaping unless some enclosing frame names it.
    ///
    /// `None` means the site did not resolve to exactly one effect — it already
    /// carries its own `axon-T964`/`axon-T965`, and raising a second error
    /// about an effect nobody could name would bury the first.
    fn raise(
        effect: Option<String>,
        loc: &Loc,
        via: &str,
        frames: &[BTreeSet<String>],
        out: &mut Row,
    ) {
        let Some(effect) = effect else { return };
        if frames.iter().any(|f| f.contains(&effect)) {
            return;
        }
        out.entry(effect).or_insert_with(|| RowOrigin {
            loc: loc.clone(),
            via: via.to_string(),
        });
    }

    // ════════════════════════════════════════════════════════════════
    //  D9 — the entry points
    // ════════════════════════════════════════════════════════════════

    /// Every way a flow can be entered from OUTSIDE any handler scope.
    ///
    /// This enumeration is the check. A missing entry point is a hole through
    /// which an undischarged effect reaches the runtime with a green build —
    /// which is the exact failure D9 forbids — so each one is named, and the
    /// list is:
    ///
    /// 1. a top-level `run F(…)` declaration;
    /// 2. an `axonendpoint E { execute: F }` — the deployed HTTP surface, and
    ///    the one an intra-flow-only check would have missed most expensively;
    /// 3. a `run F(…)` inside a daemon `listen` body (a scheduled autonomous
    ///    entry, outside every flow body).
    fn check_entry_points(&mut self) {
        let mut leaks: Vec<(String, String, RowOrigin, Loc)> = Vec::new();

        for decl in &self.program.declarations {
            match decl {
                Declaration::Run(r) => {
                    if let Some(row) = self.rows.get(&r.flow_name) {
                        for (effect, origin) in row {
                            leaks.push((
                                format!("top-level `run {}`", r.flow_name),
                                effect.clone(),
                                origin.clone(),
                                r.loc.clone(),
                            ));
                        }
                    }
                }
                Declaration::AxonEndpoint(ep) => {
                    if let Some(row) = self.rows.get(&ep.execute_flow) {
                        for (effect, origin) in row {
                            leaks.push((
                                format!(
                                    "axonendpoint '{}' (`execute: {}`)",
                                    ep.name, ep.execute_flow
                                ),
                                effect.clone(),
                                origin.clone(),
                                ep.loc.clone(),
                            ));
                        }
                    }
                }
                Declaration::Daemon(d) => {
                    for l in &d.listeners {
                        self.collect_listen_leaks(d.name.as_str(), l, &mut leaks);
                    }
                }
                _ => {}
            }
        }

        for (entry, effect, origin, loc) in leaks {
            self.diagnostics.push(EffectDiagnostic {
                message: format!(
                    "axon-T966 {entry} reaches a `perform` of effect '{effect}' that NO \
                     enclosing `handle` discharges (raised at line {}, via {}). the design plan D9: \
                     an undischarged effect is a COMPILE error — there is no runtime \
                     fallback, because a `perform` with no handler has nowhere to yield to \
                     and the program would have deployed before anyone found out. Wrap the \
                     entry in `handle {effect} {{ … }} in {{ … }}`, or handle it inside the \
                     flow.",
                    origin.loc.line, origin.via
                ),
                loc,
            });
        }
    }

    fn collect_listen_leaks(
        &self,
        daemon: &str,
        listener: &ListenStep,
        leaks: &mut Vec<(String, String, RowOrigin, Loc)>,
    ) {
        for step in &listener.body {
            if let FlowStep::Run(r) = step {
                if let Some(row) = self.rows.get(&r.flow_name) {
                    for (effect, origin) in row {
                        leaks.push((
                            format!("daemon '{daemon}' listen body (`run {}`)", r.flow_name),
                            effect.clone(),
                            origin.clone(),
                            r.loc.clone(),
                        ));
                    }
                }
            }
        }
    }
}
