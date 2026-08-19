//! v2.83.0 — **the `agent` executor**: `strategy:` as a CONTROL policy.
//!
//! # What was wrong
//!
//! `agent` was advertised, type-checked against two closed catalogs
//! (`VALID_AGENT_STRATEGIES`, `VALID_ON_STUCK_POLICIES` in the frontend),
//! lowered to `IRAgent` with eleven fields — and **executed by nothing**.
//! Measured 2026-08-08: no reader of `ir.agents` outside attestation and PCC,
//! no reader of `IRAgent::strategy` / `max_iterations` / `on_stuck` /
//! `max_cost` anywhere in the dispatcher. `advertised.rs` carried it as
//! `Unaudited` — v2.67.0 never checked it, and it turns out there was nothing to
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
//! | one deliberation | `pure_shape::run_pure_shape` (v1.24.0) |
//! | a tool call | `lambda_tools::run_use_tool` (v2.8.0), already budget-gated (v2.69.0) |
//! | epistemic ceiling | `FlowEnvelope::seal` — Theorem 5.1, moved into the C23 kernel by v2.0.0 |
//! | cancellation | the `ctx.cancel` discipline honoured at every `.await` (v1.24.0) |
//! | audit trail | the per-step `StepAuditRecord` chain (v1.4.0) |
//! | creative recovery | `cognitive::run_forge` (v2.41.0) |
//!
//! Building it any other way would have meant a second implementation of each,
//! free to drift from the one under test.
//!
//! # Termination
//!
//! Every strategy terminates, and each terminates for a DIFFERENT reason —
//! which is the whole content of `strategy:` being a control policy rather
//! than a prompt flavour (ratified 2026-08-08):
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
//! cognitive runtime may pick on someone's behalf. Since 4.1.0 the COMPILER
//! refuses it first (`axon-T1216`, the same predicate `savant` has carried as
//! `axon-T877`); the refusal below is defence in depth for IR that bypassed
//! `axon check`, the same posture the channel κ-coverage law takes at `publish`.
//!
//! # The fourth strategy and the fourth bound (4.1.0)
//!
//! - `custom` — the step sequence written inside the agent block IS the
//!   policy (README: "follows a user-defined step sequence, not a generic
//!   loop"). Each step is one deliberation through the ordinary step handler,
//!   so `max_iterations` bounds it exactly as it bounds the others:
//!   `min(body.len(), max_iterations)`. The parser used to DROP that body and
//!   this module refused `custom` by name; both halves are gone.
//! - `max_time` — a wall-clock ceiling (`30s`, `2m`), checked before every
//!   deliberation against the instant the run started. It used to be parsed,
//!   lowered, and read by nothing; a bound declared and not enforced is the
//!   defect class this module exists to end, so it is enforced or refused
//!   (`axon-T1220` refuses what the shared duration grammar cannot read).
//! - `return: T` — when `T` is a declared struct type, the final answer must
//!   be a JSON object carrying T's fields; a non-conforming answer is a
//!   STUCK outcome (`bound: "return"`) routed through `on_stuck`, never a
//!   free-text success wearing a type's name.

use crate::flow_dispatcher::{DispatchCtx, DispatchError, NodeOutcome};
use crate::ir_nodes::IRAgent;

