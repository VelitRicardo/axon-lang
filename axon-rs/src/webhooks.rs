//! Webhooks — outgoing HTTP notification system for AxonServer events.
//!
//! Registers webhook endpoints that receive POST notifications when matching
//! events occur on the EventBus. Each webhook has:
//!   - URL target
//!   - Topic filters (exact or prefix match via `*`)
//!   - Optional HMAC-SHA256 secret for payload signing
//!   - Active/inactive toggle
//!   - Delivery history with retry tracking
//!
//! Webhooks are dispatched synchronously (recorded for later async delivery).
//! The registry manages CRUD operations and delivery logging.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

// ── Types ────────────────────────────────────────────────────────────────

/// A registered webhook configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    /// Unique identifier.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Target URL to POST events to.
    pub url: String,
    /// Topic filter patterns (e.g., "deploy", "daemon.*", "*").
    pub events: Vec<String>,
    /// Optional HMAC-SHA256 secret for signing payloads.
    #[serde(skip_serializing)]
    pub secret: Option<String>,
    /// Whether this webhook is active.
    pub active: bool,
    /// Creation timestamp (Unix seconds).
    pub created_at: u64,
    /// Total deliveries attempted.
    pub delivery_count: u64,
    /// Total delivery failures.
    pub failure_count: u64,
    /// Last delivery timestamp (Unix seconds).
    pub last_delivery: Option<u64>,
    /// Optional payload template with variable substitution.
    /// Variables: {{topic}}, {{timestamp}}, {{source}}, {{payload}}, {{webhook_name}}, {{webhook_id}}.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
}

/// A webhook delivery record.
#[derive(Debug, Clone, Serialize)]
pub struct WebhookDelivery {
    /// Webhook ID this delivery belongs to.
    pub webhook_id: String,
    /// Event topic that triggered the delivery.
    pub topic: String,
    /// HTTP status code (0 if connection failed).
    pub status_code: u16,
    /// Whether the delivery was successful (2xx).
    pub success: bool,
    /// Delivery timestamp (Unix seconds).
    pub timestamp: u64,
    /// Latency in milliseconds (0 if not delivered).
    pub latency_ms: u64,
    /// Error message if delivery failed.
    pub error: Option<String>,
    /// Retry attempt number (0 = first attempt).
    pub attempt: u32,
}

/// Summary of a webhook (for listing, secret masked).
#[derive(Debug, Clone, Serialize)]
pub struct WebhookSummary {
    pub id: String,
    pub name: String,
    pub url: String,
    pub events: Vec<String>,
    pub has_secret: bool,
    pub active: bool,
    pub created_at: u64,
    pub delivery_count: u64,
    pub failure_count: u64,
    pub last_delivery: Option<u64>,
}

/// Result of dispatching an event to webhooks.
#[derive(Debug, Clone, Serialize)]
pub struct DispatchResult {
    /// Number of webhooks that matched the event.
    pub matched: usize,
    /// Webhook IDs that matched.
    pub webhook_ids: Vec<String>,
}

/// Aggregate statistics across all webhooks.
#[derive(Debug, Clone, Serialize)]
pub struct WebhookStats {
    pub total_webhooks: usize,
    pub active_webhooks: usize,
    pub total_deliveries: u64,
    pub total_failures: u64,
    pub recent_deliveries: Vec<WebhookDelivery>,
}

/// A failed delivery queued for retry with exponential backoff.
#[derive(Debug, Clone, Serialize)]
pub struct RetryEntry {
    /// Webhook ID.
    pub webhook_id: String,
    /// Event topic.
    pub topic: String,
    /// Current retry attempt (starts at 1).
    pub attempt: u32,
    /// Maximum retry attempts before dead-lettering.
    pub max_attempts: u32,
    /// Unix timestamp of next retry.
    pub next_retry_at: u64,
    /// Original failure error.
    pub original_error: String,
    /// Unix timestamp when first enqueued.
    pub enqueued_at: u64,
}

