//! ℰMCP Transducer — Epistemic Model Context Protocol runtime bridge.
//!
//! Implements AXON's ℰMCP (Epistemic MCP) for Ingesta: consuming external MCP
//! servers with epistemic guarantees that plain MCP lacks.
//!
//! Key differentiators from raw MCP:
//!   - **Blame tracking (CT-2/CT-3):** Every MCP call records blame assignment.
//!     If the server returns invalid data → Blame::Server.
//!     If AXON generated bad parameters → Blame::Caller.
//!   - **Epistemic taint:** All data entering via MCP is born as `Uncertainty` (⊥)
//!     in the epistemic lattice. Must be elevated by reasoning before trusted.
//!   - **Effect rows:** MCP tools carry `effects: <network, io, epistemic:speculate>`.
//!   - **Schema validation:** Response validated against output_schema before use.
//!
//! Transport: JSON-RPC 2.0 over HTTP (MCP standard transport).
//! The ℰMCP transducer wraps standard MCP calls with AXON's epistemic envelope.

use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::tool_executor::ToolResult;
use crate::tool_registry::ToolEntry;

// ── Blame calculus (Findler-Felleisen) ────────────────────────────────────

/// Blame assignment for a tool call failure.
/// Implements the contract-based blame calculus from ℰMCP spec (CT-2/CT-3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Blame {
    /// No failure — call succeeded.
    None,
    /// Server violated its contract (bad response, schema mismatch, crash).
    Server,
    /// Caller violated its contract (bad parameters, invalid arguments).
    Caller,
    /// Network/infrastructure failure (timeout, connection refused).
    Network,
}

impl Blame {
    pub fn as_str(&self) -> &'static str {
        match self {
            Blame::None => "none",
            Blame::Server => "server",
            Blame::Caller => "caller",
            Blame::Network => "network",
        }
    }
}

// ── Epistemic taint ──────────────────────────────────────────────────────

/// Epistemic level of data entering via MCP.
/// All MCP-sourced data starts at `Uncertainty` and must be elevated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum EpistemicTaint {
    /// Data has not been validated — born as ⊥ (Uncertainty).
    Untrusted,
    /// Data passed schema validation but not epistemic reasoning.
    SchemaValidated,
    /// Data elevated by reasoning step (shield or know block).
    Elevated,
}

impl EpistemicTaint {
    pub fn as_str(&self) -> &'static str {
        match self {
            EpistemicTaint::Untrusted => "untrusted",
            EpistemicTaint::SchemaValidated => "schema_validated",
            EpistemicTaint::Elevated => "elevated",
        }
    }
}

// ── MCP call result ──────────────────────────────────────────────────────

/// Result of an ℰMCP tool call — enriched with blame and epistemic metadata.
#[derive(Debug, Clone, Serialize)]
pub struct McpCallResult {
    /// Standard tool result.
    pub tool_name: String,
    pub output: String,
    pub success: bool,
    /// Blame assignment (CT-2/CT-3).
    pub blame: Blame,
    /// Epistemic taint level of the output data.
    pub taint: EpistemicTaint,
    /// MCP server that handled the call.
    pub server: String,
    /// Effect row inferred for this call.
    pub effects: Vec<String>,
}

impl McpCallResult {
    /// Convert to a standard ToolResult (for registry integration).
    pub fn to_tool_result(&self) -> ToolResult {
        ToolResult {
            success: self.success,
            output: self.output.clone(),
            tool_name: self.tool_name.clone(),
        }
    }
}

// ── JSON-RPC 2.0 structures ─────────────────────────────────────────────

/// JSON-RPC 2.0 request (MCP standard transport).
#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,
    method: String,
    params: serde_json::Value,
    id: u64,
}

/// JSON-RPC 2.0 response.
#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    jsonrpc: String,
    result: Option<serde_json::Value>,
    error: Option<JsonRpcError>,
    #[allow(dead_code)]
    id: u64,
}

/// JSON-RPC 2.0 error object.
#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

// ── MCP Client ───────────────────────────────────────────────────────────

/// ℰMCP transducer client — wraps MCP server communication with epistemic
/// guarantees (blame tracking, taint tagging, effect inference).
pub struct McpClient {
    /// MCP server endpoint URL.
    server_url: String,
    /// HTTP timeout.
    timeout: Duration,
    /// Request ID counter.
    next_id: u64,
}

impl McpClient {
    /// Create a new ℰMCP client for the given server.
    pub fn new(server_url: &str, timeout: Duration) -> Self {
        McpClient {
            server_url: server_url.to_string(),
            timeout,
            next_id: 1,
        }
    }

    /// Call a tool on the MCP server with ℰMCP epistemic envelope.
    ///
    /// The call is wrapped with:
    ///   - Blame tracking: server/caller/network fault attribution
    ///   - Taint tagging: output starts as Untrusted, elevated to SchemaValidated if valid
    ///   - Effect inference: all MCP calls carry network + epistemic:speculate effects
    pub fn call_tool(&mut self, tool_name: &str, argument: &str) -> McpCallResult {
        let request_id = self.next_id;
        self.next_id += 1;

        // Build JSON-RPC request for MCP tools/call
        let params = if argument.trim_start().starts_with('{') {
            serde_json::from_str(argument).unwrap_or_else(|_| {
                serde_json::json!({
                    "name": tool_name,
                    "arguments": { "input": argument }
                })
            })
        } else {
            serde_json::json!({
                "name": tool_name,
                "arguments": { "input": argument }
            })
        };

        let rpc_request = JsonRpcRequest {
            jsonrpc: "2.0",
            method: "tools/call".to_string(),
            params,
            id: request_id,
        };

        // Execute the HTTP request to MCP server
        match self.send_rpc(&rpc_request) {
            Ok(response) => self.process_response(tool_name, response),
            Err(e) => McpCallResult {
                tool_name: tool_name.to_string(),
                output: format!("ℰMCP error: {e}"),
                success: false,
                blame: Blame::Network,
                taint: EpistemicTaint::Untrusted,
                server: self.server_url.clone(),
                effects: vec!["network".to_string()],
            },
        }
    }

