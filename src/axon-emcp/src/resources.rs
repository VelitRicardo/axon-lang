//! MCP resources the connected agent can read by URI.
//!
//! Where **tools** are *actions* (the agent picks one, supplies
//! arguments, gets a result), **resources** are *citation-ready
//! references* — the agent reads them when it wants to quote a fact in
//! its reply. The two layers serve the same knowledge base; the
//! difference is whether the agent's prompt includes the URL (resource)
//! or whether the agent constructs a call (tool).
//!
//! URI scheme: `axon://`. The catalogue serves four families:
//!
//! - `axon://primitives/<name>` — the markdown body for a primitive
//!   (Phase 0).
//! - `axon://grammar/<slug>` — language-level rules (top-level vs.
//!   nested table, composition rules, EBNF) (Phase 3).
//! - `axon://logic/<slug>` — when-to-use-what reasoning (flow
//!   composition, session duality) (Phase 3).
//! - `axon://compliance/<framework>` — per-framework annotation maps
//!   (HIPAA, GDPR, PCI_DSS, SOC2, SOX, GxP, FISMA,
//!   NIST_800_53) (Phase 3).

use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::knowledge::{Catalog, ReferenceKind};
use crate::server::JsonRpcError;
use crate::telemetry::Telemetry;

/// Build the `resources/list` payload. Walks every catalogue entry —
/// primitives + reference docs — so the agent can discover the full
/// surface area without first calling any tool.
pub fn list(catalog: &Arc<Catalog>) -> Vec<Value> {
    let mut out = Vec::with_capacity(catalog.primitive_count() + catalog.reference_count());
    for p in catalog.primitives() {
        out.push(json!({
            "uri": format!("axon://primitives/{}", p.name),
            "name": format!("AXON primitive: {}", p.name),
            "description": p.summary,
            "mimeType": "text/markdown",
        }));
    }
    // Reference docs are listed AFTER primitives so the agent sees
    // the per-primitive entries first (that is the most-asked-about
    // surface), and the wider grammar/logic/compliance maps next.
    for r in catalog.references() {
        let label = match r.kind {
            ReferenceKind::Grammar => "AXON grammar",
            ReferenceKind::Logic => "AXON logic",
            ReferenceKind::Compliance => "AXON compliance",
        };
        out.push(json!({
            "uri": format!("axon://{}/{}", r.kind.as_str(), r.slug),
            "name": format!("{label}: {}", r.title),
            "description": r.summary,
            "mimeType": "text/markdown",
        }));
    }
    out
}

/// Dispatch a `resources/read` request. Params shape (per MCP spec):
/// `{ "uri": "..." }`.
///
/// v1.3.0 — every read is recorded as a `resource_read` event keyed
/// by URI family (`axon://primitives/`, `axon://grammar/`, …) NOT the
/// full slug. The bounded cardinality is intentional: we count
/// usage shape, not which-document-was-read.
pub fn dispatch_read(
    params: Value,
    catalog: &Arc<Catalog>,
    telemetry: &Arc<Telemetry>,
) -> Result<Value, JsonRpcError> {
    let req: ReadParams = serde_json::from_value(params)
        .map_err(|e| JsonRpcError::invalid_params(format!("resources/read params: {e}")))?;
    let parsed = parse_axon_uri(&req.uri)?;
    let family = match &parsed {
        AxonUri::Primitive(_) => "axon://primitives/",
        AxonUri::Reference(kind, _) => match kind {
            ReferenceKind::Grammar => "axon://grammar/",
            ReferenceKind::Logic => "axon://logic/",
            ReferenceKind::Compliance => "axon://compliance/",
        },
    };
    telemetry.record_resource_read(family);
    match parsed {
        AxonUri::Primitive(name) => read_primitive(name, &req.uri, catalog),
        AxonUri::Reference(kind, slug) => read_reference(kind, slug, &req.uri, catalog),
    }
}

#[derive(Debug, Deserialize)]
struct ReadParams {
    uri: String,
}