/// A permanently failed delivery (exceeded max retries).
#[derive(Debug, Clone, Serialize)]
pub struct DeadLetterEntry {
    /// Webhook ID.
    pub webhook_id: String,
    /// Event topic.
    pub topic: String,
    /// Total attempts made.
    pub attempts: u32,
    /// Last error message.
    pub last_error: String,
    /// Unix timestamp when dead-lettered.
    pub dead_at: u64,
}

// ── Registry ────────────────────────────────────────────────────────────

/// Webhook registry — manages webhook CRUD and delivery logging.
pub struct WebhookRegistry {
    /// Registered webhooks by ID.
    webhooks: HashMap<String, WebhookConfig>,
    /// Recent delivery log (ring buffer).
    deliveries: Vec<WebhookDelivery>,
    /// Max delivery log entries.
    max_deliveries: usize,
    /// Auto-increment counter for ID generation.
    next_id: u64,
    /// Retry queue for failed deliveries.
    retry_queue: Vec<RetryEntry>,
    /// Dead letter queue for permanently failed deliveries.
    dead_letters: Vec<DeadLetterEntry>,
    /// Max retry attempts before dead-lettering (default 5).
    pub max_retry_attempts: u32,
}

impl WebhookRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        WebhookRegistry {
            webhooks: HashMap::new(),
            deliveries: Vec::new(),
            max_deliveries: 500,
            next_id: 1,
            retry_queue: Vec::new(),
            dead_letters: Vec::new(),
            max_retry_attempts: 5,
        }
    }

    /// Register a new webhook. Returns the assigned ID.
    pub fn register(
        &mut self,
        name: &str,
        url: &str,
        events: Vec<String>,
        secret: Option<String>,
    ) -> String {
        self.register_with_template(name, url, events, secret, None)
    }

    /// Register a new webhook with optional payload template. Returns the assigned ID.
    pub fn register_with_template(
        &mut self,
        name: &str,
        url: &str,
        events: Vec<String>,
        secret: Option<String>,
        template: Option<String>,
    ) -> String {
        let id = format!("wh_{}", self.next_id);
        self.next_id += 1;

        let config = WebhookConfig {
            id: id.clone(),
            name: name.to_string(),
            url: url.to_string(),
            events,
            secret,
            active: true,
            created_at: now_secs(),
            delivery_count: 0,
            failure_count: 0,
            last_delivery: None,
            template,
        };

        self.webhooks.insert(id.clone(), config);
        id
    }

    /// Unregister a webhook by ID. Returns true if found and removed.
    pub fn unregister(&mut self, id: &str) -> bool {
        self.webhooks.remove(id).is_some()
    }

    /// Get a webhook by ID.
    pub fn get(&self, id: &str) -> Option<&WebhookConfig> {
        self.webhooks.get(id)
    }

    /// Toggle active state. Returns new state if found.
    pub fn toggle(&mut self, id: &str) -> Option<bool> {
        match self.webhooks.get_mut(id) {
            Some(wh) => {
                wh.active = !wh.active;
                Some(wh.active)
            }
            None => None,
        }
    }

    /// Get the event filters for a webhook.
    pub fn get_filters(&self, id: &str) -> Option<&Vec<String>> {
        self.webhooks.get(id).map(|wh| &wh.events)
    }

    /// Set the event filters for a webhook. Returns true if found.
    pub fn set_filters(&mut self, id: &str, events: Vec<String>) -> bool {
        match self.webhooks.get_mut(id) {
            Some(wh) => { wh.events = events; true }
            None => false,
        }
    }

    /// List all webhooks as summaries (secrets masked).
    pub fn list(&self) -> Vec<WebhookSummary> {
        let mut result: Vec<WebhookSummary> = self.webhooks.values().map(|wh| {
            WebhookSummary {
                id: wh.id.clone(),
                name: wh.name.clone(),
                url: wh.url.clone(),
                events: wh.events.clone(),
                has_secret: wh.secret.is_some(),
                active: wh.active,
                created_at: wh.created_at,
                delivery_count: wh.delivery_count,
                failure_count: wh.failure_count,
                last_delivery: wh.last_delivery,
            }
        }).collect();
        result.sort_by(|a, b| a.id.cmp(&b.id));
        result
    }

    /// Check which webhooks match a given event topic.
    /// Returns IDs of matching active webhooks.
    pub fn match_topic(&self, topic: &str) -> Vec<String> {
        self.webhooks.values()
            .filter(|wh| wh.active && topic_matches(&wh.events, topic))
            .map(|wh| wh.id.clone())
            .collect()
    }

    /// Dispatch an event: find matching webhooks and record pending deliveries.
    /// Returns dispatch result with matched webhook IDs.
    /// Actual HTTP delivery is NOT performed here (that's async/external).
    pub fn dispatch(&mut self, topic: &str, _payload: &serde_json::Value, _source: &str) -> DispatchResult {
        let matching_ids = self.match_topic(topic);

        for id in &matching_ids {
            if let Some(wh) = self.webhooks.get_mut(id) {
                wh.delivery_count += 1;
                wh.last_delivery = Some(now_secs());
            }

            // Record delivery as pending (status 0 = not yet sent)
            let delivery = WebhookDelivery {
                webhook_id: id.clone(),
                topic: topic.to_string(),
                status_code: 0,
                success: false,
                timestamp: now_secs(),
                latency_ms: 0,
                error: Some("pending".to_string()),
                attempt: 0,
            };
            self.record_delivery(delivery);
        }

        DispatchResult {
            matched: matching_ids.len(),
            webhook_ids: matching_ids,
        }
    }

    /// Record a delivery result (used after actual HTTP attempt).
    pub fn record_delivery(&mut self, delivery: WebhookDelivery) {
        // Update webhook stats
        if !delivery.success && delivery.error.as_deref() != Some("pending") {
            if let Some(wh) = self.webhooks.get_mut(&delivery.webhook_id) {
                wh.failure_count += 1;
            }
        }

        self.deliveries.push(delivery);

        // Trim delivery log
        while self.deliveries.len() > self.max_deliveries {
            self.deliveries.remove(0);
        }
    }

    /// Record a completed delivery (success or failure after HTTP attempt).
    pub fn record_completed(
        &mut self,
        webhook_id: &str,
        topic: &str,
        status_code: u16,
        latency_ms: u64,
        error: Option<String>,
        attempt: u32,
    ) {
        let success = (200..300).contains(&status_code);
        if !success {
            if let Some(wh) = self.webhooks.get_mut(webhook_id) {
                wh.failure_count += 1;
            }
        }

        let delivery = WebhookDelivery {
            webhook_id: webhook_id.to_string(),
            topic: topic.to_string(),
            status_code,
            success,
            timestamp: now_secs(),
            latency_ms,
            error,
            attempt,
        };
        self.deliveries.push(delivery);

        while self.deliveries.len() > self.max_deliveries {
            self.deliveries.remove(0);
        }
    }

    /// Get recent deliveries, optionally filtered by webhook ID.
    pub fn recent_deliveries(&self, limit: usize, webhook_id: Option<&str>) -> Vec<&WebhookDelivery> {
        self.deliveries.iter().rev()
            .filter(|d| match webhook_id {
                Some(id) => d.webhook_id == id,
                None => true,
            })
            .take(limit)
            .collect()
    }

    /// Aggregate statistics.
    pub fn stats(&self) -> WebhookStats {
        let total_deliveries: u64 = self.webhooks.values().map(|w| w.delivery_count).sum();
        let total_failures: u64 = self.webhooks.values().map(|w| w.failure_count).sum();
        let recent: Vec<WebhookDelivery> = self.deliveries.iter().rev()
            .take(10)
            .cloned()
            .collect();

        WebhookStats {
            total_webhooks: self.webhooks.len(),
            active_webhooks: self.webhooks.values().filter(|w| w.active).count(),
            total_deliveries,
            total_failures,
            recent_deliveries: recent,
        }
    }

    /// Number of registered webhooks.
    pub fn count(&self) -> usize {
        self.webhooks.len()
    }

    /// Number of active webhooks.
    pub fn active_count(&self) -> usize {
        self.webhooks.values().filter(|w| w.active).count()
    }

    /// Enqueue a failed delivery for retry with exponential backoff.
    /// Returns true if enqueued, false if max attempts exceeded (dead-lettered).
    pub fn enqueue_retry(&mut self, webhook_id: &str, topic: &str, error: &str, attempt: u32) -> bool {
        let now = now_secs();
        if attempt >= self.max_retry_attempts {
            // Dead-letter it
            self.dead_letters.push(DeadLetterEntry {
                webhook_id: webhook_id.to_string(),
                topic: topic.to_string(),
                attempts: attempt,
                last_error: error.to_string(),
                dead_at: now,
            });
            if self.dead_letters.len() > 200 {
                self.dead_letters.remove(0);
            }
            return false;
        }

        // Exponential backoff: 2^attempt seconds (1, 2, 4, 8, 16, ...)
        let backoff_secs = 1u64 << attempt;
        self.retry_queue.push(RetryEntry {
            webhook_id: webhook_id.to_string(),
            topic: topic.to_string(),
            attempt: attempt + 1,
            max_attempts: self.max_retry_attempts,
            next_retry_at: now + backoff_secs,
            original_error: error.to_string(),
            enqueued_at: now,
        });
        true
    }

    /// Get due retries (next_retry_at <= now). Removes them from the queue.
    pub fn drain_due_retries(&mut self) -> Vec<RetryEntry> {
        let now = now_secs();
        let (due, remaining): (Vec<_>, Vec<_>) = self.retry_queue
            .drain(..)
            .partition(|r| r.next_retry_at <= now);
        self.retry_queue = remaining;
        due
    }

    /// View the retry queue (read-only).
    pub fn retry_queue(&self) -> &[RetryEntry] {
        &self.retry_queue
    }

    /// View the dead letter queue (read-only).
    pub fn dead_letters(&self) -> &[DeadLetterEntry] {
        &self.dead_letters
    }

    /// Number of entries in the retry queue.
    pub fn retry_queue_len(&self) -> usize {
        self.retry_queue.len()
    }

    /// Number of entries in the dead letter queue.
    pub fn dead_letters_len(&self) -> usize {
        self.dead_letters.len()
    }

    /// Compute the **HMAC-SHA256** signature of a payload, hex-encoded
    /// and prefixed `sha256=` — the GitHub / Stripe webhook-signature
    /// convention that every off-the-shelf verification library
    /// (`hmac` in Python, `crypto.createHmac` in Node, etc.) expects.
    ///
    /// - `secret` — the webhook's shared secret, used verbatim as the
    ///   HMAC key. HMAC accepts a key of any length (RFC 2104 hashes
    ///   keys longer than the block size + zero-pads shorter ones), so
    ///   no key-length precondition is imposed on adopters.
    /// - `payload` — the EXACT request-body bytes the receiver will
    ///   see; the receiver recomputes `HMAC-SHA256(secret, body)` and
    ///   compares. Use [`Self::verify_signature`] for the receiving
    ///   side — it does the comparison in constant time.
    ///
    /// The output is `"sha256="` followed by 64 lowercase hex digits
    /// (256-bit digest). This is a real keyed MAC: without the secret
    /// an attacker cannot forge a signature for a chosen payload.
    pub fn compute_signature(secret: &str, payload: &[u8]) -> String {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        use std::fmt::Write as _;

        // `new_from_slice` is infallible for HMAC — it accepts a key
        // of any length per RFC 2104. The `InvalidLength` error
        // variant exists only for fixed-key MACs; HMAC never returns
        // it, so the `expect` is unreachable by construction.
        let mut mac = <Hmac<Sha256>>::new_from_slice(secret.as_bytes())
            .expect("HMAC-SHA256 accepts a key of any length (RFC 2104)");
        mac.update(payload);
        let digest = mac.finalize().into_bytes();

        let mut out = String::with_capacity("sha256=".len() + digest.len() * 2);
        out.push_str("sha256=");
        for byte in digest {
            let _ = write!(out, "{byte:02x}");
        }
        out
    }

    /// Verify a webhook signature against a payload, in **constant
    /// time**.
    ///
    /// Recomputes `HMAC-SHA256(secret, payload)` and compares it
    /// against `provided` — the value an inbound caller supplied
    /// (e.g. an `X-Axon-Signature` header) — using a constant-time
    /// equality check. The constant-time compare denies a network
    /// attacker the timing side-channel that a byte-by-byte
    /// `==` comparison would leak (which would let them recover the
    /// correct signature one byte at a time).
    ///
    /// Returns `true` iff `provided` is exactly the correct
    /// signature. A length mismatch returns `false` immediately —
    /// the signature LENGTH is public (it is always
    /// `"sha256=" + 64 hex`), so short-circuiting on it leaks
    /// nothing secret.
    pub fn verify_signature(secret: &str, payload: &[u8], provided: &str) -> bool {
        use subtle::ConstantTimeEq;

        let expected = Self::compute_signature(secret, payload);
        if expected.len() != provided.len() {
            return false;
        }
        expected.as_bytes().ct_eq(provided.as_bytes()).into()
    }

    /// Set a payload template for a webhook. Returns true if webhook found.
    pub fn set_template(&mut self, id: &str, template: Option<String>) -> bool {
        match self.webhooks.get_mut(id) {
            Some(wh) => { wh.template = template; true }
            None => false,
        }
    }

    /// Get the payload template for a webhook.
    pub fn get_template(&self, id: &str) -> Option<Option<&str>> {
        self.webhooks.get(id).map(|wh| wh.template.as_deref())
    }

    /// Render a payload using a webhook's template (if set).
    pub fn render_payload(&self, webhook_id: &str, topic: &str, payload: &serde_json::Value, source: &str) -> serde_json::Value {
        match self.webhooks.get(webhook_id) {
            Some(wh) => match &wh.template {
                Some(tmpl) => {
                    let rendered = render_template(tmpl, topic, payload, source, &wh.name, &wh.id);
                    serde_json::from_str(&rendered).unwrap_or_else(|_| serde_json::json!({
                        "rendered": rendered,
                    }))
                }
                None => serde_json::json!({
                    "topic": topic,
                    "payload": payload,
                    "source": source,
                    "timestamp": now_secs(),
                }),
            }
            None => serde_json::json!({
                "topic": topic,
                "payload": payload,
                "source": source,
            }),
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Render a template string with variable substitution.
///
/// Supported variables: {{topic}}, {{timestamp}}, {{source}}, {{payload}},
/// {{webhook_name}}, {{webhook_id}}.
pub fn render_template(
    template: &str,
    topic: &str,
    payload: &serde_json::Value,
    source: &str,
    webhook_name: &str,
    webhook_id: &str,
) -> String {
    let payload_str = serde_json::to_string(payload).unwrap_or_default();
    template
        .replace("{{topic}}", topic)
        .replace("{{timestamp}}", &now_secs().to_string())
        .replace("{{source}}", source)
        .replace("{{payload}}", &payload_str)
        .replace("{{webhook_name}}", webhook_name)
        .replace("{{webhook_id}}", webhook_id)
}

/// Check if any event filter matches a topic.
fn topic_matches(filters: &[String], topic: &str) -> bool {
    filters.iter().any(|f| {
        if f == "*" {
            true
        } else if let Some(prefix) = f.strip_suffix(".*") {
            topic.starts_with(prefix) && (topic.len() == prefix.len() || topic.as_bytes().get(prefix.len()) == Some(&b'.'))
        } else {
            f == topic
        }
    })
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_list() {
        let mut reg = WebhookRegistry::new();
        let id = reg.register("deploy-notify", "https://example.com/hook", vec!["deploy".into()], None);
        assert_eq!(id, "wh_1");
        assert_eq!(reg.count(), 1);

        let list = reg.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "deploy-notify");
        assert_eq!(list[0].url, "https://example.com/hook");
        assert!(!list[0].has_secret);
        assert!(list[0].active);
    }

    #[test]
    fn register_with_secret() {
        let mut reg = WebhookRegistry::new();
        reg.register("secure", "https://example.com", vec!["*".into()], Some("mysecret".into()));

        let list = reg.list();
        assert!(list[0].has_secret);
        // Secret should not appear in summary
        let json = serde_json::to_value(&list[0]).unwrap();
        assert!(json.get("secret").is_none());
    }

    #[test]
    fn unregister() {
        let mut reg = WebhookRegistry::new();
        let id = reg.register("temp", "https://temp.com", vec!["*".into()], None);
        assert_eq!(reg.count(), 1);

        assert!(reg.unregister(&id));
        assert_eq!(reg.count(), 0);
        assert!(!reg.unregister(&id)); // already removed
    }

    #[test]
    fn toggle_active() {
        let mut reg = WebhookRegistry::new();
        let id = reg.register("toggler", "https://t.com", vec!["*".into()], None);

        assert_eq!(reg.toggle(&id), Some(false)); // was true, now false
        assert_eq!(reg.toggle(&id), Some(true));  // back to true
        assert_eq!(reg.toggle("nonexistent"), None);
    }

    #[test]
    fn topic_matching_exact() {
        let mut reg = WebhookRegistry::new();
        reg.register("deploy-only", "https://d.com", vec!["deploy".into()], None);

        assert_eq!(reg.match_topic("deploy").len(), 1);
        assert_eq!(reg.match_topic("deploy.success").len(), 0);
        assert_eq!(reg.match_topic("other").len(), 0);
    }

    #[test]
    fn topic_matching_prefix() {
        let mut reg = WebhookRegistry::new();
        reg.register("daemon-watcher", "https://d.com", vec!["daemon.*".into()], None);

        assert_eq!(reg.match_topic("daemon.started").len(), 1);
        assert_eq!(reg.match_topic("daemon.stopped").len(), 1);
        assert_eq!(reg.match_topic("daemon").len(), 1); // "daemon" matches "daemon.*" (prefix includes exact)
        assert_eq!(reg.match_topic("deploy").len(), 0);
    }

    #[test]
    fn topic_matching_wildcard() {
        let mut reg = WebhookRegistry::new();
        reg.register("catch-all", "https://a.com", vec!["*".into()], None);

        assert_eq!(reg.match_topic("deploy").len(), 1);
        assert_eq!(reg.match_topic("daemon.crashed").len(), 1);
        assert_eq!(reg.match_topic("anything").len(), 1);
    }

    #[test]
    fn topic_matching_multiple_filters() {
        let mut reg = WebhookRegistry::new();
        reg.register("multi", "https://m.com", vec!["deploy".into(), "config.*".into()], None);

        assert_eq!(reg.match_topic("deploy").len(), 1);
        assert_eq!(reg.match_topic("config.updated").len(), 1);
        assert_eq!(reg.match_topic("daemon.started").len(), 0);
    }

    #[test]
    fn inactive_webhook_not_matched() {
        let mut reg = WebhookRegistry::new();
        let id = reg.register("inactive", "https://i.com", vec!["*".into()], None);
        reg.toggle(&id); // deactivate

        assert_eq!(reg.match_topic("deploy").len(), 0);
    }

    #[test]
    fn dispatch_records_deliveries() {
        let mut reg = WebhookRegistry::new();
        reg.register("a", "https://a.com", vec!["deploy".into()], None);
        reg.register("b", "https://b.com", vec!["*".into()], None);

        let result = reg.dispatch("deploy", &serde_json::json!({"flow": "F1"}), "server");
        assert_eq!(result.matched, 2);
        assert_eq!(result.webhook_ids.len(), 2);

        // Delivery count incremented
        let list = reg.list();
        for wh in &list {
            assert_eq!(wh.delivery_count, 1);
            assert!(wh.last_delivery.is_some());
        }
    }

    #[test]
    fn dispatch_non_matching_topic() {
        let mut reg = WebhookRegistry::new();
        reg.register("deploy-only", "https://d.com", vec!["deploy".into()], None);

        let result = reg.dispatch("config.updated", &serde_json::json!({}), "server");
        assert_eq!(result.matched, 0);
        assert!(result.webhook_ids.is_empty());
    }

    #[test]
    fn record_completed_delivery() {
        let mut reg = WebhookRegistry::new();
        let id = reg.register("test", "https://t.com", vec!["*".into()], None);

        reg.record_completed(&id, "deploy", 200, 45, None, 0);
        reg.record_completed(&id, "deploy", 500, 120, Some("server error".into()), 0);

        let deliveries = reg.recent_deliveries(10, Some(&id));
        assert_eq!(deliveries.len(), 2);
        assert!(deliveries[0].success == false); // 500 is newest (reversed)
        assert!(deliveries[1].success == true);  // 200

        let stats = reg.stats();
        assert_eq!(stats.total_failures, 1);
    }

    #[test]
    fn recent_deliveries_filtered() {
        let mut reg = WebhookRegistry::new();
        let id1 = reg.register("a", "https://a.com", vec!["*".into()], None);
        let id2 = reg.register("b", "https://b.com", vec!["*".into()], None);

        reg.record_completed(&id1, "deploy", 200, 10, None, 0);
        reg.record_completed(&id2, "config", 200, 20, None, 0);
        reg.record_completed(&id1, "deploy", 201, 15, None, 0);

        let all = reg.recent_deliveries(10, None);
        assert_eq!(all.len(), 3);

        let a_only = reg.recent_deliveries(10, Some(&id1));
        assert_eq!(a_only.len(), 2);

        let b_only = reg.recent_deliveries(10, Some(&id2));
        assert_eq!(b_only.len(), 1);
    }

    #[test]
    fn stats_aggregation() {
        let mut reg = WebhookRegistry::new();
        let id = reg.register("stats-test", "https://s.com", vec!["*".into()], None);

        reg.dispatch("deploy", &serde_json::json!({}), "server");
        reg.dispatch("config", &serde_json::json!({}), "server");
        reg.record_completed(&id, "error", 500, 100, Some("fail".into()), 0);

        let stats = reg.stats();
        assert_eq!(stats.total_webhooks, 1);
        assert_eq!(stats.active_webhooks, 1);
        assert_eq!(stats.total_deliveries, 2);
        assert_eq!(stats.total_failures, 1);
        assert!(stats.recent_deliveries.len() >= 2);
    }

    #[test]
    fn auto_increment_ids() {
        let mut reg = WebhookRegistry::new();
        let id1 = reg.register("a", "https://a.com", vec!["*".into()], None);
        let id2 = reg.register("b", "https://b.com", vec!["*".into()], None);
        let id3 = reg.register("c", "https://c.com", vec!["*".into()], None);

        assert_eq!(id1, "wh_1");
        assert_eq!(id2, "wh_2");
        assert_eq!(id3, "wh_3");
    }

    #[test]
    fn compute_signature_deterministic() {
        let sig1 = WebhookRegistry::compute_signature("secret", b"payload");
        let sig2 = WebhookRegistry::compute_signature("secret", b"payload");
        assert_eq!(sig1, sig2);
        assert!(sig1.starts_with("sha256="));

        // Different secret produces a different signature.
        let sig3 = WebhookRegistry::compute_signature("other", b"payload");
        assert_ne!(sig1, sig3);

        // Different payload produces a different signature.
        let sig4 = WebhookRegistry::compute_signature("secret", b"payload2");
        assert_ne!(sig1, sig4);
    }

    #[test]
    fn compute_signature_emits_256_bit_digest() {
        // `sha256=` + 64 lowercase hex digits = a real 256-bit HMAC
        // digest. The pre-fix FNV-64 implementation emitted only 16
        // hex digits (64 bits) — this pins the regression shut.
        let sig = WebhookRegistry::compute_signature("k", b"body");
        let hex = sig.strip_prefix("sha256=").expect("sha256= prefix");
        assert_eq!(hex.len(), 64, "HMAC-SHA256 digest is 256 bits / 64 hex");
        assert!(
            hex.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "digest must be lowercase hex"
        );
    }

    #[test]
    fn compute_signature_matches_canonical_hmac_sha256_vectors() {
        // Known-answer tests against published HMAC-SHA256 vectors —
        // the proof that this is a REAL HMAC, not a look-alike hash.

        // Vector 1 — the widely-published Wikipedia HMAC example:
        //   HMAC-SHA256(key="key", "The quick brown fox jumps over the lazy dog")
        assert_eq!(
            WebhookRegistry::compute_signature(
                "key",
                b"The quick brown fox jumps over the lazy dog",
            ),
            "sha256=f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8",
        );

        // Vector 2 — RFC 4231 section 4.3 Test Case 2 (printable key + data):
        //   HMAC-SHA256(key="Jefe", "what do ya want for nothing?")
        assert_eq!(
            WebhookRegistry::compute_signature("Jefe", b"what do ya want for nothing?"),
            "sha256=5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843",
        );
    }

    #[test]
    fn verify_signature_accepts_correct_and_rejects_forgeries() {
        let secret = "webhook-shared-secret";
        let payload = br#"{"event":"deploy","ok":true}"#;
        let good = WebhookRegistry::compute_signature(secret, payload);

        // Correct (secret, payload, signature) triple verifies.
        assert!(WebhookRegistry::verify_signature(secret, payload, &good));

        // Tampered payload — signature no longer matches.
        assert!(!WebhookRegistry::verify_signature(
            secret,
            br#"{"event":"deploy","ok":false}"#,
            &good,
        ));

        // Wrong secret — cannot reproduce the signature.
        assert!(!WebhookRegistry::verify_signature(
            "wrong-secret",
            payload,
            &good,
        ));

        // Garbage / wrong-length signature strings are rejected
        // without panicking.
        assert!(!WebhookRegistry::verify_signature(secret, payload, ""));
        assert!(!WebhookRegistry::verify_signature(secret, payload, "sha256=deadbeef"));
        assert!(!WebhookRegistry::verify_signature(
            secret,
            payload,
            "not-even-a-signature",
        ));
    }

    #[test]
    fn summary_serializes() {
        let mut reg = WebhookRegistry::new();
        reg.register("ser-test", "https://s.com", vec!["deploy".into(), "config.*".into()], Some("secret".into()));

        let list = reg.list();
        let json = serde_json::to_value(&list[0]).unwrap();
        assert_eq!(json["name"], "ser-test");
        assert_eq!(json["url"], "https://s.com");
        assert_eq!(json["events"].as_array().unwrap().len(), 2);
        assert_eq!(json["has_secret"], true);
        assert_eq!(json["active"], true);
        assert!(json.get("secret").is_none()); // not in summary
    }

    #[test]
    fn delivery_log_trimmed() {
        let mut reg = WebhookRegistry::new();
        reg.max_deliveries = 5;
        let id = reg.register("trim", "https://t.com", vec!["*".into()], None);

        for i in 0..10 {
            reg.record_completed(&id, &format!("event_{i}"), 200, 10, None, 0);
        }

        assert_eq!(reg.deliveries.len(), 5);
        // Should keep the most recent
        assert_eq!(reg.deliveries.last().unwrap().topic, "event_9");
    }
}
