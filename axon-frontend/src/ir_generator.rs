//! AXON IR Generator — AST → IR transformation.
//!
//! Direct port of axon/compiler/ir_generator.py (Tier 1 subset).
//!
//! Tier 1 constructs produce fully typed IR nodes.
//! Tier 2+ GenericDeclarations are emitted as generic JSON objects.
//! Flow data edges and execution levels are computed.

use std::collections::HashMap;

use crate::ast::*;
use crate::ir_nodes::*;
use crate::store_schema::{StoreColumn, StoreColumnSchema};

/// §Fase 38.b (D1) — lower the parsed AST schema declaration to its
/// IR mirror. Pure + total — every AST variant has an IR variant.
fn lower_column_schema(s: &StoreColumnSchema) -> IRStoreColumnSchema {
    match s {
        StoreColumnSchema::Inline { columns, .. } => IRStoreColumnSchema::Inline {
            columns: columns.iter().map(lower_column).collect(),
        },
        StoreColumnSchema::ManifestRef { qualified_name, .. } => {
            IRStoreColumnSchema::ManifestRef {
                qualified_name: qualified_name.clone(),
            }
        }
        StoreColumnSchema::EnvVar { var_name, .. } => IRStoreColumnSchema::EnvVar {
            var_name: var_name.clone(),
        },
    }
}

fn lower_column(c: &StoreColumn) -> IRStoreColumn {
    IRStoreColumn {
        name: c.name.clone(),
        // The IR carries the canonical PascalCase name; the alias the
        // adopter wrote in source is already normalised at parse time.
        col_type: c.col_type.canonical_name().to_string(),
        primary_key: c.primary_key,
        auto_increment: c.auto_increment,
        not_null: c.not_null,
        unique: c.unique,
        default_value: c.default_value.clone(),
        // §Fase 38.x.c (D2) — round-trip the IDENTITY marker through IR.
        identity: c.identity,
        // §Fase 73.f (D1) — round-trip the `index` declaration so the
        // deploy gate sees it and materializes the (GIN / b-tree) index.
        indexed: c.indexed,
        // §Fase 73.g (D1) — round-trip the `Json<T>` shape-lens struct name
        // so the PCC `JsonShapeSoundness` proof re-derives it from the IR.
        json_shape: c.json_shape.clone(),
    }
}

pub struct IRGenerator {
    /// §Fase 109.a — per-flow context for `grad`: every rich `let`'s
    /// expression in the CURRENT flow (populated at `visit_flow` entry,
    /// read by the Grad arm of `visit_flow_step`). RefCell because the
    /// visitor is `&self` across 8 recursive call sites.
    grad_lets: std::cell::RefCell<HashMap<String, crate::ast::Expr>>,
    personas: HashMap<String, IRPersona>,
    contexts: HashMap<String, IRContext>,
    anchors: HashMap<String, IRAnchor>,
    flows: HashMap<String, IRFlow>,
    lambda_data_specs: HashMap<String, IRLambdaData>,
    /// §λ-L-E Fase 1 (Free Monad root) — Manifests / Observes, in
    /// declaration order, become nodes the Handler layer will interpret.
    intention_ops: Vec<IRIntentionOperation>,
    /// Anchor for the intention tree's own source position.
    program_line: u32,
    program_column: u32,
    /// §λ-L-E Fase 13 — channel registry for mobility detection at lowering.
    /// Names of declared channels are recorded as they're visited so
    /// `visit_emit` can pre-resolve `value_is_channel` without re-scanning
    /// the AST (parity with the Python `IREmit.value_is_channel` flag).
    channel_names: std::collections::HashSet<String>,
    /// §Fase 77.b — shield name → its `sign:` algorithm, pre-resolved in
    /// Phase 0 (order-independent, unlike `channel_names`) so a `publish`
    /// lowers with its egress algorithm regardless of declaration order.
    /// Only SIGNING shields are recorded (empty `sign:` shields are not
    /// egress-relevant).
    shield_signs: HashMap<String, String>,
    /// §Fase 114 (owed) — channel name → the σ-shield it declares
    /// (`channel C { … shield: S }`), pre-resolved in Phase 0 (order-independent,
    /// like `shield_signs`) so an `emit C(v)` lowers with its channel's shield
    /// regardless of whether the `channel` decl precedes or follows the flow.
    /// Only channels with a non-empty `shield:` are recorded.
    channel_shields: HashMap<String, String>,
    /// §Fase 114.u — resource name → (endpoint config key, capacity),
    /// pre-resolved in Phase 0 (order-independent, like `shield_signs`) so an
    /// `upstream X { resource: R }` derives its dial address and its instance
    /// bound at LOWERING regardless of declaration order. Stamping the
    /// derivation into the artifact is the §114 shield-egress discipline:
    /// every dial path reads `IRUpstream.resolve` — none can forget the wire.
    resource_channels: HashMap<String, (String, Option<i64>)>,
    /// §Fase 114.w — shield name → its compiled breach policy (only shields
    /// with a non-empty `on_breach:` are recorded), pre-resolved in Phase 0 so
    /// the policy rides `IRShieldApplyStep` / `IREmit` regardless of
    /// declaration order.
    shield_policies: HashMap<String, crate::ir_nodes::IRBreachPolicy>,
    /// §Fase 115.e — dotted module path → `.axi` interface hash for every
    /// module the EMS resolved in this compilation. Empty (every pre-§115
    /// caller) ⇒ `visit_import` lowers exactly as in v2.75.0 (the new
    /// `IRImport` fields stay at their skip-serialized defaults).
    import_resolution: std::collections::BTreeMap<String, String>,
}

impl IRGenerator {
    pub fn new() -> Self {
        IRGenerator {
            grad_lets: std::cell::RefCell::new(HashMap::new()),
            personas: HashMap::new(),
            contexts: HashMap::new(),
            anchors: HashMap::new(),
            flows: HashMap::new(),
            lambda_data_specs: HashMap::new(),
            intention_ops: Vec::new(),
            program_line: 1,
            program_column: 1,
            channel_names: std::collections::HashSet::new(),
            shield_signs: HashMap::new(),
            channel_shields: HashMap::new(),
            resource_channels: HashMap::new(),
            shield_policies: HashMap::new(),
            import_resolution: std::collections::BTreeMap::new(),
        }
    }

    /// §Fase 115.e — supply the EMS resolution map (dotted module path →
    /// interface hash) so every `IRImport` this generation lowers carries
    /// `resolved: true` + its module's interface hash. Called by the EMS
    /// driver (`crate::ems`) on the linked program; no other caller needs it.
    pub fn with_import_resolution(
        mut self,
        resolution: std::collections::BTreeMap<String, String>,
    ) -> Self {
        self.import_resolution = resolution;
        self
    }

    /// §Fase 77.b (Phase 0) — record every declared shield's non-empty
    /// `sign:` algorithm, recursing into `epistemic` blocks (the same
    /// nesting `collect_emitted_channels` honours in the type checker).
    fn collect_shield_signs(&mut self, decls: &[Declaration]) {
        for decl in decls {
            match decl {
                Declaration::Shield(s) if !s.sign.is_empty() => {
                    self.shield_signs.insert(s.name.clone(), s.sign.clone());
                }
                Declaration::Epistemic(eb) => self.collect_shield_signs(&eb.body),
                _ => {}
            }
        }
    }

    /// §Fase 114 (owed) — Phase 0 pre-pass mirroring [`collect_shield_signs`]:
    /// record each `channel C { … shield: S }`'s shield so an `emit C(v)` lowers
    /// carrying S regardless of declaration order. Only non-empty shields are
    /// recorded (an unshielded channel leaves `IREmit.shield_ref` empty → the
    /// pre-§114 emit shape).
    fn collect_channel_shields(&mut self, decls: &[Declaration]) {
        for decl in decls {
            match decl {
                Declaration::Channel(c) if !c.shield_ref.is_empty() => {
                    self.channel_shields
                        .insert(c.name.clone(), c.shield_ref.clone());
                }
                Declaration::Epistemic(eb) => self.collect_channel_shields(&eb.body),
                _ => {}
            }
        }
    }

    /// §Fase 114.u (Phase 0) — record every declared resource's channel facts
    /// (endpoint config key + capacity) so `visit_upstream` can derive the
    /// dial address and the instance bound regardless of declaration order.
    fn collect_resource_channels(&mut self, decls: &[Declaration]) {
        for decl in decls {
            match decl {
                Declaration::Resource(r) => {
                    self.resource_channels
                        .insert(r.name.clone(), (r.endpoint.clone(), r.capacity));
                }
                Declaration::Epistemic(eb) => self.collect_resource_channels(&eb.body),
                _ => {}
            }
        }
    }

    /// §Fase 114.w (Phase 0) — record every declared shield's breach policy so
    /// the enforcement nodes carry it. A shield with no `on_breach:` records
    /// nothing (halt is the fail-closed default the runtime applies anyway).
    fn collect_shield_policies(&mut self, decls: &[Declaration]) {
        for decl in decls {
            match decl {
                Declaration::Shield(sh) if !sh.on_breach.is_empty() => {
                    self.shield_policies.insert(
                        sh.name.clone(),
                        crate::ir_nodes::IRBreachPolicy {
                            on_breach: sh.on_breach.clone(),
                            quarantine: sh.quarantine.clone(),
                            deflect_message: sh.deflect_message.clone(),
                            redact: sh.redact.clone(),
                            max_retries: sh.max_retries.unwrap_or(3),
                        },
                    );
                }
                Declaration::Epistemic(eb) => self.collect_shield_policies(&eb.body),
                _ => {}
            }
        }
    }

    /// §Fase 77.b (Phase 1.5) — walk every lowered body (flows + daemon
    /// listeners, recursing into conditionals / loops / par branches /
    /// nested listen + quant bodies) for `publish` sites carrying a
    /// resolved `sign`, and stamp the algorithm onto the matching
    /// channel's IR handle. First site wins (deterministic).
    fn mark_egress_channels(ir: &mut IRProgram) {
        fn walk(nodes: &[IRFlowNode], out: &mut HashMap<String, String>) {
            for node in nodes {
                match node {
                    IRFlowNode::Publish(p) if !p.sign.is_empty() => {
                        out.entry(p.channel_ref.clone())
                            .or_insert_with(|| p.sign.clone());
                    }
                    IRFlowNode::Conditional(c) => {
                        walk(&c.then_body, out);
                        walk(&c.else_body, out);
                    }
                    IRFlowNode::ForIn(f) => walk(&f.body, out),
                    IRFlowNode::Par(p) => {
                        for branch in &p.branches {
                            walk(branch, out);
                        }
                    }
                    IRFlowNode::Listen(l) => walk(&l.body, out),
                    IRFlowNode::Quant(q) => walk(&q.body, out),
                    _ => {}
                }
            }
        }
        let mut egress: HashMap<String, String> = HashMap::new();
        for flow in &ir.flows {
            walk(&flow.steps, &mut egress);
        }
        for daemon in &ir.daemons {
            for listener in &daemon.listeners {
                walk(&listener.body, &mut egress);
            }
        }
        for channel in &mut ir.channels {
            if let Some(sign) = egress.get(&channel.name) {
                channel.egress_sign = sign.clone();
            }
        }
    }

    pub fn generate(mut self, program: &Program) -> IRProgram {
        let mut ir = IRProgram::new();
        self.program_line = program.loc.line;
        self.program_column = program.loc.column;

        // Phase 0 (§Fase 77.b): pre-resolve every declared shield's `sign:`
        // BEFORE lowering, so a `publish C within S` resolves its egress
        // algorithm regardless of declaration order (a flow may precede the
        // shield in source; the incremental `channel_names` pattern would
        // miss it).
        self.collect_shield_signs(&program.declarations);
        // §Fase 114 (owed) — same Phase 0 discipline for channel shields, so an
        // `emit C(v)` lowers with C's declared σ-shield and the runtime scans the
        // egressing value on every dispatch path.
        self.collect_channel_shields(&program.declarations);
        // §Fase 114.u — same Phase 0 discipline for resource channels, so an
        // `upstream { resource: R }` derives address + instance bound at
        // lowering (the artifact carries the wire; no dial site can miss it).
        self.collect_resource_channels(&program.declarations);
        // §Fase 114.w — and shield breach policies, so `on_breach:` rides the
        // enforcement nodes on every dispatch path by construction.
        self.collect_shield_policies(&program.declarations);

        // Phase 1: visit all declarations
        for decl in &program.declarations {
            self.visit_declaration(decl, &mut ir);
        }

        // Phase 1.5 (§Fase 77.b): mark egress channels — a channel some
        // `publish` site declared under a SIGNING shield carries the
        // resolved algorithm on its IR handle (`egress_sign`), the single
        // fact the enterprise egress worker reads. First site wins
        // (deterministic; the v1 catalog has one algorithm).
        Self::mark_egress_channels(&mut ir);

        // Phase 2: resolve run cross-references
        for run in &mut ir.runs {
            if let Some(flow) = self.flows.get(&run.flow_name) {
                run.resolved_flow = Some(flow.clone());
            }
            if let Some(persona) = self.personas.get(&run.persona_name) {
                run.resolved_persona = Some(persona.clone());
            }
            if let Some(context) = self.contexts.get(&run.context_name) {
                run.resolved_context = Some(context.clone());
            }
            for anchor_name in &run.anchor_names {
                if let Some(anchor) = self.anchors.get(anchor_name) {
                    run.resolved_anchors.push(anchor.clone());
                }
            }
        }

        // Phase 3 (§8.2.h.2): assemble the intention tree if the program
        // declared any Fase-1 cognitive-I/O operations. Empty ⇒ `None`
        // (JSON `null`), matching Python's reference behaviour.
        if !self.intention_ops.is_empty() {
            ir.intention_tree = Some(IRIntentionTree {
                node_type: "intention_tree",
                source_line: self.program_line,
                source_column: self.program_column,
                operations: std::mem::take(&mut self.intention_ops),
            });
        }

        // Phase 4 (§Fase 53.b, founder refinement B): deterministic
        // extension order. Declarations across multiple `import`ed files
        // arrive in file+source order; sorting by the extension
        // identifier makes `ir.extensions` a pure function of the
        // declared set, so the proof-bundle hash (§53.d) is stable
        // regardless of declaration order. Stable sort preserves the
        // (already-deterministic, single-file) member order within each
        // extension.
        ir.extensions.sort_by(|a, b| a.name.cmp(&b.name));

        ir
    }

