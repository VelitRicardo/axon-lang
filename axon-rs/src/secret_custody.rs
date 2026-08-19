//! v2.48.0 — the `SecretCustody` port: the runtime seam behind the
//! `backend: secrets` metadata store (v2.48.0), the `rotate … with <Tool>`
//! flow verb (v2.48.0) and the `tool { secret: }` dispatch injection (v2.48.0).
//!
//! The doctrine (`axon://logic/rotation_without_revelation`): a secret's
//! whole lifecycle — seed, use, enumerate, rotate, expire — completes
//! without the value ever entering the COGNITION space (flow bindings,
//! epistemic envelopes, LLM context, stores, wire audit). The port's
//! surface encodes it:
//!
//! - [`SecretCustody::list_metadata`] returns METADATA ONLY — there is no
//!   method that lists values.
//! - [`SecretCustody::reveal_for_dispatch`] / [`reveal_for_rotation`]
//!   reveal a value to exactly ONE consumer — the tool-exchange channel —
//!   and the dispatch handlers never bind it.
//! - [`SecretCustody::commit_rotation`] is CAS on `expected_version`:
//!   two concurrent rotators (HA daemon replicas) cannot both commit —
//!   the loser degrades with a witness instead of double-spending a
//!   refresh credential the provider may have invalidated.
//!
//! There is deliberately **no default production custody in OSS**: a
//! `rotate` (or a secret-bearing tool dispatch) reached with no port
//! configured is a loud `MissingDependency` (the v2.41.0 no-silent-stub
//! lesson, the v2.46.0 posture). The enterprise executor injects its
//! envelope-encrypted Postgres custody (v2.48.0); tests and single-process
//! adopters use [`InMemoryCustody`] — which enforces the same CAS and
//! class semantics but does NOT encrypt at rest (process memory only;
//! encrypted custody is the enterprise layer, documented honestly).
//!
//! [`reveal_for_rotation`]: SecretCustody::reveal_for_rotation

use std::collections::HashMap;
use std::sync::Mutex;

use crate::store::filter::{Connector, Filter, Operator, Rhs, SqlValue, TimeSign, TimeUnit};

// ════════════════════════════════════════════════════════════════════
//  Wire types
// ════════════════════════════════════════════════════════════════════

/// The metadata view of one custody entry — EXACTLY the four synthesized
/// columns of a `backend: secrets` store (v2.48.0). No value field exists
/// on this type by design.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretMetadata {
    /// The full secret key (e.g. `crm.hubspot`).
    pub key: String,
    /// Monotonic write version (seed = 1; every rotation +1).
    pub version: i64,
    /// Creation instant, Unix ms.
    pub created_at_ms: i64,
    /// Declared expiry, Unix ms. `None` = no declared expiry — the entry
    /// never matches an `expires_at` comparison (SQL NULL semantics).
    pub expires_at_ms: Option<i64>,
}

/// A value revealed for exactly one mediated consumer (a tool exchange).
/// The dispatch handlers thread this INTO the tool request and drop it —
/// it never reaches a binding, an envelope, or the wire audit.
#[derive(Clone)]
pub struct RevealedSecret {
    pub value: String,
    pub version: i64,
    pub expires_at_ms: Option<i64>,
}

impl std::fmt::Debug for RevealedSecret {
    /// The Debug form REDACTS the value — a stray `{:?}` in a log line
    /// must never become the revelation the doctrine forbids.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RevealedSecret")
            .field("value", &"<redacted>")
            .field("version", &self.version)
            .field("expires_at_ms", &self.expires_at_ms)
            .finish()
    }
}

/// Why a custody operation was refused. Every variant is fail-closed and
/// carries NO secret material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CustodyError {
    /// The custody backend is unreachable (DB down, …). The caller
    /// degrades with a witness — never a 500, never a silent success,
    /// never a stale value.
    Unavailable(String),
    /// No live entry under the key (deleted between enumerate + reveal,
    /// or never seeded).
    NotFound { key: String },
    /// The CAS lost: the entry's version moved past `expected` (another
    /// rotator committed first). The loser must NOT retry with the old
    /// revealed value.
    VersionConflict { key: String, expected: i64 },
    /// A policy refusal (key outside the caller's class, malformed key, …).
    Policy(String),
}

