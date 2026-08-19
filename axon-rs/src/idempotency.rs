//! v1.23.0 — Idempotency-Key store for first-class axonendpoint routes.
//!
//! Stripe-compatible Idempotency-Key semantics for POST/PUT routes,
//! the banking-grade primitive that makes safe client retries possible
//! on flaky networks. The invariant ratified in D7:
//!
//!   **same_key + same_body ⟹ same_response** (within retention window).
//!
//! ## D7 truth table (per plan vivo section 7.2)
//!
//! | request key | endpoint method | cache state | response                                |
//! |-------------|-----------------|-------------|-----------------------------------------|
//! | absent      | any             | n/a         | normal execute (no caching)             |
//! | present     | POST or PUT     | miss        | execute + cache + 200 (or original)     |
//! | present     | POST or PUT     | hit, same   | byte-identical cached body + Idempotency-Status: replayed |
//! | present     | POST or PUT     | hit, differ | 422 `idempotency_key_reused_with_different_request` |
//! | present     | GET or DELETE   | n/a         | key ignored (logged); HTTP-spec idempotent natively |
//!
//! ## Cross-tenant isolation
//!
//! Cache key = `(client_id, endpoint_path, idempotency_key)`. Two
//! tenants cannot collide on the same Idempotency-Key because the
//! `client_id` (from auth bearer or `"anonymous"` fallback) namespaces
//! the entry. This honors PCI DSS Req 8 (account-level segregation)
//! and SOC 2 CC6 (logical access controls).
//!
//! ## Retention
//!
//! Default 24h sliding window per Stripe / Plaid convention. Entries
//! older than the window are evicted lazily on lookup; a periodic
//! reaper (`reap_expired`) is exposed so a server task can run it
//! out-of-band.
//!
//! ## Pillar trace per D12
//!
//! - **MATHEMATICS** — the cache is a partial function with retention:
//!   `lookup : (client_id, path, key, body_hash, now) → Option<Response>`.
//!   Single-valued for every input; the body_hash check forbids
//!   silent body drift collapsing two distinct requests into one cached
//!   response.
//! - **LOGIC** — `same_key + same_body ⟹ same_response` invariant
//!   provably preserved when the cached response is returned verbatim
//!   (status + headers + body cloned byte-for-byte).
//! - **PHILOSOPHY** — the language honors the industry standard verbatim:
//!   Stripe / Plaid / Square clients work unchanged when pointed at
//!   axon endpoints.
//! - **COMPUTING** — D9 backwards-compat absolute: requests without
//!   the header AND endpoints without `method: POST|PUT` are unaffected;
//!   no client behavior changes.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

/// Default retention window per Stripe / Plaid convention.
pub const DEFAULT_RETENTION: Duration = Duration::from_secs(24 * 60 * 60);

/// One cached response entry. Holds enough metadata to project the
/// original wire response back verbatim (status, body, content-type)
/// AND to detect body drift for the same key (request_body_hash).
#[derive(Debug, Clone)]
pub struct IdempotencyEntry {
    /// SHA-256 of the canonicalized request body. Used to detect
    /// "same key, different body" → 422.
    pub request_body_hash: [u8; 32],
    /// HTTP status code of the cached response.
    pub status: u16,
    /// Content-Type header of the cached response (preserved verbatim
    /// so the replay matches the original wire format — JSON, SSE,
    /// ndjson, etc.).
    pub content_type: String,
    /// Cached response body bytes.
    pub body: Vec<u8>,
    /// When this entry was inserted. Used by the retention sweep to
    /// evict entries older than the configured window.
    pub inserted_at: Instant,
}

/// Composite key namespacing each entry by client + endpoint + key.
/// Cross-tenant isolation is a property of this struct's identity:
/// two clients cannot collide on the same Idempotency-Key value
/// because their `client_id` prefixes differ.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdempotencyCacheKey {
    pub client_id: String,
    pub endpoint_path: String,
    pub idempotency_key: String,
}

/// Result of a cache lookup. Total enum — every input either misses,
/// hits with a matching body (→ replay), or hits with a different
/// body for the same key (→ 422 conflict).
#[derive(Debug, Clone)]
pub enum IdempotencyVerdict {
    Miss,
    Hit(IdempotencyEntry),
    Conflict {
        /// The conflict diagnostic surfaces the cached body's hash
        /// (hex prefix) so the adopter can correlate the failing
        /// request with whatever the original request body was.
        cached_body_hash_hex: String,
    },
}

/// In-memory Idempotency-Key store. Bounded by capacity (default
/// 10_000 entries — generous for the high-traffic banking POST case);
/// once full, the oldest entry is evicted on insert.
#[derive(Debug)]
pub struct IdempotencyStore {
    entries: HashMap<IdempotencyCacheKey, IdempotencyEntry>,
    capacity: usize,
    retention: Duration,
}