    fn visit_declaration(&mut self, decl: &Declaration, ir: &mut IRProgram) {
        match decl {
            // §Fase 114.a — a TOP-LEVEL budget governs every flow that calls the
            // tools its quotas name, not just a daemon's.
            Declaration::Budget(n) => ir.budgets.push(Self::visit_budget(n)),
            Declaration::Import(n) => ir.imports.push(self.visit_import(n)),
            Declaration::Persona(n) => {
                let node = self.visit_persona(n);
                self.personas.insert(node.name.clone(), node.clone());
                ir.personas.push(node);
            }
            Declaration::Context(n) => {
                let node = self.visit_context(n);
                self.contexts.insert(node.name.clone(), node.clone());
                ir.contexts.push(node);
            }
            Declaration::Anchor(n) => {
                let node = self.visit_anchor(n);
                self.anchors.insert(node.name.clone(), node.clone());
                ir.anchors.push(node);
            }
            Declaration::Memory(n) => ir.memories.push(self.visit_memory(n)),
            Declaration::Tool(n) => ir.tools.push(self.visit_tool(n)),
            Declaration::Type(n) => ir.types.push(self.visit_type(n)),
            Declaration::Flow(n) => {
                let node = self.visit_flow(n);
                self.flows.insert(node.name.clone(), node.clone());
                ir.flows.push(node);
            }
            Declaration::Intent(_) => {} // intent is inlined into steps
            Declaration::Run(n) => ir.runs.push(self.visit_run(n)),
            Declaration::LambdaData(n) => {
                let node = self.visit_lambda_data(n);
                self.lambda_data_specs
                    .insert(node.name.clone(), node.clone());
                ir.lambda_data_specs.push(node);
            }
            Declaration::Agent(n) => ir.agents.push(self.visit_agent(n)),
            Declaration::Shield(n) => ir.shields.push(self.visit_shield(n)),
            // §Fase 71.a — temporal execution-window guard.
            Declaration::Window(n) => ir.windows.push(Self::visit_window(n)),
            Declaration::Pix(n) => ir.pix_specs.push(self.visit_pix(n)),
            Declaration::Ledger(n) => ir.ledger_specs.push(self.visit_ledger(n)),
            Declaration::Psyche(n) => ir.psyche_specs.push(self.visit_psyche(n)),
            Declaration::Corpus(n) => ir.corpus_specs.push(self.visit_corpus(n)),
            Declaration::Dataspace(n) => ir.dataspace_specs.push(self.visit_dataspace(n)),
            Declaration::Ots(n) => ir.ots_specs.push(self.visit_ots(n)),
            Declaration::Mandate(n) => ir.mandate_specs.push(self.visit_mandate(n)),
            Declaration::Compute(n) => ir.compute_specs.push(self.visit_compute(n)),
            Declaration::Daemon(n) => ir.daemons.push(self.visit_daemon(n)),
            Declaration::AxonStore(n) => ir.axonstore_specs.push(self.visit_axonstore(n)),
            Declaration::AxonEndpoint(n) => ir.endpoints.push(self.visit_axonendpoint(n)),
            // §Fase 53.b — lower the `extension` declaration into the IR.
            // Deterministic ordering is applied once, at the end of
            // `generate` (Phase 4), not here.
            Declaration::Extension(n) => ir.extensions.push(self.visit_extension(n)),
            Declaration::Resource(n) => ir.resources.push(self.visit_resource(n)),
            Declaration::Fabric(n) => ir.fabrics.push(self.visit_fabric(n)),
            Declaration::Manifest(n) => {
                let m = self.visit_manifest(n);
                // §λ-L-E Fase 1 — manifest is a provisioning intention
                // (goes to the Free-Monad tree for the Handler layer).
                self.intention_ops
                    .push(IRIntentionOperation::Manifest(m.clone()));
                ir.manifests.push(m);
            }
            Declaration::Observe(n) => {
                let o = self.visit_observe(n);
                // §λ-L-E Fase 1 — observations are intentions too.
                self.intention_ops
                    .push(IRIntentionOperation::Observe(o.clone()));
                ir.observations.push(o);
            }
            Declaration::Reconcile(n) => ir.reconciles.push(self.visit_reconcile(n)),
            Declaration::Lease(n) => ir.leases.push(self.visit_lease(n)),
            Declaration::Ensemble(n) => ir.ensembles.push(self.visit_ensemble(n)),
            Declaration::Session(n) => ir.sessions.push(self.visit_session(n)),
            Declaration::Topology(n) => ir.topologies.push(self.visit_topology(n)),
            Declaration::Socket(n) => ir.sockets.push(self.visit_socket(n)),
            Declaration::Upstream(n) => ir.upstreams.push(self.visit_upstream(n)),
            Declaration::Cors(n) => ir.cors_policies.push(self.visit_cors(n)),
            Declaration::Cache(n) => ir.caches.push(self.visit_cache(n)),
            // §Fase 92.a — lower the ephemeral-credential contract.
            Declaration::Credential(n) => ir.credentials.push(self.visit_credential(n)),
            // §Fase 87.a — lower the `savant` orchestrator into the IR.
            Declaration::Savant(n) => ir.savants.push(self.visit_savant(n)),
            // §Fase 99.b — lower the `document` declaration into the IR.
            Declaration::Document(n) => ir.documents.push(self.visit_document(n)),
            // §Fase 105 — lower the `deliver` declaration into the IR.
            Declaration::Deliver(n) => ir.deliveries.push(self.visit_deliver(n)),
            Declaration::Notify(n) => ir.notifications.push(self.visit_notify(n)),
            // §Fase 87.d — lower the `synth` tool-synthesis policy into the IR.
            Declaration::Synth(n) => ir.synths.push(self.visit_synth(n)),
            // §Fase 88.a — lower the `scope` authorization policy into the IR.
            Declaration::Scope(n) => ir.scopes.push(self.visit_scope(n)),
            // §Fase 80.g — `voice` never reaches the IR: the parser already
            // expanded it into ordinary ots/session/socket/upstream
            // declarations (in this same program), and THOSE are the
            // deployed artifact. The declaration stays in the AST purely
            // for provenance + T852 validation (D80.6: the reviewer audits
            // the expansion, which is real IR, not the sugar).
            Declaration::Voice(_) => {}
            Declaration::Observable(n) => ir.observables.push(self.visit_observable(n)),
            Declaration::Witness(n) => ir.witnesses.push(self.visit_witness(n)),
            Declaration::Immune(n) => ir.immunes.push(self.visit_immune(n)),
            Declaration::Reflex(n) => ir.reflexes.push(self.visit_reflex(n)),
            Declaration::Heal(n) => ir.heals.push(self.visit_heal(n)),
            Declaration::Component(n) => ir.components.push(self.visit_component(n)),
            Declaration::View(n) => ir.views.push(self.visit_view(n)),
            // §λ-L-E Fase 13 — Mobile typed channels (paper §3, §4).
            // Record the channel name BEFORE visiting subsequent flow
            // bodies so `IREmit.value_is_channel` resolves correctly for
            // mobility uses appearing after this declaration in source
            // order (matches Python `_channels` dict semantics).
            Declaration::Channel(n) => {
                self.channel_names.insert(n.name.clone());
                ir.channels.push(self.visit_channel(n));
            }
            Declaration::Epistemic(eb) => {
                for child in &eb.body {
                    // §Fase 99.d — record the enclosing epistemic mode on a
                    // document so the barrier re-derives identically at deploy.
                    if let Declaration::Document(d) = child {
                        let mut ird = self.visit_document(d);
                        ird.epistemic_mode = eb.mode.clone();
                        ir.documents.push(ird);
                    } else if let Declaration::Notify(nf) = child {
                        // §Fase 110 — record the enclosing epistemic mode so the
                        // T933 barrier re-derives identically at deploy.
                        let mut irn = self.visit_notify(nf);
                        irn.epistemic_mode = eb.mode.clone();
                        ir.notifications.push(irn);
                    } else if let Declaration::Deliver(dl) = child {
                        // §Fase 105 — record the enclosing epistemic mode on a
                        // delivery so the T920 barrier re-derives identically at
                        // deploy (the §99.d document discipline, egress-dual).
                        let mut ird = self.visit_deliver(dl);
                        ird.epistemic_mode = eb.mode.clone();
                        ir.deliveries.push(ird);
                    } else {
                        self.visit_declaration(child, ir);
                    }
                }
            }
            Declaration::Let(_) => {}
            Declaration::Generic(g) => {
                // Emit as generic JSON in the appropriate collection
                let val = serde_json::json!({
                    "node_type": g.keyword,
                    "source_line": g.loc.line,
                    "source_column": g.loc.column,
                    "name": g.name,
                });
                // Tier 3+ generic fallback — no typed IR collection
                let _ = val; // suppress unused warning
            }
        }
    }

    // ── Visitors ─────────────────────────────────────────────────

    fn visit_import(&self, n: &ImportNode) -> IRImport {
        // §Fase 115.e — when the EMS driver supplied a resolution map, the
        // lowered import carries the proof it resolved (+ against WHICH
        // interface). Without a map both fields stay at their defaults and
        // are skip-serialized — v2.75.0 byte parity.
        let interface_hash = self
            .import_resolution
            .get(&n.module_path.join("."))
            .cloned();
        IRImport {
            node_type: "import",
            source_line: n.loc.line,
            source_column: n.loc.column,
            module_path: n.module_path.clone(),
            names: n.names.clone(),
            resolved: interface_hash.is_some(),
            interface_hash,
        }
    }

    fn visit_persona(&self, n: &PersonaDefinition) -> IRPersona {
        IRPersona {
            node_type: "persona",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            domain: n.domain.clone(),
            tone: n.tone.clone(),
            confidence_threshold: n.confidence_threshold,
            cite_sources: n.cite_sources,
            refuse_if: n.refuse_if.clone(),
            language: n.language.clone(),
            description: n.description.clone(),
        }
    }

    fn visit_context(&self, n: &ContextDefinition) -> IRContext {
        IRContext {
            node_type: "context",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            memory_scope: n.memory_scope.clone(),
            language: n.language.clone(),
            depth: n.depth.clone(),
            max_tokens: n.max_tokens,
            temperature: n.temperature,
            cite_sources: n.cite_sources,
            now_tz: n.now_tz.clone(),
        }
    }

    fn visit_anchor(&self, n: &AnchorConstraint) -> IRAnchor {
        IRAnchor {
            node_type: "anchor",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            description: n.description.clone(),
            require: n.require.clone(),
            reject: n.reject.clone(),
            enforce: n.enforce.clone(),
            confidence_floor: n.confidence_floor,
            unknown_response: n.unknown_response.clone(),
            on_violation: n.on_violation.clone(),
            on_violation_target: n.on_violation_target.clone(),
        }
    }

    fn visit_memory(&self, n: &MemoryDefinition) -> IRMemory {
        IRMemory {
            node_type: "memory",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            store: n.store.clone(),
            backend: n.backend.clone(),
            retrieval: n.retrieval.clone(),
            decay: n.decay.clone(),
        }
    }

