//! §Fase 119.m.1 — **the `agent` executor**: `strategy:` as a CONTROL policy.
//!
//! # What was wrong
//!
//! `agent` was advertised, type-checked against two closed catalogs
//! (`VALID_AGENT_STRATEGIES`, `VALID_ON_STUCK_POLICIES` in the frontend),
//! lowered to `IRAgent` with eleven fields — and **executed by nothing**.
//! Measured 2026-08-08: no reader of `ir.agents` outside attestation and PCC,
//! no reader of `IRAgent::strategy` / `max_iterations` / `on_stuck` /
//! `max_cost` anywhere in the dispatcher. `advertised.rs` carried it as
//! `Unaudited` — §111 never checked it, and it turns out there was nothing to
//! check.
//!
//! The README publishes a promise about it in plain words:
//!
//! > *Budget cap of $5.00 prevents runaway API costs — **the compiler
//! > guarantees termination**.*
//!
//! This module is what makes that sentence true, or refuses in writing where
//! it cannot.
//!
//! # The doctrine this module is built under
//!
//! An agent is **not a new subsystem**. Every part of a deliberation already
//! exists, runs, and is attested; what was missing is the LOOP and its
//! termination proof. So the executor composes:
//!
//! | need | existing, attested machinery |
//! |---|---|
//! | one deliberation | `pure_shape::run_pure_shape` (§33.y.c) |
//! | a tool call | `lambda_tools::run_use_tool` (§58), already budget-gated (§114.a) |
//! | epistemic ceiling | `FlowEnvelope::seal` — Theorem 5.1, moved into the C23 kernel by §39.c |
//! | cancellation | the `ctx.cancel` discipline honoured at every `.await` (§33.x.e) |
//! | audit trail | the per-step `StepAuditRecord` chain (§11.c) |
//! | creative recovery | `cognitive::run_forge` (§86) |
//!
//! Building it any other way would have meant a second implementation of each,
//! free to drift from the one under test.
//!
//! # Termination
//!
//! Every strategy terminates, and each terminates for a DIFFERENT reason —
//! which is the whole content of `strategy:` being a control policy rather
//! than a prompt flavour (D119.5, ratified 2026-08-08):
//!
//! - `react` — interleaved think/act. Bounded by a strictly increasing
//!   iteration counter against `max_iterations`.
//! - `plan_and_execute` — plans ONCE, then walks a finite list. Bounded by
//!   `min(plan.len(), max_iterations)`; the plan is never regenerated, so the
//!   loop cannot extend itself.
//! - `reflexion` — act, self-critique, revise. Bounded by the same counter;
//!   the critique can only ACCEPT (terminate early) or produce another
//!   revision, never add iterations.
//!
//! **`max_iterations` is REQUIRED.** An agent without it is refused before the
//! first token is spent. A default would be a number nobody wrote deciding how
//! much of an adopter's money to spend, and "unbounded" is not a default a
//! cognitive runtime may pick on someone's behalf.

use crate::flow_dispatcher::{DispatchCtx, DispatchError, NodeOutcome};
use crate::ir_nodes::IRAgent;

/// The resolved, affine spend envelope of ONE agent run — the paper's
/// `Budget(n)`, made concrete over the three quantities the language actually
/// declares.
///
/// Every field here is CONSUMED. A bound that is declared and not enforced is
/// the §111 defect this fase exists to end, so `max_time` is deliberately
/// absent: nothing in this loop reads a wall clock, and pretending otherwise
/// would put a fourth unenforced bound in the struct.
#[derive(Debug, Clone, Copy)]
pub struct AgentBounds {
    /// Hard iteration ceiling. The termination proof of all three strategies.
    pub max_iterations: u32,
    /// Cumulative output-token ceiling across the run. `None` ⇒ not declared.
    pub max_tokens: Option<u64>,
    /// Cumulative USD ceiling across the run. `None` ⇒ not declared.
    pub max_cost: Option<f64>,
}