/// The resolved, affine spend envelope of ONE agent run — the paper's
/// `Budget(n)`, made concrete over the four quantities the language declares.
///
/// Every field here is CONSUMED. A bound that is declared and not enforced is
/// the defect class this module exists to end: `max_time` was absent from this
/// struct for exactly that reason until the loop learned to read a clock
/// (`Spend::affords` checks it before every deliberation).
#[derive(Debug, Clone, Copy)]
pub struct AgentBounds {
    /// Hard iteration ceiling. The termination proof of every strategy.
    pub max_iterations: u32,
    /// Cumulative output-token ceiling across the run. `None` ⇒ not declared.
    pub max_tokens: Option<u64>,
    /// Cumulative USD ceiling across the run. `None` ⇒ not declared.
    pub max_cost: Option<f64>,
    /// Wall-clock ceiling for the whole run, measured from `run_agent` entry.
    /// `None` ⇒ not declared. Parsed by the ONE duration grammar the checker
    /// uses (`axon_frontend::type_checker::parse_duration_ms`), so an agent
    /// that passed `axon check` cannot reach here with an unreadable value.
    pub max_time: Option<std::time::Duration>,
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
        let max_time = if agent.max_time.trim().is_empty() {
            None
        } else {
            match crate::type_checker::parse_duration_ms(&agent.max_time) {
                Some(ms) => Some(std::time::Duration::from_millis(ms)),
                None => {
                    return Err(DispatchError::BackendError {
                        name: format!("agent:{}", agent.name),
                        message: format!(
                            "agent '{}' declares `max_time: {}`, which is not a duration \
                             (`500ms`, `30s`, `2m`, `1h`). axon-T1220 refuses this at \
                             check time; refused here too rather than run with a bound \
                             that cannot be read.",
                            agent.name, agent.max_time
                        ),
                    })
                }
            }
        };
        Ok(AgentBounds {
            max_iterations,
            max_tokens: agent.max_tokens.filter(|n| *n > 0).map(|n| n as u64),
            max_cost: agent.max_cost.filter(|c| *c > 0.0),
            max_time,
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
    /// When the run started — the origin `max_time` is measured from.
    started: std::time::Instant,
}

impl Spend {
    fn new() -> Self {
        Spend {
            iterations: 0,
            tokens: 0,
            started: std::time::Instant::now(),
            // The v2.54.0 doctrine applies: this is the OSS reference price, used
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
    /// bound enforced after the spend bounds nothing — the v2.69.0 lesson, and
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
        if let Some(cap) = b.max_time {
            if self.started.elapsed() >= cap {
                return Some("max_time");
            }
        }
        None
    }
}

/// v2.83.0 — dispatch an `<Agent>(arg, …)` call.
///
/// Resolves the declaration from `ctx.agent_specs` and FAILS CLOSED when it is
/// absent. That is not politeness: the declaration is where `max_iterations`
/// lives, so an unresolved agent is an unbounded one, and a runtime that ran it
/// anyway would spend against a ceiling nobody wrote.
///
/// The arguments are v2.83.0 subjects, resolved against the flow bindings —
/// `TrendAnalyzer(Gather.output)` hands the agent the prior step's VALUE, not
/// the string `"Gather.output"`. Quoted literals keep v2.10.0's classification.
pub async fn run_agent_call(
    node: &crate::ir_nodes::IRAgentCall,
    ctx: &mut DispatchCtx,
) -> Result<NodeOutcome, DispatchError> {
    // D3 — cancel is checked at handler ENTRY, before anything else including
    // catalog resolution. Caught by v1.24.0's cancel-propagation fuzz, which
    // drives every variant pre-cancelled and demands `UpstreamCancelled`
    // uniformly: resolving first meant a cancelled run reported a MISSING AGENT
    // instead of a cancellation, which is a wrong diagnosis for the operator
    // and a broken invariant for the dispatcher.
    if ctx.cancel.is_cancelled() {
        return Err(DispatchError::UpstreamCancelled);
    }
    let Some(agent) = ctx
        .agent_specs
        .iter()
        .find(|a| a.name == node.agent_name)
        .cloned()
    else {
        return Err(DispatchError::BackendError {
            name: format!("agent:{}", node.agent_name),
            message: format!(
                "no `agent {}` is declared in this program, so its bounds — including \
                 `max_iterations` — cannot be resolved. Refused rather than run \
                 unbounded. ({} agent(s) in this catalog.)",
                node.agent_name,
                ctx.agent_specs.len()
            ),
        });
    };

    let input = node
        .arguments
        .iter()
        .map(|a| match a.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
            // A quoted argument is a literal, not a name to look up — v2.10.0's
            // classification, preserved by v2.83.0's parser.
            Some(literal) => literal.to_string(),
            None => crate::exec_context::resolve_value_reference(a, &ctx.let_bindings),
        })
        .collect::<Vec<_>>()
        .join("\n");

    run_agent(&agent, &input, ctx).await
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
        // The empty strategy is the pre-v2.83.0 shape (`strategy:` omitted).
        // ReAct is the README's own default in four of its six agents and the
        // only policy that needs no extra declaration to be meaningful.
        "react" | "" => run_react(agent, input, &bounds, &mut spend, ctx).await?,
        "plan_and_execute" => run_plan_and_execute(agent, input, &bounds, &mut spend, ctx).await?,
        "reflexion" => run_reflexion(agent, input, &bounds, &mut spend, ctx).await?,
        // `custom` means "the control policy is the step list I wrote inside
        // the agent block" — README writes `step Greet { … }` there. The body
        // survives parsing since 4.1.0 (`IRAgent.body`); an EMPTY body under
        // `custom` is `axon-T1217` at check time and refused here as well.
        "custom" => run_custom(agent, input, &bounds, &mut spend, ctx).await?,
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

    // `return: T` over a declared struct type — the answer must inhabit it.
    // Checked on the ANSWERED path only: a stuck run's partial state is already
    // labelled as not-an-answer by `on_stuck`, and double-refusing it would
    // hide which bound bit.
    let end = match end {
        AgentEnd::Answered if !agent.return_schema.is_empty() => {
            match return_schema_defect(&agent.return_schema, &output) {
                None => AgentEnd::Answered,
                Some(why) => {
                    ctx.let_bindings
                        .insert(format!("{}_return_defect", agent.name), why);
                    AgentEnd::Stuck { bound: "return" }
                }
            }
        }
        other => other,
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
        stream_on_chunk: None,
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

/// The ReAct framing. `pub` because the stub backend recognises it: a stub
/// that answers `"(stub)"` to a ReAct prompt never terminates the protocol on
/// its own, so `run --tool-mode stub` showed no iteration and no dispatch. The
/// stub answers the PROTOCOL instead (`ACT:` once, then `ANSWER:`), and the
/// loop still does every parse, grant check and dispatch itself.
pub const REACT_FRAMING: &str = "You are acting under the ReAct policy. Think, then do exactly ONE \
                       of two things. To use a tool, reply with a single line \
                       `ACT: <ToolName>`. To finish, reply `ANSWER: <your answer>`. Name \
                       only tools from the list you were given.";
/// The Reflexion critique framing (see [`REACT_FRAMING`] for why it is public).
pub const REFLEXION_CRITIQUE_FRAMING: &str = "You are self-critiquing under the Reflexion policy. If the attempt meets the \
             goal, reply exactly `ACCEPT`. Otherwise reply with the single most important \
             defect, and nothing else.";
/// The prompt line that names the granted tools; the stub backend reads the
/// first tool from it.
pub const TOOLS_LINE_PREFIX: &str = "TOOLS YOU MAY USE: ";
/// The prompt line that lists observations; `(none yet)` ⇒ first iteration.
pub const NO_OBSERVATIONS_YET: &str = "(none yet)";

/// The rendering of a declared `return:` schema inside the ANSWER instruction.
fn return_schema_instruction(agent: &IRAgent) -> String {
    if agent.return_schema.is_empty() {
        return String::new();
    }
    let fields: Vec<String> = agent
        .return_schema
        .iter()
        .map(|f| {
            let t = if f.generic_param.is_empty() {
                f.type_name.clone()
            } else {
                format!("{}<{}>", f.type_name, f.generic_param)
            };
            format!("{}{}: {t}", f.name, if f.optional { "?" } else { "" })
        })
        .collect();
    format!(
        " Your ANSWER must be a single JSON object of type {} with fields {{ {} }} — \
         no prose around it.",
        agent.return_type,
        fields.join(", ")
    )
}

/// Does `answer` inhabit the declared return schema? `None` when it does; a
/// diagnostic naming the first defect otherwise. The check is structural and
/// total: every required field present, each field's JSON shape matching its
/// declared primitive (`String` ⇒ string, `Integer` ⇒ integer number, `Float`
/// ⇒ number, `Boolean` ⇒ bool, `List<…>` ⇒ array, anything else ⇒ present).
/// Public so a gate can read the decision without a live loop.
pub fn return_schema_defect(schema: &[crate::ir_nodes::IRTypeField], answer: &str) -> Option<String> {
    let text = answer.trim();
    // A model may fence the object; strip one ``` fence if present.
    let text = text
        .strip_prefix("```json")
        .or_else(|| text.strip_prefix("```"))
        .map(|t| t.trim_end_matches("```").trim())
        .unwrap_or(text);
    let value: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => return Some(format!("the answer is not a JSON object ({e})")),
    };
    let obj = match value.as_object() {
        Some(o) => o,
        None => return Some("the answer is JSON but not an object".to_string()),
    };
    for f in schema {
        match obj.get(&f.name) {
            None | Some(serde_json::Value::Null) if f.optional => {}
            None | Some(serde_json::Value::Null) => {
                return Some(format!("required field `{}` is missing", f.name))
            }
            Some(v) => {
                let ok = match f.type_name.as_str() {
                    "String" => v.is_string(),
                    "Integer" => v.as_i64().is_some() || v.as_u64().is_some(),
                    "Float" => v.is_number(),
                    "Boolean" => v.is_boolean(),
                    "List" => v.is_array(),
                    _ => true,
                };
                if !ok {
                    return Some(format!(
                        "field `{}` is declared `{}` but the answer carries {}",
                        f.name,
                        f.type_name,
                        match v {
                            serde_json::Value::String(_) => "a string",
                            serde_json::Value::Number(_) => "a number",
                            serde_json::Value::Bool(_) => "a boolean",
                            serde_json::Value::Array(_) => "an array",
                            serde_json::Value::Object(_) => "an object",
                            serde_json::Value::Null => "null",
                        }
                    ));
                }
            }
        }
    }
    None
}

/// The tool catalog the agent is ALLOWED to reach, rendered for the prompt.
fn tool_catalog(agent: &IRAgent) -> String {
    if agent.tools.is_empty() {
        "(none — answer from your own reasoning)".to_string()
    } else {
        agent.tools.join(", ")
    }
}

/// v2.83.0 — the containment check.
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

/// v2.83.0 — the ReAct move a deliberation encodes. Public for the same
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
            "GOAL: {}\n\nINPUT: {}\n\n{TOOLS_LINE_PREFIX}{}\n\nOBSERVATIONS SO FAR:\n{}",
            agent.goal,
            input,
            tool_catalog(agent),
            if observations.is_empty() {
                NO_OBSERVATIONS_YET.to_string()
            } else {
                observations.join("\n")
            }
        );
        let framing = format!("{REACT_FRAMING}{}", return_schema_instruction(agent));

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
                // The real v2.8.0 dispatch — which means the v2.69.0 budget gate
                // and the v2.69.0 channel lease charge apply to an agent's tool
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
            REFLEXION_CRITIQUE_FRAMING.to_string(),
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
/// v2.67.0 defect wearing a field.
// ────────────────────────────────────────────────────────────────────
//  custom — the step sequence written inside the agent block
// ────────────────────────────────────────────────────────────────────

/// Walk the agent's own `step` sequence, one deliberation per step, through
/// the ordinary step handler (`dispatch_node` on an `IRFlowNode::Step`) — so
/// an agent step gets the same body-statement elevation, the same `ask:`
/// generation, the same wire events and audit row as a flow step. Bounded by
/// `min(body.len(), max_iterations)` plus the token/cost/time ceilings, checked
/// BEFORE each step like every other strategy. The result is the last step's
/// output; every step's output is also bound under its own name, as in a flow.
async fn run_custom(
    agent: &IRAgent,
    input: &str,
    bounds: &AgentBounds,
    spend: &mut Spend,
    ctx: &mut DispatchCtx,
) -> Result<(AgentEnd, String), DispatchError> {
    if agent.body.is_empty() {
        return Err(DispatchError::BackendError {
            name: format!("agent:{}", agent.name),
            message: format!(
                "agent '{}' declares `strategy: custom` with no `step` blocks — the custom \
                 policy IS the step sequence inside the agent block (axon-T1217 refuses \
                 this at check time); refused here rather than run an empty policy.",
                agent.name
            ),
        });
    }
    // The agent's input is visible to its steps under its own name and as
    // `input`, the way a flow's parameters are visible to the flow's steps.
    ctx.let_bindings.insert("input".to_string(), input.to_string());
    ctx.let_bindings
        .insert(format!("{}_input", agent.name), input.to_string());
    let mut last = String::new();
    for node in &agent.body {
        if let Some(bound) = spend.affords(bounds) {
            return Ok((AgentEnd::Stuck { bound }, last));
        }
        if ctx.cancel.is_cancelled() {
            return Err(DispatchError::UpstreamCancelled);
        }
        match Box::pin(super::dispatch_node(node, ctx)).await? {
            NodeOutcome::Completed {
                output,
                tokens_emitted,
                ..
            } => {
                spend.iterations += 1;
                spend.tokens += tokens_emitted;
                last = output;
            }
            other => {
                return Err(DispatchError::BackendError {
                    name: format!("agent:{}", agent.name),
                    message: format!(
                        "a `custom` agent step returned {other:?}, a flow-walk sentinel with \
                         no meaning inside an agent body"
                    ),
                })
            }
        }
    }
    Ok((AgentEnd::Answered, last))
}

async fn apply_on_stuck(
    agent: &IRAgent,
    bound: &'static str,
    partial: &str,
    spend: &Spend,
    ctx: &mut DispatchCtx,
) -> Result<String, DispatchError> {
    let context = if bound == "return" {
        format!(
            "agent '{}' answered, but the answer does not inhabit its declared `return: {}` \
             ({}) after {} iteration(s) (~{} output tokens, ~${:.4})",
            agent.name,
            agent.return_type,
            ctx.let_bindings
                .get(&format!("{}_return_defect", agent.name))
                .cloned()
                .unwrap_or_default(),
            spend.iterations,
            spend.tokens,
            spend.usd()
        )
    } else {
        format!(
            "agent '{}' hit `{bound}` after {} iteration(s) (~{} output tokens, ~${:.4})",
            agent.name,
            spend.iterations,
            spend.tokens,
            spend.usd()
        )
    };
    match agent.on_stuck.as_str() {
        // README agent, verbatim: "raises `AgentStuckError` with full context,
        // so the human reviews exactly where the agent got stuck".
        "escalate" => Err(DispatchError::BackendError {
            name: format!("agent:{}", agent.name),
            message: format!(
                "AgentStuckError — {context}. Last partial state:\n{partial}"
            ),
        }),
        // v2.41.0 — the real forge, seeded with the stuck context. `forge` is
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
        // v2.83.0 parks a FLOW WALK's remaining nodes, and an agent loop has no
        // remaining-node list to park. Refused by name rather than silently
        // degraded to `escalate`.
        "hibernate" => Err(DispatchError::BackendError {
            name: format!("agent:{}", agent.name),
            message: format!(
                "{context}, and `on_stuck: hibernate` has no continuation to park — \
                 `hibernate` suspends a flow walk by capturing its remaining nodes, and an \
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