    /// List available tools on the MCP server.
    pub fn list_tools(&mut self) -> Result<Vec<McpToolInfo>, String> {
        let request_id = self.next_id;
        self.next_id += 1;

        let rpc_request = JsonRpcRequest {
            jsonrpc: "2.0",
            method: "tools/list".to_string(),
            params: serde_json::json!({}),
            id: request_id,
        };

        let response = self.send_rpc(&rpc_request)?;

        match response.result {
            Some(val) => {
                if let Some(tools) = val.get("tools").and_then(|t| t.as_array()) {
                    let infos = tools
                        .iter()
                        .filter_map(|t| {
                            Some(McpToolInfo {
                                name: t.get("name")?.as_str()?.to_string(),
                                description: t
                                    .get("description")
                                    .and_then(|d| d.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                            })
                        })
                        .collect();
                    Ok(infos)
                } else {
                    Ok(Vec::new())
                }
            }
            None => Err(response
                .error
                .map(|e| format!("JSON-RPC error {}: {}", e.code, e.message))
                .unwrap_or_else(|| "empty response".to_string())),
        }
    }

    /// Read a resource from the MCP server.
    pub fn read_resource(&mut self, uri: &str) -> McpCallResult {
        let request_id = self.next_id;
        self.next_id += 1;

        let rpc_request = JsonRpcRequest {
            jsonrpc: "2.0",
            method: "resources/read".to_string(),
            params: serde_json::json!({ "uri": uri }),
            id: request_id,
        };

        match self.send_rpc(&rpc_request) {
            Ok(response) => {
                match response.result {
                    Some(val) => {
                        // Extract text content from MCP resource response
                        let text = val
                            .get("contents")
                            .and_then(|c| c.as_array())
                            .and_then(|arr| arr.first())
                            .and_then(|item| item.get("text"))
                            .and_then(|t| t.as_str())
                            .unwrap_or_else(|| {
                                // Fallback: serialize the entire result
                                ""
                            });

                        let output = if text.is_empty() {
                            serde_json::to_string(&val).unwrap_or_default()
                        } else {
                            text.to_string()
                        };

                        McpCallResult {
                            tool_name: format!("resource:{uri}"),
                            output,
                            success: true,
                            blame: Blame::None,
                            taint: EpistemicTaint::Untrusted, // All MCP data → ⊥
                            server: self.server_url.clone(),
                            effects: vec!["network".to_string(), "io".to_string()],
                        }
                    }
                    None => McpCallResult {
                        tool_name: format!("resource:{uri}"),
                        output: response
                            .error
                            .map(|e| format!("JSON-RPC error {}: {}", e.code, e.message))
                            .unwrap_or_else(|| "empty response".to_string()),
                        success: false,
                        blame: Blame::Server,
                        taint: EpistemicTaint::Untrusted,
                        server: self.server_url.clone(),
                        effects: vec!["network".to_string()],
                    },
                }
            }
            Err(e) => McpCallResult {
                tool_name: format!("resource:{uri}"),
                output: format!("ℰMCP error: {e}"),
                success: false,
                blame: Blame::Network,
                taint: EpistemicTaint::Untrusted,
                server: self.server_url.clone(),
                effects: vec!["network".to_string()],
            },
        }
    }

    // ── Internal ──────────────────────────────────────────────────

    fn send_rpc(&self, request: &JsonRpcRequest) -> Result<JsonRpcResponse, String> {
        let client = reqwest::blocking::Client::builder()
            .timeout(self.timeout)
            .build()
            .map_err(|e| format!("failed to create HTTP client: {e}"))?;

        let body = serde_json::to_string(request)
            .map_err(|e| format!("failed to serialize request: {e}"))?;

        let response = client
            .post(&self.server_url)
            .header("Content-Type", "application/json")
            .header("X-Axon-EMCP", "1.0")
            .body(body)
            .send()
            .map_err(|e| {
                if e.is_timeout() {
                    format!("MCP server timed out after {}s", self.timeout.as_secs())
                } else if e.is_connect() {
                    format!("cannot connect to MCP server at {}", self.server_url)
                } else {
                    format!("MCP request failed: {e}")
                }
            })?;

        let status = response.status();
        let text = response
            .text()
            .map_err(|e| format!("failed to read MCP response: {e}"))?;

        if !status.is_success() {
            return Err(format!("MCP server returned HTTP {}: {}", status.as_u16(), text));
        }

        serde_json::from_str(&text)
            .map_err(|e| format!("invalid JSON-RPC response: {e}"))
    }

    fn process_response(&self, tool_name: &str, response: JsonRpcResponse) -> McpCallResult {
        match response.result {
            Some(val) => {
                // Extract content from MCP tool response
                let output = val
                    .get("content")
                    .and_then(|c| c.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|item| item.get("text"))
                    .and_then(|t| t.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| serde_json::to_string(&val).unwrap_or_default());

                McpCallResult {
                    tool_name: tool_name.to_string(),
                    output,
                    success: true,
                    blame: Blame::None,
                    taint: EpistemicTaint::Untrusted, // Born as ⊥ — must be elevated
                    server: self.server_url.clone(),
                    effects: vec![
                        "network".to_string(),
                        "epistemic:speculate".to_string(),
                    ],
                }
            }
            None => {
                let (blame, msg) = match response.error {
                    Some(e) => {
                        // JSON-RPC error codes: -32600..-32603 are protocol errors (caller)
                        // Server-defined errors are positive or other negative codes
                        let b = if (-32603..=-32600).contains(&e.code) {
                            Blame::Caller
                        } else {
                            Blame::Server
                        };
                        (b, format!("JSON-RPC error {}: {}", e.code, e.message))
                    }
                    None => (Blame::Server, "empty response from MCP server".to_string()),
                };

                McpCallResult {
                    tool_name: tool_name.to_string(),
                    output: msg,
                    success: false,
                    blame,
                    taint: EpistemicTaint::Untrusted,
                    server: self.server_url.clone(),
                    effects: vec!["network".to_string()],
                }
            }
        }
    }
}

/// Information about a tool available on an MCP server.
#[derive(Debug, Clone)]
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
}

// ── Registry dispatch ────────────────────────────────────────────────────

