//! v2.76.0 — Phase 1 of the Epistemic Module System: `.axi` interfaces.
//!
//! A [`CognitiveInterface`] is the public surface of one module — every
//! top-level *named* declaration (the design decision, via [`crate::ast::declaration_surface`])
//! reduced to its **signature**: what an importer must know to type-check
//! and to trust, never the implementation (ask text, step bodies, PID
//! gains, endpoint literals).
//!
//! # The dual-hash scheme (GHC ABI hash)
//!
//! - [`CognitiveInterface::content_hash`] — SHA-256 of the source bytes;
//!   changes on ANY edit.
//! - [`CognitiveInterface::interface_hash`] — SHA-256 of the canonical
//!   `.axi` JSON (with the hash field itself excluded); changes only when
//!   the PUBLIC surface changes. A comment-only edit keeps it stable —
//! the precondition for early cutoff (v2.76.0).
//!
//! # Where the soundness line sits (AST-merge architecture)
//!
//! Signatures deliberately hide bodies. That is sound because the linked
//! program is built by **merging the module ASTs** (v2.76.0): the merged
//! semantic revalidation and the single IR generation always see full
//! declarations. Per-module validation consumes ONLY what the signature
//! carries (name + kind + the fields below), so a body-only edit never
//! invalidates a dependent's per-module pass — and can never make it
//! stale, because nothing body-derived is ever cached per-dependent.
//!
//! Hashing honors the crate's zero-runtime-dep discipline: SHA-256 is the
//! v1.31.0 hand-rolled FIPS 180-4 [`crate::store_schema_manifest::sha256_hex`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ast::{declaration_surface, Declaration, Program, TypeExpr};
use crate::module_resolver::ModulePath;
use crate::store_schema_manifest::sha256_hex;

/// `.axi` format version. Bumping it busts every compilation cache
/// wholesale (v2.76.0 law 6) — a shape change may never meet stale bytes.
pub const AXI_FORMAT_VERSION: u32 = 1;

// ════════════════════════════════════════════════════════════════════
// Epistemic floor (section 3.4 of the EMS paper)
// ════════════════════════════════════════════════════════════════════

/// The module-level epistemic guarantee, derived from content — never
/// from an annotation an author could forget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EpistemicFloor {
    Unspecified = 0,
    Speculate = 1,
    Doubt = 2,
    Believe = 3,
    Know = 4,
}

impl EpistemicFloor {
    pub fn as_str(self) -> &'static str {
        match self {
            EpistemicFloor::Unspecified => "unspecified",
            EpistemicFloor::Speculate => "speculate",
            EpistemicFloor::Doubt => "doubt",
            EpistemicFloor::Believe => "believe",
            EpistemicFloor::Know => "know",
        }
    }

    pub fn rank(self) -> u8 {
        self as u8
    }

    fn from_mode(mode: &str) -> EpistemicFloor {
        match mode {
            "know" => EpistemicFloor::Know,
            "believe" => EpistemicFloor::Believe,
            "doubt" => EpistemicFloor::Doubt,
            "speculate" => EpistemicFloor::Speculate,
            _ => EpistemicFloor::Unspecified,
        }
    }
}

/// Floor rules, highest wins (recursing into epistemic blocks):
/// anchors ⇒ know · shields ⇒ believe · `know|believe|doubt|speculate`
/// block ⇒ its level · otherwise unspecified.
pub fn compute_floor(program: &Program) -> EpistemicFloor {
    fn walk(decls: &[Declaration], floor: &mut EpistemicFloor) {
        for decl in decls {
            let candidate = match decl {
                Declaration::Anchor(_) => EpistemicFloor::Know,
                Declaration::Shield(_) => EpistemicFloor::Believe,
                Declaration::Epistemic(eb) => {
                    let level = EpistemicFloor::from_mode(&eb.mode);
                    walk(&eb.body, floor);
                    level
                }
                _ => EpistemicFloor::Unspecified,
            };
            if candidate > *floor {
                *floor = candidate;
            }
        }
    }
    let mut floor = EpistemicFloor::Unspecified;
    walk(&program.declarations, &mut floor);
    floor
}