impl std::fmt::Display for CustodyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CustodyError::Unavailable(msg) => write!(f, "custody unavailable: {msg}"),
            CustodyError::NotFound { key } => write!(f, "no live secret under key '{key}'"),
            CustodyError::VersionConflict { key, expected } => write!(
                f,
                "rotation conflict on '{key}': expected version {expected} but the entry \
                 moved — a concurrent rotator committed first (the CAS law: never \
                 double-spend a refresh credential)"
            ),
            CustodyError::Policy(msg) => write!(f, "custody policy refusal: {msg}"),
        }
    }
}

/// The port. Every method takes the tenant EXPLICITLY (the v2.47.0 posture —
/// custody runs under the caller's verified tenant, never ambient state).
///
/// Class discipline: `class_prefix` is the store's declared `class:` plus
/// a trailing dot (`crm` → `crm.`) — an implementation matches keys by
/// PREFIX and must never return an entry outside it.
#[async_trait::async_trait]
pub trait SecretCustody: Send + Sync {
    /// Enumerate the live entries of a class — METADATA ONLY.
    async fn list_metadata(
        &self,
        tenant: &str,
        class_prefix: &str,
    ) -> Result<Vec<SecretMetadata>, CustodyError>;

    /// Reveal a value for a ROTATION exchange (audited distinctly from
    /// dispatch by implementations).
    async fn reveal_for_rotation(
        &self,
        tenant: &str,
        key: &str,
    ) -> Result<RevealedSecret, CustodyError>;

    /// Commit a rotation: CAS on `expected_version` → version + 1, new
    /// value, new declared expiry. The ONLY write path a flow can cause.
    async fn commit_rotation(
        &self,
        tenant: &str,
        key: &str,
        new_value: &str,
        expires_at_ms: Option<i64>,
        expected_version: i64,
    ) -> Result<SecretMetadata, CustodyError>;

    /// Reveal a value for a `use <Tool>` dispatch injection (v2.48.0).
    async fn reveal_for_dispatch(
        &self,
        tenant: &str,
        key: &str,
    ) -> Result<RevealedSecret, CustodyError>;
}

// ════════════════════════════════════════════════════════════════════
// Metadata filter evaluation (v2.21.0 grammar over the synthesized schema)
// ════════════════════════════════════════════════════════════════════

/// Evaluate a parsed v2.21.0 `where:` [`Filter`] over metadata rows, at the
/// captured instant `now_ms`. The compile-time v1.31.0 proof already
/// guaranteed every column is one of the four synthesized ones and every
/// value type-checks; this evaluator is the runtime mirror over in-memory
/// rows (the custody path has no SQL to render). An expression the
/// evaluator cannot honor (a shape only a stale/hand-edited IR could
/// carry) is a typed `Err` — never a silently-empty match.
///
/// SQL NULL semantics: a comparison against a NULL `expires_at` is
/// UNKNOWN → the row is excluded (so `expires_at < now() + interval '10
/// minutes'` never rotates a never-expiring secret by accident).
pub fn filter_metadata(
    rows: Vec<SecretMetadata>,
    filter: &Filter,
    now_ms: i64,
) -> Result<Vec<SecretMetadata>, String> {
    if filter.is_empty() {
        return Ok(rows);
    }
    let mut out = Vec::new();
    for row in rows {
        if row_matches(&row, filter, now_ms)? {
            out.push(row);
        }
    }
    Ok(out)
}

fn row_matches(row: &SecretMetadata, filter: &Filter, now_ms: i64) -> Result<bool, String> {
    // The v1.30.0 filter grammar is a flat AND/OR chain; evaluate left to
    // right with SQL's precedence flattened the same way the SQL renderer
    // emits it (no parentheses exist in the grammar).
    let mut acc = cond_matches(row, &filter.conditions[0], now_ms)?;
    for (i, connector) in filter.connectors.iter().enumerate() {
        let rhs = cond_matches(row, &filter.conditions[i + 1], now_ms)?;
        acc = match connector {
            Connector::And => acc && rhs,
            Connector::Or => acc || rhs,
        };
    }
    Ok(acc)
}