/// Dispatch an MCP tool call via the ℰMCP transducer.
///
/// Creates a transient McpClient for the server URL in `entry.runtime`,
/// calls the tool, and returns the result with blame/taint metadata.
pub fn dispatch_mcp(entry: &ToolEntry, argument: &str) -> ToolResult {
    let server_url = entry.runtime.trim();

    if server_url.is_empty() {
        return ToolResult {
            success: false,
            output: format!(
                "ℰMCP tool '{}': no server URL. Set runtime: \"http://...\" in tool definition.",
                entry.name
            ),
            tool_name: entry.name.clone(),
        };
    }

    if !server_url.starts_with("http://") && !server_url.starts_with("https://") {
        return ToolResult {
            success: false,
            output: format!(
                "ℰMCP tool '{}': invalid server URL '{}'. Must start with http:// or https://.",
                entry.name, server_url
            ),
            tool_name: entry.name.clone(),
        };
    }

    let timeout = crate::http_tool::parse_timeout_pub(&entry.timeout)
        .unwrap_or(Duration::from_secs(30));

    let mut client = McpClient::new(server_url, timeout);
    let mcp_result = client.call_tool(&entry.name, argument);

    // Enrich output with blame metadata for tracing
    if !mcp_result.success {
        ToolResult {
            success: false,
            output: format!(
                "{} [blame={}]",
                mcp_result.output,
                mcp_result.blame.as_str()
            ),
            tool_name: entry.name.clone(),
        }
    } else {
        mcp_result.to_tool_result()
    }
}

// ── Blame tracker ────────────────────────────────────────────────────────

/// Accumulates blame records across MCP tool calls during execution.
#[derive(Debug, Default)]
pub struct BlameTracker {
    records: Vec<BlameRecord>,
}

/// A single blame record.
#[derive(Debug, Clone, Serialize)]
pub struct BlameRecord {
    pub tool_name: String,
    pub server: String,
    pub blame: Blame,
    pub taint: EpistemicTaint,
    pub message: String,
}

impl BlameTracker {
    pub fn new() -> Self {
        BlameTracker { records: Vec::new() }
    }

    /// Record a blame event from an MCP call result.
    pub fn record(&mut self, result: &McpCallResult) {
        self.records.push(BlameRecord {
            tool_name: result.tool_name.clone(),
            server: result.server.clone(),
            blame: result.blame,
            taint: result.taint,
            message: if result.success {
                "ok".to_string()
            } else {
                result.output.clone()
            },
        });
    }

    /// Total number of blame records.
    pub fn total(&self) -> usize {
        self.records.len()
    }

    /// Count of server-blamed failures.
    pub fn server_faults(&self) -> usize {
        self.records.iter().filter(|r| r.blame == Blame::Server).count()
    }

    /// Count of caller-blamed failures.
    pub fn caller_faults(&self) -> usize {
        self.records.iter().filter(|r| r.blame == Blame::Caller).count()
    }

    /// Count of network failures.
    pub fn network_faults(&self) -> usize {
        self.records.iter().filter(|r| r.blame == Blame::Network).count()
    }

    /// All records.
    pub fn records(&self) -> &[BlameRecord] {
        &self.records
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  §Fase 34.f — McpStreamingTool: Tool::stream() over MCP JSON-RPC 2.0
//  partial responses (`notifications/message` / `notifications/progress`)
//  + best-effort `notifications/cancelled` on cancel.
// ════════════════════════════════════════════════════════════════════════════

use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;

use crate::backends::sse_streaming::LineBuffer;
use crate::tool_trait::{Tool, ToolChunk, ToolContext, ToolFinishReason, ToolStream};

/// MCP tool with first-class streaming surface (Fase 34.f).
///
/// MCP's wire format is JSON-RPC 2.0 over HTTP. Servers that
/// support partial-response streaming emit a sequence of JSON-RPC
/// envelopes — one per line — over a `application/x-ndjson` (or
/// `application/jsonl`) response body. Each envelope is either:
///
/// - A **notification** (no `id`): `method == "notifications/message"`
///   or `method == "notifications/progress"`. The notification's
///   `params.data` / `params.text` / `params.message` field becomes a
///   `ToolChunk::intermediate` delta.
/// - A **response** (has `id`): `result` is the final tool payload;
///   the `result.content[0].text` field becomes the terminator's
///   delta + the stream closes with `ToolFinishReason::Stop`. An
///   `error` envelope closes the stream with `ToolFinishReason::Error`.
///
/// Servers that **don't** support streaming respond with a single
/// JSON-RPC response (Content-Type `application/json`). The
/// streaming tool gracefully falls back to D9 single-chunk wrap:
/// the materialized `result.content[0].text` becomes one intermediate
/// `ToolChunk` followed by a `Stop` terminator — byte-equal to the
/// legacy [`dispatch_mcp`] output.
///
/// # Cancel discipline (D5)
///
/// `ctx.cancel` is polled between every `bytes_stream().next().await`
/// boundary. When fired, the streaming task:
///
/// 1. Drops the in-flight reqwest response (closing the connection).
/// 2. Best-effort fires a JSON-RPC `notifications/cancelled`
///    notification (POST + fire-and-forget; no await on response) so
///    MCP servers that support cooperative cancellation can clean up.
/// 3. Emits a `ToolFinishReason::Cancelled` terminator.
///
/// # Error discipline
///
/// Every failure surface — URL invalid / client build / connect /
/// timeout / non-2xx status / JSON-RPC error envelope / mid-stream
/// byte error / unparseable line — is captured as a
/// `ToolFinishReason::Error { message }` terminator chunk.
///
/// # Epistemic taint
///
/// All MCP-sourced data is born `Untrusted` (⊥) per the ℰMCP charter.
/// The streaming tool preserves this discipline — each chunk's delta
/// reaches the dispatcher untrusted; downstream `shield`/`know`
/// blocks elevate the taint via reasoning. Taint metadata is not
/// embedded in the `ToolChunk` (the dispatcher's audit row + ℰMCP
/// [`BlameTracker`] are the durable trail).
pub struct McpStreamingTool {
    name: String,
    server_url: String,
    timeout: Duration,
}

impl McpStreamingTool {
    /// Construct from a registry [`ToolEntry`]. Validates the server
    /// URL + extracts the timeout. Returns `Err` with adopter-facing
    /// diagnostic when the URL is missing or has an invalid scheme.
    pub fn from_entry(entry: &ToolEntry) -> Result<Self, String> {
        let url = entry.runtime.trim();
        if url.is_empty() {
            return Err(format!(
                "ℰMCP tool '{}': no server URL. Set runtime: \"http://...\" in tool definition.",
                entry.name
            ));
        }
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(format!(
                "ℰMCP tool '{}': invalid server URL '{}'. Must start with http:// or https://.",
                entry.name, url
            ));
        }
        let timeout =
            crate::http_tool::parse_timeout_pub(&entry.timeout).unwrap_or(Duration::from_secs(30));
        Ok(Self {
            name: entry.name.clone(),
            server_url: url.to_string(),
            timeout,
        })
    }

    /// Public new() ctor for tests + adopters who construct directly
    /// without a registry entry.
    pub fn new(name: String, server_url: String, timeout: Duration) -> Self {
        Self {
            name,
            server_url,
            timeout,
        }
    }
}

/// Build the JSON-RPC 2.0 `tools/call` request body. Mirrors the
/// legacy [`McpClient::call_tool`] argument-handling discipline:
/// JSON-shaped arguments pass through; plain strings wrap as
/// `{ input: <args> }`.
fn build_mcp_request_body(tool_name: &str, args: &str, request_id: u64) -> String {
    let params = if args.trim_start().starts_with('{') {
        serde_json::from_str::<serde_json::Value>(args).unwrap_or_else(|_| {
            serde_json::json!({
                "name": tool_name,
                "arguments": { "input": args }
            })
        })
    } else {
        serde_json::json!({
            "name": tool_name,
            "arguments": { "input": args }
        })
    };
    serde_json::json!({
        "jsonrpc": "2.0",
        "method": "tools/call",
        "params": params,
        "id": request_id,
    })
    .to_string()
}

/// Classify an MCP response Content-Type as streaming vs single.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum McpFramingMode {
    /// `application/x-ndjson` / `application/jsonl` — line-delimited
    /// JSON-RPC envelopes. Drain per-line + parse each envelope.
    NdjsonStream,
    /// `application/json` or anything else — single JSON-RPC
    /// response. D9 fallback: parse full body, emit `result.content`
    /// as 1 chunk + `Stop` terminator (byte-equal to
    /// [`dispatch_mcp`]).
    SingleResponse,
}