impl Default for IdempotencyStore {
    fn default() -> Self {
        Self::new(10_000, DEFAULT_RETENTION)
    }
}

impl IdempotencyStore {
    pub fn new(capacity: usize, retention: Duration) -> Self {
        Self {
            entries: HashMap::new(),
            capacity,
            retention,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Hex-encode the first 8 bytes of a SHA-256 digest. Enough
    /// entropy for adopter-side correlation, doesn't leak the full
    /// hash (defense-in-depth).
    pub fn hash_prefix_hex(hash: &[u8; 32]) -> String {
        let mut s = String::with_capacity(16);
        for byte in &hash[..8] {
            s.push_str(&format!("{byte:02x}"));
        }
        s
    }

    /// Compute the canonical body hash. We hash the raw bytes the
    /// client sent — adopters submitting JSON with whitespace
    /// differences will hash DIFFERENTLY, which is the safer default
    /// (the client must canonicalize on its side if it wants
    /// semantic equality). Matches Stripe's behavior.
    pub fn hash_body(body: &[u8]) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(body);
        h.finalize().into()
    }

    /// Look up a cached entry. Three-way verdict:
    ///   - Miss: no entry for this key.
    ///   - Hit(entry): entry found, body hash matches — replay.
    ///   - Conflict: entry found, body hash MISMATCH — return 422.
    /// Expired entries are evicted lazily and reported as Miss.
    pub fn lookup(
        &mut self,
        key: &IdempotencyCacheKey,
        request_body_hash: &[u8; 32],
    ) -> IdempotencyVerdict {
        let now = Instant::now();
        let entry = match self.entries.get(key) {
            Some(e) => e.clone(),
            None => return IdempotencyVerdict::Miss,
        };
        if now.duration_since(entry.inserted_at) > self.retention {
            self.entries.remove(key);
            return IdempotencyVerdict::Miss;
        }
        if &entry.request_body_hash == request_body_hash {
            IdempotencyVerdict::Hit(entry)
        } else {
            IdempotencyVerdict::Conflict {
                cached_body_hash_hex: Self::hash_prefix_hex(&entry.request_body_hash),
            }
        }
    }

    /// Insert (or overwrite) an entry. Caller is responsible for
    /// only caching successful responses (the gate in
    /// `dynamic_endpoint_handler` only caches 2xx — preserving the
    /// semantic that retries genuinely retry execution on failure).
    pub fn insert(&mut self, key: IdempotencyCacheKey, entry: IdempotencyEntry) {
        // Evict if at capacity (oldest entry first).
        if self.entries.len() >= self.capacity && !self.entries.contains_key(&key) {
            // Find the oldest entry. Linear scan is acceptable at
            // the default capacity (10k); for larger stores a BTree
            // by insertion-time would replace this.
            if let Some(oldest_key) = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.inserted_at)
                .map(|(k, _)| k.clone())
            {
                self.entries.remove(&oldest_key);
            }
        }
        self.entries.insert(key, entry);
    }

    /// Sweep expired entries. Returns the number reaped. Intended to
    /// be called periodically by a server task to bound memory.
    pub fn reap_expired(&mut self) -> usize {
        let now = Instant::now();
        let before = self.entries.len();
        let retention = self.retention;
        self.entries
            .retain(|_, e| now.duration_since(e.inserted_at) <= retention);
        before - self.entries.len()
    }