fn cond_matches(
    row: &SecretMetadata,
    cond: &crate::store::filter::FilterCondition,
    now_ms: i64,
) -> Result<bool, String> {
    match cond.column.as_str() {
        "key" => {
            let rhs = match &cond.value {
                Rhs::Value(SqlValue::Text(s)) => s.clone(),
                other => {
                    return Err(format!(
                        "`key` compares against a string, got {other:?} — only a \
                         stale/hand-edited IR reaches this (axon-T802 covers it at \
                         compile time)"
                    ))
                }
            };
            match cond.op {
                Operator::Eq => Ok(row.key == rhs),
                Operator::Ne => Ok(row.key != rhs),
                Operator::Like => Ok(like_matches(&row.key, &rhs)),
                other => Err(format!(
                    "`key` supports = / != / LIKE, got `{other}` (ordering over keys \
                     is not meaningful)"
                )),
            }
        }
        "version" => {
            let rhs = match &cond.value {
                Rhs::Value(SqlValue::Integer(n)) => *n,
                other => {
                    return Err(format!(
                        "`version` compares against an integer, got {other:?}"
                    ))
                }
            };
            Ok(int_cmp(row.version, cond.op, rhs)?)
        }
        "created_at" => ts_cmp(Some(row.created_at_ms), cond, now_ms),
        "expires_at" => ts_cmp(row.expires_at_ms, cond, now_ms),
        other => Err(format!(
            "unknown metadata column `{other}` — the synthesized schema is \
             key / version / created_at / expires_at (axon-T801 covers it at \
             compile time)"
        )),
    }
}

fn int_cmp(lhs: i64, op: Operator, rhs: i64) -> Result<bool, String> {
    Ok(match op {
        Operator::Eq => lhs == rhs,
        Operator::Ne => lhs != rhs,
        Operator::Gt => lhs > rhs,
        Operator::Ge => lhs >= rhs,
        Operator::Lt => lhs < rhs,
        Operator::Le => lhs <= rhs,
        Operator::Like => return Err("LIKE over `version` is not meaningful".to_string()),
    })
}

/// Timestamp comparison. `lhs_ms = None` (a NULL `expires_at`) is SQL
/// UNKNOWN under every operator → excluded.
fn ts_cmp(
    lhs_ms: Option<i64>,
    cond: &crate::store::filter::FilterCondition,
    now_ms: i64,
) -> Result<bool, String> {
    let Some(lhs) = lhs_ms else { return Ok(false) };
    let rhs_ms = match &cond.value {
        Rhs::Time(tv) => time_value_ms(*tv, now_ms),
        Rhs::Value(SqlValue::Integer(n)) => *n,
        other => {
            return Err(format!(
                "`{}` compares against `now() ± interval` (or a Unix-ms integer), \
                 got {other:?}",
                cond.column
            ))
        }
    };
    int_cmp(lhs, cond.op, rhs_ms)
}

/// Resolve a v2.21.0 structural time value to Unix ms at the captured
/// instant. Month/year use the same civil-approximation the store backend
/// documents (30-day months, 365-day years) — declared, not hidden.
pub fn time_value_ms(tv: crate::store::filter::TimeValue, now_ms: i64) -> i64 {
    match tv.offset {
        None => now_ms,
        Some((sign, amount, unit)) => {
            let unit_ms: i64 = match unit {
                TimeUnit::Second => 1_000,
                TimeUnit::Minute => 60_000,
                TimeUnit::Hour => 3_600_000,
                TimeUnit::Day => 86_400_000,
                TimeUnit::Week => 604_800_000,
                TimeUnit::Month => 30 * 86_400_000,
                TimeUnit::Year => 365 * 86_400_000,
            };
            let delta = (amount as i64).saturating_mul(unit_ms);
            match sign {
                TimeSign::Plus => now_ms.saturating_add(delta),
                TimeSign::Minus => now_ms.saturating_sub(delta),
            }
        }
    }
}

