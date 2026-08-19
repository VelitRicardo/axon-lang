//! AXON Runtime — Supply-Chain Attestation (v1.2.0)
//!
//! Direct port of `axon/runtime/esk/attestation.py`.
//!
//! SBOM generator + ComplianceDossier + in-toto v1 Statement / SLSA
//! Provenance v1. Produces byte-identical JSON to the Python reference.

#![allow(dead_code)]

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{Map, Value};

use crate::ir_nodes::{IRAxonEndpoint, IRProgram};

use super::compliance;
use super::provenance::content_hash;

pub const IN_TOTO_STATEMENT_TYPE: &str = "https://in-toto.io/Statement/v1";
pub const SLSA_PROVENANCE_TYPE: &str = "https://slsa.dev/provenance/v1";

// ═══════════════════════════════════════════════════════════════════
//  SBOM
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize)]
pub struct SbomEntry {
    pub name: String,
    pub kind: String,
    pub content_hash: String,
    pub compliance: Vec<String>,
}

/// Software Bill of Materials — the serialised form mirrors Python's
/// `SupplyChainSBOM.to_dict()` including the `schema` marker.
#[derive(Debug, Clone)]
pub struct SupplyChainSBOM {
    pub program_hash: String,
    pub axon_version: String,
    pub entries: Vec<SbomEntry>,
    pub dependencies: Vec<Dependency>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Dependency {
    pub name: String,
    pub version: String,
    pub kind: String,
}

impl SupplyChainSBOM {
    pub fn add(&mut self, entry: SbomEntry) {
        self.entries.push(entry);
    }

    pub fn add_dependency(&mut self, name: impl Into<String>, version: impl Into<String>, kind: impl Into<String>) {
        self.dependencies.push(Dependency {
            name: name.into(),
            version: version.into(),
            kind: kind.into(),
        });
    }

    pub fn with_compliance(&self, labels: &[&str]) -> Vec<SbomEntry> {
        let required: std::collections::HashSet<&str> = labels.iter().copied().collect();
        self.entries
            .iter()
            .filter(|e| required.iter().all(|l| e.compliance.iter().any(|c| c == l)))
            .cloned()
            .collect()
    }

