//! v2.8.0 — the SERVER path registers program-declared tools, so a tool
//! dispatches for real instead of silently degrading to an LLM step.
//!
//! Brief #22 / #17 root cause: `execute_server_flow` built a builtins-only
//! `ToolRegistry::new()` and never called `register_from_ir`, so every
//! program-declared `tool { provider: http … }` missed the registry →
//! `dispatch` returned `None` → the step degraded to an LLM call. v2.8.0 adds the
//! one line the CLI path (`run_run`) always had. This gate locks the
//! registration → dispatch composition that the server path now runs (the
//! registry is a per-call local, so it is request-scoped — v2.8.0 D10).

use axon::ir_nodes::IRToolSpec;
use axon::tool_registry::ToolRegistry;

fn tool(name: &str, provider: &str, runtime: &str) -> IRToolSpec {
    IRToolSpec {
        node_type: "ToolDefinition",
        source_line: 1,
        source_column: 1,
        name: name.to_string(),
        provider: provider.to_string(),
        max_results: None,
        filter_expr: String::new(),
        timeout: String::new(),
        runtime: runtime.to_string(),
        resource_ref: String::new(),
        sandbox: None,
        input_schema: Vec::new(),
        output_schema: String::new(),
        parameters: Vec::new(),
        output_type: None,
        requires: Vec::new(),
        secret: String::new(),
        secret_partition: String::new(),
        effect_row: Vec::new(),
        target: None,
        risk: None,
        argv: Vec::new(),
        cache: String::new(),
        scrape: None,
    }
}

#[test]
fn unregistered_program_tool_does_not_dispatch() {
    // The pre-v2.8.0 server-path state: a fresh builtins-only registry misses a
    // program tool → None → the step degrades to an LLM call (the #22/#17 bug).
    let registry = ToolRegistry::new();
    assert!(
        registry.dispatch("BuscarEmpresa", "{}").is_none(),
        "a program tool must be unknown to a builtins-only registry (degrades to LLM)"
    );
}

#[test]
fn server_path_registration_makes_program_tools_dispatch() {
    // v2.8.0 mirrors the exact line `execute_server_flow` now runs.
    let mut registry = ToolRegistry::new();
    registry.register_from_ir(&[tool("BuscarEmpresa", "stub", "")]);
    assert!(
        registry.dispatch("BuscarEmpresa", "{\"query\":\"Acme\"}").is_some(),
        "a registered program tool must dispatch, not degrade to LLM"
    );
}

#[test]
fn http_tool_url_resolves_from_runtime_field_d7() {
    // v2.8.0 D7 — provider→URL is the tool's declared `runtime:` field; after
    // registration the entry carries it verbatim, so the v2.8.0 structured body
    // POSTs to it (no global URL table / no shared mutable cache — D10).
    let mut registry = ToolRegistry::new();
    registry.register_from_ir(&[tool("WebSearch", "http", "https://tools.kivi.io/search")]);
    let entry = registry.get("WebSearch").expect("WebSearch registered");
    assert_eq!(entry.runtime, "https://tools.kivi.io/search");
    assert_eq!(entry.provider, "http");
}

#[test]
fn registration_is_request_scoped_not_shared_d10() {
    // Two independent registries (two concurrent requests) do not see each
    // other's tools — the per-call local guarantees tenant isolation.
    let mut req_a = ToolRegistry::new();
    req_a.register_from_ir(&[tool("TenantATool", "stub", "")]);
    let req_b = ToolRegistry::new();
    assert!(
        req_b.dispatch("TenantATool", "{}").is_none(),
        "request B must not see request A's registered tool (no cross-tenant leak)"
    );
}
