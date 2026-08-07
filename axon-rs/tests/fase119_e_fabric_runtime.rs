//! §Fase 119.e — `fabric` governs: the RUNTIME half.
//!
//! §111's finding, in full: *"`provider`/`region`/`zones` are consumed by
//! NOTHING at runtime: in `LiveHandler::provision` the parameter is literally
//! `_fabrics` … Still governs nothing that runs."*
//!
//! The dry-run handler copied the fabric into a JSON blob, which is
//! *describing*, not governing. These gates pin the two things that now
//! actually happen on the path production takes:
//!
//! 1. a tool whose `resource` lives `within` a fabric carries that fabric's
//!    `(provider, region)` on its binding — the substrate reaches the channel
//!    that runs;
//! 2. a `within:` naming a fabric the program does not declare REFUSES the
//!    binding, because a channel bound to an undeclared substrate connects to
//!    something whose provider and jurisdiction are unknown while the
//!    declaration claims otherwise.

use axon::ir_nodes::{IRFabric, IRResource};
use axon::tool_registry::{ToolEntry, ToolRegistry};

fn fabric(name: &str, provider: &str, region: &str) -> IRFabric {
    IRFabric {
        node_type: "fabric",
        source_line: 0,
        source_column: 0,
        name: name.into(),
        provider: provider.into(),
        region: region.into(),
        zones: Some(3),
        ephemeral: Some(false),
        shield_ref: String::new(),
    }
}

fn resource(name: &str, within: &str) -> IRResource {
    let mut r = IRResource::new(name.into(), 0, 0);
    r.kind = "http".into();
    r.endpoint = "https://example.test".into();
    r.within = within.into();
    r
}

fn registry_with_tool(resource_ref: &str) -> ToolRegistry {
    let mut reg = ToolRegistry::new();
    reg.register(ToolEntry {
        name: "Fetcher".to_string(),
        provider: "http".to_string(),
        timeout: "10s".to_string(),
        runtime: String::new(),
        resource_ref: resource_ref.to_string(),
        capacity: None,
        substrate: None,
        sandbox: None,
        max_results: None,
        output_schema: String::new(),
        effect_row: vec!["network".to_string()],
        parameters: Vec::new(),
        secret: String::new(),
        secret_partition: String::new(),
        source: axon::tool_registry::ToolSource::Program,
        is_streaming: false,
        scrape: None,
    });
    reg
}

struct PassThroughResolver;
impl axon::resource_resolver::ResourceResolver for PassThroughResolver {
    fn resolve(
        &self,
        key: &str,
    ) -> Result<String, axon::resource_resolver::ResourceResolveError> {
        Ok(key.to_string())
    }
}

#[test]
fn the_substrate_reaches_the_channel_that_actually_runs() {
    let mut reg = registry_with_tool("Api");
    let refused = reg.resolve_from_resources_within(
        &[resource("Api", "EuCloud")],
        &PassThroughResolver,
        &[fabric("EuCloud", "aws", "eu-west-1")],
    );
    assert!(refused.is_empty(), "{refused:?}");
    let entry = reg.get("Fetcher").expect("bound");
    assert_eq!(
        entry.substrate,
        Some(("aws".to_string(), "eu-west-1".to_string())),
        "the fabric's provider/region must reach the running channel — this is \
         the exact field §111 found consumed by NOTHING"
    );
}

#[test]
fn a_within_naming_an_undeclared_fabric_refuses_the_binding() {
    let mut reg = registry_with_tool("Api");
    let refused = reg.resolve_from_resources_within(
        &[resource("Api", "GhostCloud")],
        &PassThroughResolver,
        &[fabric("EuCloud", "aws", "eu-west-1")],
    );
    assert_eq!(
        refused,
        vec!["Fetcher".to_string()],
        "a channel bound to an undeclared substrate connects to something whose \
         provider and jurisdiction are unknown"
    );
    assert!(
        reg.get("Fetcher").is_none(),
        "the refused tool is DROPPED — a dispatch must not reach a phantom"
    );
}

#[test]
fn a_resource_with_no_within_binds_unattributed_never_invented() {
    let mut reg = registry_with_tool("Api");
    let refused = reg.resolve_from_resources_within(
        &[resource("Api", "")],
        &PassThroughResolver,
        &[fabric("EuCloud", "aws", "eu-west-1")],
    );
    assert!(refused.is_empty(), "{refused:?}");
    assert_eq!(
        reg.get("Fetcher").expect("bound").substrate,
        None,
        "no `within:` means no substrate — the audit says unattributed rather \
         than picking a fabric that happens to be in scope"
    );
}

#[test]
fn the_legacy_entry_point_stays_byte_identical() {
    // `resolve_from_resources` (no fabric catalog) must behave exactly as it
    // did pre-§119.e: bind, attribute nothing, refuse nothing extra.
    let mut reg = registry_with_tool("Api");
    let refused = reg.resolve_from_resources(&[resource("Api", "EuCloud")], &PassThroughResolver);
    assert!(refused.is_empty(), "{refused:?}");
    assert_eq!(reg.get("Fetcher").expect("bound").substrate, None);
}

#[test]
fn the_audit_row_elides_an_absent_substrate_so_pre_119e_rows_are_unchanged() {
    let record = axon::axonendpoint_replay::StepAuditRecord {
        step_name: "S".into(),
        step_index: 0,
        success: true,
        tokens_emitted: 0,
        output_hash_hex: String::new(),
        effect_policy_applied: None,
        chunks_dropped: 0,
        chunks_degraded: 0,
        timestamp_ms: 0,
        tool_name: None,
        substrate: None,
        ..Default::default()
    };
    let json = serde_json::to_string(&record).expect("serialise");
    assert!(
        !json.contains("substrate"),
        "an absent substrate must be elided, not serialised as null: {json}"
    );

    let with = axon::axonendpoint_replay::StepAuditRecord {
        substrate: Some("aws/eu-west-1".into()),
        ..record
    };
    let json = serde_json::to_string(&with).expect("serialise");
    assert!(
        json.contains("\"substrate\":\"aws/eu-west-1\""),
        "the substrate travels with the row so cross-jurisdiction movement is \
         auditable: {json}"
    );
}
