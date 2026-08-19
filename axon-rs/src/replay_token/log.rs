//! [`ReplayLog`] — pluggable sink for [`crate::replay_token::ReplayToken`]s.
//!
//! Two implementations ship with 11.c:
//!
//! - [`InMemoryReplayLog`] — dev / test. Keeps tokens in a
//!   `Mutex<Vec>`.
//! - **Enterprise adapter shape** — defined as a trait any
//!   adopter/enterprise-side implementation can satisfy. The
//!   `axon_enterprise.replay.service.ReplayService` ports this
//! trait to Python, anchoring each token in the v1.4.0 audit
//!   hash chain.
//!
//! The trait is intentionally minimal — `append`, `get`, `since` —
//! so the enterprise sink only has to wire the three operations
//! against its persistence layer.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;

use crate::replay_token::token::ReplayToken;

#[derive(Debug)]
pub enum ReplayLogError {
    TokenNotFound { token_hash_hex: String },
    Backend(String),
}

impl std::fmt::Display for ReplayLogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TokenNotFound { token_hash_hex } => {
                write!(f, "replay token {token_hash_hex:?} not found")
            }
            Self::Backend(msg) => write!(f, "replay log backend: {msg}"),
        }
    }
}

impl std::error::Error for ReplayLogError {}

/// Append-only log of replay tokens.
///
/// The trait is `async` because enterprise sinks will speak to
/// Postgres / audit-chain writers; in-memory impls don't actually
/// await anything but still implement the trait signature.
#[async_trait]
pub trait ReplayLog: Send + Sync {
    async fn append(
        &self,
        token: ReplayToken,
    ) -> Result<(), ReplayLogError>;

    async fn get(
        &self,
        token_hash_hex: &str,
    ) -> Result<ReplayToken, ReplayLogError>;

    /// Return tokens for a single flow identifier, ordered by
    /// timestamp ascending. `flow_id` is an opaque string the
    /// recorder chose (typically the flow's execution id).
    async fn tokens_for_flow(
        &self,
        flow_id: &str,
    ) -> Result<Vec<ReplayToken>, ReplayLogError>;
}

// ── In-memory impl for tests + dev ───────────────────────────────────

#[derive(Debug, Default)]
pub struct InMemoryReplayLog {
    // Keyed by token_hash_hex for O(1) get.
    by_hash: Mutex<HashMap<String, ReplayToken>>,
    // Keyed by flow_id; value is an ordered list of hashes.
    by_flow: Mutex<HashMap<String, Vec<String>>>,
}

impl InMemoryReplayLog {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn len(&self) -> usize {
        self.by_hash.lock().expect("poisoned").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[async_trait]
impl ReplayLog for InMemoryReplayLog {
    async fn append(
        &self,
        token: ReplayToken,
    ) -> Result<(), ReplayLogError> {
        // Flow id is expected as an `inputs.flow_id` or
        // `inputs._flow_id` convention; adopters that prefer a
        // different key wrap the log and adapt. We look both up.
        let flow_id = token
            .inputs
            .get("flow_id")
            .and_then(|v| v.as_str())
            .or_else(|| {
                token
                    .inputs
                    .get("_flow_id")
                    .and_then(|v| v.as_str())
            })
            .unwrap_or("")
            .to_string();

        let mut by_hash = self.by_hash.lock().expect("poisoned");
        let mut by_flow = self.by_flow.lock().expect("poisoned");
        by_hash.insert(token.token_hash_hex.clone(), token.clone());
        by_flow
            .entry(flow_id)
            .or_default()
            .push(token.token_hash_hex.clone());
        Ok(())
    }

    async fn get(
        &self,
        token_hash_hex: &str,
    ) -> Result<ReplayToken, ReplayLogError> {
        let by_hash = self.by_hash.lock().expect("poisoned");
        by_hash
            .get(token_hash_hex)
            .cloned()
            .ok_or_else(|| ReplayLogError::TokenNotFound {
                token_hash_hex: token_hash_hex.to_string(),
            })
    }

    async fn tokens_for_flow(
        &self,
        flow_id: &str,
    ) -> Result<Vec<ReplayToken>, ReplayLogError> {
        let by_hash = self.by_hash.lock().expect("poisoned");
        let by_flow = self.by_flow.lock().expect("poisoned");
        let hashes = by_flow.get(flow_id).cloned().unwrap_or_default();
        let mut out = Vec::with_capacity(hashes.len());
        for h in hashes {
            if let Some(t) = by_hash.get(&h) {
                out.push(t.clone());
            }
        }
        // Timestamp-asc — stable ordering for replay executors.
        out.sort_by_key(|t| t.timestamp);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay_token::token::{ReplayTokenBuilder, SamplingParams};
    use chrono::{TimeZone, Utc};
    use serde_json::json;

    fn token(flow_id: &str, effect: &str, seq: i64) -> ReplayToken {
        ReplayTokenBuilder::new()
            .effect_name(effect)
            .inputs(json!({"flow_id": flow_id, "seq": seq}))
            .outputs(json!({"ok": true}))
            .model_version("axon.builtin.test.v1")
            .sampling(SamplingParams::default())
            .timestamp(
                Utc.with_ymd_and_hms(2026, 4, 22, 12, 0, seq as u32)
                    .unwrap(),
            )
            .nonce([seq as u8; 16])
            .mint()
    }

    #[tokio::test]
    async fn append_and_get_roundtrip() {
        let log = InMemoryReplayLog::new();
        let t = token("flow-1", "call_tool:x", 0);
        let hash = t.token_hash_hex.clone();
        log.append(t.clone()).await.unwrap();
        let fetched = log.get(&hash).await.unwrap();
        assert_eq!(fetched, t);
    }

    #[tokio::test]
    async fn get_unknown_token_errors() {
        let log = InMemoryReplayLog::new();
        let err = log.get("deadbeef").await.unwrap_err();
        matches!(err, ReplayLogError::TokenNotFound { .. });
    }

    #[tokio::test]
    async fn tokens_for_flow_sorted_by_timestamp() {
        let log = InMemoryReplayLog::new();
        let t2 = token("flow-a", "step2", 2);
        let t1 = token("flow-a", "step1", 1);
        let t3 = token("flow-a", "step3", 3);
        // Insert out of order.
        log.append(t2.clone()).await.unwrap();
        log.append(t1.clone()).await.unwrap();
        log.append(t3.clone()).await.unwrap();
        let out = log.tokens_for_flow("flow-a").await.unwrap();
        assert_eq!(
            out.iter().map(|t| t.effect_name.clone()).collect::<Vec<_>>(),
            vec!["step1", "step2", "step3"],
        );
    }

    #[tokio::test]
    async fn tokens_for_unknown_flow_returns_empty() {
        let log = InMemoryReplayLog::new();
        let out = log.tokens_for_flow("missing").await.unwrap();
        assert!(out.is_empty());
    }
}
