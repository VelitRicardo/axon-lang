//! MCP protocol over stdio + JSON-RPC 2.0.
//!
//! The Model Context Protocol (MCP, Anthropic 2024) is a minimal
//! JSON-RPC 2.0 dialect:
//!
//! - Transport: line-delimited UTF-8 JSON frames on stdin/stdout (one
//!   frame per line; trailing newline mandatory).
//! - Handshake: client sends `initialize` (with its protocol-version);
//!   server replies with its own protocolVersion + capabilities; client
//!   sends `notifications/initialized`; ready.
//! - Request/response: `id` echoes; `result` xor `error`.
//! - Notifications: `id` absent → no reply.
//! - Methods served: `initialize`, `ping`,
//!   `tools/list`, `tools/call`,
//!   `resources/list`, `resources/read`,
//!   `prompts/list`, `prompts/get`.
//!
//! Anything beyond this subset returns `-32601 method not found`.
//! Everything is dispatched off a single match in [`dispatch`].

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::knowledge::Catalog;
use crate::telemetry::Telemetry;
use crate::{prompts, resources, tools};

/// The MCP protocol version this server speaks. Bumped only when the
/// server's externally-visible JSON-RPC shape changes incompatibly.
pub const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

/// Server-side product identifier (surfaced in the `initialize`
/// response so the agent can show it to the user / log it).
pub const SERVER_NAME: &str = "axon-emcp";

