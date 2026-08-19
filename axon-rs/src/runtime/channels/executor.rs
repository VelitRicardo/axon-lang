//! AXON Runtime — Rust executor integration for typed channels (v1.6.0).
//!
//! The Python side (`axon/runtime/executor.py`) gained four dispatch
//! branches in v1.6.0 + v1.6.0 (`emit_apply` / `publish_apply` /
//! `discover_apply` / `listen_apply`) so a flow's channel surface
//! executes end-to-end on the Python interpreter. The Rust crate
//! exposed `TypedEventBus` standalone in 13.f.2, but a Rust-native
//! flow runner that orchestrates IR steps had no equivalent
//! integration: a Rust adopter who wanted to drive an `IRProgram`
//! through the runtime had to wire the bus, value-ref resolution,
//! and capability/alias scope by hand.
//!
//! 13.l closes that. This module provides:
//!
//! - [`RunContext`]: mirror of Python's `ContextManager` for the
//!   typed-channel concern. Holds the per-unit `TypedEventBus`,
//!   `discovered_handles`, `capabilities`, and step results.
//!   Implements `resolve_value_ref` with the same lookup order
//!   (discovered handles ▶ variables ▶ step results) and
//!   dotted-access walk over both serde JSON values and `String`
//!   maps.
//! - [`dispatch_emit`] / [`dispatch_publish`] / [`dispatch_discover`]
//!   / [`dispatch_listen`]: async functions that consume an IR step
//!   plus a `&RunContext` and route through `TypedEventBus`.
//! - [`bootstrap_run_context`]: builds a `RunContext` from an
//!   `IRProgram` (registers every `IRChannel` on a fresh
//!   `TypedEventBus`).
//!
//! The dispatch surface is intentionally byte-identical (in semantics)
//! to the Python handlers: same lookup precedence for value_ref, same
//! one-shot capability consumption, same alias binding rules. Rust
//! adopters who want a fully-orchestrated `axon run` Rust binary can
//! compose these primitives directly; a future sub-phase wires them
//! into `axon-rs/src/runner.rs::execute_real`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axon_frontend::ir_nodes::{IRDiscover, IREmit, IRListenStep, IRProgram, IRPublish};

use super::typed::{
    Capability, TypedChannelError, TypedChannelHandle, TypedEventBus, TypedPayload,
};

/// Errors surfaced by the `dispatch_*` functions. Each variant tags
/// the channel-op kind so adopters can route failures back to the
/// originating IR step. Mirrors the `channel_op:{op}` `details` tag
/// the Python `AxonRuntimeError` carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchError {
    /// `emit` could not resolve `value_ref`, or the bus refused the payload.
    EmitFailure(String),
    /// `publish` failed at the bus level (D8 gate, missing shield, etc.).
    PublishFailure(String),
    /// `discover` could not find a recorded capability or the bus refused.
    DiscoverFailure(String),
    /// `listen` could not subscribe / receive on the named channel.
    ListenFailure(String),
}

impl std::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DispatchError::EmitFailure(m) => write!(f, "channel_op:emit — {m}"),
            DispatchError::PublishFailure(m) => write!(f, "channel_op:publish — {m}"),
            DispatchError::DiscoverFailure(m) => write!(f, "channel_op:discover — {m}"),
            DispatchError::ListenFailure(m) => write!(f, "channel_op:listen — {m}"),
        }
    }
}

impl std::error::Error for DispatchError {}

impl From<TypedChannelError> for DispatchError {
    fn from(e: TypedChannelError) -> Self {
        // Default to EmitFailure when callers convert raw errors;
        // each dispatch_* function re-wraps with the right variant.
        DispatchError::EmitFailure(e.to_string())
    }
}

/// A value reachable by `value_ref` during channel-op dispatch. Rust
/// adopters fill the run context with whatever shape their step
/// outputs use; we only require the two access modes the dotted-access
/// resolver needs (mapping access by string key + nested handle/JSON).
#[derive(Debug, Clone)]
pub enum RunValue {
    /// Primitive / structured payload; `serde_json::Value` covers
    /// scalars, arrays, and recursive objects so dotted access can
    /// walk arbitrary JSON.
    Json(serde_json::Value),
    /// A live channel handle from the registry. Returned from
    /// `discover` and from `listen` on a second-order channel.
    Handle(TypedChannelHandle),
}