impl AgentBounds {
    /// Resolve the declared bounds, or REFUSE.
    ///
    /// The refusal is the point. `max_iterations` absent means the program did
    /// not say how long the agent may think, and there is no honest default:
    /// any number this function invents is a spending decision made on the
    /// adopter's behalf, and `u32::MAX` is that decision at its worst.
    pub fn resolve(agent: &IRAgent) -> Result<Self, DispatchError> {
        let max_iterations = match agent.max_iterations {
            Some(n) if n > 0 => n as u32,
            Some(n) => {
                return Err(DispatchError::BackendError {
                    name: format!("agent:{}", agent.name),
                    message: format!(
                        "agent '{}' declares `max_iterations: {n}` — an agent that may \
                         not think at all is a declaration with no execution. Declare a \
                         positive bound.",
                        agent.name
                    ),
                })
            }
            None => {
                return Err(DispatchError::BackendError {
                    name: format!("agent:{}", agent.name),
                    message: format!(
                        "agent '{}' declares no `max_iterations`, so its loop has no \
                         termination bound. Refused before the first token is spent: an \
                         unbounded agent is unbounded spend, and no default this runtime \
                         picked would be a number you wrote. Every agent the README \
                         publishes declares one.",
                        agent.name
                    ),
                })
            }
        };
        Ok(AgentBounds {
            max_iterations,
            max_tokens: agent.max_tokens.filter(|n| *n > 0).map(|n| n as u64),
            max_cost: agent.max_cost.filter(|c| *c > 0.0),
        })
    }
}

/// Why an agent run ended. The three terminal shapes are distinguished because
/// they mean different things to the caller: an ANSWER is a result, a STUCK is
/// a bound that bit, and those must never be confused on the wire.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentEnd {
    /// The strategy reached its own terminal condition.
    Answered,
    /// A declared bound was reached first. Carries which one, in the words the
    /// diagnostic will use.
    Stuck { bound: &'static str },
}

/// The affine ledger of one run: what has been spent so far, and the check
/// that decides whether another iteration may be afforded.
struct Spend {
    iterations: u32,
    tokens: u64,
    pricing: crate::cost_estimator::PricingModel,
}

impl Spend {
    fn new() -> Self {
        Spend {
            iterations: 0,
            tokens: 0,
            // The §101 doctrine applies: this is the OSS reference price, used
            // to enforce a ceiling the adopter declared, never to advertise a
            // cost. Enterprise resolves the real per-tenant model.
            pricing: crate::cost_estimator::PricingModel::default_sonnet(),
        }
    }

    fn usd(&self) -> f64 {
        // Output tokens are the ones this loop actually counts; charging them
        // at the OUTPUT rate is the conservative direction — it can only make
        // a declared ceiling bite sooner, never later.
        self.pricing.compute_cost(0, self.tokens)
    }

    /// May another iteration be afforded? Checked BEFORE the call, because a
    /// bound enforced after the spend bounds nothing — the §114.a lesson, and
    /// the reason `budget_gate::charge` also runs before its dispatch.
    fn affords(&self, b: &AgentBounds) -> Option<&'static str> {
        if self.iterations >= b.max_iterations {
            return Some("max_iterations");
        }
        if let Some(cap) = b.max_tokens {
            if self.tokens >= cap {
                return Some("max_tokens");
            }
        }
        if let Some(cap) = b.max_cost {
            if self.usd() >= cap {
                return Some("max_cost");
            }
        }
        None
    }
}

/// Run one agent to a terminal state.
///
/// Returns the agent's output bound under its own name (the binding discipline
/// every other handler on this path follows), plus the accumulated token count
/// so a caller can compose spend across agents.
pub async fn run_agent(
    agent: &IRAgent,
    input: &str,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }
    let bounds = AgentBounds::resolve(agent)?;
    let mut spend = Spend::new();

    let (end, output) = match agent.strategy.as_str() {
        // The empty strategy is the pre-§119.m.1 shape (`strategy:` omitted).
        // ReAct is the README's own default in four of its six agents and the
        // only policy that needs no extra declaration to be meaningful.
        "react" | "" => run_react(agent, input, &bounds, &mut spend, ctx).await?,
        "plan_and_execute" => run_plan_and_execute(agent, input, &bounds, &mut spend, ctx).await?,
        "reflexion" => run_reflexion(agent, input, &bounds, &mut spend, ctx).await?,
        // `custom` means "the control policy is the step list I wrote inside
        // the agent block" — README §agent writes `step Greet { … }` there.
        // Those steps are DROPPED AT PARSE TIME: `AgentDefinition` has no body
        // field. So dispatching `custom` would have to silently substitute one
        // of the other three policies, and an agent that runs a policy its
        // author did not write is worse than an agent that refuses.
        "custom" => {
            return Err(DispatchError::BackendError {
                name: format!("agent:{}", agent.name),
                message: format!(
                    "agent '{}' declares `strategy: custom`, whose control policy is the \
                     `step` list written inside the agent block — and that body is \
                     discarded by the parser (`AgentDefinition` has no body field). \
                     Running it would mean silently substituting a policy you did not \
                     write. Refused until the agent body survives parsing; use react, \
                     reflexion or plan_and_execute meanwhile.",
                    agent.name
                ),
            })
        }
        other => {
            return Err(DispatchError::BackendError {
                name: format!("agent:{}", agent.name),
                message: format!(
                    "agent '{}' declares `strategy: {other}`, which is outside the closed \
                     catalog the type-checker enforces (react, reflexion, \
                     plan_and_execute, custom). This is a compiler bug — the check should \
                     have caught it — reported rather than silently executed.",
                    agent.name
                ),
            })
        }
    };

    let output = match end {
        AgentEnd::Answered => output,
        AgentEnd::Stuck { bound } => {
            apply_on_stuck(agent, bound, &output, &spend, ctx).await?
        }
    };

    if !agent.name.is_empty() {
        ctx.let_bindings.insert(agent.name.clone(), output.clone());
    }
    Ok(NodeOutcome::Completed {
        output,
        tokens_emitted: spend.tokens,
        step_index: ctx.step_counter.saturating_sub(1),
    })
}