    fn visit_tool(&self, n: &ToolDefinition) -> IRToolSpec {
        let effect_row = match &n.effects {
            Some(eff) => {
                let mut row = eff.effects.clone();
                if !eff.epistemic_level.is_empty() {
                    row.push(format!("epistemic:{}", eff.epistemic_level));
                }
                row
            }
            None => Vec::new(),
        };

        IRToolSpec {
            node_type: "tool_spec",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            provider: n.provider.clone(),
            max_results: n.max_results,
            filter_expr: n.filter_expr.clone(),
            timeout: n.timeout.clone(),
            runtime: n.runtime.clone(),
            resource_ref: n.resource_ref.clone(),
            sandbox: n.sandbox,
            input_schema: Vec::new(),
            output_schema: String::new(),
            // §Fase 58.c — carry the typed input schema + output type into the
            // IR (the §32 input_schema/output_schema above stay the validation
            // hints; these are the D1 type contract).
            parameters: n
                .parameters
                .iter()
                .map(|p| {
                    let mut type_name = p.type_expr.name.clone();
                    if !p.type_expr.generic_param.is_empty() {
                        type_name.push('<');
                        type_name.push_str(&p.type_expr.generic_param);
                        type_name.push('>');
                    }
                    crate::ir_nodes::IRToolParam {
                        name: p.name.clone(),
                        type_name,
                        optional: p.type_expr.optional,
                    }
                })
                .collect(),
            output_type: n.output_type.clone(),
            // §Fase 116.a — the required authorization scopes (D116.9; elided
            // when empty, IR-SHA stable for every pre-§116 tool).
            requires: n.requires.clone(),
            // §Fase 94.c — the dispatch-injection secret KEY (elided when
            // empty; the value NEVER rides the IR — it lives in custody).
            secret: n.secret.clone(),
            // §Fase 95.a — the partition parameter selecting the per-call
            // key segment (elided when empty; IR-SHA stable for every
            // pre-§95 tool). Only the parameter NAME travels, never a value.
            secret_partition: n.secret_partition.clone(),
            effect_row,
            // §Fase 84.b — Remote Hands technician fields (elided from the IR
            // when unset, per the `skip_serializing_if` on `IRToolSpec`).
            target: n.target.clone(),
            risk: n.risk.clone(),
            argv: n.argv.clone(),
            // §Fase 85.b — the cache-policy reference (elided when empty).
            cache: n.cache.clone(),
            // §Fase 98.b — the web-acquisition config (elided from the IR
            // when absent, per the `skip_serializing_if` on `IRToolSpec`).
            scrape: n.scrape.as_ref().map(|s| crate::ir_nodes::IRScrapeSpec {
                node_type: "scrape_spec",
                engine: s.engine.clone(),
                impersonate: s.impersonate.clone(),
                render_wait: s.render_wait.clone(),
                proxy: s.proxy.clone(),
                respect_robots: s.respect_robots,
                extract: s.extract.clone(),
                adaptive: s.adaptive,
                similarity_floor: s.similarity_floor,
                follow: s.follow.clone(),
                max_depth: s.max_depth,
                max_pages: s.max_pages,
                concurrency: s.concurrency,
                politeness: s.politeness.clone(),
                checkpoint: s.checkpoint.clone(),
            }),
        }
    }

    fn visit_type(&self, n: &TypeDefinition) -> IRType {
        let fields = n
            .fields
            .iter()
            .map(|f| IRTypeField {
                node_type: "type_field",
                source_line: f.loc.line,
                source_column: f.loc.column,
                name: f.name.clone(),
                type_name: f.type_expr.name.clone(),
                generic_param: f.type_expr.generic_param.clone(),
                optional: f.type_expr.optional,
            })
            .collect();

        let (range_min, range_max) = match &n.range_constraint {
            Some(rc) => (Some(rc.min_value), Some(rc.max_value)),
            None => (None, None),
        };

        let where_expression = match &n.where_clause {
            Some(wc) => wc.expression.clone(),
            None => String::new(),
        };

        IRType {
            node_type: "type_def",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            fields,
            range_min,
            range_max,
            where_expression,
            compliance: n.compliance.clone(),
        }
    }

    fn visit_flow(&self, n: &FlowDefinition) -> IRFlow {
        let parameters: Vec<IRParameter> = n
            .parameters
            .iter()
            .map(|p| IRParameter {
                node_type: "parameter",
                source_line: p.loc.line,
                source_column: p.loc.column,
                name: p.name.clone(),
                type_name: p.type_expr.name.clone(),
                generic_param: p.type_expr.generic_param.clone(),
                optional: p.type_expr.optional,
            })
            .collect();

        let (return_type_name, return_type_generic, return_type_optional) = match &n.return_type {
            Some(rt) => (rt.name.clone(), rt.generic_param.clone(), rt.optional),
            None => (String::new(), String::new(), false),
        };

        // Collect all flow body nodes as typed IR
        // §Fase 109.a — collect the flow's rich `let` expressions so the
        // Grad arm can differentiate them (recursing into nested bodies;
        // T932 enforces the "prior, same flow" discipline at check time).
        {
            fn collect_rich_lets(
                steps: &[FlowStep],
                out: &mut HashMap<String, crate::ast::Expr>,
            ) {
                for st in steps {
                    match st {
                        FlowStep::Let(l) => {
                            if let Some(e) = &l.value_ast {
                                out.insert(l.identifier.clone(), e.clone());
                            }
                        }
                        FlowStep::If(c) => {
                            collect_rich_lets(&c.then_body, out);
                            collect_rich_lets(&c.else_body, out);
                        }
                        FlowStep::ForIn(f) => collect_rich_lets(&f.body, out),
                        FlowStep::Par(pb) => {
                            pb.branches.iter().for_each(|b| collect_rich_lets(b, out))
                        }
                        FlowStep::Warden(w) => collect_rich_lets(&w.body, out),
                        _ => {}
                    }
                }
            }
            let mut map = HashMap::new();
            collect_rich_lets(&n.body, &mut map);
            *self.grad_lets.borrow_mut() = map;
        }
        let steps: Vec<IRFlowNode> = n.body.iter().map(|fs| self.visit_flow_step(fs)).collect();

        // Compute data edges from Step nodes: if step B's given references "A.output", create edge A → B
        let mut edges: Vec<IRDataEdge> = Vec::new();
        let step_names: Vec<String> = steps
            .iter()
            .filter_map(|n| {
                if let IRFlowNode::Step(s) = n {
                    Some(s.name.clone())
                } else {
                    None
                }
            })
            .collect();
        for node in &steps {
            if let IRFlowNode::Step(step) = node {
                if !step.given.is_empty() {
                    let given_root = step.given.split('.').next().unwrap_or("");
                    if step_names.contains(&given_root.to_string()) && given_root != step.name {
                        edges.push(IRDataEdge {
                            node_type: "data_edge",
                            source_line: step.source_line,
                            source_column: step.source_column,
                            source_step: given_root.to_string(),
                            target_step: step.name.clone(),
                            type_name: "Any".to_string(),
                        });
                    }
                }
            }
        }

        // Compute execution levels (topological ordering) — Step nodes only
        let execution_levels = self.compute_execution_levels(&steps, &edges);

        IRFlow {
            node_type: "flow",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            parameters,
            return_type_name,
            return_type_generic,
            return_type_optional,
            steps,
            edges,
            execution_levels,
        }
    }

    /// §Fase 70.a — lower a pure-expression AST node into its IR form. Pure
    /// structural map; operators become canonical lowercase strings.
    fn lower_expr(e: &Expr) -> IRExpr {
        match e {
            Expr::Lit(l) => IRExpr::Lit {
                lit: match l {
                    ExprLit::Int(i) => IRExprLit::Int { value: *i },
                    ExprLit::Float(f) => IRExprLit::Float { value: *f },
                    ExprLit::Bool(b) => IRExprLit::Bool { value: *b },
                    ExprLit::Str(s) => IRExprLit::Str { value: s.clone() },
                },
            },
            Expr::Ref(p) => IRExpr::Ref { path: p.clone() },
            Expr::Unary(op, operand) => IRExpr::Unary {
                op: match op {
                    UnOp::Neg => "neg",
                    UnOp::Not => "not",
                }
                .to_string(),
                operand: Box::new(Self::lower_expr(operand)),
            },
            Expr::Binary(op, lhs, rhs) => IRExpr::Binary {
                op: match op {
                    BinOp::Add => "add",
                    BinOp::Sub => "sub",
                    BinOp::Mul => "mul",
                    BinOp::Div => "div",
                    BinOp::Mod => "mod",
                    BinOp::Eq => "eq",
                    BinOp::Ne => "ne",
                    BinOp::Lt => "lt",
                    BinOp::Le => "le",
                    BinOp::Gt => "gt",
                    BinOp::Ge => "ge",
                    BinOp::And => "and",
                    BinOp::Or => "or",
                }
                .to_string(),
                lhs: Box::new(Self::lower_expr(lhs)),
                rhs: Box::new(Self::lower_expr(rhs)),
            },
            Expr::Call(builtin, args) => IRExpr::Call {
                builtin: builtin.surface().to_string(),
                args: args.iter().map(Self::lower_expr).collect(),
            },
            Expr::Field(base, field) => IRExpr::Field {
                base: Box::new(Self::lower_expr(base)),
                field: field.clone(),
            },
            Expr::Index(base, index) => IRExpr::Index {
                base: Box::new(Self::lower_expr(base)),
                index: Box::new(Self::lower_expr(index)),
            },
        }
    }