/// SQL `LIKE` over a key: `%` = any run, `_` = any one char. No escape
/// sequence in v1 (keys cannot contain `%`/`_` anyway — the key charset
/// is `[a-z0-9][a-z0-9_.-]*`, where `_` is literal; a pattern `_` still
/// matches it as any-one-char, the SQL behavior).
fn like_matches(text: &str, pattern: &str) -> bool {
    fn rec(t: &[u8], p: &[u8]) -> bool {
        match p.first() {
            None => t.is_empty(),
            Some(b'%') => {
                // Greedy-or-empty: try consuming 0..=len chars.
                (0..=t.len()).any(|i| rec(&t[i..], &p[1..]))
            }
            Some(b'_') => !t.is_empty() && rec(&t[1..], &p[1..]),
            Some(c) => t.first() == Some(c) && rec(&t[1..], &p[1..]),
        }
    }
    rec(text.as_bytes(), pattern.as_bytes())
}

// ════════════════════════════════════════════════════════════════════
//  Shared retrieve surface (ONE implementation for both executors —
// the v1.31.0 no-path-divergence lesson)
// ════════════════════════════════════════════════════════════════════

/// Serialize metadata rows to the retrieve envelope both execution paths
/// bind: `{"store":"secrets","rows":[{key,version,created_at,expires_at}…],
/// "count":N}`. Timestamps are RFC 3339 UTC (`expires_at` null when
/// undeclared). No value field can appear — [`SecretMetadata`] has none.
pub fn metadata_envelope(rows: &[SecretMetadata]) -> String {
    let json_rows: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "key": r.key,
                "version": r.version,
                "created_at": rfc3339_utc(r.created_at_ms),
                "expires_at": r.expires_at_ms.map(rfc3339_utc),
            })
        })
        .collect();
    serde_json::json!({
        "store": "secrets",
        "rows": json_rows,
        "count": rows.len(),
    })
    .to_string()
}

fn rfc3339_utc(ms: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms)
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_else(|| ms.to_string())
}

/// The complete `retrieve` over a `backend: secrets` store: enumerate the
/// class → apply the v2.21.0 filter → order → limit → envelope. ONE function
/// used by every execution path. Errors are strings the caller wraps in
/// its own typed error (dispatcher `BackendError` / runner `StoreError`).
///
/// v1 honest scope: `aggregate:` / `group_by:` over custody metadata are
/// refused loudly (documented in the plan vivo section 7) — count-style
/// questions read `count` off the envelope.
pub async fn retrieve_metadata(
    custody: &dyn SecretCustody,
    tenant: &str,
    class: &str,
    where_expr: &str,
    order_by: &str,
    limit_expr: &str,
    aggregate: &str,
    group_by: &str,
    bindings: &HashMap<String, String>,
) -> Result<(String, usize), String> {
    if !aggregate.trim().is_empty() || !group_by.trim().is_empty() {
        return Err(
            "aggregate:/group_by: over a secrets store are not supported in v1 — \
             the envelope carries `count`; open an issue if a real aggregation \
             need appears"
                .to_string(),
        );
    }
    let class_prefix = format!("{class}.");
    let mut rows = custody
        .list_metadata(tenant, &class_prefix)
        .await
        .map_err(|e| e.to_string())?;

    if !where_expr.trim().is_empty() {
        let filter = crate::store::filter::parse_filter(where_expr, bindings)
            .map_err(|e| format!("secrets-store `where:` did not compile: {e}"))?;
        rows = filter_metadata(rows, &filter, now_unix_ms())?;
    }

    // v2.21.0 `order_by:` — the four metadata columns only, `col [asc|desc]`
    // terms, comma-separated (the same surface the SQL path renders).
    for term in order_by
        .split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    // Stable sorts compose right-to-left: applying the LAST term first
    // leaves earlier terms dominant — multi-term order matches SQL.
    {
        let mut parts = term.split_whitespace();
        let col = parts.next().unwrap_or_default();
        let dir = parts.next().unwrap_or("asc").to_ascii_lowercase();
        let desc = match dir.as_str() {
            "asc" => false,
            "desc" => true,
            other => return Err(format!("bad order_by direction `{other}`")),
        };
        match col {
            "key" => rows.sort_by(|a, b| a.key.cmp(&b.key)),
            "version" => rows.sort_by_key(|r| r.version),
            "created_at" => rows.sort_by_key(|r| r.created_at_ms),
            // NULLs sort LAST ascending (Postgres default), FIRST when
            // descending — mirrored by mapping None to i64::MAX.
            "expires_at" => rows.sort_by_key(|r| r.expires_at_ms.unwrap_or(i64::MAX)),
            other => return Err(format!("bad order_by column `{other}`")),
        }
        if desc {
            rows.reverse();
        }
    }

    if !limit_expr.trim().is_empty() {
        let resolved = crate::exec_context::interpolate_vars(limit_expr, bindings);
        let n: usize = resolved
            .trim()
            .parse()
            .map_err(|_| format!("bad limit `{resolved}` — expected a non-negative integer"))?;
        rows.truncate(n);
    }

    let count = rows.len();
    Ok((metadata_envelope(&rows), count))
}