impl RunValue {
    pub fn as_json(&self) -> Option<&serde_json::Value> {
        match self {
            RunValue::Json(v) => Some(v),
            RunValue::Handle(_) => None,
        }
    }
    pub fn as_handle(&self) -> Option<&TypedChannelHandle> {
        match self {
            RunValue::Handle(h) => Some(h),
            RunValue::Json(_) => None,
        }
    }
}

/// Per-unit run context — the Rust mirror of Python's
/// `ContextManager` for the typed-channel concern.
///
/// All mutable state lives behind `Mutex` so the dispatch functions
/// can be called from a multi-threaded executor. `Arc<TypedEventBus>`
/// because the bus itself is already internally synchronised.
pub struct RunContext {
    bus: Arc<TypedEventBus>,
    /// Discovered handle alias scope (`discover X as alias`). Lookup
    /// order #1 in `resolve_value_ref`.
    discovered_handles: Mutex<HashMap<String, TypedChannelHandle>>,
    /// Variable scope (flow params, listen alias for scalar payloads).
    variables: Mutex<HashMap<String, RunValue>>,
    /// Step results — set after each step completes. Used by `emit`
    /// when the value_ref points to a previously-completed step.
    step_results: Mutex<HashMap<String, RunValue>>,
    /// Capability tokens that `publish` produced, keyed by the channel
    /// name they expose. Consumed by `discover` (one-shot).
    capabilities: Mutex<HashMap<String, Capability>>,
}

impl RunContext {
    /// Wrap an existing bus in a fresh context.
    pub fn new(bus: Arc<TypedEventBus>) -> Self {
        RunContext {
            bus,
            discovered_handles: Mutex::new(HashMap::new()),
            variables: Mutex::new(HashMap::new()),
            step_results: Mutex::new(HashMap::new()),
            capabilities: Mutex::new(HashMap::new()),
        }
    }

    pub fn bus(&self) -> &TypedEventBus {
        &self.bus
    }

    /// Bootstrap a context from an `IRProgram`. Every `IRChannel` is
    /// registered on a fresh `TypedEventBus` whose registry is the
    /// canonical source for typed handles during the run.
    pub fn from_ir_program(ir: &IRProgram) -> Self {
        let bus = Arc::new(TypedEventBus::from_ir_program(ir));
        Self::new(bus)
    }

    pub fn set_variable(&self, name: impl Into<String>, value: RunValue) {
        self.variables.lock().unwrap().insert(name.into(), value);
    }

    pub fn get_variable(&self, name: &str) -> Option<RunValue> {
        self.variables.lock().unwrap().get(name).cloned()
    }

    pub fn set_step_result(&self, name: impl Into<String>, value: RunValue) {
        self.step_results.lock().unwrap().insert(name.into(), value);
    }

    pub fn get_step_result(&self, name: &str) -> Option<RunValue> {
        self.step_results.lock().unwrap().get(name).cloned()
    }

    pub fn bind_discovered_handle(
        &self, alias: impl Into<String>, handle: TypedChannelHandle,
    ) {
        self.discovered_handles
            .lock()
            .unwrap()
            .insert(alias.into(), handle);
    }

    pub fn discovered_handles_snapshot(&self) -> HashMap<String, TypedChannelHandle> {
        self.discovered_handles.lock().unwrap().clone()
    }

    pub fn record_capability(&self, channel: impl Into<String>, cap: Capability) {
        self.capabilities.lock().unwrap().insert(channel.into(), cap);
    }

    pub fn take_capability(&self, channel: &str) -> Option<Capability> {
        self.capabilities.lock().unwrap().remove(channel)
    }