    /// Reconfigure retention (for tests + per-endpoint future tuning).
    pub fn set_retention(&mut self, retention: Duration) {
        self.retention = retention;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(c: &str, p: &str, k: &str) -> IdempotencyCacheKey {
        IdempotencyCacheKey {
            client_id: c.to_string(),
            endpoint_path: p.to_string(),
            idempotency_key: k.to_string(),
        }
    }

    fn entry(body: &str, status: u16) -> (IdempotencyEntry, [u8; 32]) {
        let body_bytes = body.as_bytes().to_vec();
        let hash = IdempotencyStore::hash_body(&body_bytes);
        (
            IdempotencyEntry {
                request_body_hash: hash,
                status,
                content_type: "application/json".to_string(),
                body: body_bytes,
                inserted_at: Instant::now(),
            },
            hash,
        )
    }

    #[test]
    fn miss_on_empty_store() {
        let mut s = IdempotencyStore::default();
        let h = IdempotencyStore::hash_body(b"{}");
        assert!(matches!(
            s.lookup(&key("c1", "/p", "k1"), &h),
            IdempotencyVerdict::Miss
        ));
    }

    #[test]
    fn hit_on_same_key_and_body() {
        let mut s = IdempotencyStore::default();
        let (e, h) = entry("{\"amount\":42}", 200);
        s.insert(key("c1", "/p", "k1"), e.clone());
        let verdict = s.lookup(&key("c1", "/p", "k1"), &h);
        match verdict {
            IdempotencyVerdict::Hit(got) => {
                assert_eq!(got.status, 200);
                assert_eq!(got.body, e.body);
            }
            _ => panic!("expected Hit"),
        }
    }

    #[test]
    fn conflict_on_same_key_different_body() {
        let mut s = IdempotencyStore::default();
        let (e, _h) = entry("{\"amount\":42}", 200);
        s.insert(key("c1", "/p", "k1"), e);
        let h_other = IdempotencyStore::hash_body(b"{\"amount\":99}");
        match s.lookup(&key("c1", "/p", "k1"), &h_other) {
            IdempotencyVerdict::Conflict { cached_body_hash_hex } => {
                assert_eq!(cached_body_hash_hex.len(), 16);
            }
            _ => panic!("expected Conflict"),
        }
    }

    #[test]
    fn cross_tenant_isolation() {
        let mut s = IdempotencyStore::default();
        let (e, h) = entry("{\"x\":1}", 200);
        s.insert(key("c1", "/p", "k1"), e);
        // Different client_id, same key — must be a miss.
        assert!(matches!(
            s.lookup(&key("c2", "/p", "k1"), &h),
            IdempotencyVerdict::Miss
        ));
        // Different path, same key — must be a miss.
        assert!(matches!(
            s.lookup(&key("c1", "/other", "k1"), &h),
            IdempotencyVerdict::Miss
        ));
    }

    #[test]
    fn retention_expiry_evicts_old_entry() {
        let mut s = IdempotencyStore::new(10, Duration::from_millis(0));
        let (e, h) = entry("{}", 200);
        s.insert(key("c1", "/p", "k1"), e);
        std::thread::sleep(Duration::from_millis(2));
        assert!(matches!(
            s.lookup(&key("c1", "/p", "k1"), &h),
            IdempotencyVerdict::Miss
        ));
        // Eviction happens during lookup — store should be empty.
        assert_eq!(s.len(), 0);
    }

    #[test]
    fn reap_expired_returns_count() {
        let mut s = IdempotencyStore::new(10, Duration::from_millis(0));
        let (e1, _) = entry("{\"a\":1}", 200);
        let (e2, _) = entry("{\"a\":2}", 200);
        s.insert(key("c1", "/p", "k1"), e1);
        s.insert(key("c1", "/p", "k2"), e2);
        assert_eq!(s.len(), 2);
        std::thread::sleep(Duration::from_millis(2));
        assert_eq!(s.reap_expired(), 2);
        assert_eq!(s.len(), 0);
    }

    #[test]
    fn capacity_eviction_drops_oldest_on_overflow() {
        let mut s = IdempotencyStore::new(2, DEFAULT_RETENTION);
        let (e1, h1) = entry("{\"a\":1}", 200);
        s.insert(key("c1", "/p", "k1"), e1);
        std::thread::sleep(Duration::from_millis(1));
        let (e2, _) = entry("{\"a\":2}", 200);
        s.insert(key("c1", "/p", "k2"), e2);
        std::thread::sleep(Duration::from_millis(1));
        let (e3, _) = entry("{\"a\":3}", 200);
        s.insert(key("c1", "/p", "k3"), e3);
        assert_eq!(s.len(), 2);
        // k1 (oldest) was evicted.
        assert!(matches!(
            s.lookup(&key("c1", "/p", "k1"), &h1),
            IdempotencyVerdict::Miss
        ));
    }

    #[test]
    fn hash_prefix_hex_is_16_chars_lowercase() {
        let h = IdempotencyStore::hash_body(b"hello");
        let prefix = IdempotencyStore::hash_prefix_hex(&h);
        assert_eq!(prefix.len(), 16);
        for c in prefix.chars() {
            assert!(c.is_ascii_hexdigit() && !c.is_ascii_uppercase());
        }
    }

    #[test]
    fn hash_body_deterministic() {
        // Same bytes ⟹ same hash. The fundamental invariant.
        let a = IdempotencyStore::hash_body(b"{\"x\":1}");
        let b = IdempotencyStore::hash_body(b"{\"x\":1}");
        assert_eq!(a, b);
    }

    #[test]
    fn hash_body_sensitive_to_whitespace() {
        // Whitespace differences hash differently — adopters who want
        // semantic equality must canonicalize on the client. Matches
        // Stripe's documented behavior.
        let a = IdempotencyStore::hash_body(b"{\"x\":1}");
        let b = IdempotencyStore::hash_body(b"{ \"x\": 1 }");
        assert_ne!(a, b);
    }
}