// ════════════════════════════════════════════════════════════════════
//  Export signatures
// ════════════════════════════════════════════════════════════════════

/// One exported declaration's signature. Six kinds carry structured
/// fields (the ones cross-module validation and trust decisions consume);
/// every other named kind exports as `Other { kind }` — name + kind is
/// exactly what the checker's symbol table needs for it, and nothing
/// body-derived is ever consumed cross-module before the merged
/// revalidation (which sees full declarations).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ExportSignature {
    Persona {
        domain: Vec<String>,
        tone: String,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        confidence_threshold: Option<f64>,
    },
    Anchor {
        /// SHA-256 over the constraint's semantic fields — hides the
        /// text, detects any change to the enforced meaning.
        constraint_hash: String,
        on_violation: String,
    },
    Flow {
        params: Vec<(String, String)>,
        output_type: String,
        step_count: usize,
    },
    Shield {
        scan: Vec<String>,
        on_breach: String,
    },
    Tool {
        effects: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        risk: Option<String>,
        provider: String,
        /// v2.77.0 — the authorization scopes the tool requires.
        /// PUBLIC surface: an importer must see the scope demands (axon-T956
        /// coverage), and the interface_hash must cover it so a `requires:`
        /// change invalidates every dependent (early-cutoff soundness, v2.76.0).
        /// Elided when empty ⇒ every pre-v2.77.0 tool's `.axi` is byte-identical.
        #[serde(skip_serializing_if = "Vec::is_empty", default)]
        requires: Vec<String>,
    },
    Resource {
        resource_kind: String,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        capacity: Option<i64>,
        lifetime: String,
    },
    Other {
        other_kind: String,
    },
}

impl ExportSignature {
    /// The symbol-table kind string (parity with the type-checker's
    /// `register_declarations` — see `declaration_surface`).
    pub fn kind(&self) -> &str {
        match self {
            ExportSignature::Persona { .. } => "persona",
            ExportSignature::Anchor { .. } => "anchor",
            ExportSignature::Flow { .. } => "flow",
            ExportSignature::Shield { .. } => "shield",
            ExportSignature::Tool { .. } => "tool",
            ExportSignature::Resource { .. } => "resource",
            ExportSignature::Other { other_kind } => other_kind,
        }
    }
}

fn type_spelling(t: &TypeExpr) -> String {
    let mut s = t.name.clone();
    if !t.generic_param.is_empty() {
        s.push('<');
        s.push_str(&t.generic_param);
        s.push('>');
    }
    if t.optional {
        s.push('?');
    }
    s
}

// ════════════════════════════════════════════════════════════════════
//  CognitiveInterface — the .axi
// ════════════════════════════════════════════════════════════════════

/// The `.axi` — a module's compiled cognitive interface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CognitiveInterface {
    pub format_version: u32,
    /// Dotted module path (`axon.security`).
    pub module: String,
    pub epistemic_floor: EpistemicFloor,
    /// SHA-256 of the module's source bytes.
    pub content_hash: String,
    /// SHA-256 of this interface's canonical JSON (this field excluded).
    pub interface_hash: String,
    /// Name → signature, deterministically ordered.
    pub exports: BTreeMap<String, ExportSignature>,
}

impl CognitiveInterface {
    /// Serialize to canonical `.axi` JSON (pretty, stable field order —
    /// the byte shape the interface hash is computed over and the cache
    /// persists).
    pub fn to_axi_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("interface serializes")
    }

    pub fn from_axi_json(s: &str) -> Option<CognitiveInterface> {
        serde_json::from_str(s).ok()
    }
}