#[derive(Debug)]
enum AxonUri<'a> {
    /// `axon://primitives/<name>` — Phase 0.
    Primitive(&'a str),
    /// `axon://(grammar|logic|compliance)/<slug>` — Phase 3.
    Reference(ReferenceKind, &'a str),
}

fn parse_axon_uri(uri: &str) -> Result<AxonUri<'_>, JsonRpcError> {
    let rest = uri.strip_prefix("axon://").ok_or_else(|| JsonRpcError {
        code: -32602,
        message: format!(
            "unsupported scheme in `{uri}` — this server serves `axon://` URIs only"
        ),
        data: None,
    })?;
    let (kind, tail) = rest.split_once('/').ok_or_else(|| JsonRpcError {
        code: -32602,
        message: format!("malformed axon:// URI `{uri}` — expected `axon://<kind>/<path>`"),
        data: None,
    })?;
    match kind {
        "primitives" => Ok(AxonUri::Primitive(tail)),
        "grammar" => Ok(AxonUri::Reference(ReferenceKind::Grammar, tail)),
        "logic" => Ok(AxonUri::Reference(ReferenceKind::Logic, tail)),
        "compliance" => Ok(AxonUri::Reference(ReferenceKind::Compliance, tail)),
        other => Err(JsonRpcError {
            code: -32601,
            message: format!(
                "unknown axon:// resource kind `{other}` — supported kinds: \
                 `primitives`, `grammar`, `logic`, `compliance`"
            ),
            data: None,
        }),
    }
}

fn read_primitive(name: &str, uri: &str, catalog: &Arc<Catalog>) -> Result<Value, JsonRpcError> {
    let prim = catalog.primitive(name).ok_or_else(|| JsonRpcError {
        code: -32602,
        message: format!(
            "unknown primitive `{name}` in `{uri}` — call axon.primitives to list available names"
        ),
        data: None,
    })?;
    // The `contents` array carries one entry per "fragment" of the
    // resource. We surface a single markdown blob: the frontmatter
    // header (machine-friendly summary) plus the prose body.
    let header = format!(
        "<!-- axon-emcp metadata -->\n\
         - **name**: `{}`\n- **summary**: {}\n- **category**: {}\n\
         - **top-level**: {}\n- **since**: {}\n\n",
        prim.name,
        prim.summary,
        prim.category.as_str(),
        prim.top_level,
        prim.since,
    );
    let text = format!("{header}{}", prim.body);
    Ok(json!({
        "contents": [
            {
                "uri": uri,
                "mimeType": "text/markdown",
                "text": text,
            }
        ]
    }))
}