    fn visit_flow_step(&self, fs: &FlowStep) -> IRFlowNode {
        match fs {
            FlowStep::Step(s) => IRFlowNode::Step(IRStep {
                node_type: "step",
                source_line: s.loc.line,
                source_column: s.loc.column,
                name: s.name.clone(),
                persona_ref: s.persona_ref.clone(),
                given: s.given.clone(),
                ask: s.ask.clone(),
                use_tool: None,
                probe: None,
                reason: None,
                weave: None,
                output_type: s.output_type.clone(),
                confidence_floor: s.confidence_floor,
                navigate_ref: s.navigate_ref.clone(),
                apply_ref: s.apply_ref.clone(),
                requires_context: s.requires_context,
                now_tz: s.now_tz.clone(),
                pix_ops: s.pix_ops.iter().map(|op| self.visit_flow_step(op)).collect(),
                guards: s
                    .guards
                    .iter()
                    .map(|g| crate::ir_nodes::IRStepGuard {
                        kind: g.kind.clone(),
                        name: g.name.clone(),
                        target: g.target.clone(),
                        binding: g.binding.clone(),
                    })
                    .collect(),
                body: Vec::new(),
            }),
            // §Fase 92.b — `mint <Credential> as <binding>`.
            FlowStep::Mint(s) => IRFlowNode::Mint(crate::ir_nodes::IRMintStep {
                node_type: "mint",
                source_line: s.loc.line,
                source_column: s.loc.column,
                credential_ref: s.credential_ref.clone(),
                binding: s.binding.clone(),
            }),
            // §Fase 94.b — `rotate <SecretsStore> [where "…"] with <Tool>
            // as <binding>`.
            FlowStep::Rotate(s) => IRFlowNode::Rotate(crate::ir_nodes::IRRotateStep {
                node_type: "rotate",
                source_line: s.loc.line,
                source_column: s.loc.column,
                store_ref: s.store_ref.clone(),
                where_expr: s.where_expr.clone(),
                tool_ref: s.tool_ref.clone(),
                binding: s.binding.clone(),
            }),
            FlowStep::Probe(s) => IRFlowNode::Probe(IRProbe {
                node_type: "probe",
                source_line: s.loc.line,
                source_column: s.loc.column,
                target: s.target.clone(),
            }),
            FlowStep::Reason(s) => IRFlowNode::Reason(IRReasonStep {
                node_type: "reason",
                source_line: s.loc.line,
                source_column: s.loc.column,
                strategy: s.strategy.clone(),
                target: s.target.clone(),
                // §Fase 119.f.8 — the block form's fields. Elided when empty so
                // every pre-§119.f.8 program's IR JSON stays byte-identical
                // (the §68.b/§91.a no-IR-SHA-drift discipline).
                given: s.given.clone(),
                ask: s.ask.clone(),
                depth: s.depth,
            }),
            FlowStep::Validate(s) => IRFlowNode::Validate(IRValidateStep {
                node_type: "validate",
                source_line: s.loc.line,
                source_column: s.loc.column,
                target: s.target.clone(),
                rule: s.rule.clone(),
            }),
            FlowStep::Refine(s) => IRFlowNode::Refine(IRRefineStep {
                node_type: "refine",
                source_line: s.loc.line,
                source_column: s.loc.column,
                target: s.target.clone(),
                strategy: s.strategy.clone(),
            }),
            FlowStep::Weave(s) => IRFlowNode::Weave(IRWeaveStep {
                node_type: "weave",
                source_line: s.loc.line,
                source_column: s.loc.column,
                sources: s.sources.clone(),
                target: s.target.clone(),
                format_type: s.format_type.clone(),
                priority: s.priority.clone(),
                style: s.style.clone(),
                // §Fase 119.f.9 — elided when empty, so no IR-SHA drift.
                include: s.include.clone(),
            }),
            FlowStep::UseTool(s) => IRFlowNode::UseTool(IRUseToolStep {
                node_type: "use_tool",
                source_line: s.loc.line,
                source_column: s.loc.column,
                tool_name: s.tool_name.clone(),
                // §Fase 58.b — `LegacyPositional` projects its string verbatim
                // (D5, unchanged IR). `Named` keeps the legacy `argument` empty
                // and carries its pairs in `named_args` below (§58.c).
                argument: s.args.legacy_argument(),
                // §Fase 58.c — structured keyword args survive to the IR.
                named_args: match &s.args {
                    UseArgs::Named(pairs) => pairs
                        .iter()
                        .map(|(name, value, value_kind)| crate::ir_nodes::IRNamedArg {
                            name: name.clone(),
                            value: value.clone(),
                            value_kind: value_kind.clone(),
                        })
                        .collect(),
                    UseArgs::LegacyPositional(_) => Vec::new(),
                },
            }),
            FlowStep::Remember(s) => IRFlowNode::Remember(IRRememberStep {
                node_type: "remember",
                source_line: s.loc.line,
                source_column: s.loc.column,
                expression: s.expression.clone(),
                memory_target: s.memory_target.clone(),
            }),
            FlowStep::Recall(s) => IRFlowNode::Recall(IRRecallStep {
                node_type: "recall",
                source_line: s.loc.line,
                source_column: s.loc.column,
                query: s.query.clone(),
                memory_source: s.memory_source.clone(),
            }),
            FlowStep::If(s) => IRFlowNode::Conditional(IRConditional {
                node_type: "conditional",
                source_line: s.loc.line,
                source_column: s.loc.column,
                condition: s.condition.clone(),
                comparison_op: s.comparison_op.clone(),
                comparison_value: s.comparison_value.clone(),
                then_body: s
                    .then_body
                    .iter()
                    .map(|fs| self.visit_flow_step(fs))
                    .collect(),
                else_body: s
                    .else_body
                    .iter()
                    .map(|fs| self.visit_flow_step(fs))
                    .collect(),
                conditions: s.conditions.clone(),
                conjunctor: s.conjunctor.clone(),
                // §Fase 70.a — lower the expression form when present (rich
                // conditions only; legacy-shaped ones keep `cond = None`).
                cond: s.cond.as_ref().map(Self::lower_expr),
            }),
            FlowStep::ForIn(s) => IRFlowNode::ForIn(IRForIn {
                node_type: "for_in",
                source_line: s.loc.line,
                source_column: s.loc.column,
                variable: s.variable.clone(),
                iterable: s.iterable.clone(),
                body: s.body.iter().map(|fs| self.visit_flow_step(fs)).collect(),
            }),
            FlowStep::Let(s) => IRFlowNode::Let(IRLetBinding {
                node_type: "let_binding",
                source_line: s.loc.line,
                source_column: s.loc.column,
                target: s.identifier.clone(),
                value: s.value_expr.clone(),
                value_kind: if s.value_kind.is_empty() {
                    "literal".to_string()
                } else {
                    s.value_kind.clone()
                },
                // §Fase 70.f — lower the expression form when present.
                value_ast: s.value_ast.as_ref().map(Self::lower_expr),
            }),
            FlowStep::Return(s) => IRFlowNode::Return(IRReturnStep {
                node_type: "return",
                source_line: s.loc.line,
                source_column: s.loc.column,
                value_expr: s.value_expr.clone(),
            }),
            // Fase 19.e — break / continue. Both are payload-free at
            // both AST and IR level; the runner translates them into
            // sentinel exceptions caught by the enclosing for-in loop.
            FlowStep::Break(s) => IRFlowNode::Break(IRBreakStep {
                node_type: "break",
                source_line: s.loc.line,
                source_column: s.loc.column,
            }),
            FlowStep::Continue(s) => IRFlowNode::Continue(IRContinueStep {
                node_type: "continue",
                source_line: s.loc.line,
                source_column: s.loc.column,
            }),
            FlowStep::LambdaDataApply(s) => IRFlowNode::LambdaDataApply(IRLambdaDataApply {
                node_type: "lambda_data_apply",
                source_line: s.loc.line,
                source_column: s.loc.column,
                lambda_data_name: s.lambda_data_name.clone(),
                target: s.target.clone(),
                output_type: s.output_type.clone(),
            }),
            FlowStep::Par(s) => IRFlowNode::Par(IRParallelBlock {
                node_type: "parallel_block",
                source_line: s.loc.line,
                source_column: s.loc.column,
                // §Fase 65 — lower each AST branch (a Vec<FlowStep>) into a
                // flow-IR body so the dispatcher runs them concurrently.
                branches: s
                    .branches
                    .iter()
                    .map(|branch| branch.iter().map(|stmt| self.visit_flow_step(stmt)).collect())
                    .collect(),
            }),
            FlowStep::Hibernate(s) => IRFlowNode::Hibernate(IRHibernateStep {
                node_type: "hibernate",
                source_line: s.loc.line,
                source_column: s.loc.column,
                event_name: s.event_name.clone(),
                timeout: s.timeout.clone(),
            }),
            FlowStep::Deliberate(s) => IRFlowNode::Deliberate(IRDeliberateBlock {
                node_type: "deliberate",
                source_line: s.loc.line,
                source_column: s.loc.column,
            }),
            FlowStep::Consensus(s) => IRFlowNode::Consensus(IRConsensusBlock {
                node_type: "consensus",
                source_line: s.loc.line,
                source_column: s.loc.column,
            }),
            FlowStep::Forge(s) => IRFlowNode::Forge(IRForgeBlock {
                node_type: "forge",
                source_line: s.loc.line,
                source_column: s.loc.column,
                name: s.name.clone(),
                seed: s.seed.clone(),
                output_type: s.output_type.clone(),
                mode: s.mode.clone(),
                novelty: s.novelty,
                depth: s.depth,
                branches: s.branches,
                constraints_ref: s.constraints_ref.clone(),
            }),
            FlowStep::Grad(s) => {
                // §Fase 109.a — differentiate AT COMPILE TIME: the artifact
                // the IR carries IS the (simplified) derivative. A miss or a
                // non-differentiable construct on this path means a stale
                // artifact (T931/T932 refused it at check time on the happy
                // path) — emit the empty shape; the runtime fails CLOSED on
                // it and PCC `GradientSoundness` refutes it.
                let lets = self.grad_lets.borrow();
                let (original, derivatives) = match lets.get(&s.target) {
                    Some(expr) => {
                        let mut ds = Vec::with_capacity(s.wrt.len());
                        let mut ok = true;
                        for var in &s.wrt {
                            match crate::expr_diff::grad(expr, var) {
                                Ok(d) => ds.push(Self::lower_expr(&d)),
                                Err(_) => {
                                    ok = false;
                                    break;
                                }
                            }
                        }
                        if ok && !s.wrt.is_empty() {
                            (Some(Self::lower_expr(expr)), ds)
                        } else {
                            (None, Vec::new())
                        }
                    }
                    None => (None, Vec::new()),
                };
                IRFlowNode::Grad(IRGradStep {
                    node_type: "grad",
                    source_line: s.loc.line,
                    source_column: s.loc.column,
                    target: s.target.clone(),
                    wrt: s.wrt.clone(),
                    output: if s.output.is_empty() {
                        format!("d_{}", s.target)
                    } else {
                        s.output.clone()
                    },
                    original,
                    derivatives,
                })
            }
            FlowStep::Focus(s) => IRFlowNode::Focus(IRFocusStep {
                node_type: "focus",
                source_line: s.loc.line,
                source_column: s.loc.column,
                expression: s.expression.clone(),
                where_expr: s.where_expr.clone(),
                select: s.select.clone(),
                output: s.output.clone(),
            }),
            FlowStep::Associate(s) => IRFlowNode::Associate(IRAssociateStep {
                node_type: "associate",
                source_line: s.loc.line,
                source_column: s.loc.column,
                left: s.left.clone(),
                right: s.right.clone(),
                using_field: s.using_field.clone(),
                output: s.output.clone(),
            }),
            FlowStep::Aggregate(s) => IRFlowNode::Aggregate(IRAggregateStep {
                node_type: "aggregate",
                source_line: s.loc.line,
                source_column: s.loc.column,
                target: s.target.clone(),
                group_by: s.group_by.clone(),
                alias: s.alias.clone(),
                compute: s.compute.clone(),
                where_expr: s.where_expr.clone(),
            }),
            FlowStep::ExploreStep(s) => IRFlowNode::Explore(IRExploreStep {
                node_type: "explore",
                source_line: s.loc.line,
                source_column: s.loc.column,
                target: s.target.clone(),
                limit: s.limit,
                output: s.output.clone(),
            }),
            FlowStep::Ingest(s) => IRFlowNode::Ingest(IRIngestStep {
                node_type: "ingest",
                source_line: s.loc.line,
                source_column: s.loc.column,
                source: s.source.clone(),
                target: s.target.clone(),
                format: s.format.clone(),
                max_bytes: s.max_bytes,
                max_rows: s.max_rows,
            }),
            FlowStep::ShieldApply(s) => IRFlowNode::ShieldApply(IRShieldApplyStep {
                node_type: "shield_apply",
                source_line: s.loc.line,
                source_column: s.loc.column,
                shield_name: s.shield_name.clone(),
                target: s.target.clone(),
                output_type: s.output_type.clone(),
                // §Fase 114.w — the shield's breach policy rides the step.
                breach_policy: self.shield_policies.get(&s.shield_name).cloned(),
            }),
            FlowStep::Stream(s) => IRFlowNode::Stream(IRStreamBlock {
                node_type: "stream",
                source_line: s.loc.line,
                source_column: s.loc.column,
                // §Fase 111.e — lower the body. It used to be discarded at parse
                // time, which is why `run_stream` had nothing to run.
                body: s.body.iter().map(|st| self.visit_flow_step(st)).collect(),
            }),
            FlowStep::Navigate(s) => IRFlowNode::Navigate(IRNavigateStep {
                depth: s.depth,
                node_type: "navigate",
                source_line: s.loc.line,
                source_column: s.loc.column,
                pix_ref: s.pix_name.clone(),
                corpus_ref: s.corpus_name.clone(),
                query: s.query_expr.clone(),
                trail_enabled: s.trail_enabled,
                output_name: s.output_name.clone(),
                seed: s.seed.clone(),
                budget: s.budget,
                where_expr: s.where_expr.clone(),
            }),
            FlowStep::Drill(s) => IRFlowNode::Drill(IRDrillStep {
                node_type: "drill",
                source_line: s.loc.line,
                source_column: s.loc.column,
                pix_ref: s.pix_name.clone(),
                subtree_path: s.subtree_path.clone(),
                query: s.query_expr.clone(),
                output_name: s.output_name.clone(),
            }),
            FlowStep::Trail(s) => IRFlowNode::Trail(IRTrailStep {
                node_type: "trail",
                source_line: s.loc.line,
                source_column: s.loc.column,
                navigate_ref: s.navigate_ref.clone(),
            }),
            FlowStep::Corroborate(s) => IRFlowNode::Corroborate(IRCorroborateStep {
                node_type: "corroborate",
                source_line: s.loc.line,
                source_column: s.loc.column,
                navigate_ref: s.navigate_ref.clone(),
                output_name: s.output_name.clone(),
            }),
            FlowStep::OtsApply(s) => IRFlowNode::OtsApply(IROtsApplyStep {
                node_type: "ots_apply",
                source_line: s.loc.line,
                source_column: s.loc.column,
                ots_name: s.ots_name.clone(),
                target: s.target.clone(),
                output_type: s.output_type.clone(),
            }),
            FlowStep::MandateApply(s) => IRFlowNode::MandateApply(IRMandateApplyStep {
                node_type: "mandate_apply",
                source_line: s.loc.line,
                source_column: s.loc.column,
                mandate_name: s.mandate_name.clone(),
                target: s.target.clone(),
                output_type: s.output_type.clone(),
            }),
            FlowStep::ComputeApply(s) => IRFlowNode::ComputeApply(IRComputeApplyStep {
                node_type: "compute_apply",
                source_line: s.loc.line,
                source_column: s.loc.column,
                compute_name: s.compute_name.clone(),
                arguments: s.arguments.clone(),
                output_name: s.output_name.clone(),
            }),
            // §Fase 119.m.3 — `<Agent>(arg, …)`.
            FlowStep::AgentCall(s) => IRFlowNode::AgentCall(crate::ir_nodes::IRAgentCall {
                node_type: "agent_call",
                source_line: s.loc.line,
                source_column: s.loc.column,
                agent_name: s.agent_name.clone(),
                arguments: s.arguments.clone(),
            }),
            FlowStep::Listen(s) => IRFlowNode::Listen(self.lower_listen(s)),
            // §λ-L-E Fase 13 — Mobile typed channel reductions.
            FlowStep::Emit(s) => {
                // §Fase 114 (owed) — the target channel's declared σ-shield
                // (Phase 0 pre-pass; empty ⇒ unshielded channel). The runtime
                // scans the emitted value through it before the value leaves.
                let shield_ref = self
                    .channel_shields
                    .get(&s.channel_ref)
                    .cloned()
                    .unwrap_or_default();
                // §Fase 114.w — and the shield's breach policy rides beside it.
                let breach_policy = if shield_ref.is_empty() {
                    None
                } else {
                    self.shield_policies.get(&shield_ref).cloned()
                };
                IRFlowNode::Emit(IREmit {
                    node_type: "emit",
                    source_line: s.loc.line,
                    source_column: s.loc.column,
                    channel_ref: s.channel_ref.clone(),
                    value_ref: s.value_ref.clone(),
                    value_is_channel: self.channel_names.contains(&s.value_ref),
                    shield_ref,
                    breach_policy,
                })
            }
            FlowStep::Publish(s) => IRFlowNode::Publish(IRPublish {
                node_type: "publish",
                source_line: s.loc.line,
                source_column: s.loc.column,
                channel_ref: s.channel_ref.clone(),
                shield_ref: s.shield_ref.clone(),
                // §Fase 77.b — resolve the shield's egress algorithm at
                // lowering (Phase 0 pre-pass; empty ⇒ pure π-calc publish).
                sign: self
                    .shield_signs
                    .get(&s.shield_ref)
                    .cloned()
                    .unwrap_or_default(),
            }),
            FlowStep::Discover(s) => IRFlowNode::Discover(IRDiscover {
                node_type: "discover",
                source_line: s.loc.line,
                source_column: s.loc.column,
                capability_ref: s.capability_ref.clone(),
                alias: s.alias.clone(),
            }),
            FlowStep::DaemonStep(s) => IRFlowNode::DaemonStep(IRDaemonStepNode {
                node_type: "daemon",
                source_line: s.loc.line,
                source_column: s.loc.column,
                daemon_ref: s.daemon_ref.clone(),
            }),
            FlowStep::Persist(s) => IRFlowNode::Persist(IRPersistStep {
                node_type: "persist",
                source_line: s.loc.line,
                source_column: s.loc.column,
                store_name: s.store_name.clone(),
                fields: s.fields.clone(),
            }),
            FlowStep::Retrieve(s) => IRFlowNode::Retrieve(IRRetrieveStep {
                node_type: "retrieve",
                source_line: s.loc.line,
                source_column: s.loc.column,
                store_name: s.store_name.clone(),
                where_expr: s.where_expr.clone(),
                alias: s.alias.clone(),
                order_by: s.order_by.clone(),
                limit_expr: s.limit_expr.clone(),
                // §Fase 76.d — the aggregate surface (raw; elided when empty).
                aggregate: s.aggregate.clone(),
                group_by: s.group_by.clone(),
                // §Fase 85.b — the cache-policy reference (elided when empty).
                cache: s.cache.clone(),
            }),
            FlowStep::Mutate(s) => IRFlowNode::Mutate(IRMutateStep {
                node_type: "mutate",
                source_line: s.loc.line,
                source_column: s.loc.column,
                store_name: s.store_name.clone(),
                where_expr: s.where_expr.clone(),
                fields: s.fields.clone(),
            }),
            FlowStep::Purge(s) => IRFlowNode::Purge(IRPurgeStep {
                node_type: "purge",
                source_line: s.loc.line,
                source_column: s.loc.column,
                store_name: s.store_name.clone(),
                where_expr: s.where_expr.clone(),
            }),
            FlowStep::Transact(s) => IRFlowNode::Transact(IRTransactBlock {
                node_type: "transact",
                source_line: s.loc.line,
                source_column: s.loc.column,
            }),
            // §Fase 51.a — lower the `quant` block; the body lowers recursively
            // (like `par` branches) so the nested flow-IR is preserved.
            FlowStep::Warden(s) => IRFlowNode::Warden(crate::ir_nodes::IRWarden {
                node_type: "warden",
                source_line: s.loc.line,
                source_column: s.loc.column,
                target: s.target.clone(),
                scope_ref: s.scope_ref.clone(),
                body: s.body.iter().map(|stmt| self.visit_flow_step(stmt)).collect(),
            }),
            FlowStep::Quant(s) => IRFlowNode::Quant(crate::ir_nodes::IRQuant {
                node_type: "quant",
                source_line: s.loc.line,
                source_column: s.loc.column,
                encoding: s.encoding.clone(),
                observable: s.observable.clone(),
                qubits: s.qubits,
                depth: s.depth,
                bandwidth: s.bandwidth,
                reupload: s.reupload,
                effect: s.effect.clone(),
                body: s.body.iter().map(|stmt| self.visit_flow_step(stmt)).collect(),
            }),
            // §Fase 51.d.2 — the `yield` measurement point.
            FlowStep::Yield(s) => IRFlowNode::Yield(crate::ir_nodes::IRYield {
                node_type: "yield",
                source_line: s.loc.line,
                source_column: s.loc.column,
                value_expr: s.value_expr.clone(),
                value_kind: s.value_kind.clone(),
            }),
            // §Fase 52.c — `run <Flow>(args)` flow-step → reuse the run IR.
            FlowStep::Run(s) => IRFlowNode::Run(self.visit_run(s)),
            FlowStep::GenericStep(_) => {
                // Should not occur — all flow steps have dedicated handlers
                IRFlowNode::Step(IRStep {
                    node_type: "step",
                    source_line: 0,
                    source_column: 0,
                    name: String::new(),
                    persona_ref: String::new(),
                    given: String::new(),
                    ask: String::new(),
                    use_tool: None,
                    probe: None,
                    reason: None,
                    weave: None,
                    output_type: String::new(),
                    confidence_floor: None,
                    navigate_ref: String::new(),
                    apply_ref: String::new(),
                    requires_context: None,
                    now_tz: None,
                    pix_ops: Vec::new(),
                    guards: Vec::new(),
                    body: Vec::new(),
                })
            }
        }
    }

