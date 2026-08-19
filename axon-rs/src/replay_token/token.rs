//! [`ReplayToken`] canonical shape + hash derivation.
//!
//! Hash input is built as:
//!
//! ```text
//!   effect_name  ∥ RS ∥
//!   canonical_json(inputs)  ∥ RS ∥
//!   canonical_json(outputs)  ∥ RS ∥
//!   model_version  ∥ RS ∥
//!   canonical_json(sampling)  ∥ RS ∥
//!   timestamp_rfc3339  ∥ RS ∥
//!   nonce_hex
//! ```
//!
//! where `RS = 0x1E` (ASCII Record Separator). The canonicaliser
//! matches the one already used by the v1.4.0 audit chain
//! (recursive key sort, UTF-8 encoding, ASCII-safe escapes) so a
//! token can be re-hashed by the enterprise audit writer with zero
//! translation.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Sampling parameters captured at the point of a non-deterministic
/// effect (LLM inference, random sampling). Replayability depends on
/// the provider honouring `seed`; providers that ignore `seed` get
/// their effects marked `@non_replayable` in the tool descriptor and
/// the checker rejects their use in a `@sensitive` context.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SamplingParams {
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub top_p: Option<f64>,
    #[serde(default)]
    pub top_k: Option<i64>,
    #[serde(default)]
    pub seed: Option<i64>,
    #[serde(default)]
    pub max_tokens: Option<i64>,
    /// Extra provider-specific knobs (`frequency_penalty`, `stop`,
    /// tool choice strategy, …). The recorder copies whatever it
    /// was given so replay catches provider-specific defaults.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub extras: Value,
}

impl Default for SamplingParams {
    fn default() -> Self {
        Self {
            temperature: None,
            top_p: None,
            top_k: None,
            seed: None,
            max_tokens: None,
            extras: Value::Null,
        }
    }
}

/// One replay receipt. Immutable once minted.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplayToken {
    /// Canonical identifier of the effect invoked
    /// (`call_tool:send_slack`, `llm_infer:gpt_4o`, `db_read:customers`).
    pub effect_name: String,
    /// Inputs as given to the effect. Canonicalised then hashed into
    /// `inputs_hash_hex`; retained here in structured form for
    /// replay executors.
    pub inputs: Value,
    pub inputs_hash_hex: String,
    /// Outputs the effect produced. Same canonical treatment as
    /// inputs.
    pub outputs: Value,
    pub outputs_hash_hex: String,
    /// Model identifier (free string). For deterministic, non-LLM
    /// effects adopters use a stable version slug like
    /// `axon.builtin.db_read.v1`. For LLMs this is the provider's
    /// model id (`gpt-4o-2024-11-20`, `claude-opus-4-7`) at call
    /// time.
    pub model_version: String,
    pub sampling: SamplingParams,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub timestamp: DateTime<Utc>,
    /// 128-bit random nonce; prevents collision between two
    /// semantically identical invocations.
    pub nonce_hex: String,
    /// Derived hex SHA-256 of the canonical hash input. Stable
    /// across Rust + Python implementations.
    pub token_hash_hex: String,
}

impl ReplayToken {
    /// Build a fresh token, computing every derived hash. Callers
    /// usually go through [`ReplayTokenBuilder`] for ergonomics but
    /// the direct constructor is public for explicit constructions
    /// in tests / adapters.
    pub fn mint(
        effect_name: impl Into<String>,
        inputs: Value,
        outputs: Value,
        model_version: impl Into<String>,
        sampling: SamplingParams,
        timestamp: DateTime<Utc>,
        nonce: [u8; 16],
    ) -> Self {
        let effect_name = effect_name.into();
        let model_version = model_version.into();
        let inputs_hash_hex = hex(&canonical_hash(&inputs));
        let outputs_hash_hex = hex(&canonical_hash(&outputs));
        let nonce_hex = hex(&nonce);
        let token_hash_hex = hex(&derive_token_hash(
            &effect_name,
            &inputs,
            &outputs,
            &model_version,
            &sampling,
            timestamp,
            &nonce,
        ));
        ReplayToken {
            effect_name,
            inputs,
            inputs_hash_hex,
            outputs,
            outputs_hash_hex,
            model_version,
            sampling,
            timestamp,
            nonce_hex,
            token_hash_hex,
        }
    }
}