    /// Resolve an `emit` value_ref against the live state.
    ///
    /// Lookup order #1 → #3, walking nested segments after dots:
    ///   1. `discovered_handles[head]`
    ///   2. `variables[head]`
    ///   3. `step_results[head]`
    ///
    /// On dotted paths, after the head the remaining segments walk
    /// either:
    ///   - JSON object access (`Json`),
    ///   - struct-like field access on the handle struct (very few
    ///     fields; we expose the same surface the Python `getattr`
    ///     reaches on a `TypedChannelHandle` — `name`, `message`,
    ///     `qos`, `lifetime`, `persistence`, `shield_ref`).
    pub fn resolve_value_ref(&self, value_ref: &str) -> Result<RunValue, DispatchError> {
        if value_ref.is_empty() {
            return Err(DispatchError::EmitFailure(
                "value_ref is empty".to_string(),
            ));
        }
        let mut segments = value_ref.split('.');
        let head = segments.next().expect("at least one segment by split");
        // Resolve the head against the three scopes in priority order
        // (discovered handles ▶ variables ▶ step results). Each scope
        // is acquired in its own block so the MutexGuard drops before
        // the next acquire — temporary `if let Some(_) = lock().get(_)`
        // patterns previously kept guards alive across the whole
        // if-else chain, which deadlocked the error-path acquire of
        // the same locks. Fixed (v1.6.0).
        let from_handles = {
            let dh = self.discovered_handles.lock().unwrap();
            dh.get(head).cloned()
        };
        let from_vars = if from_handles.is_none() {
            let vars = self.variables.lock().unwrap();
            vars.get(head).cloned()
        } else {
            None
        };
        let from_steps = if from_handles.is_none() && from_vars.is_none() {
            let steps = self.step_results.lock().unwrap();
            steps.get(head).cloned()
        } else {
            None
        };
        let mut current = if let Some(h) = from_handles {
            RunValue::Handle(h)
        } else if let Some(v) = from_vars {
            v
        } else if let Some(v) = from_steps {
            v
        } else {
            // Snapshot every scope's keys for the error message — each
            // acquire is its own block so the guards drop before the
            // String formatting completes.
            let vars: Vec<String> = self.variables.lock().unwrap().keys().cloned().collect();
            let steps: Vec<String> = self.step_results.lock().unwrap().keys().cloned().collect();
            let dh: Vec<String> = self.discovered_handles.lock().unwrap().keys().cloned().collect();
            return Err(DispatchError::EmitFailure(format!(
                "value_ref '{value_ref}' — head segment '{head}' is not a \
                 variable, step result, or discovered handle. \
                 Variables: {vars:?}; Step results: {steps:?}; \
                 Discovered handles: {dh:?}",
            )));
        };

        for seg in segments {
            current = walk_one_segment(&current, seg, value_ref)?;
        }
        Ok(current)
    }
}

fn walk_one_segment(
    current: &RunValue, seg: &str, full_ref: &str,
) -> Result<RunValue, DispatchError> {
    match current {
        RunValue::Json(v) => match v {
            serde_json::Value::Object(map) => map.get(seg).cloned().map(RunValue::Json).ok_or_else(
                || DispatchError::EmitFailure(format!(
                    "value_ref '{full_ref}' — key '{seg}' missing on object value",
                )),
            ),
            _ => Err(DispatchError::EmitFailure(format!(
                "value_ref '{full_ref}' — cannot walk '{seg}' on JSON value of type {}",
                json_type_name(v),
            ))),
        },
        RunValue::Handle(h) => match seg {
            "name" => Ok(RunValue::Json(serde_json::Value::String(h.name.clone()))),
            "message" => Ok(RunValue::Json(serde_json::Value::String(h.message.clone()))),
            "qos" => Ok(RunValue::Json(serde_json::Value::String(h.qos.clone()))),
            "lifetime" => Ok(RunValue::Json(serde_json::Value::String(h.lifetime.clone()))),
            "persistence" => Ok(RunValue::Json(serde_json::Value::String(h.persistence.clone()))),
            "shield_ref" => Ok(RunValue::Json(serde_json::Value::String(h.shield_ref.clone()))),
            other => Err(DispatchError::EmitFailure(format!(
                "value_ref '{full_ref}' — handle has no field '{other}'. \
                 Allowed: name, message, qos, lifetime, persistence, shield_ref",
            ))),
        },
    }
}