/// Run the stdio MCP loop. Returns `Ok(())` on clean EOF (the agent
/// closed the pipe); `Err` on a fatal transport failure.
///
/// `telemetry` is the per-instance v1.3.0 registry. Recording is
/// always-on (in-memory); the JSONL sink + deployment-ID propagation
/// activate when the corresponding env vars are set (see
/// [`crate::telemetry::TelemetryConfig::from_env`]).
pub async fn run_stdio(catalog: Catalog, telemetry: Arc<Telemetry>) -> std::io::Result<()> {
    // v1.4.0 — caller hands us the shared `Arc<Telemetry>` already
    // so the OTLP pusher (background task spawned in `main`) can
    // hold its own clone and read the same in-process state every
    // recorder writes to.
    let catalog = Arc::new(catalog);
    let mut stdin = BufReader::new(tokio::io::stdin());
    let mut stdout = tokio::io::stdout();
    let mut line = String::new();
    loop {
        line.clear();
        let n = stdin.read_line(&mut line).await?;
        if n == 0 {
            // EOF — agent closed stdin. Clean shutdown.
            return Ok(());
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        tracing::debug!(bytes = trimmed.len(), "← request");
        let response = handle_one(trimmed, &catalog, &telemetry).await;
        if let Some(resp_bytes) = response {
            stdout.write_all(&resp_bytes).await?;
            stdout.write_all(b"\n").await?;
            stdout.flush().await?;
            tracing::debug!(bytes = resp_bytes.len(), "→ response");
        }
        // Notifications (no `id`) produce no response — we silently
        // continue without writing to stdout.
    }
}

/// Parse one JSON-RPC frame, dispatch it, and serialise the reply.
///
/// Returns `Some(bytes)` for a request (a reply is owed) and `None` for
/// a notification (no reply by JSON-RPC spec).
async fn handle_one(
    line: &str,
    catalog: &Arc<Catalog>,
    telemetry: &Arc<Telemetry>,
) -> Option<Vec<u8>> {
    // First, try to parse as a generic `Request` so we can recover the
    // `id` for error responses even when the method/params are malformed.
    let parsed: Result<Request, serde_json::Error> = serde_json::from_str(line);
    let req = match parsed {
        Ok(r) => r,
        Err(e) => {
            // We can't recover the id — JSON-RPC section 5 says respond with a
            // null-id error in this case.
            return Some(error_response(
                Value::Null,
                JsonRpcError::parse_error(&e.to_string()),
            ));
        }
    };

    let is_notification = req.id.is_none();
    let id = req.id.clone().unwrap_or(Value::Null);

    let outcome = dispatch(&req, catalog, telemetry).await;

    if is_notification {
        // No reply per JSON-RPC section 4.1.
        return None;
    }
    Some(match outcome {
        Ok(result) => success_response(id, result),
        Err(err) => error_response(id, err),
    })
}

/// Method router. Every supported MCP method dispatches here; unknown
/// methods produce `-32601 method not found`.
async fn dispatch(
    req: &Request,
    catalog: &Arc<Catalog>,
    telemetry: &Arc<Telemetry>,
) -> Result<Value, JsonRpcError> {
    match req.method.as_str() {
        // ── Lifecycle ────────────────────────────────────────────────────
        "initialize" => Ok(initialize_response()),
        "notifications/initialized" => Ok(Value::Null), // ignored, no-op
        "ping" => Ok(Value::Object(serde_json::Map::new())),

        // ── Tools ────────────────────────────────────────────────────────
        "tools/list" => Ok(json!({ "tools": tools::list() })),
        "tools/call" => tools::dispatch_call(req.params.clone(), catalog, telemetry).await,

        // ── Resources ────────────────────────────────────────────────────
        "resources/list" => Ok(json!({ "resources": resources::list(catalog) })),
        "resources/read" => resources::dispatch_read(req.params.clone(), catalog, telemetry),

        // ── Prompts (Phase 5) ───────────────────────────────────────────
        "prompts/list" => Ok(json!({ "prompts": prompts::list(catalog) })),
        "prompts/get" => prompts::dispatch_get(req.params.clone(), catalog, telemetry),

        // ── Anything else: per JSON-RPC section 5.1, `-32601 method not found`.
        other => Err(JsonRpcError {
            code: -32601,
            message: format!("method not found: `{other}`"),
            data: None,
        }),
    }
}

/// The `initialize` response carries the server's protocol-version +
/// capabilities. We advertise tools, resources, AND prompts (Phase 5)
/// — the three host-facing MCP surfaces this server implements. None
/// of them push change-notifications; `listChanged: false` everywhere.
fn initialize_response() -> Value {
    json!({
        "protocolVersion": MCP_PROTOCOL_VERSION,
        "serverInfo": {
            "name": SERVER_NAME,
            "version": env!("CARGO_PKG_VERSION"),
        },
        "capabilities": {
            "tools":     { "listChanged": false },
            "resources": { "listChanged": false, "subscribe": false },
            "prompts":   { "listChanged": false },
            // Future phases may add: "logging": { … }, "sampling": { … }.
        },
        "instructions": include_str!("server_instructions.txt"),
    })
}

// ─── JSON-RPC 2.0 frame types ────────────────────────────────────────────

/// One JSON-RPC 2.0 request frame. `params` is captured as `Value` so
/// each tool can deserialise the shape it expects without forcing the
/// dispatcher to know every schema.
#[derive(Debug, Deserialize)]
struct Request {
    /// `"2.0"` — JSON-RPC version tag. Required by spec. We accept any
    /// value (some agents send `"2"` or omit it) to be permissive.
    #[serde(default)]
    #[allow(dead_code)]
    jsonrpc: String,
    /// The method name (e.g. `"tools/list"`).
    method: String,
    /// Method-specific parameters. `null` / missing is fine; the tool
    /// handler decides what shape it needs.
    #[serde(default)]
    params: Value,
    /// Request id. **Absent** ⇒ notification (no reply). Present ⇒ must
    /// be echoed in the reply.
    #[serde(default)]
    id: Option<Value>,
}

/// JSON-RPC 2.0 error object.
#[derive(Debug, Serialize, Clone)]
pub struct JsonRpcError {
    /// Numeric error code (-32700 to -32603 are reserved by JSON-RPC).
    pub code: i64,
    /// Human-readable message — surfaced to the agent verbatim.
    pub message: String,
    /// Optional machine-readable detail. We attach typed diagnostic data
    /// here for `axon.check` etc.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcError {
    /// `-32700` — invalid JSON received by the server (parse error).
    pub fn parse_error(detail: &str) -> Self {
        Self { code: -32700, message: format!("parse error: {detail}"), data: None }
    }
    /// `-32602` — the method exists but the params are invalid.
    pub fn invalid_params(detail: impl Into<String>) -> Self {
        Self { code: -32602, message: detail.into(), data: None }
    }
    /// `-32603` — internal server fault. Used sparingly; most errors
    /// have a more specific code.
    pub fn internal(detail: impl Into<String>) -> Self {
        Self { code: -32603, message: detail.into(), data: None }
    }
}

fn success_response(id: Value, result: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    }))
    .expect("serialising a JSON-RPC success frame cannot fail")
}