    fn compute_execution_levels(
        &self,
        steps: &[IRFlowNode],
        edges: &[IRDataEdge],
    ) -> Vec<Vec<String>> {
        // Extract Step-only names for DAG computation
        let step_nodes: Vec<&IRStep> = steps
            .iter()
            .filter_map(|n| {
                if let IRFlowNode::Step(s) = n {
                    Some(s)
                } else {
                    None
                }
            })
            .collect();

        if step_nodes.is_empty() {
            return Vec::new();
        }

        // Build dependency map
        let mut deps: HashMap<String, Vec<String>> = HashMap::new();
        for step in &step_nodes {
            deps.insert(step.name.clone(), Vec::new());
        }
        for edge in edges {
            deps.entry(edge.target_step.clone())
                .or_default()
                .push(edge.source_step.clone());
        }

        let mut levels: Vec<Vec<String>> = Vec::new();
        let mut placed: Vec<String> = Vec::new();

        loop {
            let mut level: Vec<String> = Vec::new();
            for step in &step_nodes {
                if placed.contains(&step.name) {
                    continue;
                }
                let step_deps = deps.get(&step.name).cloned().unwrap_or_default();
                if step_deps.iter().all(|d| placed.contains(d)) {
                    level.push(step.name.clone());
                }
            }
            if level.is_empty() {
                break;
            }
            placed.extend(level.clone());
            levels.push(level);
        }

        levels
    }

    // ── Tier 2 visitors ───────────────────────────────────────────

    fn visit_agent(&self, n: &AgentDefinition) -> IRAgent {
        IRAgent {
            node_type: "agent",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            goal: n.goal.clone(),
            tools: n.tools.clone(),
            memory_ref: n.memory_ref.clone(),
            strategy: n.strategy.clone(),
            on_stuck: n.on_stuck.clone(),
            shield_ref: n.shield_ref.clone(),
            max_iterations: n.max_iterations,
            max_tokens: n.max_tokens,
            max_time: n.max_time.clone(),
            max_cost: n.max_cost,
        }
    }

    /// §Fase 71.a — lower a temporal execution-window guard.
    fn visit_window(n: &WindowDefinition) -> IRWindow {
        IRWindow {
            node_type: "window",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            timezone: n.timezone.clone(),
            allow: n
                .allow
                .iter()
                .map(|s| IRWindowSpan {
                    day_start: s.day_start.clone(),
                    day_end: s.day_end.clone(),
                    hour_start: s.hour_start,
                    hour_end: s.hour_end,
                })
                .collect(),
            exclude: n.exclude.clone(),
            // §71.c default — an unset policy defers (the safe choice: never
            // run outside the window, retry when it opens).
            on_outside: if n.on_outside.is_empty() {
                "defer".to_string()
            } else {
                n.on_outside.clone()
            },
        }
    }

    /// §Fase 72.a — lower a `budget { … }` block. An omitted `on_exhausted`
    /// lowers to `block` (the fail-closed default: never over-emit).
    fn visit_budget(n: &BudgetBlock) -> IRBudget {
        IRBudget {
            node_type: "budget",
            source_line: n.loc.line,
            source_column: n.loc.column,
            // §Fase 114.a — empty for the anonymous daemon-attached form.
            name: n.name.clone(),
            quotas: n
                .quotas
                .iter()
                .map(|q| IRBudgetQuota {
                    kind: q.kind.clone(),
                    limit: q.limit,
                    period: q.period.clone(),
                    effect: q.effect.clone(),
                })
                .collect(),
            on_exhausted: if n.on_exhausted.is_empty() {
                "block".to_string()
            } else {
                n.on_exhausted.clone()
            },
        }
    }

    fn visit_shield(&self, n: &ShieldDefinition) -> IRShield {
        // §8.2.h — Python parity: strategy defaults "pattern"; Option<T> collapses to concrete zeros.
        let strategy = if n.strategy.is_empty() {
            "pattern".to_string()
        } else {
            n.strategy.clone()
        };
        IRShield {
            node_type: "shield",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            scan: n.scan.clone(),
            strategy,
            on_breach: n.on_breach.clone(),
            severity: n.severity.clone(),
            quarantine: n.quarantine.clone(),
            max_retries: n.max_retries.unwrap_or(0),
            confidence_threshold: n.confidence_threshold.unwrap_or(0.0),
            allow_tools: n.allow_tools.clone(),
            deny_tools: n.deny_tools.clone(),
            sandbox: n.sandbox.unwrap_or(false),
            redact: n.redact.clone(),
            log: n.log.clone(),
            deflect_message: n.deflect_message.clone(),
            taint: n.taint.clone(),
            compliance: n.compliance.clone(),
            sign: n.sign.clone(),
        }
    }

    fn visit_pix(&self, n: &PixDefinition) -> IRPix {
        IRPix {
            node_type: "pix",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            source: n.source.clone(),
            depth: n.depth,
            branching: n.branching,
            model: n.model.clone(),
        }
    }

    /// §Fase 62.0 — lower a `ledger` declaration to its audit-chain IR node.
    fn visit_ledger(&self, n: &LedgerDefinition) -> IRLedger {
        IRLedger {
            node_type: "ledger",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            source: n.source.clone(),
            depth: n.depth,
            branching: n.branching,
            model: n.model.clone(),
        }
    }

    fn visit_psyche(&self, n: &PsycheDefinition) -> IRPsyche {
        IRPsyche {
            node_type: "psyche",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            dimensions: n.dimensions.clone(),
            manifold_noise: n.manifold_noise,
            manifold_momentum: n.manifold_momentum,
            safety_constraints: n.safety_constraints.clone(),
            quantum_enabled: n.quantum_enabled,
            inference_mode: n.inference_mode.clone(),
        }
    }

    fn visit_corpus(&self, n: &CorpusDefinition) -> IRCorpus {
        IRCorpus {
            node_type: "corpus",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            documents: n.documents.clone(),
            // §Fase 63.A — lower the typed weighted edges (the MDN graph).
            relations: n
                .relations
                .iter()
                .map(|r| IRCorpusRelation {
                    etype: r.etype.clone(),
                    from: r.from.clone(),
                    to: r.to.clone(),
                    weight: r.weight,
                })
                .collect(),
            adaptive: n.adaptive,
            mcp_server: n.mcp_server.clone(),
            mcp_resource_uri: n.mcp_resource_uri.clone(),
            // §Fase 64.A — lower the dynamic store-sourced backing (None for the
            // static §63 corpus → serde-skipped → byte-identical IR).
            store_source: n.store_source.as_ref().map(|s| IRCorpusStoreSource {
                doc_store: s.doc_store.clone(),
                doc_id: s.doc_id_col.clone(),
                doc_title: s.doc_title_col.clone(),
                edge_store: s.edge_store.clone(),
                edge_from: s.edge_from_col.clone(),
                edge_to: s.edge_to_col.clone(),
                edge_type: s.edge_type_col.clone(),
                edge_weight: s.edge_weight_col.clone(),
            }),
        }
    }

    fn visit_dataspace(&self, n: &DataspaceDefinition) -> IRDataspace {
        IRDataspace {
            node_type: "dataspace",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            // §108.b — canonicalize the declared type (aliases resolved
            // here, once): the engine's deploy hook and the §108.d PCC
            // class read ONE spelling. A type the checker refused (T928)
            // never reaches IR generation on the happy path; a raw
            // passthrough survives only in a stale/hand-edited artifact,
            // which is the PCC class's problem, not this visitor's.
            columns: n
                .columns
                .iter()
                .map(|c| crate::ir_nodes::IRDataspaceColumn {
                    name: c.name.clone(),
                    column_type: crate::ast::DataspaceColumnType::from_token(&c.declared_type)
                        .map(|t| t.canonical_name().to_string())
                        .unwrap_or_else(|| c.declared_type.clone()),
                })
                .collect(),
        }
    }

    fn visit_ots(&self, n: &OtsDefinition) -> IROts {
        IROts {
            node_type: "ots",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            teleology: n.teleology.clone(),
            homotopy_search: n.homotopy_search.clone(),
            loss_function: n.loss_function.clone(),
        }
    }

    fn visit_mandate(&self, n: &MandateDefinition) -> IRMandate {
        IRMandate {
            node_type: "mandate",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            constraint: n.constraint.clone(),
            kp: n.kp,
            ki: n.ki,
            kd: n.kd,
            tolerance: n.tolerance,
            max_steps: n.max_steps,
            drift_bound: n.drift_bound,
            lipschitz: n.lipschitz,
            on_violation: n.on_violation.clone(),
        }
    }

    fn visit_compute(&self, n: &ComputeDefinition) -> IRCompute {
        IRCompute {
            node_type: "compute",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            shield_ref: n.shield_ref.clone(),
            // §Fase 111.f — the parameters and the body now reach the IR. Before
            // this, `IRCompute` carried only `name` + `shield_ref`, so the
            // "deterministic muscle" had nothing to flex: no inputs, no term.
            parameters: n
                .parameters
                .iter()
                .map(|p| IRParameter {
                    node_type: "parameter",
                    source_line: p.loc.line,
                    source_column: p.loc.column,
                    name: p.name.clone(),
                    type_name: p.type_expr.name.clone(),
                    generic_param: p.type_expr.generic_param.clone(),
                    optional: p.type_expr.optional,
                })
                .collect(),
            return_type: n.return_type.clone(),
            body: n.body.as_ref().map(Self::lower_expr),
        }
    }