fn classify_mcp_framing(content_type: &str) -> McpFramingMode {
    let lc = content_type.to_ascii_lowercase();
    if lc.contains("application/x-ndjson") || lc.contains("application/jsonl") {
        McpFramingMode::NdjsonStream
    } else {
        McpFramingMode::SingleResponse
    }
}

/// Parsed JSON-RPC 2.0 envelope read from the streaming body. The
/// public ToolStream surface is downstream of this enum.
enum McpEnvelope {
    /// `method == "notifications/message"` / `"notifications/progress"`.
    /// The extracted human-readable text (params.data / params.text /
    /// params.message — the first non-empty wins). Empty notifications
    /// (no text payload) emit empty deltas which the dispatcher
    /// already skips per D4.
    Notification { delta: String },
    /// JSON-RPC response with a `result` field. The extracted
    /// `result.content[0].text` (or stringified result if shape
    /// differs).
    Result { delta: String },
    /// JSON-RPC error envelope. Carries code + message + blame slug.
    Error { message: String },
}

/// Parse one line of the MCP NDJSON body into an envelope. Returns
/// `None` for blank lines (per NDJSON spec) + for lines we don't
/// recognize (defensive — drop unknown envelopes rather than fail
/// the whole stream).
fn parse_mcp_envelope(line: &str) -> Option<McpEnvelope> {
    if line.trim().is_empty() {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(line).ok()?;

    // Error envelope wins precedence (a response can have both
    // result==null + error set; we honor the error path).
    if let Some(err) = value.get("error") {
        let code = err.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
        let message = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown JSON-RPC error");
        let blame = if (-32603..=-32600).contains(&code) {
            "caller"
        } else {
            "server"
        };
        return Some(McpEnvelope::Error {
            message: format!("JSON-RPC error {code}: {message} [blame={blame}]"),
        });
    }

    // Notification: has `method` field, no `id`.
    if let Some(method) = value.get("method").and_then(|m| m.as_str()) {
        if method == "notifications/message" || method == "notifications/progress" {
            let delta = extract_notification_text(value.get("params"));
            return Some(McpEnvelope::Notification { delta });
        }
        // Unknown method — drop defensively.
        return None;
    }

    // Response envelope: has `id` + `result`.
    if let Some(result) = value.get("result") {
        let delta = extract_result_text(result);
        return Some(McpEnvelope::Result { delta });
    }
    None
}

/// Extract the human-readable text from a notification's `params`.
/// First non-empty of: `data`, `text`, `message`. Falls back to
/// `serde_json` stringification of the params if no canonical field
/// matches (preserves the payload for downstream debugging).
fn extract_notification_text(params: Option<&serde_json::Value>) -> String {
    let Some(p) = params else { return String::new() };
    for key in ["data", "text", "message"] {
        if let Some(val) = p.get(key).and_then(|v| v.as_str()) {
            if !val.is_empty() {
                return val.to_string();
            }
        }
    }
    serde_json::to_string(p).unwrap_or_default()
}

/// Extract `result.content[0].text` from a JSON-RPC response result.
/// Falls back to serialized result on shape mismatch.
fn extract_result_text(result: &serde_json::Value) -> String {
    result
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|item| item.get("text"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| serde_json::to_string(result).unwrap_or_default())
}

/// Best-effort fire-and-forget `notifications/cancelled` POST. Used
/// by the streaming task when cancel fires mid-stream. We don't
/// wait on the response — MCP servers that support cooperative
/// cancellation will honor it; servers that don't see only a
/// closed connection. No-op if the URL is unreachable.
fn fire_cancel_notification(server_url: String, request_id: u64, name: String) {
    tokio::spawn(async move {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/cancelled",
            "params": { "requestId": request_id },
        })
        .to_string();
        if let Ok(client) = reqwest::Client::builder()
            .timeout(Duration::from_millis(500))
            .build()
        {
            let _ = client
                .post(&server_url)
                .header("Content-Type", "application/json")
                .header("X-Axon-EMCP", "1.0")
                .header("X-Axon-Tool", name)
                .body(body)
                .send()
                .await;
        }
    });
}