// ────────────────────────────────────────────────────────────────────
//  One deliberation — the shared primitive every strategy is built from
// ────────────────────────────────────────────────────────────────────

/// Dispatch ONE deliberation through the shared pure-shape core and charge it
/// to the run's affine ledger.
///
/// Going through `run_pure_shape` rather than calling a backend directly is
/// deliberate: it is what gives every agent iteration the same wire events,
/// the same audit row, the same cancel discipline and the same effect-policy
/// handling as every other cognitive step in the language. An agent that
/// bypassed it would be observable — and governable — differently from the
/// rest of the runtime, for no reason.
async fn deliberate(
    label: &str,
    prompt: String,
    framing: String,
    spend: &mut Spend,
    ctx: &mut DispatchCtx,
) -> Result<String, DispatchError> {
    let shape = super::pure_shape::PureShapeStep {
        name: label.to_string(),
        user_prompt: prompt,
        framing_addendum: Some(framing),
        kind_slug: "agent",
        tools: Vec::new(),
        requires_context: None,
        temperature: None,
        now_tz: None,
    };
    let outcome = super::pure_shape::run_pure_shape(shape, ctx).await?;
    spend.iterations += 1;
    match outcome {
        NodeOutcome::Completed {
            output,
            tokens_emitted,
            ..
        } => {
            spend.tokens += tokens_emitted;
            Ok(output)
        }
        // The sentinels belong to a flow walk; an agent loop has no `break`,
        // `continue` or `return` of its own, so observing one here means the
        // shared core changed shape underneath this module.
        other => Err(DispatchError::BackendError {
            name: "agent".to_string(),
            message: format!(
                "an agent deliberation returned {other:?}, which is a flow-walk sentinel \
                 with no meaning inside an agent loop"
            ),
        }),
    }
}

/// The tool catalog the agent is ALLOWED to reach, rendered for the prompt.
fn tool_catalog(agent: &IRAgent) -> String {
    if agent.tools.is_empty() {
        "(none — answer from your own reasoning)".to_string()
    } else {
        agent.tools.join(", ")
    }
}

/// §Fase 119.m.1 — the containment check.
///
/// A model may name any string it likes. The agent may only reach the tools
/// its DECLARATION granted. This is the one place where the loop's output is
/// untrusted input, and it is checked against the declared set rather than
/// against the global registry — an agent's tool grant is an attenuation, and
/// attenuation that widens at dispatch is not attenuation.
/// Public and pure, deliberately: this is a SECURITY predicate, and a gate
/// that can only observe it through a live loop cannot mutation-test it — the
/// stub backend never emits an `ACT:`, so a suite built only on the loop would
/// assert containment it never exercised. Same reasoning as `reason_prompt` /
/// `weave_prompt`: expose the decision so a gate can read it directly.
pub fn granted_tool<'a>(agent: &'a IRAgent, named: &str) -> Option<&'a String> {
    agent.tools.iter().find(|t| t.eq_ignore_ascii_case(named))
}