fn json_type_name(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

// ─── DISPATCH FUNCTIONS ───────────────────────────────────────────────

/// Execute an `IREmit` step against the run context.
///
/// Resolves `value_ref` per the dotted-access rules, then routes to
/// `bus.emit` either as scalar (`TypedPayload::Scalar`) or as
/// mobility (`TypedPayload::Handle`) per the IR's `value_is_channel`
/// flag.
pub async fn dispatch_emit(
    ir: &IREmit, ctx: &RunContext,
) -> Result<(), DispatchError> {
    if ir.value_is_channel {
        // Resolve the handle. First check discovered_handles by name;
        // fall back to the bus registry via get_handle (canonical
        // declared channel).
        let handle = if let Some(h) = ctx
            .discovered_handles
            .lock()
            .unwrap()
            .get(&ir.value_ref)
            .cloned()
        {
            h
        } else {
            match ctx.bus.get_handle(&ir.value_ref) {
                Ok(h) => h,
                Err(_) => {
                    return Err(DispatchError::EmitFailure(format!(
                        "emit on '{}' carries a channel handle but '{}' is not in scope \
                         (no discovered alias, no declared channel)",
                        ir.channel_ref, ir.value_ref,
                    )));
                }
            }
        };
        ctx.bus
            .emit(&ir.channel_ref, TypedPayload::Handle(handle))
            .await
            .map_err(|e| DispatchError::EmitFailure(e.to_string()))?;
        return Ok(());
    }
    // Scalar path — resolve via dotted-access rules.
    let value = ctx.resolve_value_ref(&ir.value_ref)?;
    match value {
        RunValue::Json(j) => ctx
            .bus
            .emit(&ir.channel_ref, TypedPayload::Scalar(j))
            .await
            .map_err(|e| DispatchError::EmitFailure(e.to_string())),
        RunValue::Handle(h) => Err(DispatchError::EmitFailure(format!(
            "emit on '{}' is scalar (value_is_channel=false) but value_ref '{}' \
             resolved to a TypedChannelHandle for '{}' — set value_is_channel=true \
             at IR-generation time for mobility",
            ir.channel_ref, ir.value_ref, h.name,
        ))),
    }
}

/// Execute an `IRPublish` step. Records the returned `Capability` in
/// the context keyed by channel name so a later `IRDiscover` consumes it.
pub async fn dispatch_publish(
    ir: &IRPublish, ctx: &RunContext,
) -> Result<Capability, DispatchError> {
    let cap = ctx
        .bus
        .publish(&ir.channel_ref, &ir.shield_ref)
        .await
        .map_err(|e| DispatchError::PublishFailure(e.to_string()))?;
    ctx.record_capability(ir.channel_ref.clone(), cap.clone());
    Ok(cap)
}

/// Execute an `IRDiscover` step. Pops the capability the matching
/// `publish` recorded earlier in the unit, hands it to `bus.discover`,
/// and binds the resulting handle under `alias` in the discovered-
/// handles scope so subsequent emits / value_refs resolve it.
pub async fn dispatch_discover(
    ir: &IRDiscover, ctx: &RunContext,
) -> Result<TypedChannelHandle, DispatchError> {
    let cap = ctx.take_capability(&ir.capability_ref).ok_or_else(|| {
        DispatchError::DiscoverFailure(format!(
            "no capability recorded for channel '{}'. Did a `publish {} within …` \
             step run earlier in this unit?",
            ir.capability_ref, ir.capability_ref,
        ))
    })?;
    let handle = ctx
        .bus
        .discover(&cap)
        .await
        .map_err(|e| DispatchError::DiscoverFailure(e.to_string()))?;
    ctx.bind_discovered_handle(ir.alias.clone(), handle.clone());
    Ok(handle)
}

/// Execute an `IRListenStep` step (free-standing in flow body — single-
/// event receive). Subscribes to the channel, awaits one event, binds
/// the payload under `event_alias` in the right scope (discovered_
/// handles for mobility, variables for scalar), and returns the
/// payload so the caller can iterate `ir.children` (left to the
/// outer orchestrator since IRListenStep.children is currently typed as
/// `Vec<IRFlowNode>` and dispatch of arbitrary flow steps is the
/// orchestrator's job, not this module's).
pub async fn dispatch_listen(
    ir: &IRListenStep, ctx: &RunContext,
) -> Result<RunValue, DispatchError> {
    if !ir.channel_is_ref {
        // Legacy string-topic path. The Rust runtime bus only knows
        // typed channels in 13.l; the legacy path is supported in
        // Python via the broadcast EventBus but the Rust runtime
        // doesn't yet expose that surface. Surface a clear error
        // rather than misroute.
        return Err(DispatchError::ListenFailure(format!(
            "listen on legacy string-topic '{}' is not supported by the Rust \
             runtime in 13.l — use a typed `channel` declaration (D4 canonical \
             form) or the Python interpreter for D4 dual-mode programs",
            ir.channel,
        )));
    }
    let event = ctx
        .bus
        .receive(&ir.channel)
        .await
        .map_err(|e| DispatchError::ListenFailure(e.to_string()))?;
    let bound = match event.payload {
        TypedPayload::Handle(h) => {
            ctx.bind_discovered_handle(ir.event_alias.clone(), h.clone());
            RunValue::Handle(h)
        }
        TypedPayload::Scalar(j) => {
            let v = RunValue::Json(j);
            ctx.set_variable(ir.event_alias.clone(), v.clone());
            v
        }
    };
    Ok(bound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axon_frontend::ir_nodes::{IRChannel, IRDiscover, IREmit, IRListenStep, IRPublish};

    fn ir_channel(name: &str, message: &str, shield: &str) -> IRChannel {
        IRChannel {
            node_type: "IRChannel",
            source_line: 0,
            source_column: 0,
            name: name.to_string(),
            message: message.to_string(),
            qos: "at_least_once".to_string(),
            lifetime: "affine".to_string(),
            persistence: "ephemeral".to_string(),
            shield_ref: shield.to_string(),
            egress_sign: String::new(),
        }
    }

    fn make_ctx(channels: Vec<IRChannel>) -> RunContext {
        let bus = Arc::new(TypedEventBus::new());
        for ch in &channels {
            bus.register_from_ir(ch);
        }
        RunContext::new(bus)
    }

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all().build().unwrap();
        rt.block_on(f)
    }

    // ── resolve_value_ref ───────────────────────────────────────────

    #[test]
    fn resolve_bare_identifier_step_result() {
        let ctx = make_ctx(vec![]);
        ctx.set_step_result("Build", RunValue::Json(serde_json::json!({"output": "x"})));
        let v = ctx.resolve_value_ref("Build").unwrap();
        assert!(matches!(v, RunValue::Json(_)));
    }

    #[test]
    fn resolve_dotted_walk_json_object() {
        let ctx = make_ctx(vec![]);
        ctx.set_step_result("Build", RunValue::Json(serde_json::json!({
            "output": {"value": 42}
        })));
        let v = ctx.resolve_value_ref("Build.output.value").unwrap();
        match v {
            RunValue::Json(serde_json::Value::Number(n)) => {
                assert_eq!(n.as_i64(), Some(42));
            }
            other => panic!("expected number, got {other:?}"),
        }
    }

    #[test]
    fn resolve_handle_field_access() {
        let ctx = make_ctx(vec![ir_channel("Inner", "Bytes", "")]);
        let h = ctx.bus.get_handle("Inner").unwrap();
        ctx.bind_discovered_handle("alias", h);
        let v = ctx.resolve_value_ref("alias.message").unwrap();
        match v {
            RunValue::Json(serde_json::Value::String(s)) => assert_eq!(s, "Bytes"),
            other => panic!("expected string, got {other:?}"),
        }
    }

    #[test]
    fn resolve_unknown_head_lists_candidates() {
        let ctx = make_ctx(vec![]);
        ctx.set_step_result("Build", RunValue::Json(serde_json::json!({})));
        ctx.set_variable("v", RunValue::Json(serde_json::json!(0)));
        let err = ctx.resolve_value_ref("Missing.field").unwrap_err();
        let s = err.to_string();
        assert!(s.contains("Build") && s.contains('v'),
            "candidates list missing: {s}");
    }

    #[test]
    fn resolve_discovered_handle_shadows_variable() {
        let ctx = make_ctx(vec![ir_channel("Real", "Bytes", "")]);
        ctx.set_variable("alias", RunValue::Json(serde_json::json!("shadowed")));
        let h = ctx.bus.get_handle("Real").unwrap();
        ctx.bind_discovered_handle("alias", h);
        let v = ctx.resolve_value_ref("alias").unwrap();
        assert!(matches!(v, RunValue::Handle(_)));
    }

    // ── dispatch_emit ──────────────────────────────────────────────

    #[test]
    fn emit_scalar_dispatches_through_bus() {
        let ctx = make_ctx(vec![ir_channel("Orders", "Bytes", "")]);
        ctx.set_step_result(
            "Build",
            RunValue::Json(serde_json::json!({"output": {"id": 7}})),
        );
        let ir = IREmit {
            node_type: "IREmit", source_line: 0, source_column: 0,
            channel_ref: "Orders".to_string(),
            value_ref: "Build.output".to_string(),
            value_is_channel: false,
            shield_ref: String::new(),
        
            breach_policy: None,
            scan: Vec::new(),
        };
        block_on(dispatch_emit(&ir, &ctx)).unwrap();
        let event = block_on(ctx.bus.receive("Orders")).unwrap();
        match event.payload {
            TypedPayload::Scalar(v) => assert_eq!(v["id"], 7),
            other => panic!("expected scalar, got {other:?}"),
        }
    }

    #[test]
    fn emit_unknown_value_ref_yields_dispatch_error() {
        let ctx = make_ctx(vec![ir_channel("Orders", "Bytes", "")]);
        let ir = IREmit {
            node_type: "IREmit", source_line: 0, source_column: 0,
            channel_ref: "Orders".to_string(),
            value_ref: "Missing".to_string(),
            value_is_channel: false,
            shield_ref: String::new(),
        
            breach_policy: None,
            scan: Vec::new(),
        };
        let err = block_on(dispatch_emit(&ir, &ctx)).unwrap_err();
        assert!(matches!(err, DispatchError::EmitFailure(_)));
    }

    // ── publish + discover ─────────────────────────────────────────

    #[test]
    fn publish_records_capability_and_discover_consumes_it() {
        let ctx = make_ctx(vec![ir_channel("Topic", "Bytes", "Gate")]);
        let pub_ir = IRPublish {
            node_type: "IRPublish", source_line: 0, source_column: 0,
            channel_ref: "Topic".to_string(),
            shield_ref: "Gate".to_string(),
            sign: String::new(),
        };
        let cap = block_on(dispatch_publish(&pub_ir, &ctx)).unwrap();
        assert_eq!(cap.channel_name, "Topic");
        // ctx has a capability for Topic.
        let disc_ir = IRDiscover {
            node_type: "IRDiscover", source_line: 0, source_column: 0,
            capability_ref: "Topic".to_string(),
            alias: "live".to_string(),
        };
        let h = block_on(dispatch_discover(&disc_ir, &ctx)).unwrap();
        assert_eq!(h.name, "Topic");
        // Alias is now bound.
        assert!(ctx.discovered_handles_snapshot().contains_key("live"));
    }

    #[test]
    fn discover_without_publish_yields_dispatch_error() {
        let ctx = make_ctx(vec![ir_channel("Topic", "Bytes", "Gate")]);
        let disc_ir = IRDiscover {
            node_type: "IRDiscover", source_line: 0, source_column: 0,
            capability_ref: "Topic".to_string(),
            alias: "x".to_string(),
        };
        let err = block_on(dispatch_discover(&disc_ir, &ctx)).unwrap_err();
        assert!(matches!(err, DispatchError::DiscoverFailure(_)));
    }

    #[test]
    fn publish_unpublishable_channel_surfaces_failure() {
        let ctx = make_ctx(vec![ir_channel("Topic", "Bytes", "")]); // no shield
        let ir = IRPublish {
            node_type: "IRPublish", source_line: 0, source_column: 0,
            channel_ref: "Topic".to_string(),
            shield_ref: "Gate".to_string(),
            sign: String::new(),
        };
        let err = block_on(dispatch_publish(&ir, &ctx)).unwrap_err();
        assert!(matches!(err, DispatchError::PublishFailure(_)));
    }

    // ── listen ──────────────────────────────────────────────────────

    #[test]
    fn listen_typed_receives_scalar_and_binds_variable() {
        let ctx = make_ctx(vec![ir_channel("Orders", "Bytes", "")]);
        // Pre-seed an event so receive resolves immediately.
        block_on(ctx.bus.emit(
            "Orders", TypedPayload::Scalar(serde_json::json!({"id": 9})),
        )).unwrap();
        let ir = IRListenStep {
            node_type: "IRListenStep", source_line: 0, source_column: 0,
            channel: "Orders".to_string(),
            channel_is_ref: true,
            event_alias: "ev".to_string(),
            body: Vec::new(),
        };
        let v = block_on(dispatch_listen(&ir, &ctx)).unwrap();
        assert!(matches!(v, RunValue::Json(_)));
        // Alias landed in variables (scalar payload).
        assert!(ctx.get_variable("ev").is_some());
    }

    #[test]
    fn listen_legacy_string_topic_rejected_with_clear_message() {
        let ctx = make_ctx(vec![]);
        let ir = IRListenStep {
            node_type: "IRListenStep", source_line: 0, source_column: 0,
            channel: "orders".to_string(),
            channel_is_ref: false,
            event_alias: "ev".to_string(),
            body: Vec::new(),
        };
        let err = block_on(dispatch_listen(&ir, &ctx)).unwrap_err();
        let msg = err.to_string();
        assert!(matches!(err, DispatchError::ListenFailure(_)));
        assert!(msg.contains("legacy string-topic"));
    }

    // ── from_ir_program ──────────────────────────────────────────────

    #[test]
    fn from_ir_program_registers_all_channels() {
        let mut ir = IRProgram::new();
        ir.channels.push(ir_channel("A", "Bytes", ""));
        ir.channels.push(ir_channel("B", "Channel<Bytes>", "Gate"));
        let ctx = RunContext::from_ir_program(&ir);
        let names = ctx.bus.channel_names();
        assert_eq!(names, vec!["A".to_string(), "B".to_string()]);
    }
}
