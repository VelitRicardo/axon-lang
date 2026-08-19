//! v2.77.0 — shared Meta Graph API transport for the Facebook Pages and
//! Instagram connectors (both speak the same Graph surface: Bearer auth, the
//! `{"error":{"message","code"}}` envelope, JSON pointer extraction).

use crate::connector::ConnectorError;

/// Execute a Graph request with `Authorization: Bearer <token>`, mapping
/// transport failures and the Graph error envelope into typed
/// [`ConnectorError`]s. The token is a header — never a query param or a log.
pub(crate) fn execute(
    req: reqwest::blocking::RequestBuilder,
    token: &str,
) -> Result<serde_json::Value, ConnectorError> {
    let resp = req
        .bearer_auth(token)
        .send()
        .map_err(|e| ConnectorError::Transport(e.to_string()))?;
    let status = resp.status().as_u16();
    let body: serde_json::Value = resp
        .json()
        .map_err(|e| ConnectorError::Transport(format!("non-JSON response: {e}")))?;
    if status >= 400 {
        let message = body
            .pointer("/error/message")
            .and_then(|v| v.as_str())
            .unwrap_or("(no Graph error message)")
            .to_string();
        return Err(ConnectorError::Platform { status, message });
    }
    Ok(body)
}

/// Extract a string at a JSON pointer, empty when absent (Graph omits fields the
/// token cannot see — an absent value is empty, never fabricated).
pub(crate) fn str_at(v: &serde_json::Value, pointer: &str) -> String {
    v.pointer(pointer)
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .to_string()
}

/// Build a Graph URL: `<base>/<version>/<path>` (trimming stray slashes).
pub(crate) fn url(base: &str, version: &str, path: &str) -> String {
    format!(
        "{}/{}/{}",
        base.trim_end_matches('/'),
        version,
        path.trim_start_matches('/')
    )
}