/// Read a reference document by `(kind, slug)`. Mirrors `read_primitive`
/// — same `contents` shape, same machine-friendly metadata header,
/// same content-type. The agent's reader does not need a different
/// codepath per resource family.
fn read_reference(
    kind: ReferenceKind,
    slug: &str,
    uri: &str,
    catalog: &Arc<Catalog>,
) -> Result<Value, JsonRpcError> {
    let refr = catalog.reference(kind, slug).ok_or_else(|| JsonRpcError {
        code: -32602,
        message: format!(
            "unknown {kind_str} reference `{slug}` in `{uri}` — \
             call resources/list to see every available URI",
            kind_str = kind.as_str(),
        ),
        data: None,
    })?;
    let header = format!(
        "<!-- axon-emcp metadata -->\n\
         - **kind**: `{}`\n- **slug**: `{}`\n- **title**: {}\n\
         - **summary**: {}\n\n",
        refr.kind.as_str(),
        refr.slug,
        refr.title,
        refr.summary,
    );
    let text = format!("{header}{}", refr.body);
    Ok(json!({
        "contents": [
            {
                "uri": uri,
                "mimeType": "text/markdown",
                "text": text,
            }
        ]
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::Category;
    use std::io::Write;
    use std::sync::Arc;

    /// Throwaway telemetry registry for resources tests — JSONL sink
    /// disabled, deployment ID empty. Cheap per test.
    fn tel() -> Arc<Telemetry> {
        Arc::new(Telemetry::new(crate::telemetry::TelemetryConfig {
            jsonl_sink: None,
            deployment_id: "".into(),
            max_samples: 1000,
        }))
    }

    fn catalog_with(name: &str) -> Arc<Catalog> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "axon-emcp-restest-{}-{n}-{name}",
            std::process::id(),
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let prims = dir.join("primitives");
        std::fs::create_dir_all(&prims).unwrap();
        let mut f = std::fs::File::create(prims.join(format!("{name}.md"))).unwrap();
        write!(
            f,
            "---\nname: {name}\nsummary: s\ncategory: session_types\n\
             top_level: true\nsince: cycle X\n---\n\nBody.\n"
        )
        .unwrap();
        Arc::new(Catalog::load_from(&dir).unwrap())
    }

    #[test]
    fn list_emits_one_resource_per_primitive() {
        let cat = catalog_with("socket");
        let list = list(&cat);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0]["uri"], "axon://primitives/socket");
        assert_eq!(list[0]["mimeType"], "text/markdown");
    }

    #[test]
    fn dispatch_read_returns_markdown_with_metadata_header() {
        let cat = catalog_with("socket");
        let v = dispatch_read(json!({ "uri": "axon://primitives/socket" }), &cat, &tel()).unwrap();
        let text = v["contents"][0]["text"].as_str().unwrap();
        assert!(text.contains("axon-emcp metadata"));
        assert!(text.contains("**name**: `socket`"));
        assert!(text.contains("**top-level**: true"));
        assert!(text.contains("Body."));
    }

    #[test]
    fn dispatch_read_rejects_unsupported_scheme() {
        let cat = catalog_with("socket");
        let err = dispatch_read(json!({ "uri": "https://example.com/x" }), &cat, &tel())
            .expect_err("must reject");
        assert!(err.message.contains("unsupported scheme"));
    }

    #[test]
    fn dispatch_read_rejects_unknown_resource_kind() {
        let cat = catalog_with("socket");
        let err = dispatch_read(json!({ "uri": "axon://does_not_exist/x" }), &cat, &tel())
            .expect_err("must reject");
        assert!(err.message.contains("unknown axon:// resource kind"));
    }

    #[test]
    fn dispatch_read_rejects_unknown_primitive() {
        let cat = catalog_with("socket");
        let err = dispatch_read(json!({ "uri": "axon://primitives/nope" }), &cat, &tel())
            .expect_err("must reject");
        assert!(err.message.contains("unknown primitive"));
    }

    // Sanity that the Category enum import resolves (avoids unused-import
    // warnings on this module while the resources surface is small).
    #[test]
    fn category_str_smoke() {
        assert_eq!(Category::SessionTypes.as_str(), "session_types");
    }

    // ── Phase 3 — reference docs (grammar / logic / compliance) ────────

    /// Build a corpus root with one primitive + one reference doc per
    /// kind. Returns the catalog wrapped in an `Arc` for the dispatch
    /// surface tests.
    fn catalog_with_references() -> Arc<Catalog> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "axon-emcp-restest-refs-{}-{n}",
            std::process::id(),
        ));
        let _ = std::fs::remove_dir_all(&dir);

        // primitives/socket.md
        let prims = dir.join("primitives");
        std::fs::create_dir_all(&prims).unwrap();
        let mut f = std::fs::File::create(prims.join("socket.md")).unwrap();
        write!(
            f,
            "---\nname: socket\nsummary: s\ncategory: session_types\n\
             top_level: true\nsince: v2.3.0\n---\n\nBody.\n"
        )
        .unwrap();

        // grammar/top_level.md
        let gram = dir.join("grammar");
        std::fs::create_dir_all(&gram).unwrap();
        let mut f = std::fs::File::create(gram.join("top_level.md")).unwrap();
        write!(
            f,
            "---\nname: top_level\nsummary: gram-summary\n\
             title: Top-level table\n---\n\nGrammar body.\n"
        )
        .unwrap();

        // logic/flow_composition.md
        let logic = dir.join("logic");
        std::fs::create_dir_all(&logic).unwrap();
        let mut f = std::fs::File::create(logic.join("flow_composition.md")).unwrap();
        write!(
            f,
            "---\nname: flow_composition\nsummary: when-to-nest\n\
             title: Flow composition\n---\n\nLogic body.\n"
        )
        .unwrap();

        // compliance/hipaa.md
        let comp = dir.join("compliance");
        std::fs::create_dir_all(&comp).unwrap();
        let mut f = std::fs::File::create(comp.join("hipaa.md")).unwrap();
        write!(
            f,
            "---\nname: hipaa\nsummary: HIPAA map\n\
             title: HIPAA reference\n---\n\nCompliance body.\n"
        )
        .unwrap();

        Arc::new(Catalog::load_from(&dir).unwrap())
    }

    #[test]
    fn list_emits_primitives_and_each_reference_kind() {
        let cat = catalog_with_references();
        let list = list(&cat);
        // 1 primitive + 3 references = 4 entries.
        assert_eq!(list.len(), 4, "list payload count drift: {list:#?}");
        let uris: Vec<&str> = list
            .iter()
            .map(|v| v["uri"].as_str().unwrap())
            .collect();
        assert!(uris.contains(&"axon://primitives/socket"));
        assert!(uris.contains(&"axon://grammar/top_level"));
        assert!(uris.contains(&"axon://logic/flow_composition"));
        assert!(uris.contains(&"axon://compliance/hipaa"));
    }

    #[test]
    fn list_orders_primitives_before_references() {
        let cat = catalog_with_references();
        let list = list(&cat);
        // The very first entry must be a primitive — every reference
        // entry should appear after every primitive entry, so the
        // agent's first paged scan sees the most-asked-about surface.
        let first_primitive_idx = list
            .iter()
            .position(|v| v["uri"].as_str().unwrap().starts_with("axon://primitives/"))
            .unwrap();
        let first_reference_idx = list
            .iter()
            .position(|v| !v["uri"].as_str().unwrap().starts_with("axon://primitives/"))
            .unwrap();
        assert!(
            first_primitive_idx < first_reference_idx,
            "primitives must appear before references in the resources/list payload"
        );
    }

    #[test]
    fn read_grammar_returns_body_with_metadata_header() {
        let cat = catalog_with_references();
        let v = dispatch_read(
            json!({ "uri": "axon://grammar/top_level" }),
            &cat, &tel(),
        )
        .unwrap();
        let text = v["contents"][0]["text"].as_str().unwrap();
        assert!(text.contains("**kind**: `grammar`"));
        assert!(text.contains("**slug**: `top_level`"));
        assert!(text.contains("**title**: Top-level table"));
        assert!(text.contains("Grammar body."));
        assert_eq!(v["contents"][0]["mimeType"], "text/markdown");
    }

    #[test]
    fn read_logic_returns_body() {
        let cat = catalog_with_references();
        let v = dispatch_read(
            json!({ "uri": "axon://logic/flow_composition" }),
            &cat, &tel(),
        )
        .unwrap();
        let text = v["contents"][0]["text"].as_str().unwrap();
        assert!(text.contains("**kind**: `logic`"));
        assert!(text.contains("Logic body."));
    }

    #[test]
    fn read_compliance_returns_body() {
        let cat = catalog_with_references();
        let v = dispatch_read(
            json!({ "uri": "axon://compliance/hipaa" }),
            &cat, &tel(),
        )
        .unwrap();
        let text = v["contents"][0]["text"].as_str().unwrap();
        assert!(text.contains("**kind**: `compliance`"));
        assert!(text.contains("**slug**: `hipaa`"));
        assert!(text.contains("Compliance body."));
    }

    #[test]
    fn read_rejects_unknown_grammar_slug() {
        let cat = catalog_with_references();
        let err = dispatch_read(
            json!({ "uri": "axon://grammar/does_not_exist" }),
            &cat, &tel(),
        )
        .expect_err("unknown slugs must surface a structured error");
        assert!(err.message.contains("unknown grammar reference"));
    }

    #[test]
    fn read_rejects_unknown_compliance_slug() {
        let cat = catalog_with_references();
        let err = dispatch_read(
            json!({ "uri": "axon://compliance/does_not_exist" }),
            &cat, &tel(),
        )
        .expect_err("unknown slugs must surface a structured error");
        assert!(err.message.contains("unknown compliance reference"));
    }

    #[test]
    fn reference_loader_rejects_frontmatter_name_mismatch() {
        // Phase 3 invariant — the frontmatter `name:` MUST match
        // the file stem. A file `pci_dss.md` declaring `name: hipaa`
        // is an invitation to shadow the real hipaa entry; reject it.
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "axon-emcp-restest-mismatch-{}-{n}",
            std::process::id(),
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let prims = dir.join("primitives");
        std::fs::create_dir_all(&prims).unwrap();
        let comp = dir.join("compliance");
        std::fs::create_dir_all(&comp).unwrap();
        let mut f = std::fs::File::create(comp.join("pci_dss.md")).unwrap();
        write!(
            f,
            "---\nname: hipaa\nsummary: x\ntitle: t\n---\n\nbody\n"
        )
        .unwrap();
        let err = Catalog::load_from(&dir).expect_err("mismatched name must fail");
        let msg = format!("{err}");
        assert!(
            msg.contains("does not match file stem"),
            "diagnostic must explain the mismatch: {msg}"
        );
    }
}