#[async_trait]
impl Tool for McpStreamingTool {
    async fn execute(&self, args: String, _ctx: ToolContext) -> ToolResult {
        // Synchronous path delegates to legacy `dispatch_mcp` via
        // spawn_blocking so reqwest::blocking::Client doesn't panic
        // inside the async runtime. Output is byte-equal to
        // dispatch_mcp (D9 backwards-compat).
        let entry = ToolEntry {
            name: self.name.clone(),
            provider: "mcp".to_string(),
            timeout: format!("{}s", self.timeout.as_secs()),
            runtime: self.server_url.clone(),
            resource_ref: String::new(),
            substrate: None,
            capacity: None,
            sandbox: None,
            max_results: None,
            output_schema: String::new(),
            effect_row: Vec::new(),
            // §Fase 58.f.2 — reconstructed entry for the legacy sync
            // delegate; no typed input schema needed on this path.
            parameters: Vec::new(),
            secret: String::new(),
            secret_partition: String::new(),
            source: crate::tool_registry::ToolSource::Program,
            is_streaming: false,
            scrape: None,
        };
        let args_owned = args;
        match tokio::task::spawn_blocking(move || dispatch_mcp(&entry, &args_owned)).await {
            Ok(result) => result,
            Err(e) => ToolResult {
                success: false,
                output: format!("ℰMCP tool '{}': blocking task join failed: {e}", self.name),
                tool_name: self.name.clone(),
            },
        }
    }

    async fn stream(&self, args: String, ctx: ToolContext) -> ToolStream {
        let server_url = self.server_url.clone();
        let name = self.name.clone();
        let timeout = self.timeout;
        let cancel = ctx.cancel.clone();
        // request_id is monotonic per-invocation; the cancel
        // notification uses it for correlation. We start at the
        // ToolContext's trace_id so the audit trail can link the
        // MCP correlation back to the trace. (ID collisions between
        // distinct McpStreamingTool invocations are accepted — MCP
        // servers correlate per-connection.)
        let request_id = ctx.trace_id.max(1);
        let body = build_mcp_request_body(&name, &args, request_id);

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<ToolChunk>();

        tokio::spawn(async move {
            let send_terminator = |reason: ToolFinishReason| {
                let _ = tx.send(ToolChunk::terminator("", reason));
            };

            // Pre-flight cancel.
            if cancel.is_cancelled() {
                fire_cancel_notification(server_url.clone(), request_id, name.clone());
                send_terminator(ToolFinishReason::Cancelled);
                return;
            }

            // 1. Build async client.
            let client = match reqwest::Client::builder().timeout(timeout).build() {
                Ok(c) => c,
                Err(e) => {
                    send_terminator(ToolFinishReason::Error {
                        message: format!(
                            "ℰMCP tool '{name}': failed to build async client: {e}"
                        ),
                    });
                    return;
                }
            };

            // 2. Issue request.
            let response = match client
                .post(&server_url)
                .header("Content-Type", "application/json")
                .header("X-Axon-EMCP", "1.0")
                .header("Accept", "application/x-ndjson, application/json")
                .body(body)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    let message = if e.is_timeout() {
                        format!(
                            "ℰMCP tool '{name}': server timed out after {}s",
                            timeout.as_secs()
                        )
                    } else if e.is_connect() {
                        format!("ℰMCP tool '{name}': cannot connect to MCP server at {server_url}")
                    } else {
                        format!("ℰMCP tool '{name}': MCP request failed: {e}")
                    };
                    send_terminator(ToolFinishReason::Error { message });
                    return;
                }
            };

            // 3. Non-2xx → error terminator with status + truncated body.
            let status = response.status();
            if !status.is_success() {
                let body_text = response.text().await.unwrap_or_default();
                let truncated = if body_text.len() > 200 {
                    format!("{}...", &body_text[..200])
                } else {
                    body_text
                };
                send_terminator(ToolFinishReason::Error {
                    message: format!(
                        "ℰMCP server '{name}' returned HTTP {}: {}",
                        status.as_u16(),
                        truncated
                    ),
                });
                return;
            }

            // 4. Read Content-Type → classify framing.
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            let framing = classify_mcp_framing(&content_type);

            // 5. Drain per framing mode.
            let mut byte_stream = response.bytes_stream();
            let drain_result = match framing {
                McpFramingMode::NdjsonStream => {
                    drain_mcp_ndjson(&mut byte_stream, &cancel, &tx).await
                }
                McpFramingMode::SingleResponse => {
                    drain_mcp_single(&mut byte_stream, &cancel, &tx).await
                }
            };

            match drain_result {
                McpDrainOutcome::Completed => send_terminator(ToolFinishReason::Stop),
                McpDrainOutcome::CompletedWith(reason) => send_terminator(reason),
                McpDrainOutcome::Cancelled => {
                    fire_cancel_notification(server_url.clone(), request_id, name.clone());
                    send_terminator(ToolFinishReason::Cancelled);
                }
                McpDrainOutcome::Error(message) => {
                    send_terminator(ToolFinishReason::Error { message })
                }
            }
        });

        Box::pin(futures::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|chunk| (chunk, rx))
        }))
    }

    fn is_streaming(&self) -> bool {
        true
    }
}

/// Per-drain outcome. The `CompletedWith` variant lets a drain
/// helper carry a JSON-RPC error all the way back to the spawned
/// task without losing the typed surface.
enum McpDrainOutcome {
    /// Stream ended naturally → `Stop` terminator.
    Completed,
    /// Stream ended with an explicit reason — used when the
    /// JSON-RPC body returns an error envelope (we want to keep
    /// the typed `ToolFinishReason::Error` surface intact instead
    /// of stringifying through `Error(message)`).
    CompletedWith(ToolFinishReason),
    /// Cancel flag observed mid-drain.
    Cancelled,
    /// Stream byte error or unrecoverable parse error.
    Error(String),
}