// ════════════════════════════════════════════════════════════════════
//  Rotation exchange envelope (the reserved tool wire contract)
// ════════════════════════════════════════════════════════════════════

/// Build the reserved request body of ONE rotation exchange. The tool
/// receives the CURRENT value under `axon_rotation` and must respond
/// with `axon_rotated` ([`parse_rotated_response`]). This is the only
/// channel a custody value ever travels — custody → tool → custody.
pub fn rotation_request_body(key: &str, revealed: &RevealedSecret) -> String {
    serde_json::json!({
        "axon_rotation": {
            "key": key,
            "value": revealed.value,
            "version": revealed.version,
            "expires_at": revealed.expires_at_ms.map(rfc3339_utc),
        }
    })
    .to_string()
}

/// Parse a rotation tool's response: `{"axon_rotated": {"value": "...",
/// "expires_at_ms": 123 | "expires_at": "<RFC3339>" | absent}}`. A
/// response without the reserved field is a per-key failure (the tool
/// did not perform the exchange contract), never a silent success.
pub fn parse_rotated_response(raw: &str) -> Result<(String, Option<i64>), String> {
    let v: serde_json::Value = serde_json::from_str(raw)
        .map_err(|e| format!("rotation tool response is not JSON: {e}"))?;
    let rotated = v
        .get("axon_rotated")
        .ok_or("rotation tool response carries no `axon_rotated` field")?;
    let value = rotated
        .get("value")
        .and_then(|x| x.as_str())
        .ok_or("`axon_rotated.value` missing or not a string")?
        .to_string();
    if value.is_empty() {
        return Err("`axon_rotated.value` is empty — refusing to commit an empty secret".into());
    }
    let expires_at_ms = if let Some(ms) = rotated.get("expires_at_ms").and_then(|x| x.as_i64()) {
        Some(ms)
    } else if let Some(s) = rotated.get("expires_at").and_then(|x| x.as_str()) {
        Some(
            chrono::DateTime::parse_from_rfc3339(s)
                .map_err(|e| format!("`axon_rotated.expires_at` is not RFC 3339: {e}"))?
                .timestamp_millis(),
        )
    } else {
        None
    };
    Ok((value, expires_at_ms))
}

// ════════════════════════════════════════════════════════════════════
//  InMemoryCustody — the reference implementation
// ════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
struct Entry {
    value: String,
    version: i64,
    created_at_ms: i64,
    expires_at_ms: Option<i64>,
}

/// The reference in-process custody: full class/CAS/NULL semantics,
/// process-memory storage. NOT encrypted at rest (the enterprise v2.48.0
/// custody is the production surface — envelope AES-256-GCM over
/// Postgres); this exists so tests and single-process adopters exercise
/// the exact port contract.
#[derive(Default)]
pub struct InMemoryCustody {
    entries: Mutex<HashMap<(String, String), Entry>>,
}

impl InMemoryCustody {
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed an entry (the in-process mirror of `POST /tenant/secrets`).
    /// A re-seed of a live key bumps the version — the v2.48.0 upsert law.
    pub fn seed(&self, tenant: &str, key: &str, value: &str, expires_at_ms: Option<i64>) {
        let now_ms = now_unix_ms();
        let mut entries = self.entries.lock().unwrap();
        let slot = entries.entry((tenant.to_string(), key.to_string()));
        match slot {
            std::collections::hash_map::Entry::Occupied(mut o) => {
                let e = o.get_mut();
                e.value = value.to_string();
                e.version += 1;
                e.expires_at_ms = expires_at_ms;
            }
            std::collections::hash_map::Entry::Vacant(v) => {
                v.insert(Entry {
                    value: value.to_string(),
                    version: 1,
                    created_at_ms: now_ms,
                    expires_at_ms,
                });
            }
        }
    }