/// Extract the `.axi` interface of one module (Phase 1).
pub fn generate_interface(
    module: &ModulePath,
    program: &Program,
    source: &str,
) -> CognitiveInterface {
    let mut exports: BTreeMap<String, ExportSignature> = BTreeMap::new();

    fn collect(decls: &[Declaration], exports: &mut BTreeMap<String, ExportSignature>) {
        for decl in decls {
            if let Declaration::Epistemic(eb) = decl {
                collect(&eb.body, exports);
                continue;
            }
            let Some((name, kind, _loc)) = declaration_surface(decl) else {
                continue;
            };
            if name.is_empty() {
                continue;
            }
            let sig = match decl {
                Declaration::Persona(n) => ExportSignature::Persona {
                    domain: n.domain.clone(),
                    tone: n.tone.clone(),
                    confidence_threshold: n.confidence_threshold,
                },
                Declaration::Anchor(n) => {
                    let mut basis = String::new();
                    basis.push_str(&n.require);
                    basis.push('\u{1f}');
                    basis.push_str(&n.reject.join("\u{1f}"));
                    basis.push('\u{1f}');
                    basis.push_str(&n.enforce);
                    basis.push('\u{1f}');
                    basis.push_str(&n.unknown_response);
                    basis.push('\u{1f}');
                    if let Some(cf) = n.confidence_floor {
                        basis.push_str(&format!("{cf:.6}"));
                    }
                    basis.push('\u{1f}');
                    basis.push_str(&n.on_violation_target);
                    ExportSignature::Anchor {
                        constraint_hash: sha256_hex(basis.as_bytes()),
                        on_violation: n.on_violation.clone(),
                    }
                }
                Declaration::Flow(n) => ExportSignature::Flow {
                    params: n
                        .parameters
                        .iter()
                        .map(|p| (p.name.clone(), type_spelling(&p.type_expr)))
                        .collect(),
                    output_type: n
                        .return_type
                        .as_ref()
                        .map(type_spelling)
                        .unwrap_or_default(),
                    step_count: n.body.len(),
                },
                Declaration::Shield(n) => ExportSignature::Shield {
                    scan: n.scan.clone(),
                    on_breach: n.on_breach.clone(),
                },
                Declaration::Tool(n) => ExportSignature::Tool {
                    effects: n
                        .effects
                        .as_ref()
                        .map(|e| e.effects.clone())
                        .unwrap_or_default(),
                    risk: n.risk.clone(),
                    provider: n.provider.clone(),
                    requires: n.requires.clone(),
                },
                Declaration::Resource(n) => ExportSignature::Resource {
                    resource_kind: n.kind.clone(),
                    capacity: n.capacity,
                    lifetime: n.lifetime.clone(),
                },
                _ => ExportSignature::Other { other_kind: kind },
            };
            exports.insert(name, sig);
        }
    }
    collect(&program.declarations, &mut exports);

    let mut interface = CognitiveInterface {
        format_version: AXI_FORMAT_VERSION,
        module: module.dotted(),
        epistemic_floor: compute_floor(program),
        content_hash: sha256_hex(source.as_bytes()),
        interface_hash: String::new(),
        exports,
    };
    // The interface hash covers ONLY the public surface (module path,
    // floor, exports) — BOTH hash fields are zeroed in the hashed bytes.
    // Hashing the content hash too would drag every source edit into the
    // interface identity and kill early cutoff (the exact property the
    // dual-hash scheme exists to provide).
    let mut hashed = interface.clone();
    hashed.content_hash = String::new();
    interface.interface_hash = sha256_hex(hashed.to_axi_json().as_bytes());
    interface
}

// ════════════════════════════════════════════════════════════════════
//  ModuleRegistry
// ════════════════════════════════════════════════════════════════════

/// The resolved interfaces of every module in a compilation, keyed by
/// module path. What the type-checker's module mode (v2.76.0) and the ECC
/// (v2.76.0) consume. Deterministic by construction.
#[derive(Debug, Default)]
pub struct ModuleRegistry {
    modules: BTreeMap<ModulePath, CognitiveInterface>,
}