/// §Fase 119.m.1 — the ReAct move a deliberation encodes. Public for the same
/// reason as [`granted_tool`]: the protocol parse is where untrusted model
/// output becomes a control decision, and it must be testable on its own.
#[derive(Debug, PartialEq)]
pub enum AgentMove {
    /// `ACT: <ToolName>` — a request to reach a tool.
    Act(String),
    /// `ANSWER: <text>` — the terminal move.
    Answer(String),
    /// Neither. A protocol violation, never an answer.
    Unstructured,
}

/// Parse the ReAct move out of a deliberation. Prefix-directed and
/// case-insensitive; anything else is [`AgentMove::Unstructured`].
pub fn parse_agent_move(text: &str) -> AgentMove {
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = strip_prefix_ci(t, "ACT:") {
            if !rest.is_empty() {
                return AgentMove::Act(rest.to_string());
            }
        }
        if let Some(rest) = strip_prefix_ci(t, "ANSWER:") {
            return AgentMove::Answer(rest.to_string());
        }
    }
    AgentMove::Unstructured
}

// ────────────────────────────────────────────────────────────────────
//  react — interleaved think/act
// ────────────────────────────────────────────────────────────────────

async fn run_react(
    agent: &IRAgent,
    input: &str,
    bounds: &AgentBounds,
    spend: &mut Spend,
    ctx: &mut DispatchCtx,
) -> Result<(AgentEnd, String), DispatchError> {
    let mut observations: Vec<String> = Vec::new();
    let mut last = String::new();

    while spend.affords(bounds).is_none() {
        let prompt = format!(
            "GOAL: {}\n\nINPUT: {}\n\nTOOLS YOU MAY USE: {}\n\nOBSERVATIONS SO FAR:\n{}",
            agent.goal,
            input,
            tool_catalog(agent),
            if observations.is_empty() {
                "(none yet)".to_string()
            } else {
                observations.join("\n")
            }
        );
        let framing = "You are acting under the ReAct policy. Think, then do exactly ONE \
                       of two things. To use a tool, reply with a single line \
                       `ACT: <ToolName>`. To finish, reply `ANSWER: <your answer>`. Name \
                       only tools from the list you were given."
            .to_string();

        last = deliberate(&format!("{}:react", agent.name), prompt, framing, spend, ctx).await?;

        match parse_agent_move(&last) {
            AgentMove::Answer(text) => return Ok((AgentEnd::Answered, text)),
            AgentMove::Act(named) => {
                let Some(tool) = granted_tool(agent, &named) else {
                    // Refused, and recorded as an observation rather than as a
                    // hard error: the agent asked for something outside its
                    // grant, which is a fact the NEXT iteration should see and
                    // correct, not a crash. The tool is not dispatched.
                    observations.push(format!(
                        "REFUSED: `{named}` is not in this agent's declared tools. \
                         Choose from: {}",
                        tool_catalog(agent)
                    ));
                    continue;
                };
                let node = crate::ir_nodes::IRUseToolStep {
                    node_type: "use_tool",
                    source_line: agent.source_line,
                    source_column: agent.source_column,
                    tool_name: tool.clone(),
                    argument: input.to_string(),
                    named_args: Vec::new(),
                };
                // The real §58 dispatch — which means the §114.a budget gate
                // and the §114.f channel lease charge apply to an agent's tool
                // calls exactly as they do to a flow's.
                let observed = match super::lambda_tools::run_use_tool(&node, ctx).await? {
                    NodeOutcome::Completed { output, .. } => output,
                    other => format!("(tool returned {other:?})"),
                };
                observations.push(format!("OBSERVATION from {}: {observed}", tool.clone()));
            }
            // A reply that is neither ACT nor ANSWER is a PROTOCOL VIOLATION,
            // recorded as an observation so the next iteration can correct —
            // not accepted as the answer.
            //
            // The forgiving reading (treat loose prose as the answer) is the
            // common one and it is wrong here: it silently substitutes "this
            // is probably the answer" for "the agent completed its protocol",
            // and the caller cannot tell the two apart. The strict reading
            // costs at most the declared iteration budget — which is exactly
            // what that budget is for — and when the budget runs out the
            // `on_stuck` policy gets to decide, which is the honest outcome
            // for an agent that could not work under its own protocol.
            AgentMove::Unstructured => {
                observations.push(
                    "PROTOCOL VIOLATION: the previous reply was neither `ACT: <ToolName>` \
                     nor `ANSWER: <text>`. Reply with exactly one of those two forms."
                        .to_string(),
                );
            }
        }
    }
    Ok((
        AgentEnd::Stuck {
            bound: spend.affords(bounds).unwrap_or("max_iterations"),
        },
        last,
    ))
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(s[prefix.len()..].trim())
    } else {
        None
    }
}