    /// Test-surface: the current version of a key (`None` = absent).
    pub fn version_of(&self, tenant: &str, key: &str) -> Option<i64> {
        self.entries
            .lock()
            .unwrap()
            .get(&(tenant.to_string(), key.to_string()))
            .map(|e| e.version)
    }

    fn meta_of(key: &str, e: &Entry) -> SecretMetadata {
        SecretMetadata {
            key: key.to_string(),
            version: e.version,
            created_at_ms: e.created_at_ms,
            expires_at_ms: e.expires_at_ms,
        }
    }
}

fn now_unix_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[async_trait::async_trait]
impl SecretCustody for InMemoryCustody {
    async fn list_metadata(
        &self,
        tenant: &str,
        class_prefix: &str,
    ) -> Result<Vec<SecretMetadata>, CustodyError> {
        let entries = self.entries.lock().unwrap();
        let mut out: Vec<SecretMetadata> = entries
            .iter()
            .filter(|((t, k), _)| t == tenant && k.starts_with(class_prefix))
            .map(|((_, k), e)| Self::meta_of(k, e))
            .collect();
        out.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(out)
    }

    async fn reveal_for_rotation(
        &self,
        tenant: &str,
        key: &str,
    ) -> Result<RevealedSecret, CustodyError> {
        let entries = self.entries.lock().unwrap();
        entries
            .get(&(tenant.to_string(), key.to_string()))
            .map(|e| RevealedSecret {
                value: e.value.clone(),
                version: e.version,
                expires_at_ms: e.expires_at_ms,
            })
            .ok_or_else(|| CustodyError::NotFound { key: key.to_string() })
    }

    async fn commit_rotation(
        &self,
        tenant: &str,
        key: &str,
        new_value: &str,
        expires_at_ms: Option<i64>,
        expected_version: i64,
    ) -> Result<SecretMetadata, CustodyError> {
        let mut entries = self.entries.lock().unwrap();
        let e = entries
            .get_mut(&(tenant.to_string(), key.to_string()))
            .ok_or_else(|| CustodyError::NotFound { key: key.to_string() })?;
        if e.version != expected_version {
            return Err(CustodyError::VersionConflict {
                key: key.to_string(),
                expected: expected_version,
            });
        }
        e.value = new_value.to_string();
        e.version += 1;
        e.expires_at_ms = expires_at_ms;
        Ok(Self::meta_of(key, e))
    }