    pub fn to_value(&self) -> Value {
        let mut m = Map::new();
        m.insert("schema".into(), "axon.esk.sbom.v1".into());
        m.insert("axon_version".into(), self.axon_version.clone().into());
        m.insert("program_hash".into(), self.program_hash.clone().into());
        m.insert(
            "entries".into(),
            Value::Array(self.entries.iter().map(sbom_entry_to_value).collect()),
        );
        m.insert(
            "dependencies".into(),
            Value::Array(self.dependencies.iter().map(|d| {
                let mut dm = Map::new();
                dm.insert("name".into(), d.name.clone().into());
                dm.insert("version".into(), d.version.clone().into());
                dm.insert("kind".into(), d.kind.clone().into());
                Value::Object(dm)
            }).collect()),
        );
        m.insert("entry_count".into(), (self.entries.len() as i64).into());
        Value::Object(m)
    }
}

fn sbom_entry_to_value(e: &SbomEntry) -> Value {
    let mut m = Map::new();
    m.insert("name".into(), e.name.clone().into());
    m.insert("kind".into(), e.kind.clone().into());
    m.insert("content_hash".into(), e.content_hash.clone().into());
    m.insert(
        "compliance".into(),
        Value::Array(e.compliance.iter().cloned().map(Value::String).collect()),
    );
    Value::Object(m)
}

// ═══════════════════════════════════════════════════════════════════
//  Per-kind iteration (matches Python `_KIND_TO_ATTR`)
// ═══════════════════════════════════════════════════════════════════

/// Walk every declaration bucket in Python's canonical order and yield
/// `(kind_label, name, compliance, content_hash)` tuples. The content
/// hash is derived from the IR node's serialised JSON — equivalent to
/// Python's `node.to_dict()` under `content_hash`.
fn walk_program<F>(program: &IRProgram, mut visit: F)
where
    F: FnMut(&str, &str, Vec<String>, String),
{
    // Small helper that serialises a node to canonical JSON hash.
    fn hash_node<T: Serialize>(node: &T) -> String {
        let v = serde_json::to_value(node).expect("ir node serialise");
        content_hash(&v)
    }

    // Iteration order MUST match Python's `_KIND_TO_ATTR` exactly —
    // otherwise the SBOM entries list differs and `program_hash` drifts.
    for n in &program.personas {
        visit("persona", &n.name, Vec::new(), hash_node(n));
    }
    for n in &program.contexts {
        visit("context", &n.name, Vec::new(), hash_node(n));
    }
    for n in &program.anchors {
        visit("anchor", &n.name, Vec::new(), hash_node(n));
    }
    for n in &program.tools {
        visit("tool", &n.name, Vec::new(), hash_node(n));
    }
    for n in &program.memories {
        visit("memory", &n.name, Vec::new(), hash_node(n));
    }
    for n in &program.types {
        visit("type", &n.name, n.compliance.clone(), hash_node(n));
    }
    for n in &program.flows {
        visit("flow", &n.name, Vec::new(), hash_node(n));
    }
    for n in &program.agents {
        visit("agent", &n.name, Vec::new(), hash_node(n));
    }
    for n in &program.shields {
        visit("shield", &n.name, n.compliance.clone(), hash_node(n));
    }
    for n in &program.daemons {
        visit("daemon", &n.name, Vec::new(), hash_node(n));
    }
    for n in &program.axonstore_specs {
        visit("axonstore", &n.name, Vec::new(), hash_node(n));
    }
    for n in &program.endpoints {
        visit("axonendpoint", &n.name, n.compliance.clone(), hash_node(n));
    }
    for n in &program.resources {
        visit("resource", &n.name, Vec::new(), hash_node(n));
    }
    for n in &program.fabrics {
        visit("fabric", &n.name, Vec::new(), hash_node(n));
    }
    for n in &program.manifests {
        visit("manifest", &n.name, n.compliance.clone(), hash_node(n));
    }
    for n in &program.observations {
        visit("observe", &n.name, Vec::new(), hash_node(n));
    }
    for n in &program.reconciles {
        visit("reconcile", &n.name, Vec::new(), hash_node(n));
    }
    for n in &program.leases {
        visit("lease", &n.name, Vec::new(), hash_node(n));
    }
    for n in &program.ensembles {
        visit("ensemble", &n.name, Vec::new(), hash_node(n));
    }
    for n in &program.sessions {
        visit("session", &n.name, Vec::new(), hash_node(n));
    }
    for n in &program.topologies {
        visit("topology", &n.name, Vec::new(), hash_node(n));
    }
    for n in &program.immunes {
        visit("immune", &n.name, Vec::new(), hash_node(n));
    }
    for n in &program.reflexes {
        visit("reflex", &n.name, Vec::new(), hash_node(n));
    }
    for n in &program.heals {
        visit("heal", &n.name, Vec::new(), hash_node(n));
    }
    for n in &program.components {
        visit("component", &n.name, Vec::new(), hash_node(n));
    }
    for n in &program.views {
        visit("view", &n.name, Vec::new(), hash_node(n));
    }
}

/// Build an SBOM from an IRProgram — pure function, deterministic.
pub fn generate_sbom(program: &IRProgram, axon_version: &str) -> SupplyChainSBOM {
    let mut entries: Vec<SbomEntry> = Vec::new();
    walk_program(program, |kind, name, compliance, hash| {
        entries.push(SbomEntry {
            name: name.into(),
            kind: kind.into(),
            content_hash: hash,
            compliance,
        });
    });
    // Program hash = content_hash({"entries": [...]}) — mirrors Python.
    let mut payload = Map::new();
    payload.insert(
        "entries".into(),
        Value::Array(entries.iter().map(sbom_entry_to_value).collect()),
    );
    let program_hash = content_hash(&Value::Object(payload));
    SupplyChainSBOM {
        program_hash,
        axon_version: axon_version.into(),
        entries,
        dependencies: Vec::new(),
    }
}

// ═══════════════════════════════════════════════════════════════════
//  Compliance dossier
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct ComplianceDossier {
    pub program_hash: String,
    pub classes_covered: Vec<String>,
    pub sectors: Vec<String>,
    pub entries_per_class: BTreeMap<String, i64>,
    pub shielded_endpoints: Vec<String>,
    pub unshielded_regulated: Vec<String>,
    pub axon_version: String,
}

impl ComplianceDossier {
    pub fn to_value(&self) -> Value {
        let mut m = Map::new();
        m.insert("schema".into(), "axon.esk.compliance.v1".into());
        m.insert("axon_version".into(), self.axon_version.clone().into());
        m.insert("program_hash".into(), self.program_hash.clone().into());
        let mut classes = self.classes_covered.clone();
        classes.sort();
        m.insert(
            "classes_covered".into(),
            Value::Array(classes.into_iter().map(Value::String).collect()),
        );
        let mut sectors = self.sectors.clone();
        sectors.sort();
        m.insert(
            "sectors".into(),
            Value::Array(sectors.into_iter().map(Value::String).collect()),
        );
        let mut epc = Map::new();
        for (k, v) in &self.entries_per_class {
            epc.insert(k.clone(), Value::from(*v));
        }
        m.insert("entries_per_class".into(), Value::Object(epc));
        m.insert(
            "shielded_endpoints".into(),
            Value::Array(
                self.shielded_endpoints.iter().cloned().map(Value::String).collect(),
            ),
        );
        m.insert(
            "unshielded_regulated".into(),
            Value::Array(
                self.unshielded_regulated.iter().cloned().map(Value::String).collect(),
            ),
        );
        Value::Object(m)
    }
}

pub fn generate_dossier(program: &IRProgram, axon_version: &str) -> ComplianceDossier {
    let registry = compliance::registry();
    let mut all_classes: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut entries_per_class: BTreeMap<String, i64> = BTreeMap::new();

    walk_program(program, |_kind, _name, labels, _hash| {
        for label in labels {
            if registry.contains_key(&label) {
                all_classes.insert(label.clone());
                *entries_per_class.entry(label).or_insert(0) += 1;
            }
        }
    });

    let mut shielded_endpoints: Vec<String> = Vec::new();
    let mut unshielded_regulated: Vec<String> = Vec::new();
    for ep in &program.endpoints {
        classify_endpoint(ep, &mut shielded_endpoints, &mut unshielded_regulated);
    }

    let sbom = generate_sbom(program, axon_version);
    let sectors: Vec<String> = compliance::classify_sector(all_classes.iter().cloned())
        .into_iter()
        .collect();
    ComplianceDossier {
        program_hash: sbom.program_hash,
        classes_covered: all_classes.into_iter().collect(),
        sectors,
        entries_per_class,
        shielded_endpoints,
        unshielded_regulated,
        axon_version: axon_version.into(),
    }
}

fn classify_endpoint(
    ep: &IRAxonEndpoint,
    shielded: &mut Vec<String>,
    unshielded: &mut Vec<String>,
) {
    if !ep.compliance.is_empty() {
        if !ep.shield_ref.is_empty() {
            shielded.push(ep.name.clone());
        } else {
            unshielded.push(ep.name.clone());
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
//  in-toto v1 Statement (SLSA Provenance v1 predicate)
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct InTotoStatement {
    pub subject_name: String,
    pub subject_digest_sha256: String,
    pub predicate: Value,
    pub predicate_type: String,
    pub statement_type: String,
}

impl InTotoStatement {
    pub fn to_value(&self) -> Value {
        let mut m = Map::new();
        m.insert("_type".into(), self.statement_type.clone().into());
        let mut subj = Map::new();
        subj.insert("name".into(), self.subject_name.clone().into());
        let mut digest = Map::new();
        digest.insert("sha256".into(), self.subject_digest_sha256.clone().into());
        subj.insert("digest".into(), Value::Object(digest));
        m.insert("subject".into(), Value::Array(vec![Value::Object(subj)]));
        m.insert("predicateType".into(), self.predicate_type.clone().into());
        m.insert("predicate".into(), self.predicate.clone());
        Value::Object(m)
    }
}

pub fn generate_in_toto_statement(
    program: &IRProgram,
    axon_version: &str,
    builder_id: &str,
    subject_name: &str,
) -> InTotoStatement {
    let sbom = generate_sbom(program, axon_version);

    // buildDefinition.externalParameters
    let mut external_params = Map::new();
    external_params.insert("sbom_hash".into(), sbom.program_hash.clone().into());
    external_params.insert("axon_version".into(), axon_version.into());
    external_params.insert("entry_count".into(), (sbom.entries.len() as i64).into());

    // buildDefinition
    let mut build_def = Map::new();
    build_def.insert(
        "buildType".into(),
        "https://axon-lang.io/builds/compile@v1".into(),
    );
    build_def.insert("externalParameters".into(), Value::Object(external_params));
    build_def.insert("internalParameters".into(), Value::Object(Map::new()));
    let resolved_deps: Vec<Value> = sbom
        .dependencies
        .iter()
        .map(|d| {
            let mut dm = Map::new();
            dm.insert("name".into(), d.name.clone().into());
            dm.insert(
                "uri".into(),
                format!("pkg:{}/{}@{}", d.kind, d.name, d.version).into(),
            );
            Value::Object(dm)
        })
        .collect();
    build_def.insert("resolvedDependencies".into(), Value::Array(resolved_deps));

    // runDetails.builder
    let mut builder = Map::new();
    builder.insert("id".into(), builder_id.into());
    let mut version = Map::new();
    version.insert("axon".into(), axon_version.into());
    builder.insert("version".into(), Value::Object(version));

    // runDetails.metadata
    let mut metadata = Map::new();
    metadata.insert("invocationId".into(), sbom.program_hash.clone().into());
    metadata.insert("finishedOn".into(), Value::Null);

    // runDetails.byproducts
    let byproducts: Vec<Value> = sbom
        .entries
        .iter()
        .map(|e| {
            let mut bm = Map::new();
            bm.insert("name".into(), e.name.clone().into());
            bm.insert("uri".into(), format!("axon:{}:{}", e.kind, e.name).into());
            let mut digest = Map::new();
            digest.insert("sha256".into(), e.content_hash.clone().into());
            bm.insert("digest".into(), Value::Object(digest));
            Value::Object(bm)
        })
        .collect();

    let mut run_details = Map::new();
    run_details.insert("builder".into(), Value::Object(builder));
    run_details.insert("metadata".into(), Value::Object(metadata));
    run_details.insert("byproducts".into(), Value::Array(byproducts));

    let mut predicate = Map::new();
    predicate.insert("buildDefinition".into(), Value::Object(build_def));
    predicate.insert("runDetails".into(), Value::Object(run_details));

    InTotoStatement {
        subject_name: subject_name.into(),
        subject_digest_sha256: sbom.program_hash,
        predicate: Value::Object(predicate),
        predicate_type: SLSA_PROVENANCE_TYPE.into(),
        statement_type: IN_TOTO_STATEMENT_TYPE.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir_generator::IRGenerator;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn compile(source: &str) -> IRProgram {
        let tokens = Lexer::new(source, "t").tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        IRGenerator::new().generate(&program)
    }

    #[test]
    fn sbom_entries_cover_all_declaration_kinds() {
        let ir = compile(r#"
            resource Db { kind: postgres lifetime: linear }
            fabric Vpc { provider: aws }
            manifest M { resources: [Db] fabric: Vpc }
            observe O from M { sources: [prom] quorum: 1 }
        "#);
        let sbom = generate_sbom(&ir, "1.0.0");
        let kinds: Vec<&str> = sbom.entries.iter().map(|e| e.kind.as_str()).collect();
        assert!(kinds.contains(&"resource"));
        assert!(kinds.contains(&"fabric"));
        assert!(kinds.contains(&"manifest"));
        assert!(kinds.contains(&"observe"));
    }

    #[test]
    fn sbom_program_hash_deterministic() {
        let ir = compile(r#"
            resource Db { kind: postgres lifetime: linear }
            fabric Vpc { provider: aws }
            manifest M { resources: [Db] fabric: Vpc }
        "#);
        let a = generate_sbom(&ir, "1.0.0").program_hash;
        let b = generate_sbom(&ir, "1.0.0").program_hash;
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
    }

    #[test]
    fn sbom_program_hash_changes_on_any_declaration_edit() {
        let ir_a = compile("resource A { kind: postgres }");
        let ir_b = compile("resource A { kind: redis }");
        assert_ne!(
            generate_sbom(&ir_a, "1.0.0").program_hash,
            generate_sbom(&ir_b, "1.0.0").program_hash
        );
    }

    #[test]
    fn sbom_entry_compliance_surfaces_for_regulated_manifests() {
        let ir = compile(r#"
            resource Db { kind: postgres }
            fabric Vpc { provider: aws }
            manifest M {
                resources: [Db] fabric: Vpc
                compliance: [HIPAA, SOC2]
            }
        "#);
        let sbom = generate_sbom(&ir, "1.0.0");
        let m = sbom.entries.iter().find(|e| e.kind == "manifest" && e.name == "M").unwrap();
        assert_eq!(m.compliance, vec!["HIPAA", "SOC2"]);
    }

    #[test]
    fn dossier_classifies_covered_classes_and_sectors() {
        let ir = compile(r#"
            type PatientRecord compliance [HIPAA, GDPR] { id: String }
            resource Db { kind: postgres }
            fabric Vpc { provider: aws }
            manifest M {
                resources: [Db] fabric: Vpc
                compliance: [HIPAA, PCI_DSS]
            }
        "#);
        let d = generate_dossier(&ir, "1.0.0");
        let mut classes = d.classes_covered.clone();
        classes.sort();
        assert_eq!(classes, vec!["GDPR", "HIPAA", "PCI_DSS"]);
        let mut sectors = d.sectors.clone();
        sectors.sort();
        // GDPR/cross-sector + HIPAA/healthcare + PCI_DSS/financial
        assert!(sectors.contains(&"cross-sector".to_string()));
        assert!(sectors.contains(&"healthcare".to_string()));
        assert!(sectors.contains(&"financial".to_string()));
    }

    #[test]
    fn dossier_flags_unshielded_regulated_endpoints() {
        let ir = compile(r#"
            type R compliance [HIPAA] { x: String }
            flow F(r: R) -> R { step S { ask: "x" output: R } }
            axonendpoint E {
                method: POST path: "/p" body: R execute: F output: R
                compliance: [HIPAA]
            }
        "#);
        let d = generate_dossier(&ir, "1.0.0");
        assert_eq!(d.unshielded_regulated, vec!["E"]);
        assert!(d.shielded_endpoints.is_empty());
    }

    #[test]
    fn dossier_flags_shielded_endpoints_when_guard_present() {
        let ir = compile(r#"
            type R compliance [HIPAA] { x: String }
            flow F(r: R) -> R { step S { ask: "x" output: R } }
            shield Guard {
                scan: [prompt_injection]
                on_breach: halt
                severity: high
                compliance: [HIPAA]
            }
            axonendpoint E {
                method: POST path: "/p" body: R execute: F output: R
                shield: Guard
                compliance: [HIPAA]
            }
        "#);
        let d = generate_dossier(&ir, "1.0.0");
        assert_eq!(d.shielded_endpoints, vec!["E"]);
        assert!(d.unshielded_regulated.is_empty());
    }

    #[test]
    fn in_toto_statement_carries_slsa_provenance_v1() {
        let ir = compile(r#"
            resource Db { kind: postgres }
            fabric Vpc { provider: aws }
            manifest M { resources: [Db] fabric: Vpc }
        "#);
        let stmt = generate_in_toto_statement(
            &ir, "1.0.0", "https://axon-lang.io/builders/compiler@v1", "axon-program",
        );
        let v = stmt.to_value();
        assert_eq!(v["_type"], IN_TOTO_STATEMENT_TYPE);
        assert_eq!(v["predicateType"], SLSA_PROVENANCE_TYPE);
        assert_eq!(v["subject"][0]["name"], "axon-program");
        assert_eq!(v["subject"][0]["digest"]["sha256"].as_str().unwrap().len(), 64);
    }

    #[test]
    fn in_toto_statement_byproducts_list_every_sbom_entry() {
        let ir = compile(r#"
            resource Db { kind: postgres }
            fabric Vpc { provider: aws }
            manifest M { resources: [Db] fabric: Vpc }
        "#);
        let stmt = generate_in_toto_statement(&ir, "1.0.0", "b", "subj");
        let v = stmt.to_value();
        let byproducts = v["predicate"]["runDetails"]["byproducts"].as_array().unwrap();
        assert_eq!(byproducts.len(), 3); // resource + fabric + manifest
    }

    #[test]
    fn in_toto_statement_is_deterministic_on_equal_input() {
        let ir = compile(r#"
            resource R { kind: postgres }
            fabric V { provider: aws }
            manifest M { resources: [R] fabric: V }
        "#);
        let a = generate_in_toto_statement(&ir, "1.0.0", "b", "s").to_value();
        let b = generate_in_toto_statement(&ir, "1.0.0", "b", "s").to_value();
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap()
        );
    }
}