    /// §Fase 52.a — lower one `listen` listener (channel + alias + handler
    /// body) to its IR. Shared by the flow-step `FlowStep::Listen` arm and the
    /// daemon listener list so both carry the (now-executing) body.
    fn lower_listen(&self, s: &ListenStep) -> IRListenStep {
        IRListenStep {
            node_type: "listen",
            source_line: s.loc.line,
            source_column: s.loc.column,
            channel: s.channel.clone(),
            channel_is_ref: s.channel_is_ref,
            event_alias: s.event_alias.clone(),
            body: s.body.iter().map(|st| self.visit_flow_step(st)).collect(),
        }
    }

    fn visit_daemon(&self, n: &DaemonDefinition) -> IRDaemon {
        IRDaemon {
            node_type: "daemon",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            goal: n.goal.clone(),
            tools: n.tools.clone(),
            memory_ref: n.memory_ref.clone(),
            strategy: n.strategy.clone(),
            on_stuck: n.on_stuck.clone(),
            shield_ref: n.shield_ref.clone(),
            // §Fase 71.c — the daemon's `window:` temporal binding.
            window_ref: n.window_ref.clone(),
            // §Fase 72.a — the daemon's `budget { … }` rate limit.
            budget: n.budget.as_ref().map(Self::visit_budget),
            max_tokens: n.max_tokens,
            max_time: n.max_time.clone(),
            max_cost: n.max_cost,
            // §Fase 52.a — listeners-with-bodies now survive lowering (were dropped).
            listeners: n.listeners.iter().map(|l| self.lower_listen(l)).collect(),
            // §Fase 52.d — the daemon's declared capability scope.
            requires_capabilities: n.requires_capabilities.clone(),
        }
    }

    /// §Fase 99.b — lower a `document` declaration into the IR. The effect row
    /// is flattened the same way tool effect rows are (base + `epistemic:`
    /// synthesis is not needed here — a document row carries `io`/`storage`/
    /// `sensitive:*`/`legal:*`). Block order is preserved for determinism.
    fn visit_document(&self, n: &crate::ast::DocumentDefinition) -> crate::ir_nodes::IRDocument {
        let effect_row = n
            .effects
            .as_ref()
            .map(|e| e.effects.clone())
            .unwrap_or_default();
        crate::ir_nodes::IRDocument {
            node_type: "document",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            target: n.target.clone(),
            template: n.template.clone(),
            provenance: n.provenance.clone(),
            effect_row,
            epistemic_mode: String::new(),
            blocks: n.blocks.iter().map(|b| self.visit_doc_block(b)).collect(),
        }
    }

    fn visit_doc_block(&self, b: &crate::ast::DocBlock) -> crate::ir_nodes::IRDocBlock {
        crate::ir_nodes::IRDocBlock {
            kind: b.kind.clone(),
            fields: b.fields.iter().map(|(k, v)| self.visit_doc_field(k, v)).collect(),
            children: b.children.iter().map(|c| self.visit_doc_block(c)).collect(),
        }
    }

    fn visit_doc_field(
        &self,
        name: &str,
        value: &crate::ast::DocScalar,
    ) -> crate::ir_nodes::IRDocField {
        use crate::ast::DocScalar;
        let (kind, value_str, items) = match value {
            DocScalar::Text(s) => ("text", s.clone(), Vec::new()),
            DocScalar::Ref(s) => ("ref", s.clone(), Vec::new()),
            DocScalar::Int(i) => ("int", i.to_string(), Vec::new()),
            DocScalar::Bool(b) => ("bool", b.to_string(), Vec::new()),
            DocScalar::List(items) => ("list", String::new(), items.clone()),
        };
        crate::ir_nodes::IRDocField {
            name: name.to_string(),
            kind,
            value: value_str,
            items,
        }
    }

    /// §Fase 105 — lower a `deliver` declaration into the IR. Operation order
    /// is preserved for determinism; field values reuse the document field
    /// lowering (`text`/`ref`/`list`/`int`/`bool`).
    fn visit_notify(&self, n: &crate::ast::NotifyDefinition) -> crate::ir_nodes::IRNotify {
        crate::ir_nodes::IRNotify {
            node_type: "notify",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            channel: n.channel.clone(),
            to_secret: n.to_secret.clone(),
            template: n.template.clone(),
            window: n.window.clone(),
            provenance: n.provenance.clone(),
            effects: n
                .effects
                .as_ref()
                .map(|e| e.effects.clone())
                .unwrap_or_default(),
            epistemic_mode: String::new(),
        }
    }

    fn visit_deliver(&self, n: &crate::ast::DeliverDefinition) -> crate::ir_nodes::IRDeliver {
        let effect_row = n
            .effects
            .as_ref()
            .map(|e| e.effects.clone())
            .unwrap_or_default();
        crate::ir_nodes::IRDeliver {
            node_type: "deliver",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            target: n.target.clone(),
            provenance: n.provenance.clone(),
            secret: n.secret.clone(),
            effect_row,
            epistemic_mode: String::new(),
            ops: n.ops.iter().map(|o| self.visit_deliver_op(o)).collect(),
        }
    }

    fn visit_deliver_op(&self, o: &crate::ast::DeliverOp) -> crate::ir_nodes::IRDeliverOp {
        crate::ir_nodes::IRDeliverOp {
            kind: o.kind.clone(),
            fields: o.fields.iter().map(|(k, v)| self.visit_doc_field(k, v)).collect(),
        }
    }

    /// §Fase 87.a — lower a `savant` orchestrator (surface only; the checker
    /// owns catalog/ref/budget validation).
    fn visit_savant(&self, n: &SavantDefinition) -> IRSavant {
        IRSavant {
            node_type: "savant",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            domain: n.domain.clone(),
            cognition: n.cognition.as_ref().map(Self::visit_savant_cognition),
            memory: n.memory.as_ref().map(Self::visit_savant_memory),
            budget: n.budget.as_ref().map(Self::visit_savant_budget),
            mandates: n
                .mandates
                .iter()
                .map(Self::visit_savant_mandate)
                .collect(),
        }
    }

    /// §Fase 88.a — lower a `scope` authorization policy (surface only; the
    /// checker owns catalog + non-empty + resolution validation).
    fn visit_scope(&self, n: &ScopeDefinition) -> crate::ir_nodes::IRScope {
        crate::ir_nodes::IRScope {
            node_type: "scope",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            targets: n.targets.clone(),
            depth: n.depth.clone(),
            approver: n.approver.clone(),
        }
    }

    fn visit_savant_cognition(n: &SavantCognition) -> IRSavantCognition {
        IRSavantCognition {
            depth: n.depth.clone(),
            entropic_threshold: n.entropic_threshold,
            divergence: n.divergence.clone(),
        }
    }

    fn visit_savant_memory(n: &SavantMemory) -> IRSavantMemory {
        IRSavantMemory {
            backend: n.backend.clone(),
            corpus_graph: n.corpus_graph,
            isolation_level: n.isolation_level.clone(),
        }
    }

    fn visit_savant_budget(n: &SavantBudget) -> IRSavantBudget {
        IRSavantBudget {
            max_iterations: n.max_iterations,
            max_tool_synth: n.max_tool_synth,
        }
    }

    fn visit_savant_mandate(n: &SavantMandate) -> IRSavantMandate {
        IRSavantMandate {
            name: n.name.clone(),
            objective: n.objective.clone(),
            output_type: n.output_type.clone(),
        }
    }

    /// §Fase 87.d — lower a `synth` policy. `review` lowers to the fail-closed
    /// default `required` when omitted (never a silent "no review").
    fn visit_synth(&self, n: &SynthDefinition) -> IRSynth {
        IRSynth {
            node_type: "synth",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            target: n.target.clone(),
            risk: n.risk.clone(),
            language: n.language.clone(),
            sandbox: n.sandbox.clone(),
            review: if n.review.is_empty() {
                "required".to_string()
            } else {
                n.review.clone()
            },
            max_lines: n.max_lines,
        }
    }

    fn visit_axonstore(&self, n: &AxonStoreDefinition) -> IRAxonStore {
        IRAxonStore {
            node_type: "axonstore",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            backend: n.backend.clone(),
            connection: n.connection.clone(),
            // §Fase 113 — the `resource` this store runs on (empty = the legacy
            // un-resourced form; skip-if-empty on the wire ⇒ no IR-SHA drift).
            resource_ref: n.resource_ref.clone(),
            confidence_floor: n.confidence_floor,
            isolation: n.isolation.clone(),
            on_breach: n.on_breach.clone(),
            capability: n.capability.clone(),
            class: n.class.clone(),
            // §Fase 38.b (D1) — thread the parsed column-schema
            // declaration (if any) through to the IR. The IR mirror
            // preserves the tagged-union shape (inline / manifest_ref /
            // env_var) and the canonical PascalCase column-type name.
            //
            // §Fase 94.a — a `backend: secrets` store carries the FIXED
            // synthesized metadata schema instead (an adopter-declared
            // schema on a secrets store is `axon-T900` and never reaches
            // a shipped IR): the artifact stays self-describing, so PCC
            // and the enterprise deploy gate re-derive the law's shape
            // from the IR alone.
            column_schema: if n.backend == "secrets" {
                Some(lower_column_schema(
                    &crate::store_schema::secrets_metadata_schema(
                        n.loc.line,
                        n.loc.column,
                    ),
                ))
            } else {
                n.column_schema.as_ref().map(lower_column_schema)
            },
        }
    }

    /// §Fase 53.b — lower an `extension` declaration to its IR mirror.
    /// Pure structural lowering; the category/no-shadowing/provenance
    /// invariants are enforced by the §53.c type-checker before this IR
    /// is consumed by §53.d PCC.
    fn visit_extension(&self, n: &crate::ast::ExtensionDefinition) -> crate::ir_nodes::IRExtension {
        crate::ir_nodes::IRExtension {
            node_type: "extension",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            category: n.category.clone(),
            members: n
                .members
                .iter()
                .map(|m| crate::ir_nodes::IRExtensionMember {
                    name: m.name.clone(),
                    semantics: m.semantics.clone(),
                    default_confidence: m.default_confidence,
                })
                .collect(),
        }
    }

    fn visit_axonendpoint(&self, n: &AxonEndpointDefinition) -> IRAxonEndpoint {
        // §8.2.h — Python emits `node_type: "endpoint"`; retries collapses Option<i64> → i64.
        IRAxonEndpoint {
            node_type: "endpoint",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            method: n.method.clone(),
            path: n.path.clone(),
            body_type: n.body_type.clone(),
            execute_flow: n.execute_flow.clone(),
            output_type: n.output_type.clone(),
            shield_ref: n.shield_ref.clone(),
            retries: n.retries.unwrap_or(0),
            timeout: n.timeout.clone(),
            compliance: n.compliance.clone(),
            // §Fase 37.y (D1) — IR mirror of `AxonEndpointDefinition.path_params`.
            // Direct clone (Vec<String>); the IR JSON omits the field
            // when the path has no placeholders (D5 backwards-compat).
            path_params: n.path_params.clone(),
            // §Fase 37.y (D2) — Lower each AST `TypeField` to an
            // `IRTypeField`. The catalog validation already happened
            // at parse time; the IR layer just shape-translates.
            query_params: n
                .query_params
                .iter()
                .map(|f| crate::ir_nodes::IRTypeField {
                    node_type: "type_field",
                    source_line: f.loc.line,
                    source_column: f.loc.column,
                    name: f.name.clone(),
                    type_name: f.type_expr.name.clone(),
                    generic_param: f.type_expr.generic_param.clone(),
                    optional: f.type_expr.optional,
                })
                .collect(),
            // §Fase 51.x — lower the `requires:` capability scopes into
            // the IR so the PCC CapabilityContainment property can read
            // them. Direct clone; IR JSON omits the field when empty
            // (D5 backwards-compat).
            requires_capabilities: n.requires_capabilities.clone(),
            // §Fase 83.a — the `cors: <Name>` reference. Direct clone;
            // IR JSON omits the field when empty (D83.5 / zero IR-SHA drift).
            cors_ref: n.cors_ref.clone(),
            // §Fase 89.a — the explicit authorization-coverage opt-out.
            // Direct clone; IR JSON omits it when false (zero IR-SHA drift).
            public: n.public,
        }
    }

    /// §λ-L-E Fase 1 — Resource IR lowering.
    fn visit_resource(&self, n: &ResourceDefinition) -> IRResource {
        IRResource {
            node_type: "resource",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            kind: n.kind.clone(),
            endpoint: n.endpoint.clone(),
            capacity: n.capacity,
            lifetime: n.lifetime.clone(),
            certainty_floor: n.certainty_floor,
            shield_ref: n.shield_ref.clone(),
            // §Fase 113 — the fabric this resource lives in.
            within: n.within.clone(),
        }
    }

    /// §λ-L-E Fase 1 — Fabric IR lowering.
    fn visit_fabric(&self, n: &FabricDefinition) -> IRFabric {
        IRFabric {
            node_type: "fabric",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            provider: n.provider.clone(),
            region: n.region.clone(),
            zones: n.zones,
            ephemeral: n.ephemeral,
            shield_ref: n.shield_ref.clone(),
        }
    }

    /// §λ-L-E Fase 1 — Manifest IR lowering.
    fn visit_manifest(&self, n: &ManifestDefinition) -> IRManifest {
        IRManifest {
            node_type: "manifest",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            resources: n.resources.clone(),
            fabric_ref: n.fabric_ref.clone(),
            region: n.region.clone(),
            zones: n.zones,
            compliance: n.compliance.clone(),
        }
    }