    async fn reveal_for_dispatch(
        &self,
        tenant: &str,
        key: &str,
    ) -> Result<RevealedSecret, CustodyError> {
        // Same read as rotation in the reference impl; implementations
        // audit the two distinctly.
        self.reveal_for_rotation(tenant, key).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::filter::parse_filter;

    fn bindings() -> std::collections::HashMap<String, String> {
        std::collections::HashMap::new()
    }

    #[tokio::test]
    async fn list_is_class_scoped_and_metadata_only() {
        let c = InMemoryCustody::new();
        c.seed("t1", "crm.hubspot", "tok-a", Some(1_000));
        c.seed("t1", "crm.zoho", "tok-b", None);
        c.seed("t1", "llm.kimi", "key-c", None);
        c.seed("t2", "crm.hubspot", "tok-d", None);
        let rows = c.list_metadata("t1", "crm.").await.unwrap();
        assert_eq!(rows.len(), 2, "class-scoped: llm.* and t2 excluded");
        assert_eq!(rows[0].key, "crm.hubspot");
        assert_eq!(rows[0].version, 1);
    }

    #[tokio::test]
    async fn reseed_bumps_version() {
        let c = InMemoryCustody::new();
        c.seed("t1", "crm.hubspot", "tok-a", None);
        c.seed("t1", "crm.hubspot", "tok-b", None);
        assert_eq!(c.version_of("t1", "crm.hubspot"), Some(2));
    }

    #[tokio::test]
    async fn commit_rotation_cas_conflict_is_typed() {
        let c = InMemoryCustody::new();
        c.seed("t1", "crm.hubspot", "tok-a", None);
        // Winner commits at expected 1 → version 2.
        let meta = c
            .commit_rotation("t1", "crm.hubspot", "tok-b", Some(9_999), 1)
            .await
            .unwrap();
        assert_eq!(meta.version, 2);
        assert_eq!(meta.expires_at_ms, Some(9_999));
        // Loser (still holding expected 1) must get the conflict.
        let err = c
            .commit_rotation("t1", "crm.hubspot", "tok-c", None, 1)
            .await
            .unwrap_err();
        assert!(matches!(err, CustodyError::VersionConflict { .. }));
        // The winner's value stands.
        let revealed = c.reveal_for_rotation("t1", "crm.hubspot").await.unwrap();
        assert_eq!(revealed.value, "tok-b");
    }

    #[tokio::test]
    async fn rotation_of_missing_key_is_not_found() {
        let c = InMemoryCustody::new();
        let err = c.reveal_for_rotation("t1", "ghost.key").await.unwrap_err();
        assert!(matches!(err, CustodyError::NotFound { .. }));
    }

    #[test]
    fn revealed_secret_debug_redacts_the_value() {
        let r = RevealedSecret {
            value: "super-secret-token".into(),
            version: 3,
            expires_at_ms: None,
        };
        let dbg = format!("{r:?}");
        assert!(!dbg.contains("super-secret-token"), "{dbg}");
        assert!(dbg.contains("<redacted>"), "{dbg}");
    }

    #[test]
    fn filter_expires_at_lt_now_plus_interval() {
        let now_ms = 1_000_000_000_000;
        let rows = vec![
            SecretMetadata {
                key: "crm.expiring".into(),
                version: 1,
                created_at_ms: 0,
                // 5 minutes from now — inside the 10-minute window.
                expires_at_ms: Some(now_ms + 5 * 60_000),
            },
            SecretMetadata {
                key: "crm.fresh".into(),
                version: 1,
                created_at_ms: 0,
                // 2 hours from now — outside.
                expires_at_ms: Some(now_ms + 2 * 3_600_000),
            },
            SecretMetadata {
                key: "crm.never".into(),
                version: 1,
                created_at_ms: 0,
                // NULL expiry — UNKNOWN under every comparison, excluded.
                expires_at_ms: None,
            },
        ];
        let f =
            parse_filter("expires_at < now() + interval '10 minutes'", &bindings()).unwrap();
        let matched = filter_metadata(rows, &f, now_ms).unwrap();
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].key, "crm.expiring");
    }

    #[test]
    fn filter_key_like_and_version_and_connector() {
        let now_ms = 1_000;
        let rows = vec![
            SecretMetadata {
                key: "crm.hubspot".into(),
                version: 3,
                created_at_ms: 0,
                expires_at_ms: None,
            },
            SecretMetadata {
                key: "crm.zoho".into(),
                version: 1,
                created_at_ms: 0,
                expires_at_ms: None,
            },
        ];
        let f = parse_filter("key LIKE 'crm.hub%' AND version >= 2", &bindings()).unwrap();
        let matched = filter_metadata(rows, &f, now_ms).unwrap();
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].key, "crm.hubspot");
    }

    #[test]
    fn filter_unknown_column_is_a_typed_error_not_empty() {
        let rows = vec![SecretMetadata {
            key: "crm.hubspot".into(),
            version: 1,
            created_at_ms: 0,
            expires_at_ms: None,
        }];
        let f = parse_filter("access_token = 'x'", &bindings()).unwrap();
        let err = filter_metadata(rows, &f, 0).unwrap_err();
        assert!(err.contains("unknown metadata column"), "{err}");
    }

    #[test]
    fn like_semantics() {
        assert!(like_matches("crm.hubspot", "crm.%"));
        assert!(like_matches("crm.hubspot", "%hub%"));
        assert!(!like_matches("llm.kimi", "crm.%"));
        assert!(like_matches("crm.a", "crm._"));
        assert!(!like_matches("crm.ab", "crm._"));
    }
}