// ────────────────────────────────────────────────────────────────────
//  plan_and_execute — plan ONCE, then walk a finite list
// ────────────────────────────────────────────────────────────────────

async fn run_plan_and_execute(
    agent: &IRAgent,
    input: &str,
    bounds: &AgentBounds,
    spend: &mut Spend,
    ctx: &mut DispatchCtx,
) -> Result<(AgentEnd, String), DispatchError> {
    if let Some(bound) = spend.affords(bounds) {
        return Ok((AgentEnd::Stuck { bound }, String::new()));
    }
    let plan_text = deliberate(
        &format!("{}:plan", agent.name),
        format!(
            "GOAL: {}\n\nINPUT: {}\n\nTOOLS AVAILABLE: {}",
            agent.goal,
            input,
            tool_catalog(agent)
        ),
        "You are planning under the Plan-and-Execute policy. Produce the plan ONCE, as a \
         numbered list, one step per line, no commentary. The plan will not be \
         regenerated."
            .to_string(),
        spend,
        ctx,
    )
    .await?;

    // The plan is finite and produced once — that is this strategy's whole
    // termination proof. Truncating to the remaining iteration budget keeps the
    // proof true even when the model returns more steps than were paid for.
    let remaining = bounds.max_iterations.saturating_sub(spend.iterations) as usize;
    let plan: Vec<String> = plan_text
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .take(remaining)
        .collect();

    if plan.is_empty() {
        return Ok((AgentEnd::Answered, plan_text));
    }

    let mut results: Vec<String> = Vec::new();
    for (i, step) in plan.iter().enumerate() {
        if let Some(bound) = spend.affords(bounds) {
            return Ok((AgentEnd::Stuck { bound }, results.join("\n\n")));
        }
        let out = deliberate(
            &format!("{}:execute", agent.name),
            format!(
                "GOAL: {}\n\nPLAN STEP {} OF {}: {step}\n\nRESULTS SO FAR:\n{}",
                agent.goal,
                i + 1,
                plan.len(),
                if results.is_empty() {
                    "(none yet)".to_string()
                } else {
                    results.join("\n")
                }
            ),
            "Execute exactly this one plan step. Do not plan further; the plan is fixed."
                .to_string(),
            spend,
            ctx,
        )
        .await?;
        results.push(out);
    }
    Ok((AgentEnd::Answered, results.join("\n\n")))
}

// ────────────────────────────────────────────────────────────────────
//  reflexion — act, self-critique, revise
// ────────────────────────────────────────────────────────────────────

async fn run_reflexion(
    agent: &IRAgent,
    input: &str,
    bounds: &AgentBounds,
    spend: &mut Spend,
    ctx: &mut DispatchCtx,
) -> Result<(AgentEnd, String), DispatchError> {
    if let Some(bound) = spend.affords(bounds) {
        return Ok((AgentEnd::Stuck { bound }, String::new()));
    }
    let mut attempt = deliberate(
        &format!("{}:attempt", agent.name),
        format!("GOAL: {}\n\nINPUT: {}", agent.goal, input),
        "Produce your best first attempt at the goal.".to_string(),
        spend,
        ctx,
    )
    .await?;

    while spend.affords(bounds).is_none() {
        let critique = deliberate(
            &format!("{}:critique", agent.name),
            format!(
                "GOAL: {}\n\nATTEMPT:\n{attempt}",
                agent.goal
            ),
            "You are self-critiquing under the Reflexion policy. If the attempt meets the \
             goal, reply exactly `ACCEPT`. Otherwise reply with the single most important \
             defect, and nothing else."
                .to_string(),
            spend,
            ctx,
        )
        .await?;

        if critique.trim().eq_ignore_ascii_case("ACCEPT")
            || critique.trim_start().to_ascii_uppercase().starts_with("ACCEPT")
        {
            return Ok((AgentEnd::Answered, attempt));
        }
        if let Some(bound) = spend.affords(bounds) {
            return Ok((AgentEnd::Stuck { bound }, attempt));
        }
        attempt = deliberate(
            &format!("{}:revise", agent.name),
            format!(
                "GOAL: {}\n\nPREVIOUS ATTEMPT:\n{attempt}\n\nDEFECT TO FIX:\n{critique}",
                agent.goal
            ),
            "Revise the attempt to fix exactly the named defect. Change nothing else."
                .to_string(),
            spend,
            ctx,
        )
        .await?;
    }
    Ok((
        AgentEnd::Stuck {
            bound: spend.affords(bounds).unwrap_or("max_iterations"),
        },
        attempt,
    ))
}