    /// §λ-L-E Fase 1 — Observe IR lowering.
    fn visit_observe(&self, n: &ObserveDefinition) -> IRObserve {
        IRObserve {
            node_type: "observe",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            target: n.target.clone(),
            sources: n.sources.clone(),
            quorum: n.quorum,
            timeout: n.timeout.clone(),
            on_partition: if n.on_partition.is_empty() {
                "fail".to_string()
            } else {
                n.on_partition.clone()
            },
            certainty_floor: n.certainty_floor,
        }
    }

    /// §λ-L-E Fase 3 — Reconcile IR lowering.
    fn visit_reconcile(&self, n: &ReconcileDefinition) -> IRReconcile {
        IRReconcile {
            node_type: "reconcile",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            observe_ref: n.observe_ref.clone(),
            threshold: n.threshold,
            tolerance: n.tolerance,
            on_drift: if n.on_drift.is_empty() {
                "provision".to_string()
            } else {
                n.on_drift.clone()
            },
            shield_ref: n.shield_ref.clone(),
            mandate_ref: n.mandate_ref.clone(),
            max_retries: n.max_retries,
        }
    }

    /// §λ-L-E Fase 3 — Lease IR lowering.
    fn visit_lease(&self, n: &LeaseDefinition) -> IRLease {
        IRLease {
            node_type: "lease",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            resource_ref: n.resource_ref.clone(),
            duration: n.duration.clone(),
            acquire: if n.acquire.is_empty() {
                "on_start".to_string()
            } else {
                n.acquire.clone()
            },
            on_expire: if n.on_expire.is_empty() {
                "anchor_breach".to_string()
            } else {
                n.on_expire.clone()
            },
        }
    }

    /// §λ-L-E Fase 5 — Immune IR lowering.
    fn visit_immune(&self, n: &ImmuneDefinition) -> IRImmune {
        IRImmune {
            node_type: "immune",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            watch: n.watch.clone(),
            sensitivity: n.sensitivity,
            baseline: if n.baseline.is_empty() {
                "learned".to_string()
            } else {
                n.baseline.clone()
            },
            window: n.window,
            scope: n.scope.clone(),
            tau: n.tau.clone(),
            decay: if n.decay.is_empty() {
                "exponential".to_string()
            } else {
                n.decay.clone()
            },
        }
    }

    /// §λ-L-E Fase 5 — Reflex IR lowering.
    fn visit_reflex(&self, n: &ReflexDefinition) -> IRReflex {
        IRReflex {
            node_type: "reflex",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            trigger: n.trigger.clone(),
            on_level: if n.on_level.is_empty() {
                "doubt".to_string()
            } else {
                n.on_level.clone()
            },
            action: n.action.clone(),
            scope: n.scope.clone(),
            sla: n.sla.clone(),
        }
    }

    /// §λ-L-E Fase 5 — Heal IR lowering.
    fn visit_heal(&self, n: &HealDefinition) -> IRHeal {
        IRHeal {
            node_type: "heal",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            source: n.source.clone(),
            on_level: if n.on_level.is_empty() {
                "doubt".to_string()
            } else {
                n.on_level.clone()
            },
            mode: if n.mode.is_empty() {
                "human_in_loop".to_string()
            } else {
                n.mode.clone()
            },
            scope: n.scope.clone(),
            review_sla: n.review_sla.clone(),
            shield_ref: n.shield_ref.clone(),
            max_patches: n.max_patches,
        }
    }

    /// §λ-L-E Fase 9 — Component IR lowering.
    fn visit_component(&self, n: &ComponentDefinition) -> IRComponent {
        IRComponent {
            node_type: "component",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            renders: n.renders.clone(),
            via_shield: n.via_shield.clone(),
            on_interact: n.on_interact.clone(),
            render_hint: if n.render_hint.is_empty() {
                "custom".to_string()
            } else {
                n.render_hint.clone()
            },
        }
    }

    /// §λ-L-E Fase 9 — View IR lowering.
    fn visit_view(&self, n: &ViewDefinition) -> IRView {
        IRView {
            node_type: "view",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            title: n.title.clone(),
            components: n.components.clone(),
            route: n.route.clone(),
        }
    }

    /// §λ-L-E Fase 4 — Session IR lowering.
    fn visit_session(&self, n: &SessionDefinition) -> IRSession {
        let roles = n
            .roles
            .iter()
            .map(|r| IRSessionRole {
                node_type: "session_role",
                source_line: r.loc.line,
                source_column: r.loc.column,
                name: r.name.clone(),
                steps: r
                    .steps
                    .iter()
                    .map(|s| self.lower_session_step_ir(s))
                    .collect(),
            })
            .collect();
        IRSession {
            node_type: "session",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            roles,
        }
    }

    /// §Fase 41.b — recursively lower a session step, including the nested
    /// `select`/`branch` choice sub-protocols (each branch is its own ordered
    /// step sequence). Mirrors the AST `SessionStep`/`SessionBranch` shape.
    fn lower_session_step_ir(&self, s: &SessionStep) -> IRSessionStep {
        IRSessionStep {
            node_type: "session_step",
            source_line: s.loc.line,
            source_column: s.loc.column,
            op: s.op.clone(),
            message_type: s.message_type.clone(),
            branches: s
                .branches
                .iter()
                .map(|b| IRSessionBranch {
                    node_type: "session_branch",
                    label: b.label.clone(),
                    steps: b
                        .steps
                        .iter()
                        .map(|st| self.lower_session_step_ir(st))
                        .collect(),
                })
                .collect(),
            // §Fase 79.b — interrupt-only fields; empty/false (and thus skipped
            // in the serialized IR) for every other op.
            binder: s.binder.clone(),
            resumable: s.resumable,
        }
    }

    /// §λ-L-E Fase 4 — Topology IR lowering.
    fn visit_topology(&self, n: &TopologyDefinition) -> IRTopology {
        IRTopology {
            node_type: "topology",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            nodes: n.nodes.clone(),
            edges: n
                .edges
                .iter()
                .map(|e| IRTopologyEdge {
                    node_type: "topology_edge",
                    source_line: e.loc.line,
                    source_column: e.loc.column,
                    source: e.source.clone(),
                    target: e.target.clone(),
                    session_ref: e.session_ref.clone(),
                })
                .collect(),
        }
    }

    /// §λ-L-E Fase 3 — Ensemble IR lowering.
    fn visit_ensemble(&self, n: &EnsembleDefinition) -> IREnsemble {
        IREnsemble {
            node_type: "ensemble",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            observations: n.observations.clone(),
            quorum: n.quorum,
            aggregation: if n.aggregation.is_empty() {
                "majority".to_string()
            } else {
                n.aggregation.clone()
            },
            certainty_mode: if n.certainty_mode.is_empty() {
                "min".to_string()
            } else {
                n.certainty_mode.clone()
            },
        }
    }

    fn visit_lambda_data(&self, n: &LambdaDataDefinition) -> IRLambdaData {
        IRLambdaData {
            node_type: "lambda_data",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            ontology: n.ontology.clone(),
            certainty: n.certainty,
            temporal_frame_start: n.temporal_frame_start.clone(),
            temporal_frame_end: n.temporal_frame_end.clone(),
            provenance: n.provenance.clone(),
            derivation: n.derivation.clone(),
        }
    }

    fn visit_run(&self, n: &RunStatement) -> IRRun {
        IRRun {
            node_type: "run",
            source_line: n.loc.line,
            source_column: n.loc.column,
            flow_name: n.flow_name.clone(),
            arguments: n.arguments.clone(),
            persona_name: n.persona.clone(),
            context_name: n.context.clone(),
            anchor_names: n.anchors.clone(),
            on_failure: n.on_failure.clone(),
            on_failure_params: n
                .on_failure_params
                .iter()
                .map(|(k, v)| vec![k.clone(), v.clone()])
                .collect(),
            output_to: n.output_to.clone(),
            effort: n.effort.clone(),
            resolved_flow: None,
            resolved_persona: None,
            resolved_context: None,
            resolved_anchors: Vec::new(),
        }
    }

    // ──────────────────────────────────────────────────────────────────
    //  §λ-L-E Fase 13 — Mobile Typed Channels (paper_mobile_channels.md)
    //  Declarative channels lower to IRChannel; emit/publish/discover
    //  are step-level reductions handled in `visit_flow_step`.
    // ──────────────────────────────────────────────────────────────────

    fn visit_channel(&self, n: &ChannelDefinition) -> IRChannel {
        IRChannel {
            node_type: "channel",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            message: n.message.clone(),
            qos: n.qos.clone(),
            lifetime: n.lifetime.clone(),
            persistence: n.persistence.clone(),
            shield_ref: n.shield_ref.clone(),
            // §Fase 77.b — stamped by `mark_egress_channels` (Phase 1.5)
            // once every publish site is lowered.
            egress_sign: String::new(),
        }
    }

    /// §Fase 41.b — compile a `socket` to its IR (the typed-WS transport
    /// binding; axon-rs realises the endpoint from this).
    fn visit_socket(&self, n: &SocketDefinition) -> IRSocket {
        IRSocket {
            node_type: "socket",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            protocol: n.protocol.clone(),
            backpressure_credit: n.backpressure_credit,
            reconnect: n.reconnect,
            legal_basis: n.legal_basis.clone(),
        }
    }

    /// §Fase 80.b — compile an `upstream` to its IR (the outbound vendor
    /// connection; axon-rs dials + transcodes from this alone — a new vendor
    /// is a new declaration, never new Rust code).
    fn visit_upstream(&self, n: &UpstreamDefinition) -> IRUpstream {
        IRUpstream {
            node_type: "upstream",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            transport: n.transport.clone(),
            protocol: n.protocol.clone(),
            role: n.role.clone(),
            // §Fase 114.u — a resourced upstream DERIVES its dial address from
            // the resource's `endpoint` (a per-tenant config key, axon-T944).
            // The derivation happens HERE, at lowering, so every dial path —
            // OSS voice legs, enterprise session legs, tests — reads the same
            // stamped `resolve` by construction (the §114 multi-path lesson).
            resolve: if n.resource_ref.is_empty() {
                n.resolve.clone()
            } else {
                self.resource_channels
                    .get(&n.resource_ref)
                    .map(|(endpoint, _)| endpoint.clone())
                    .unwrap_or_default()
            },
            resource_ref: n.resource_ref.clone(),
            // §Fase 114.u — max concurrent connection INSTANCES, from
            // `resource.capacity`. None for an un-resourced upstream (and for
            // a resource with no declared capacity): unbounded, the pre-114.u
            // behaviour, honestly unchanged.
            capacity: if n.resource_ref.is_empty() {
                None
            } else {
                self.resource_channels
                    .get(&n.resource_ref)
                    .and_then(|(_, cap)| *cap)
            },
            secret: n.secret.clone(),
            auth_kind: n.auth_kind.clone(),
            auth_name: n.auth_name.clone(),
            auth_prefix: n.auth_prefix.clone(),
            map: n
                .map
                .iter()
                .map(|r| IRUpstreamMapRule {
                    node_type: "upstream_map_rule",
                    direction: r.direction.clone(),
                    message: r.message.clone(),
                    framing: r.framing.clone(),
                    tag: r.tag.clone(),
                    when_field: r.when_field.clone(),
                    when_value: r.when_value.clone(),
                })
                .collect(),
            reconnect: n.reconnect.as_ref().map(|r| IRUpstreamReconnect {
                backoff_ms: r.backoff_ms,
                max_attempts: r.max_attempts,
                on_exhausted: r.on_exhausted.clone(),
            }),
            overflow: n.overflow.clone(),
            backpressure_credit: n.backpressure_credit,
            preset: n.preset.clone(),
        }
    }

    /// §Fase 83.a — lower a `cors Name { … }` declaration. Field-shape
    /// checks (wildcard+credentials T853, origin-glob T854, closed-method
    /// T855) already ran at type-check time — lowering is a pure
    /// shape-translation, no validation logic here.
    fn visit_cors(&self, n: &crate::ast::CorsDefinition) -> IRCors {
        IRCors {
            node_type: "cors",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            allow_origins: n.allow_origins.clone(),
            allow_methods: n.allow_methods.clone(),
            allow_headers: n.allow_headers.clone(),
            allow_credentials: n.allow_credentials,
            max_age: n.max_age.clone(),
            expose_headers: n.expose_headers.clone(),
        }
    }

    /// §Fase 92.a — lower an ephemeral-credential contract. The duration
    /// literal converts to SECONDS here (one arithmetic-ready
    /// representation for every consumer); an unparseable literal lowers
    /// to `0`, which `axon-T894` rejects before the IR ships.
    fn visit_credential(
        &self,
        n: &crate::ast::CredentialDefinition,
    ) -> crate::ir_nodes::IRCredential {
        crate::ir_nodes::IRCredential {
            node_type: "credential",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            ttl_secs: crate::duration_literal_to_secs(&n.ttl).unwrap_or(0),
            grants: n.grants.clone(),
        }
    }

    /// §Fase 85.b — lower a `cache` policy declaration (pure shape translation;
    /// every law already ran in the §85.c checker).
    fn visit_cache(&self, n: &crate::ast::CacheDefinition) -> crate::ir_nodes::IRCache {
        crate::ir_nodes::IRCache {
            node_type: "cache",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            backend: n.backend.clone(),
            ttl: n.ttl.clone(),
            key_params: n.key_params.clone(),
            default_policy: n.default_policy,
            apply_to_effects: n.apply_to_effects.clone(),
            invalidate_on: n.invalidate_on.clone(),
        }
    }