/// Ergonomic builder; adopters populate fields and call `.mint()`.
#[derive(Debug, Default)]
pub struct ReplayTokenBuilder {
    effect_name: Option<String>,
    inputs: Option<Value>,
    outputs: Option<Value>,
    model_version: Option<String>,
    sampling: SamplingParams,
    timestamp: Option<DateTime<Utc>>,
    nonce: Option<[u8; 16]>,
}

impl ReplayTokenBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn effect_name(mut self, name: impl Into<String>) -> Self {
        self.effect_name = Some(name.into());
        self
    }
    pub fn inputs(mut self, v: Value) -> Self {
        self.inputs = Some(v);
        self
    }
    pub fn outputs(mut self, v: Value) -> Self {
        self.outputs = Some(v);
        self
    }
    pub fn model_version(mut self, s: impl Into<String>) -> Self {
        self.model_version = Some(s.into());
        self
    }
    pub fn sampling(mut self, s: SamplingParams) -> Self {
        self.sampling = s;
        self
    }
    pub fn timestamp(mut self, ts: DateTime<Utc>) -> Self {
        self.timestamp = Some(ts);
        self
    }
    pub fn nonce(mut self, bytes: [u8; 16]) -> Self {
        self.nonce = Some(bytes);
        self
    }

    pub fn mint(self) -> ReplayToken {
        let effect_name = self.effect_name.expect("effect_name required");
        let inputs = self.inputs.unwrap_or(Value::Null);
        let outputs = self.outputs.unwrap_or(Value::Null);
        let model_version = self.model_version.unwrap_or_else(|| "unset".into());
        let timestamp = self.timestamp.unwrap_or_else(Utc::now);
        let nonce = self.nonce.unwrap_or_else(generate_nonce);
        ReplayToken::mint(
            effect_name,
            inputs,
            outputs,
            model_version,
            self.sampling,
            timestamp,
            nonce,
        )
    }
}

// ── Canonical JSON hashing ──────────────────────────────────────────

/// Compute SHA-256 of `v` after canonical JSON encoding. The
/// canonical form sorts object keys recursively and omits optional
/// whitespace — identical to the v1.4.0 audit-chain canonicaliser.
pub fn canonical_hash(v: &Value) -> [u8; 32] {
    let canonical = canonicalize(v);
    let mut h = Sha256::new();
    h.update(canonical.as_bytes());
    let out = h.finalize();
    let mut array = [0u8; 32];
    array.copy_from_slice(&out);
    array
}

/// RFC 8785-style canonical JSON — keys sorted, no whitespace,
/// ASCII-safe escapes. serde_json with `to_writer_pretty(false)`
/// already omits whitespace; key sorting we do here.
fn canonicalize(v: &Value) -> String {
    let sorted = sort_object_keys(v.clone());
    serde_json::to_string(&sorted).expect("canonical JSON encoding")
}

fn sort_object_keys(v: Value) -> Value {
    match v {
        Value::Object(map) => {
            // serde_json::Map preserves insertion order. Re-insert
            // in sorted order so the encoder emits sorted output.
            let mut entries: Vec<(String, Value)> = map.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            let mut sorted_map = serde_json::Map::new();
            for (k, inner) in entries {
                sorted_map.insert(k, sort_object_keys(inner));
            }
            Value::Object(sorted_map)
        }
        Value::Array(items) => {
            Value::Array(items.into_iter().map(sort_object_keys).collect())
        }
        other => other,
    }
}

fn derive_token_hash(
    effect_name: &str,
    inputs: &Value,
    outputs: &Value,
    model_version: &str,
    sampling: &SamplingParams,
    timestamp: DateTime<Utc>,
    nonce: &[u8; 16],
) -> [u8; 32] {
    const RS: u8 = 0x1E;
    let mut h = Sha256::new();
    h.update(effect_name.as_bytes());
    h.update([RS]);
    h.update(canonicalize(inputs).as_bytes());
    h.update([RS]);
    h.update(canonicalize(outputs).as_bytes());
    h.update([RS]);
    h.update(model_version.as_bytes());
    h.update([RS]);
    h.update(
        canonicalize(&serde_json::to_value(sampling).expect("sampling serialisable"))
            .as_bytes(),
    );
    h.update([RS]);
    h.update(timestamp.to_rfc3339().as_bytes());
    h.update([RS]);
    h.update(nonce);
    let out = h.finalize();
    let mut array = [0u8; 32];
    array.copy_from_slice(&out);
    array
}