impl ModuleRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, path: ModulePath, interface: CognitiveInterface) {
        self.modules.insert(path, interface);
    }

    pub fn interface(&self, path: &ModulePath) -> Option<&CognitiveInterface> {
        self.modules.get(path)
    }

    /// The exported kind of `name` in module `path`, if any.
    pub fn export_kind(&self, path: &ModulePath, name: &str) -> Option<&str> {
        self.modules
            .get(path)
            .and_then(|i| i.exports.get(name))
            .map(|sig| sig.kind())
    }

    pub fn len(&self) -> usize {
        self.modules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&ModulePath, &CognitiveInterface)> {
        self.modules.iter()
    }
}

// ════════════════════════════════════════════════════════════════════
//  Unit tests (integration suite: tests/interfaces.rs)
// ════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn parse(source: &str) -> Program {
        let tokens = Lexer::new(source, "test.axon").tokenize().expect("lex");
        Parser::new(tokens).parse().expect("parse")
    }

    fn mp(dotted: &str) -> ModulePath {
        ModulePath(dotted.split('.').map(str::to_string).collect())
    }

    const SECURITY: &str = r#"
persona Expert {
  domain: ["medicine", "diagnostics"]
  tone: precise
  confidence_threshold: 0.9
}

anchor NoHallucination {
  require: source_citation
  confidence_floor: 0.75
  on_violation: raise AnchorBreachError
}
"#;

    #[test]
    fn floor_anchor_means_know() {
        assert_eq!(compute_floor(&parse(SECURITY)), EpistemicFloor::Know);
    }

    #[test]
    fn floor_shield_means_believe() {
        let p = parse("shield S { scan: [pii_leak] on_breach: halt }\n");
        assert_eq!(compute_floor(&p), EpistemicFloor::Believe);
    }

    #[test]
    fn floor_unspecified_without_evidence() {
        let p = parse("persona P { domain: [\"x\"] }\n");
        assert_eq!(compute_floor(&p), EpistemicFloor::Unspecified);
    }

    #[test]
    fn interface_hides_description_but_carries_signature() {
        let p = parse(SECURITY);
        let i = generate_interface(&mp("axon.security"), &p, SECURITY);
        let json = i.to_axi_json();
        assert!(json.contains("\"diagnostics\""));
        assert!(json.contains("constraint_hash"));
        assert!(!json.contains("source_citation"), "anchor text must be hidden");
        assert_eq!(i.exports.len(), 2);
    }

    #[test]
    fn interface_hash_stable_under_comment_edit() {
        let with_comment = format!("// a comment\n{SECURITY}");
        let p1 = parse(SECURITY);
        let p2 = parse(&with_comment);
        let i1 = generate_interface(&mp("m"), &p1, SECURITY);
        let i2 = generate_interface(&mp("m"), &p2, &with_comment);
        assert_ne!(i1.content_hash, i2.content_hash);
        assert_eq!(i1.interface_hash, i2.interface_hash, "early-cutoff precondition");
    }

    #[test]
    fn interface_hash_changes_when_surface_changes() {
        let changed = SECURITY.replace("0.9", "0.8");
        let i1 = generate_interface(&mp("m"), &parse(SECURITY), SECURITY);
        let i2 = generate_interface(&mp("m"), &parse(&changed), &changed);
        assert_ne!(i1.interface_hash, i2.interface_hash);
    }

    #[test]
    fn roundtrip_axi_json() {
        let i = generate_interface(&mp("m"), &parse(SECURITY), SECURITY);
        let back = CognitiveInterface::from_axi_json(&i.to_axi_json()).unwrap();
        assert_eq!(i, back);
    }

    #[test]
    fn hash_determinism() {
        let a = generate_interface(&mp("m"), &parse(SECURITY), SECURITY);
        let b = generate_interface(&mp("m"), &parse(SECURITY), SECURITY);
        assert_eq!(a.interface_hash, b.interface_hash);
        assert_eq!(a.content_hash, b.content_hash);
    }
}