    /// §Fase 51.c.2 — lower a Pauli-sum observable declaration.
    fn visit_observable(&self, n: &crate::ast::ObservableDefinition) -> crate::ir_nodes::IRObservable {
        crate::ir_nodes::IRObservable {
            node_type: "observable",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            qubits: n.qubits,
            terms: n
                .terms
                .iter()
                .map(|t| crate::ir_nodes::IRPauliTerm {
                    coefficient: t.coefficient,
                    pauli: t.pauli.clone(),
                })
                .collect(),
        }
    }

    /// §Fase 69.a — lower a `witness` declaration into IR (verbatim refs +
    /// metric/threshold/baseline; the deploy/runtime evaluator computes the verdict).
    fn visit_witness(&self, n: &crate::ast::WitnessDefinition) -> crate::ir_nodes::IRWitness {
        crate::ir_nodes::IRWitness {
            node_type: "witness",
            source_line: n.loc.line,
            source_column: n.loc.column,
            name: n.name.clone(),
            claim: n.claim.clone(),
            baseline: n.baseline.clone(),
            metric: n.metric.clone(),
            threshold: n.threshold,
            data: n.data.clone(),
        }
    }
}

// ── §λ-L-E Fase 13 — Mobile Typed Channels IR generator tests ───────────────

#[cfg(test)]
mod fase13_ir_tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn compile(src: &str) -> IRProgram {
        let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
        let prog = Parser::new(tokens).parse().expect("parse");
        IRGenerator::new().generate(&prog)
    }

    #[test]
    fn channel_lowered_with_all_fields() {
        let src = r#"
            type Order { id: String }
            shield Gate { scan: [pii_leak] }
            channel C { message: Order qos: at_least_once lifetime: affine persistence: ephemeral shield: Gate }
        "#;
        let ir = compile(src);
        assert_eq!(ir.channels.len(), 1);
        let c = &ir.channels[0];
        assert_eq!(c.name, "C");
        assert_eq!(c.message, "Order");
        assert_eq!(c.qos, "at_least_once");
        assert_eq!(c.lifetime, "affine");
        assert_eq!(c.persistence, "ephemeral");
        assert_eq!(c.shield_ref, "Gate");
    }

    #[test]
    fn channel_second_order_message_preserved() {
        let ir = compile(
            r#"
            type Order { id: String }
            channel C1 { message: Order }
            channel C2 { message: Channel<Order> }
            channel C3 { message: Channel<Channel<Order>> }
        "#,
        );
        let names_to_msgs: std::collections::HashMap<_, _> = ir
            .channels
            .iter()
            .map(|c| (c.name.clone(), c.message.clone()))
            .collect();
        assert_eq!(names_to_msgs.get("C1"), Some(&"Order".to_string()));
        assert_eq!(names_to_msgs.get("C2"), Some(&"Channel<Order>".to_string()));
        assert_eq!(
            names_to_msgs.get("C3"),
            Some(&"Channel<Channel<Order>>".to_string())
        );
    }

    #[test]
    fn emit_value_is_channel_resolves_at_lowering() {
        let ir = compile(
            r#"
            type Order { id: String }
            channel Inner { message: Order }
            channel Outer { message: Channel<Order> }
            flow f() -> O { emit Outer(Inner) }
        "#,
        );
        let flow = &ir.flows[0];
        match &flow.steps[0] {
            IRFlowNode::Emit(e) => {
                assert_eq!(e.channel_ref, "Outer");
                assert_eq!(e.value_ref, "Inner");
                assert!(e.value_is_channel, "Inner is a registered channel");
            }
            other => panic!("expected Emit, got {:?}", other),
        }
    }

    #[test]
    fn emit_scalar_payload_value_is_channel_false() {
        let ir = compile(
            r#"
            type Order { id: String }
            channel Out { message: Order }
            flow f() -> O { emit Out(payload) }
        "#,
        );
        let flow = &ir.flows[0];
        match &flow.steps[0] {
            IRFlowNode::Emit(e) => {
                assert!(!e.value_is_channel, "scalar payload");
            }
            other => panic!("expected Emit, got {:?}", other),
        }
    }

    #[test]
    fn publish_lowered_with_shield_ref() {
        let ir = compile(
            r#"
            type Order { id: String }
            shield Gate { scan: [pii_leak] }
            channel C { message: Order shield: Gate }
            flow f() -> Cap { publish C within Gate }
        "#,
        );
        match &ir.flows[0].steps[0] {
            IRFlowNode::Publish(p) => {
                assert_eq!(p.channel_ref, "C");
                assert_eq!(p.shield_ref, "Gate");
            }
            other => panic!("expected Publish, got {:?}", other),
        }
    }

    #[test]
    fn discover_lowered_with_alias() {
        let ir = compile(
            r#"
            type Order { id: String }
            shield Gate { scan: [pii_leak] }
            channel C { message: Order shield: Gate }
            flow f() -> O { discover C as ch }
        "#,
        );
        match &ir.flows[0].steps[0] {
            IRFlowNode::Discover(d) => {
                assert_eq!(d.capability_ref, "C");
                assert_eq!(d.alias, "ch");
            }
            other => panic!("expected Discover, got {:?}", other),
        }
    }

    #[test]
    fn json_serialization_works() {
        let ir = compile(
            r#"
            type Order { id: String }
            channel C { message: Order }
            flow f() -> O { emit C(payload) }
        "#,
        );
        let json = serde_json::to_string(&ir).expect("serialize");
        assert!(json.contains(r#""node_type":"channel""#));
        assert!(json.contains(r#""node_type":"emit""#));
        assert!(json.contains(r#""value_is_channel":false"#));
    }
}

#[cfg(test)]
mod fase19_ir_tests {
    //! Fase 19.e — Rust mirror of break/continue keywords. The Python
    //! frontend already lowers BreakStatement → IRBreak and
    //! ContinueStatement → IRContinue (see Fase 19.e Python commit);
    //! these tests guard the Rust side at the IR-generator boundary
    //! so cross-stack parity goldens (Fase 19.h) compare on aligned
    //! shapes.

    use super::*;
    use crate::ir_nodes::IRFlowNode;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    /// Compile a minimal flow whose for-in body is the supplied
    /// snippet, and return the body's IR list.
    fn for_body_ir(body_src: &str) -> Vec<IRFlowNode> {
        let src = format!(
            "flow Probe() -> Out {{ for x in items.list {{ {body_src} }} }}"
        );
        let tokens = Lexer::new(&src, "<test>").tokenize().expect("lex");
        let prog = Parser::new(tokens).parse().expect("parse");
        let ir = IRGenerator::new().generate(&prog);
        let flow = ir
            .flows
            .iter()
            .find(|f| f.name == "Probe")
            .expect("flow Probe in IR");
        match flow.steps.first().expect("flow has at least one step") {
            IRFlowNode::ForIn(inner) => inner.body.clone(),
            other => panic!("expected ForIn, got {other:?}"),
        }
    }

    #[test]
    fn break_keyword_lowers_to_ir_break() {
        let body = for_body_ir("break");
        assert_eq!(body.len(), 1);
        match &body[0] {
            IRFlowNode::Break(b) => assert_eq!(b.node_type, "break"),
            other => panic!("expected IRFlowNode::Break, got {other:?}"),
        }
    }

    #[test]
    fn continue_keyword_lowers_to_ir_continue() {
        let body = for_body_ir("continue");
        assert_eq!(body.len(), 1);
        match &body[0] {
            IRFlowNode::Continue(c) => assert_eq!(c.node_type, "continue"),
            other => panic!("expected IRFlowNode::Continue, got {other:?}"),
        }
    }

    #[test]
    fn break_outside_loop_rejected_by_parser() {
        // A flow with `break` at the top level (not inside a for-in)
        // must fail to parse — the loop_depth scope check rejects it.
        let src = "flow F() -> Out { break }";
        let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
        let result = Parser::new(tokens).parse();
        assert!(result.is_err(), "parser must reject break outside loop");
    }

    #[test]
    fn continue_outside_loop_rejected_by_parser() {
        let src = "flow F() -> Out { continue }";
        let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
        let result = Parser::new(tokens).parse();
        assert!(result.is_err(), "parser must reject continue outside loop");
    }

    #[test]
    fn break_continue_serialize_with_node_type_field() {
        let body = for_body_ir("break\ncontinue");
        let json = serde_json::to_string(&body).expect("serialize");
        assert!(json.contains(r#""node_type":"break""#));
        assert!(json.contains(r#""node_type":"continue""#));
    }
}

// ════════════════════════════════════════════════════════════════════
//  §Fase 37.y.3 — IR mirror for path_params + query_params + D5
//  IR-JSON byte-identity backwards-compat.
// ════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod fase37y_ir_mirror_tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn lower_endpoint(src: &str) -> crate::ir_nodes::IRAxonEndpoint {
        let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
        let prog = Parser::new(tokens).parse().expect("parse");
        let ir = IRGenerator::new().generate(&prog);
        ir.endpoints
            .into_iter()
            .next()
            .expect("at least one endpoint in IR")
    }

    #[test]
    fn ir_carries_path_params_from_ast() {
        let src = r#"
            axonendpoint write {
                method: POST
                path: "/api/tenants/{tenant_id}/secrets/{secret_name}"
                body: SecretWriteRequest
                execute: Write
            }
        "#;
        let ep = lower_endpoint(src);
        assert_eq!(
            ep.path_params,
            vec!["tenant_id".to_string(), "secret_name".to_string()],
            "IR.path_params mirrors AST.path_params 1:1"
        );
    }

    #[test]
    fn ir_carries_query_params_with_type_field_shape() {
        let src = r#"
            axonendpoint list {
                method: GET
                path: "/api/users"
                query: { status: Text, limit: Int?, after: Uuid? }
                execute: ListUsers
            }
        "#;
        let ep = lower_endpoint(src);
        assert_eq!(ep.query_params.len(), 3);
        assert_eq!(ep.query_params[0].name, "status");
        assert_eq!(ep.query_params[0].type_name, "Text");
        assert!(!ep.query_params[0].optional);
        assert_eq!(ep.query_params[1].name, "limit");
        assert_eq!(ep.query_params[1].type_name, "Int");
        assert!(ep.query_params[1].optional);
        assert_eq!(ep.query_params[2].name, "after");
        assert_eq!(ep.query_params[2].type_name, "Uuid");
        assert!(ep.query_params[2].optional);
        // node_type stays canonical for downstream JSON consumers.
        assert_eq!(ep.query_params[0].node_type, "type_field");
    }

    #[test]
    fn d5_byte_identity_when_no_path_or_query() {
        // The load-bearing D5 backwards-compat assertion: an endpoint
        // with no path placeholders AND no query block produces IR
        // JSON byte-identical to the pre-v1.38.5 output. The new
        // fields use `skip_serializing_if = Vec::is_empty` so they
        // simply don't appear in the JSON.
        let src = r#"
            axonendpoint hello {
                method: GET
                path: "/api/hello"
                body: HelloRequest
                execute: Hello
            }
        "#;
        let ep = lower_endpoint(src);
        let json = serde_json::to_string(&ep).expect("serialize");
        assert!(
            !json.contains("path_params"),
            "D5: absent `path_params` key when empty. Got: {json}"
        );
        assert!(
            !json.contains("query_params"),
            "D5: absent `query_params` key when empty. Got: {json}"
        );
    }

    #[test]
    fn ir_json_emits_path_params_when_present() {
        let src = r#"
            axonendpoint x {
                method: GET
                path: "/api/users/{id}"
                execute: X
            }
        "#;
        let ep = lower_endpoint(src);
        let json = serde_json::to_string(&ep).expect("serialize");
        assert!(
            json.contains(r#""path_params":["id"]"#),
            "path_params present in JSON. Got: {json}"
        );
    }

    #[test]
    fn ir_json_emits_query_params_as_type_field_array() {
        let src = r#"
            axonendpoint x {
                method: GET
                path: "/api/x"
                query: { status: Text? }
                execute: X
            }
        "#;
        let ep = lower_endpoint(src);
        let json = serde_json::to_string(&ep).expect("serialize");
        assert!(json.contains("query_params"), "key present: {json}");
        assert!(json.contains(r#""name":"status""#), "field name: {json}");
        assert!(json.contains(r#""type_name":"Text""#), "type_name: {json}");
        assert!(json.contains(r#""optional":true"#), "optional: {json}");
    }

    #[test]
    fn ir_round_trips_kivi_corpus() {
        // The end-to-end combined corpus from the kivi adopter report —
        // both 37.y.1 (path) and 37.y.2 (query) round-trip through IR.
        let src = r#"
            axonendpoint write_secret {
                method: POST
                path: "/api/tenants/{tenant_id}/secrets/{secret_name}"
                query: { dry_run: Bool?, overwrite: Bool? }
                body: SecretWriteRequest
                execute: WriteSecret
            }
        "#;
        let ep = lower_endpoint(src);
        assert_eq!(ep.path_params, vec!["tenant_id", "secret_name"]);
        assert_eq!(ep.query_params.len(), 2);
        assert_eq!(ep.query_params[0].name, "dry_run");
        assert!(ep.query_params[0].optional);
        assert_eq!(ep.body_type, "SecretWriteRequest");
        assert_eq!(ep.method, "POST");
    }
}