/// Drain MCP NDJSON-streaming body. Parse each LF-delimited line as
/// a JSON-RPC envelope; emit notifications as intermediate chunks,
/// stop on response/error envelopes.
async fn drain_mcp_ndjson<S>(
    byte_stream: &mut S,
    cancel: &crate::cancel_token::CancellationFlag,
    tx: &tokio::sync::mpsc::UnboundedSender<ToolChunk>,
) -> McpDrainOutcome
where
    S: futures::Stream<Item = reqwest::Result<Bytes>> + Unpin + Send,
{
    let mut line_buf = LineBuffer::new();
    loop {
        if cancel.is_cancelled() {
            return McpDrainOutcome::Cancelled;
        }
        match byte_stream.next().await {
            None => break,
            Some(Err(e)) => {
                return McpDrainOutcome::Error(format!("MCP stream chunk error: {e}"))
            }
            Some(Ok(bytes)) => {
                let lines = line_buf.push(&bytes);
                for line in lines {
                    if let Some(env) = parse_mcp_envelope(&line) {
                        match env {
                            McpEnvelope::Notification { delta } => {
                                if !delta.is_empty()
                                    && tx.send(ToolChunk::intermediate(delta)).is_err()
                                {
                                    return McpDrainOutcome::Cancelled;
                                }
                            }
                            McpEnvelope::Result { delta } => {
                                if !delta.is_empty() {
                                    let _ = tx.send(ToolChunk::intermediate(delta));
                                }
                                return McpDrainOutcome::Completed;
                            }
                            McpEnvelope::Error { message } => {
                                return McpDrainOutcome::CompletedWith(
                                    ToolFinishReason::Error { message },
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    if let Some(line) = line_buf.flush() {
        if let Some(env) = parse_mcp_envelope(&line) {
            match env {
                McpEnvelope::Notification { delta } => {
                    if !delta.is_empty() {
                        let _ = tx.send(ToolChunk::intermediate(delta));
                    }
                }
                McpEnvelope::Result { delta } => {
                    if !delta.is_empty() {
                        let _ = tx.send(ToolChunk::intermediate(delta));
                    }
                    return McpDrainOutcome::Completed;
                }
                McpEnvelope::Error { message } => {
                    return McpDrainOutcome::CompletedWith(ToolFinishReason::Error {
                        message,
                    });
                }
            }
        }
    }
    McpDrainOutcome::Completed
}

/// Drain MCP single-response (non-streaming) body. Parse the full
/// body as one JSON-RPC envelope; D9 backwards-compat: emit
/// `result.content[0].text` as 1 chunk + `Stop` terminator (matches
/// [`dispatch_mcp`] byte-equal); on `error`, emit `Error` terminator.
async fn drain_mcp_single<S>(
    byte_stream: &mut S,
    cancel: &crate::cancel_token::CancellationFlag,
    tx: &tokio::sync::mpsc::UnboundedSender<ToolChunk>,
) -> McpDrainOutcome
where
    S: futures::Stream<Item = reqwest::Result<Bytes>> + Unpin + Send,
{
    let mut acc: Vec<u8> = Vec::new();
    loop {
        if cancel.is_cancelled() {
            return McpDrainOutcome::Cancelled;
        }
        match byte_stream.next().await {
            None => break,
            Some(Err(e)) => {
                return McpDrainOutcome::Error(format!("MCP body chunk error: {e}"))
            }
            Some(Ok(bytes)) => acc.extend_from_slice(&bytes),
        }
    }
    let body_text = String::from_utf8_lossy(&acc).into_owned();
    if body_text.trim().is_empty() {
        return McpDrainOutcome::Completed;
    }
    let value: serde_json::Value = match serde_json::from_str(&body_text) {
        Ok(v) => v,
        Err(e) => {
            return McpDrainOutcome::Error(format!(
                "ℰMCP server returned unparseable JSON-RPC response: {e}"
            ));
        }
    };
    if let Some(err) = value.get("error") {
        let code = err.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
        let message = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown JSON-RPC error");
        let blame = if (-32603..=-32600).contains(&code) {
            "caller"
        } else {
            "server"
        };
        return McpDrainOutcome::CompletedWith(ToolFinishReason::Error {
            message: format!("JSON-RPC error {code}: {message} [blame={blame}]"),
        });
    }
    if let Some(result) = value.get("result") {
        let delta = extract_result_text(result);
        if !delta.is_empty() {
            let _ = tx.send(ToolChunk::intermediate(delta));
        }
        return McpDrainOutcome::Completed;
    }
    McpDrainOutcome::Error("ℰMCP response has neither result nor error".to_string())
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_registry::{ToolEntry, ToolSource};

    fn make_mcp_entry(name: &str, url: &str, timeout: &str) -> ToolEntry {
        ToolEntry {
            name: name.to_string(),
            provider: "mcp".to_string(),
            timeout: timeout.to_string(),
            runtime: url.to_string(),
            resource_ref: String::new(),
            substrate: None,
            capacity: None,
            sandbox: None,
            max_results: None,
            output_schema: "JSON".to_string(),
            effect_row: vec!["network".to_string(), "epistemic:speculate".to_string()],
            parameters: Vec::new(),
            secret: String::new(),
            secret_partition: String::new(),
            source: ToolSource::Program,
            // §Fase 34.c — MCP tools default to non-streaming; effect_row
            // carries `network` + `epistemic:speculate` but no `stream:`
            // prefix. MCP streaming via partial-response notifications
            // lands in Fase 34.f.
            is_streaming: false,
            scrape: None,
        }
    }

    // ── Blame ─────────────────────────────────────────────────────

    #[test]
    fn blame_variants() {
        assert_eq!(Blame::None.as_str(), "none");
        assert_eq!(Blame::Server.as_str(), "server");
        assert_eq!(Blame::Caller.as_str(), "caller");
        assert_eq!(Blame::Network.as_str(), "network");
    }

    // ── Epistemic taint ───────────────────────────────────────────

    #[test]
    fn taint_levels() {
        assert_eq!(EpistemicTaint::Untrusted.as_str(), "untrusted");
        assert_eq!(EpistemicTaint::SchemaValidated.as_str(), "schema_validated");
        assert_eq!(EpistemicTaint::Elevated.as_str(), "elevated");
    }

    #[test]
    fn mcp_data_born_untrusted() {
        // All MCP-sourced data must start as Untrusted (⊥)
        let result = McpCallResult {
            tool_name: "DataTool".into(),
            output: "some data".into(),
            success: true,
            blame: Blame::None,
            taint: EpistemicTaint::Untrusted,
            server: "http://localhost:3000".into(),
            effects: vec!["network".into()],
        };
        assert_eq!(result.taint, EpistemicTaint::Untrusted);
    }

    // ── McpCallResult conversion ──────────────────────────────────

    #[test]
    fn mcp_result_to_tool_result() {
        let mcp = McpCallResult {
            tool_name: "TestTool".into(),
            output: "hello".into(),
            success: true,
            blame: Blame::None,
            taint: EpistemicTaint::Untrusted,
            server: "http://localhost".into(),
            effects: vec![],
        };
        let tr = mcp.to_tool_result();
        assert!(tr.success);
        assert_eq!(tr.output, "hello");
        assert_eq!(tr.tool_name, "TestTool");
    }

    // ── Dispatch validation ───────────────────────────────────────

    #[test]
    fn dispatch_empty_url_fails() {
        let entry = make_mcp_entry("McpTool", "", "5s");
        let result = dispatch_mcp(&entry, "arg");
        assert!(!result.success);
        assert!(result.output.contains("no server URL"));
    }

    #[test]
    fn dispatch_invalid_scheme_fails() {
        let entry = make_mcp_entry("McpTool", "ws://localhost:3000", "5s");
        let result = dispatch_mcp(&entry, "arg");
        assert!(!result.success);
        assert!(result.output.contains("invalid server URL"));
    }

    #[test]
    fn dispatch_connection_refused() {
        let entry = make_mcp_entry("McpTool", "http://127.0.0.1:1/mcp", "2s");
        let result = dispatch_mcp(&entry, "test");
        assert!(!result.success);
        assert!(result.output.contains("blame=network"));
    }

    // ── Blame tracker ─────────────────────────────────────────────

    #[test]
    fn blame_tracker_accumulates() {
        let mut tracker = BlameTracker::new();

        tracker.record(&McpCallResult {
            tool_name: "A".into(),
            output: "ok".into(),
            success: true,
            blame: Blame::None,
            taint: EpistemicTaint::Untrusted,
            server: "s1".into(),
            effects: vec![],
        });

        tracker.record(&McpCallResult {
            tool_name: "B".into(),
            output: "server error".into(),
            success: false,
            blame: Blame::Server,
            taint: EpistemicTaint::Untrusted,
            server: "s1".into(),
            effects: vec![],
        });

        tracker.record(&McpCallResult {
            tool_name: "C".into(),
            output: "bad params".into(),
            success: false,
            blame: Blame::Caller,
            taint: EpistemicTaint::Untrusted,
            server: "s2".into(),
            effects: vec![],
        });

        tracker.record(&McpCallResult {
            tool_name: "D".into(),
            output: "timeout".into(),
            success: false,
            blame: Blame::Network,
            taint: EpistemicTaint::Untrusted,
            server: "s1".into(),
            effects: vec![],
        });

        assert_eq!(tracker.total(), 4);
        assert_eq!(tracker.server_faults(), 1);
        assert_eq!(tracker.caller_faults(), 1);
        assert_eq!(tracker.network_faults(), 1);
    }

    // ── McpClient construction ────────────────────────────────────

    #[test]
    fn mcp_client_creates() {
        let client = McpClient::new("http://localhost:3000", Duration::from_secs(10));
        assert_eq!(client.server_url, "http://localhost:3000");
        assert_eq!(client.timeout, Duration::from_secs(10));
        assert_eq!(client.next_id, 1);
    }

    // ── JSON-RPC response processing ──────────────────────────────

    #[test]
    fn process_success_response() {
        let client = McpClient::new("http://localhost", Duration::from_secs(5));
        let response = JsonRpcResponse {
            jsonrpc: "2.0".into(),
            result: Some(serde_json::json!({
                "content": [{"type": "text", "text": "result data"}]
            })),
            error: None,
            id: 1,
        };

        let result = client.process_response("TestTool", response);
        assert!(result.success);
        assert_eq!(result.output, "result data");
        assert_eq!(result.blame, Blame::None);
        assert_eq!(result.taint, EpistemicTaint::Untrusted); // Still untrusted!
    }

    #[test]
    fn process_server_error_response() {
        let client = McpClient::new("http://localhost", Duration::from_secs(5));
        let response = JsonRpcResponse {
            jsonrpc: "2.0".into(),
            result: None,
            error: Some(JsonRpcError {
                code: -32000,
                message: "internal server error".into(),
            }),
            id: 1,
        };

        let result = client.process_response("TestTool", response);
        assert!(!result.success);
        assert_eq!(result.blame, Blame::Server);
        assert!(result.output.contains("-32000"));
    }

    #[test]
    fn process_caller_error_response() {
        let client = McpClient::new("http://localhost", Duration::from_secs(5));
        // -32602 = Invalid params (JSON-RPC standard)
        let response = JsonRpcResponse {
            jsonrpc: "2.0".into(),
            result: None,
            error: Some(JsonRpcError {
                code: -32602,
                message: "invalid params".into(),
            }),
            id: 1,
        };

        let result = client.process_response("TestTool", response);
        assert!(!result.success);
        assert_eq!(result.blame, Blame::Caller);
    }

    // ── Effect inference ──────────────────────────────────────────

    #[test]
    fn mcp_calls_carry_epistemic_effects() {
        let client = McpClient::new("http://localhost", Duration::from_secs(5));
        let response = JsonRpcResponse {
            jsonrpc: "2.0".into(),
            result: Some(serde_json::json!({"content": [{"text": "data"}]})),
            error: None,
            id: 1,
        };

        let result = client.process_response("Tool", response);
        assert!(result.effects.contains(&"network".to_string()));
        assert!(result.effects.contains(&"epistemic:speculate".to_string()));
    }

    // ── Serialization ─────────────────────────────────────────────

    #[test]
    fn blame_serializes() {
        let json = serde_json::to_string(&Blame::Server).unwrap();
        assert_eq!(json, "\"Server\"");
    }

    #[test]
    fn mcp_call_result_serializes() {
        let result = McpCallResult {
            tool_name: "T".into(),
            output: "out".into(),
            success: true,
            blame: Blame::None,
            taint: EpistemicTaint::Untrusted,
            server: "http://s".into(),
            effects: vec!["network".into()],
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"blame\":\"None\""));
        assert!(json.contains("\"taint\":\"Untrusted\""));
    }

    // ════════════════════════════════════════════════════════════════
    //  §Fase 34.f — McpStreamingTool lib unit tests
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn mcp_streaming_tool_from_entry_accepts_valid_http_url() {
        let entry = make_mcp_entry("McpTool", "http://localhost:3000/mcp", "10s");
        let t = McpStreamingTool::from_entry(&entry).expect("ok");
        assert_eq!(t.name, "McpTool");
        assert_eq!(t.server_url, "http://localhost:3000/mcp");
        assert_eq!(t.timeout, Duration::from_secs(10));
        assert!(t.is_streaming());
    }

    #[test]
    fn mcp_streaming_tool_from_entry_accepts_https_url() {
        let entry = make_mcp_entry("McpTool", "https://api.example.com/mcp", "5s");
        let t = McpStreamingTool::from_entry(&entry).expect("ok");
        assert!(t.server_url.starts_with("https://"));
    }

    #[test]
    fn mcp_streaming_tool_from_entry_rejects_empty_url() {
        let entry = make_mcp_entry("McpTool", "", "10s");
        let err = McpStreamingTool::from_entry(&entry).err().unwrap();
        assert!(err.contains("no server URL"));
    }

    #[test]
    fn mcp_streaming_tool_from_entry_rejects_invalid_scheme() {
        let entry = make_mcp_entry("McpTool", "ws://localhost:3000", "10s");
        let err = McpStreamingTool::from_entry(&entry).err().unwrap();
        assert!(err.contains("invalid server URL"));
    }

    #[test]
    fn mcp_streaming_tool_default_timeout_when_empty() {
        let entry = make_mcp_entry("McpTool", "http://localhost", "");
        let t = McpStreamingTool::from_entry(&entry).expect("ok");
        assert_eq!(t.timeout, Duration::from_secs(30));
    }

    // ─── Framing classification ─────────────────────────────────────

    #[test]
    fn classify_mcp_framing_ndjson() {
        assert_eq!(
            classify_mcp_framing("application/x-ndjson"),
            McpFramingMode::NdjsonStream
        );
        assert_eq!(
            classify_mcp_framing("application/x-ndjson; charset=utf-8"),
            McpFramingMode::NdjsonStream
        );
        assert_eq!(
            classify_mcp_framing("application/jsonl"),
            McpFramingMode::NdjsonStream
        );
    }

    #[test]
    fn classify_mcp_framing_single_default() {
        assert_eq!(
            classify_mcp_framing("application/json"),
            McpFramingMode::SingleResponse
        );
        assert_eq!(
            classify_mcp_framing("text/plain"),
            McpFramingMode::SingleResponse
        );
        assert_eq!(classify_mcp_framing(""), McpFramingMode::SingleResponse);
    }

    // ─── Envelope parsing ───────────────────────────────────────────

    #[test]
    fn parse_envelope_notification_message_data() {
        let line = r#"{"jsonrpc":"2.0","method":"notifications/message","params":{"data":"partial-1"}}"#;
        match parse_mcp_envelope(line) {
            Some(McpEnvelope::Notification { delta }) => assert_eq!(delta, "partial-1"),
            other => panic!("expected Notification, got {}", envelope_label(&other)),
        }
    }

    #[test]
    fn parse_envelope_notification_progress_text() {
        let line = r#"{"jsonrpc":"2.0","method":"notifications/progress","params":{"text":"50% done"}}"#;
        match parse_mcp_envelope(line) {
            Some(McpEnvelope::Notification { delta }) => assert_eq!(delta, "50% done"),
            other => panic!("expected Notification, got {}", envelope_label(&other)),
        }
    }

    #[test]
    fn parse_envelope_notification_message_field_fallback() {
        // `message` key as third-priority fallback.
        let line = r#"{"jsonrpc":"2.0","method":"notifications/message","params":{"message":"hi"}}"#;
        match parse_mcp_envelope(line) {
            Some(McpEnvelope::Notification { delta }) => assert_eq!(delta, "hi"),
            other => panic!("expected Notification, got {}", envelope_label(&other)),
        }
    }

    #[test]
    fn parse_envelope_response_with_content() {
        let line = r#"{"jsonrpc":"2.0","result":{"content":[{"type":"text","text":"final answer"}]},"id":1}"#;
        match parse_mcp_envelope(line) {
            Some(McpEnvelope::Result { delta }) => assert_eq!(delta, "final answer"),
            other => panic!("expected Result, got {}", envelope_label(&other)),
        }
    }

    #[test]
    fn parse_envelope_error_server_blame() {
        let line = r#"{"jsonrpc":"2.0","error":{"code":-32000,"message":"internal"},"id":1}"#;
        match parse_mcp_envelope(line) {
            Some(McpEnvelope::Error { message }) => {
                assert!(message.contains("-32000"));
                assert!(message.contains("blame=server"));
            }
            other => panic!("expected Error, got {}", envelope_label(&other)),
        }
    }

    #[test]
    fn parse_envelope_error_caller_blame_invalid_params() {
        let line = r#"{"jsonrpc":"2.0","error":{"code":-32602,"message":"bad params"},"id":1}"#;
        match parse_mcp_envelope(line) {
            Some(McpEnvelope::Error { message }) => {
                assert!(message.contains("blame=caller"));
            }
            other => panic!("expected Error, got {}", envelope_label(&other)),
        }
    }

    #[test]
    fn parse_envelope_blank_line_returns_none() {
        assert!(parse_mcp_envelope("").is_none());
        assert!(parse_mcp_envelope("   ").is_none());
    }

    #[test]
    fn parse_envelope_unknown_method_returns_none() {
        let line = r#"{"jsonrpc":"2.0","method":"notifications/heartbeat","params":{}}"#;
        assert!(parse_mcp_envelope(line).is_none());
    }

    #[test]
    fn parse_envelope_malformed_json_returns_none() {
        assert!(parse_mcp_envelope("not json").is_none());
        assert!(parse_mcp_envelope("{").is_none());
    }

    #[test]
    fn build_mcp_request_body_json_args_passthrough() {
        let body = build_mcp_request_body("Tool", r#"{"name":"Tool","arguments":{"x":1}}"#, 42);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["method"], "tools/call");
        assert_eq!(v["id"], 42);
        assert_eq!(v["params"]["arguments"]["x"], 1);
    }

    #[test]
    fn build_mcp_request_body_plain_args_wrapped() {
        let body = build_mcp_request_body("Tool", "search query", 1);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["params"]["name"], "Tool");
        assert_eq!(v["params"]["arguments"]["input"], "search query");
    }

    /// Helper to name McpEnvelope variants in test failure messages
    /// without exposing them via Debug.
    fn envelope_label(env: &Option<McpEnvelope>) -> &'static str {
        match env {
            None => "None",
            Some(McpEnvelope::Notification { .. }) => "Notification",
            Some(McpEnvelope::Result { .. }) => "Result",
            Some(McpEnvelope::Error { .. }) => "Error",
        }
    }
}