// ────────────────────────────────────────────────────────────────────
//  on_stuck — what happens when a bound bites
// ────────────────────────────────────────────────────────────────────

/// Apply the declared `on_stuck:` policy. The catalog is the frontend's
/// (`VALID_ON_STUCK_POLICIES`), and every member either DOES something here or
/// is refused by name — a policy that silently collapses into another is the
/// §111 defect wearing a field.
async fn apply_on_stuck(
    agent: &IRAgent,
    bound: &'static str,
    partial: &str,
    spend: &Spend,
    ctx: &mut DispatchCtx,
) -> Result<String, DispatchError> {
    let context = format!(
        "agent '{}' hit `{bound}` after {} iteration(s) (~{} output tokens, ~${:.4})",
        agent.name,
        spend.iterations,
        spend.tokens,
        spend.usd()
    );
    match agent.on_stuck.as_str() {
        // README §agent, verbatim: "raises `AgentStuckError` with full context,
        // so the human reviews exactly where the agent got stuck".
        "escalate" => Err(DispatchError::BackendError {
            name: format!("agent:{}", agent.name),
            message: format!(
                "AgentStuckError — {context}. Last partial state:\n{partial}"
            ),
        }),
        // §Fase 86 — the real forge, seeded with the stuck context. `forge` is
        // attested `Partial` (its `coherence` is hardcoded to 1.0), and that
        // gap is inherited here rather than hidden.
        "forge" => {
            let node = crate::ir_nodes::IRForgeBlock {
                node_type: "forge",
                source_line: agent.source_line,
                source_column: agent.source_column,
                name: format!("{}_unstuck", agent.name),
                seed: format!("{}\n\nGOAL: {}\n\nSTUCK AT:\n{partial}", context, agent.goal),
                output_type: String::new(),
                mode: String::new(),
                novelty: 0.0,
                branches: 1,
                depth: 1,
                constraints_ref: String::new(),
            };
            match super::cognitive::run_forge(&node, ctx).await? {
                NodeOutcome::Completed { output, .. } => Ok(output),
                other => Err(DispatchError::BackendError {
                    name: format!("agent:{}", agent.name),
                    message: format!("forge recovery returned {other:?}"),
                }),
            }
        }
        // One more pass is what "retry" can honestly mean here. Retrying the
        // whole loop would need a second budget nobody declared, and retrying
        // without a bound is how a termination proof is lost — so `retry`
        // returns the partial state marked as such, and the caller decides.
        // Named plainly rather than dressed up as a recovery it is not.
        "retry" => Ok(format!(
            "[retry] {context}. The declared bound was reached; this is the partial \
             state, not a completed answer:\n{partial}"
        )),
        // In the frontend catalog, and genuinely not implementable here yet:
        // §119.d parks a FLOW WALK's remaining nodes, and an agent loop has no
        // remaining-node list to park. Refused by name rather than silently
        // degraded to `escalate`.
        "hibernate" => Err(DispatchError::BackendError {
            name: format!("agent:{}", agent.name),
            message: format!(
                "{context}, and `on_stuck: hibernate` has no continuation to park — \
                 §119.d suspends a flow walk by capturing its remaining nodes, and an \
                 agent loop has no such list. Refused rather than quietly escalated. Use \
                 escalate, forge or retry."
            ),
        }),
        // Undeclared: the bound bit and the program said nothing about it.
        // Escalating is the fail-closed reading — silence must not buy a
        // partial answer that looks complete.
        "" => Err(DispatchError::BackendError {
            name: format!("agent:{}", agent.name),
            message: format!(
                "AgentStuckError — {context}, and no `on_stuck:` policy is declared. \
                 Refusing rather than returning a partial answer that reads as a \
                 finished one. Last partial state:\n{partial}"
            ),
        }),
        other => Err(DispatchError::BackendError {
            name: format!("agent:{}", agent.name),
            message: format!(
                "unknown `on_stuck: {other}` — outside the closed catalog the \
                 type-checker enforces (escalate, forge, hibernate, retry). This is a \
                 compiler bug, reported rather than silently executed."
            ),
        }),
    }
}