fn error_response(id: Value, err: JsonRpcError) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": err,
    }))
    .expect("serialising a JSON-RPC error frame cannot fail")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat() -> Arc<Catalog> {
        Arc::new(Catalog::empty_for_tests())
    }

    /// Throwaway telemetry registry for server-side wire tests —
    /// JSONL sink disabled, deployment ID empty, default 1k sample
    /// window. Cheap to construct per test.
    fn tel() -> Arc<Telemetry> {
        Arc::new(Telemetry::new(crate::telemetry::TelemetryConfig {
            jsonl_sink: None,
            deployment_id: "".into(),
            max_samples: 1000,
        }))
    }

    #[tokio::test]
    async fn initialize_carries_version_capabilities_and_instructions() {
        let req = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
        let resp = handle_one(req, &cat(), &tel()).await.expect("reply owed");
        let v: Value = serde_json::from_slice(&resp).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 1);
        assert_eq!(v["result"]["protocolVersion"], MCP_PROTOCOL_VERSION);
        assert_eq!(v["result"]["serverInfo"]["name"], SERVER_NAME);
        assert!(v["result"]["capabilities"]["tools"].is_object());
        assert!(v["result"]["capabilities"]["resources"].is_object());
        assert!(v["result"]["instructions"].as_str().unwrap().contains("AXON"));
    }

    #[tokio::test]
    async fn notification_produces_no_reply() {
        let req = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
        let resp = handle_one(req, &cat(), &tel()).await;
        assert!(resp.is_none(), "notifications must not yield a reply");
    }

    #[tokio::test]
    async fn unknown_method_returns_method_not_found() {
        let req = r#"{"jsonrpc":"2.0","id":7,"method":"axon.does_not_exist"}"#;
        let resp = handle_one(req, &cat(), &tel()).await.expect("reply owed");
        let v: Value = serde_json::from_slice(&resp).unwrap();
        assert_eq!(v["error"]["code"], -32601);
        assert!(v["error"]["message"].as_str().unwrap().contains("not found"));
    }

    #[tokio::test]
    async fn malformed_json_returns_parse_error_with_null_id() {
        let resp = handle_one("{ not valid json", &cat(), &tel()).await.expect("reply owed");
        let v: Value = serde_json::from_slice(&resp).unwrap();
        assert_eq!(v["error"]["code"], -32700);
        assert_eq!(v["id"], Value::Null);
    }

    #[tokio::test]
    async fn tools_list_returns_an_array() {
        let req = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#;
        let resp = handle_one(req, &cat(), &tel()).await.expect("reply owed");
        let v: Value = serde_json::from_slice(&resp).unwrap();
        let tools = v["result"]["tools"].as_array().expect("tools array");
        assert!(!tools.is_empty(), "we ship at least one tool on day 0");
        // Every tool advertises {name, description, inputSchema}.
        for t in tools {
            assert!(t["name"].is_string());
            assert!(t["description"].is_string());
            assert!(t["inputSchema"].is_object());
        }
    }

    #[tokio::test]
    async fn resources_list_returns_an_array() {
        let req = r#"{"jsonrpc":"2.0","id":3,"method":"resources/list"}"#;
        let resp = handle_one(req, &cat(), &tel()).await.expect("reply owed");
        let v: Value = serde_json::from_slice(&resp).unwrap();
        assert!(v["result"]["resources"].is_array());
    }

    // ── Phase 5: prompts/list + prompts/get ─────────────────────────────

    /// Build a catalog backed by the embedded corpus — `prompts/get`
    /// needs the real prompt bodies, not an empty stub.
    fn cat_embedded() -> Arc<Catalog> {
        Arc::new(Catalog::load_embedded().expect("embedded corpus must load"))
    }

    #[tokio::test]
    async fn initialize_advertises_prompts_capability() {
        let req = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
        let resp = handle_one(req, &cat(), &tel()).await.expect("reply owed");
        let v: Value = serde_json::from_slice(&resp).unwrap();
        // The prompts capability must be visible at handshake time —
        // hosts gate the `prompts/*` UI on this declaration.
        assert!(
            v["result"]["capabilities"]["prompts"].is_object(),
            "initialize must advertise the prompts capability"
        );
        assert_eq!(
            v["result"]["capabilities"]["prompts"]["listChanged"], false,
            "we do not push prompt-list-changed notifications"
        );
    }

    #[tokio::test]
    async fn prompts_list_returns_an_array_of_entries() {
        let req = r#"{"jsonrpc":"2.0","id":4,"method":"prompts/list"}"#;
        let resp = handle_one(req, &cat_embedded(), &tel()).await.expect("reply owed");
        let v: Value = serde_json::from_slice(&resp).unwrap();
        let prompts = v["result"]["prompts"].as_array().expect("prompts array");
        assert!(prompts.len() >= 3, "Phase 5 ships ≥ 3 prompts");
        // Every entry advertises {name, description, arguments}.
        for p in prompts {
            assert!(p["name"].is_string());
            assert!(p["description"].is_string());
            assert!(p["arguments"].is_array());
        }
    }

    #[tokio::test]
    async fn prompts_get_renders_known_prompt_with_arguments() {
        let req = serde_json::to_string(&json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "prompts/get",
            "params": {
                "name": "flow_design",
                "arguments": { "intent": "summarise a patient record" }
            }
        }))
        .unwrap();
        let resp = handle_one(&req, &cat_embedded(), &tel()).await.expect("reply owed");
        let v: Value = serde_json::from_slice(&resp).unwrap();
        // Per MCP spec — reply carries `{ description, messages: [...] }`.
        assert!(v["result"]["description"].is_string());
        let msgs = v["result"]["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
        let text = msgs[0]["content"]["text"].as_str().unwrap();
        assert!(text.contains("summarise a patient record"));
    }

    #[tokio::test]
    async fn prompts_get_unknown_name_surfaces_structured_error() {
        let req = serde_json::to_string(&json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "prompts/get",
            "params": { "name": "does_not_exist", "arguments": {} }
        }))
        .unwrap();
        let resp = handle_one(&req, &cat_embedded(), &tel()).await.expect("reply owed");
        let v: Value = serde_json::from_slice(&resp).unwrap();
        assert_eq!(v["error"]["code"], -32602);
        assert!(v["error"]["message"].as_str().unwrap().contains("unknown prompt"));
    }
}