fn generate_nonce() -> [u8; 16] {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    bytes
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixed_nonce() -> [u8; 16] {
        [1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
    }

    fn fixed_timestamp() -> DateTime<Utc> {
        use chrono::TimeZone;
        Utc.with_ymd_and_hms(2026, 4, 22, 12, 0, 0).unwrap()
    }

    #[test]
    fn mint_sets_every_derived_hash() {
        let token = ReplayToken::mint(
            "call_tool:send_slack",
            json!({"channel": "#ops", "text": "hi"}),
            json!({"ok": true, "ts": "1700000000.000"}),
            "axon.builtin.slack.v1",
            SamplingParams::default(),
            fixed_timestamp(),
            fixed_nonce(),
        );
        assert_eq!(token.effect_name, "call_tool:send_slack");
        assert_eq!(token.inputs_hash_hex.len(), 64);
        assert_eq!(token.outputs_hash_hex.len(), 64);
        assert_eq!(token.token_hash_hex.len(), 64);
        assert_eq!(token.nonce_hex, "0102030405060708090a0b0c0d0e0f10");
    }

    #[test]
    fn canonical_hash_is_key_order_independent() {
        let a = json!({"a": 1, "b": 2, "c": 3});
        let b = json!({"c": 3, "a": 1, "b": 2});
        assert_eq!(canonical_hash(&a), canonical_hash(&b));
    }

    #[test]
    fn canonical_hash_propagates_to_nested_objects() {
        let a = json!({"outer": {"a": 1, "b": 2}});
        let b = json!({"outer": {"b": 2, "a": 1}});
        assert_eq!(canonical_hash(&a), canonical_hash(&b));
    }

    #[test]
    fn token_hash_is_deterministic_for_identical_inputs() {
        let nonce = fixed_nonce();
        let ts = fixed_timestamp();
        let inputs = json!({"prompt": "hi", "user_id": "u-1"});
        let outputs = json!({"text": "hello"});

        let t1 = ReplayToken::mint(
            "llm_infer",
            inputs.clone(),
            outputs.clone(),
            "claude-opus-4-7",
            SamplingParams {
                temperature: Some(0.7),
                top_p: Some(0.95),
                seed: Some(42),
                ..Default::default()
            },
            ts,
            nonce,
        );
        let t2 = ReplayToken::mint(
            "llm_infer",
            inputs,
            outputs,
            "claude-opus-4-7",
            SamplingParams {
                temperature: Some(0.7),
                top_p: Some(0.95),
                seed: Some(42),
                ..Default::default()
            },
            ts,
            nonce,
        );
        assert_eq!(t1.token_hash_hex, t2.token_hash_hex);
    }

    #[test]
    fn token_hash_differs_when_model_version_differs() {
        let t_old = ReplayToken::mint(
            "llm_infer",
            json!({"x": 1}),
            json!({"y": 2}),
            "claude-opus-4-7",
            SamplingParams::default(),
            fixed_timestamp(),
            fixed_nonce(),
        );
        let t_new = ReplayToken::mint(
            "llm_infer",
            json!({"x": 1}),
            json!({"y": 2}),
            "claude-opus-4-8",
            SamplingParams::default(),
            fixed_timestamp(),
            fixed_nonce(),
        );
        assert_ne!(t_old.token_hash_hex, t_new.token_hash_hex);
    }

    #[test]
    fn builder_works() {
        let t = ReplayTokenBuilder::new()
            .effect_name("db_read:customers")
            .inputs(json!({"where": {"id": 42}}))
            .outputs(json!({"name": "Acme"}))
            .model_version("axon.builtin.db_read.v1")
            .timestamp(fixed_timestamp())
            .nonce(fixed_nonce())
            .mint();
        assert_eq!(t.effect_name, "db_read:customers");
        assert_eq!(t.token_hash_hex.len(), 64);
    }

    #[test]
    fn random_nonce_differs_across_mints() {
        let t1 = ReplayTokenBuilder::new()
            .effect_name("x")
            .timestamp(fixed_timestamp())
            .mint();
        let t2 = ReplayTokenBuilder::new()
            .effect_name("x")
            .timestamp(fixed_timestamp())
            .mint();
        assert_ne!(t1.nonce_hex, t2.nonce_hex);
        assert_ne!(t1.token_hash_hex, t2.token_hash_hex);
    }
}
