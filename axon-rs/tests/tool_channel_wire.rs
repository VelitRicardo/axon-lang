//! v2.69.0 — **the WIRE: a tool on a `resource` derives its channel from it.**
//!
//! v2.69.0 gave the channel a name (`tool { resource: R }`) and refused the
//! absolute `runtime:` URL. That, alone, would be a LABEL — the plan named the
//! trap in advance: a reference that resolves while the tool still connects
//! through its own runtime leaves `endpoint`, `capacity` and `lifetime` governing
//! nothing.
//!
//! So the reference does not merely point. These tests pin the derivation:
//!
//! - the tool's **endpoint** is the resolved `resource.endpoint` (a config key);
//! - the tool's **concurrency** is `resource.capacity` — a bound that did not
//! exist before v2.69.0 (a `par` over N items opened N connections to a vendor
//!   that tolerated ten);
//! - an unresolvable endpoint **refuses** the tool, never a silent fallthrough.

use axon::ir_nodes::{IRResource, IRToolSpec};
use axon::resource_resolver::MapResourceResolver;
use axon::tool_registry::ToolRegistry;

fn resource(name: &str, endpoint: &str, capacity: Option<i64>) -> IRResource {
    IRResource {
        node_type: "resource",
        source_line: 0,
        source_column: 0,
        name: name.into(),
        kind: "https".into(),
        endpoint: endpoint.into(),
        capacity,
        lifetime: "persistent".into(),
        certainty_floor: None,
        shield_ref: String::new(),
        within: String::new(),
    }
}

/// Compile a one-tool program to its `IRToolSpec` (avoids hand-writing the whole
/// struct literal, and exercises the real lowering of `resource:`/`runtime:`).
fn http_tool_spec(name: &str, resource_ref: &str, runtime: &str) -> IRToolSpec {
    let src = format!(
        "resource {resource_ref} {{ kind: https  endpoint: placeholder.key }}\n\
         tool {name} {{ provider: http  resource: {resource_ref}  runtime: {runtime} }}\n"
    );
    let tokens = axon_frontend::lexer::Lexer::new(&src, "<t>").tokenize().unwrap();
    let prog = axon_frontend::parser::Parser::new(tokens).parse().unwrap();
    let ir = axon_frontend::ir_generator::IRGenerator::new().generate(&prog);
    ir.tools.into_iter().find(|t| t.name == name).unwrap()
}

fn resolver() -> MapResourceResolver {
    MapResourceResolver::new().with("vendor.search.base", "https://api.vendor.com")
}

/// **The tool's endpoint comes from the resource's config key, and its
/// concurrency from `resource.capacity`.**
///
/// If the endpoint were still the raw slug and the capacity `None`, the resource
/// would be decorative and v2.69.0 would have moved nothing.
#[test]
fn a_tool_derives_its_endpoint_and_capacity_from_its_resource() {
    let mut reg = ToolRegistry::new();
    reg.register_from_ir(&[http_tool_spec("Search", "SearchApi", "search")]);

    let refused = reg.resolve_from_resources(
        &[resource("SearchApi", "vendor.search.base", Some(8))],
        &resolver(),
    );
    assert!(refused.is_empty(), "the endpoint resolves, so nothing is refused");

    let entry = reg.get("Search").expect("the tool is registered");
    assert_eq!(
        entry.runtime, "https://api.vendor.com/search",
        "the endpoint is the RESOLVED resource address, with the slug `runtime:` as the path"
    );
    assert_eq!(
        entry.capacity,
        Some(8),
        "the concurrency bound is `resource.capacity`. Before v2.69.0 a tool had NO bound — a `par` \
         over N items opened N connections to a vendor that tolerated ten."
    );
    assert_eq!(entry.resource_ref, "SearchApi");
}

/// **An unresolvable endpoint REFUSES the tool.** The entry is dropped, so a
/// dispatch of it fails honestly rather than reaching a phantom address — the
/// v2.67.0/v2.67.0 deny-by-default posture, on the tool channel.
#[test]
fn an_unresolvable_endpoint_refuses_the_tool_it_never_connects_nowhere() {
    let mut reg = ToolRegistry::new();
    reg.register_from_ir(&[http_tool_spec("Search", "SearchApi", "search")]);

    let refused = reg.resolve_from_resources(
        &[resource("SearchApi", "vendor.unconfigured", Some(8))],
        &MapResourceResolver::new(), // the key is not set
    );
    assert_eq!(refused, vec!["Search".to_string()], "the tool must be refused");
    assert!(
        reg.get("Search").is_none(),
        "a channel whose address could not be resolved must not remain dispatchable"
    );
}

/// A tool naming a resource the program does not declare is refused (axon-T950
/// catches it at compile; this is the runtime backstop).
#[test]
fn a_tool_on_a_phantom_resource_is_refused() {
    let mut reg = ToolRegistry::new();
    reg.register_from_ir(&[http_tool_spec("Search", "NoSuchApi", "search")]);
    let refused = reg.resolve_from_resources(&[], &resolver());
    assert_eq!(refused, vec!["Search".to_string()]);
    assert!(reg.get("Search").is_none());
}

/// **A legacy tool — no `resource:` — is untouched.** Same runtime, no capacity
/// bound. v2.69.0 does not change what already runs.
#[test]
fn a_legacy_tool_with_no_resource_is_unchanged() {
    let mut reg = ToolRegistry::new();
    let src = "tool Search { provider: http  runtime: search }\n";
    let tokens = axon_frontend::lexer::Lexer::new(src, "<t>").tokenize().unwrap();
    let prog = axon_frontend::parser::Parser::new(tokens).parse().unwrap();
    let ir = axon_frontend::ir_generator::IRGenerator::new().generate(&prog);
    reg.register_from_ir(&ir.tools);

    let refused = reg.resolve_from_resources(&[resource("Other", "x.y", Some(4))], &resolver());
    assert!(refused.is_empty());

    let entry = reg.get("Search").expect("still registered");
    assert_eq!(entry.runtime, "search", "the legacy slug runtime is untouched");
    assert_eq!(entry.capacity, None, "a resource-less tool has no capacity bound");
}

/// `capacity:` absent on the resource ⇒ the tool is still derived (endpoint
/// resolves) but stays unbounded. Capacity is optional discipline, not required.
#[test]
fn a_resource_without_capacity_derives_the_endpoint_but_leaves_concurrency_unbounded() {
    let mut reg = ToolRegistry::new();
    reg.register_from_ir(&[http_tool_spec("Search", "SearchApi", "search")]);
    reg.resolve_from_resources(&[resource("SearchApi", "vendor.search.base", None)], &resolver());

    let entry = reg.get("Search").unwrap();
    assert_eq!(entry.runtime, "https://api.vendor.com/search");
    assert_eq!(entry.capacity, None);
}
