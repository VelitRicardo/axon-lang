//! Storage Backend — trait abstraction for persistent state in AxonServer.
//!
//! Defines `StorageBackend` — an async trait for persisting server state across
//! restarts. Two implementations:
//!   - `InMemoryBackend`: No-op — state lives only in memory (development/testing)
//!   - `PostgresBackend`: Full PostgreSQL persistence (production) — in `storage_postgres.rs`
//!
//! Architecture ready for future backends: Oracle, MariaDB, MySQL, etc.
//! Each backend simply implements this trait.
//!
//! The server uses write-through: in-memory state is the source of truth during
//! process lifetime; the backend provides durability across restarts.

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── Error type ─────────────────────────────────────────────────────────────

/// Storage operation error.
#[derive(Debug, Clone)]
pub enum StorageError {
    /// Database connection or pool error.
    ConnectionError(String),
    /// Query execution error.
    QueryError(String),
    /// Serialization/deserialization error.
    SerializationError(String),
    /// Requested entity not found.
    NotFound(String),
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::ConnectionError(s) => write!(f, "connection error: {s}"),
            StorageError::QueryError(s) => write!(f, "query error: {s}"),
            StorageError::SerializationError(s) => write!(f, "serialization error: {s}"),
            StorageError::NotFound(s) => write!(f, "not found: {s}"),
        }
    }
}

// ── Portable row types ─────────────────────────────────────────────────────

/// Portable trace record for storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceRow {
    pub tenant_id: String,
    pub trace_id: u64,
    pub flow_name: String,
    pub status: String,
    pub steps_executed: u32,
    pub latency_ms: u64,
    pub tokens_input: u64,
    pub tokens_output: u64,
    pub anchor_checks: u32,
    pub anchor_breaches: u32,
    pub errors: u32,
    pub retries: u32,
    pub source_file: String,
    pub backend: String,
    pub client_key: String,
    pub replay_of: Option<u64>,
    pub correlation_id: Option<String>,
    pub events: Value,
    pub annotations: Value,
}

/// Portable session entry for storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRow {
    pub tenant_id: String,
    pub scope: String,
    pub key: String,
    pub value: String,
    pub source_step: String,
}

/// Portable daemon record for storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonRow {
    pub tenant_id: String,
    pub name: String,
    pub state: String,
    pub source_file: String,
    pub flow_name: String,
    pub event_count: u64,
    pub restart_count: u32,
    pub trigger_topic: Option<String>,
    pub output_topic: Option<String>,
    pub lifecycle_events: Value,
}

/// Portable audit entry for storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRow {
    pub tenant_id: String,
    pub action: String,
    pub actor: String,
    pub target: String,
    pub detail: Value,
}

/// Portable AxonStore instance for storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AxonStoreRow {
    pub tenant_id: String,
    pub name: String,
    pub ontology: String,
    pub entries: Value,
    pub created_at: u64,
    pub total_ops: u64,
}

/// Portable Dataspace instance for storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataspaceRow {
    pub tenant_id: String,
    pub name: String,
    pub ontology: String,
    pub entries: Value,
    pub associations: Value,
    pub created_at: u64,
    pub total_ops: u64,
    pub next_id: u64,
}

/// Portable hibernation session for storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HibernationRow {
    pub tenant_id: String,
    pub id: String,
    pub name: String,
    pub operation: String,
    pub status: String,
    pub checkpoints: Value,
    pub resumed_from: Option<i32>,
    pub created_at: u64,
    pub last_status_change: u64,
    pub next_checkpoint_id: u32,
}

/// Portable event record for storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventRow {
    pub tenant_id: String,
    pub topic: String,
    pub source: String,
    pub payload: Value,
}

/// Portable cached execution result for storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheRow {
    pub tenant_id: String,
    pub flow_name: String,
    pub cache_key: String,
    pub result: Value,
    pub ttl_secs: Option<i32>,
    pub hit_count: u64,
}

/// Portable cost record for storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostRow {
    pub tenant_id: String,
    pub flow_name: String,
    pub backend: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
}

/// Portable schedule entry for storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleRow {
    pub tenant_id: String,
    pub name: String,
    pub flow_name: String,
    pub interval_secs: u64,
    pub enabled: bool,
    pub backend: String,
    pub last_run: u64,
    pub next_run: u64,
    pub run_count: u64,
    pub error_count: u64,
}

// ── Storage Backend Trait ──────────────────────────────────────────────────

/// Async trait for persistent storage backends.
///
/// All methods are async and return `Result<T, StorageError>`.
/// Implementations must be `Send + Sync` for use across async tasks.
pub trait StorageBackend: Send + Sync {
    // ── Traces ──
    fn save_trace(&self, trace: &TraceRow) -> impl std::future::Future<Output = Result<(), StorageError>> + Send;
    fn load_traces(&self, limit: usize, offset: usize) -> impl std::future::Future<Output = Result<Vec<TraceRow>, StorageError>> + Send;
    fn get_trace(&self, trace_id: u64) -> impl std::future::Future<Output = Result<Option<TraceRow>, StorageError>> + Send;
    fn delete_traces(&self, ids: &[u64]) -> impl std::future::Future<Output = Result<u64, StorageError>> + Send;

    // ── Sessions ──
    fn save_session(&self, entry: &SessionRow) -> impl std::future::Future<Output = Result<(), StorageError>> + Send;
    fn load_sessions(&self, scope: &str) -> impl std::future::Future<Output = Result<Vec<SessionRow>, StorageError>> + Send;
    fn delete_session(&self, scope: &str, key: &str) -> impl std::future::Future<Output = Result<bool, StorageError>> + Send;

    // ── Daemons ──
    fn save_daemon(&self, daemon: &DaemonRow) -> impl std::future::Future<Output = Result<(), StorageError>> + Send;
    fn load_daemons(&self) -> impl std::future::Future<Output = Result<Vec<DaemonRow>, StorageError>> + Send;
    fn delete_daemon(&self, name: &str) -> impl std::future::Future<Output = Result<bool, StorageError>> + Send;

    // ── Audit ──
    fn append_audit(&self, entry: &AuditRow) -> impl std::future::Future<Output = Result<(), StorageError>> + Send;
    fn query_audit(&self, limit: usize) -> impl std::future::Future<Output = Result<Vec<AuditRow>, StorageError>> + Send;

    // ── AxonStores ──
    fn save_axon_store(&self, store: &AxonStoreRow) -> impl std::future::Future<Output = Result<(), StorageError>> + Send;
    fn load_axon_stores(&self) -> impl std::future::Future<Output = Result<Vec<AxonStoreRow>, StorageError>> + Send;
    fn delete_axon_store(&self, name: &str) -> impl std::future::Future<Output = Result<bool, StorageError>> + Send;

    // ── Dataspaces ──
    fn save_dataspace(&self, ds: &DataspaceRow) -> impl std::future::Future<Output = Result<(), StorageError>> + Send;
    fn load_dataspaces(&self) -> impl std::future::Future<Output = Result<Vec<DataspaceRow>, StorageError>> + Send;
    fn delete_dataspace(&self, name: &str) -> impl std::future::Future<Output = Result<bool, StorageError>> + Send;

    // ── Hibernations ──
    fn save_hibernation(&self, session: &HibernationRow) -> impl std::future::Future<Output = Result<(), StorageError>> + Send;
    fn load_hibernations(&self) -> impl std::future::Future<Output = Result<Vec<HibernationRow>, StorageError>> + Send;

    // ── Events ──
    fn append_event(&self, event: &EventRow) -> impl std::future::Future<Output = Result<(), StorageError>> + Send;
    fn query_events(&self, topic: Option<&str>, limit: usize) -> impl std::future::Future<Output = Result<Vec<EventRow>, StorageError>> + Send;

    // ── Cache ──
    fn save_cache_entry(&self, entry: &CacheRow) -> impl std::future::Future<Output = Result<(), StorageError>> + Send;
    fn load_cache_entries(&self) -> impl std::future::Future<Output = Result<Vec<CacheRow>, StorageError>> + Send;
    fn evict_expired_cache(&self) -> impl std::future::Future<Output = Result<u64, StorageError>> + Send;

    // ── Cost tracking ──
    fn record_cost(&self, cost: &CostRow) -> impl std::future::Future<Output = Result<(), StorageError>> + Send;
    fn query_costs(&self, flow: Option<&str>, limit: usize) -> impl std::future::Future<Output = Result<Vec<CostRow>, StorageError>> + Send;

    // ── Schedules ──
    fn save_schedule(&self, schedule: &ScheduleRow) -> impl std::future::Future<Output = Result<(), StorageError>> + Send;
    fn load_schedules(&self) -> impl std::future::Future<Output = Result<Vec<ScheduleRow>, StorageError>> + Send;
    fn delete_schedule(&self, name: &str) -> impl std::future::Future<Output = Result<bool, StorageError>> + Send;

    // ── Health ──
    fn is_healthy(&self) -> impl std::future::Future<Output = bool> + Send;
}

// ── InMemoryBackend ────────────────────────────────────────────────────────

/// No-op storage backend — all writes succeed, all loads return empty.
/// Used when no DATABASE_URL is configured (development/testing).
pub struct InMemoryBackend;

impl InMemoryBackend {
    pub fn new() -> Self {
        InMemoryBackend
    }
}

impl StorageBackend for InMemoryBackend {
    async fn save_trace(&self, _trace: &TraceRow) -> Result<(), StorageError> { Ok(()) }
    async fn load_traces(&self, _limit: usize, _offset: usize) -> Result<Vec<TraceRow>, StorageError> { Ok(vec![]) }
    async fn get_trace(&self, _trace_id: u64) -> Result<Option<TraceRow>, StorageError> { Ok(None) }
    async fn delete_traces(&self, _ids: &[u64]) -> Result<u64, StorageError> { Ok(0) }

    async fn save_session(&self, _entry: &SessionRow) -> Result<(), StorageError> { Ok(()) }
    async fn load_sessions(&self, _scope: &str) -> Result<Vec<SessionRow>, StorageError> { Ok(vec![]) }
    async fn delete_session(&self, _scope: &str, _key: &str) -> Result<bool, StorageError> { Ok(false) }

    async fn save_daemon(&self, _daemon: &DaemonRow) -> Result<(), StorageError> { Ok(()) }
    async fn load_daemons(&self) -> Result<Vec<DaemonRow>, StorageError> { Ok(vec![]) }
    async fn delete_daemon(&self, _name: &str) -> Result<bool, StorageError> { Ok(false) }

    async fn append_audit(&self, _entry: &AuditRow) -> Result<(), StorageError> { Ok(()) }
    async fn query_audit(&self, _limit: usize) -> Result<Vec<AuditRow>, StorageError> { Ok(vec![]) }

    async fn save_axon_store(&self, _store: &AxonStoreRow) -> Result<(), StorageError> { Ok(()) }
    async fn load_axon_stores(&self) -> Result<Vec<AxonStoreRow>, StorageError> { Ok(vec![]) }
    async fn delete_axon_store(&self, _name: &str) -> Result<bool, StorageError> { Ok(false) }

    async fn save_dataspace(&self, _ds: &DataspaceRow) -> Result<(), StorageError> { Ok(()) }
    async fn load_dataspaces(&self) -> Result<Vec<DataspaceRow>, StorageError> { Ok(vec![]) }
    async fn delete_dataspace(&self, _name: &str) -> Result<bool, StorageError> { Ok(false) }

    async fn save_hibernation(&self, _session: &HibernationRow) -> Result<(), StorageError> { Ok(()) }
    async fn load_hibernations(&self) -> Result<Vec<HibernationRow>, StorageError> { Ok(vec![]) }

    async fn append_event(&self, _event: &EventRow) -> Result<(), StorageError> { Ok(()) }
    async fn query_events(&self, _topic: Option<&str>, _limit: usize) -> Result<Vec<EventRow>, StorageError> { Ok(vec![]) }

    async fn save_cache_entry(&self, _entry: &CacheRow) -> Result<(), StorageError> { Ok(()) }
    async fn load_cache_entries(&self) -> Result<Vec<CacheRow>, StorageError> { Ok(vec![]) }
    async fn evict_expired_cache(&self) -> Result<u64, StorageError> { Ok(0) }

    async fn record_cost(&self, _cost: &CostRow) -> Result<(), StorageError> { Ok(()) }
    async fn query_costs(&self, _flow: Option<&str>, _limit: usize) -> Result<Vec<CostRow>, StorageError> { Ok(vec![]) }

    async fn save_schedule(&self, _schedule: &ScheduleRow) -> Result<(), StorageError> { Ok(()) }
    async fn load_schedules(&self) -> Result<Vec<ScheduleRow>, StorageError> { Ok(vec![]) }
    async fn delete_schedule(&self, _name: &str) -> Result<bool, StorageError> { Ok(false) }

    async fn is_healthy(&self) -> bool { true }
}

// ── Storage Dispatcher ─────────────────────────────────────────────────────

/// Concrete dispatcher that delegates to the configured storage backend.
/// Uses an enum instead of `dyn` trait to avoid dyn-compatibility issues
/// with async return types.
pub enum StorageDispatcher {
    InMemory(InMemoryBackend),
    // v2.81.0 — the SQL variant, behind `postgres`. The PORT
    // (`StorageBackend`) and the in-memory implementation stay in every build, so
    // a lean binary still has a working storage plane — it just cannot be pointed
    // at Postgres.
    #[cfg(feature = "postgres")]
    Postgres(crate::storage_postgres::PostgresBackend),
}

impl StorageDispatcher {
    pub fn in_memory() -> Self {
        StorageDispatcher::InMemory(InMemoryBackend::new())
    }

    /// v2.81.0 / the design decision — the pool type is named through [`crate::db_pool`],
    /// the module that owns pool construction, rather than reached for directly
    /// in `sqlx`. Same type, one fewer module that has to know which driver the
    /// project uses. This is the `auth_scope` / `auth_middleware` treatment of
    /// v2.81.0 applied to the storage seam: a module that turns out not to need
    /// a dependency is worth more than a module that is gated.
    #[cfg(feature = "postgres")]
    pub fn postgres(pool: crate::db_pool::PgPool) -> Self {
        StorageDispatcher::Postgres(crate::storage_postgres::PostgresBackend::new(pool))
    }
}

/// Macro to delegate all StorageBackend methods to the inner variant.
macro_rules! dispatch {
    ($self:expr, $method:ident $(, $arg:expr)*) => {
        match $self {
            StorageDispatcher::InMemory(b) => b.$method($($arg),*).await,
            #[cfg(feature = "postgres")]
            StorageDispatcher::Postgres(b) => b.$method($($arg),*).await,
        }
    };
}

impl StorageBackend for StorageDispatcher {
    async fn save_trace(&self, trace: &TraceRow) -> Result<(), StorageError> { dispatch!(self, save_trace, trace) }
    async fn load_traces(&self, limit: usize, offset: usize) -> Result<Vec<TraceRow>, StorageError> { dispatch!(self, load_traces, limit, offset) }
    async fn get_trace(&self, trace_id: u64) -> Result<Option<TraceRow>, StorageError> { dispatch!(self, get_trace, trace_id) }
    async fn delete_traces(&self, ids: &[u64]) -> Result<u64, StorageError> { dispatch!(self, delete_traces, ids) }

    async fn save_session(&self, entry: &SessionRow) -> Result<(), StorageError> { dispatch!(self, save_session, entry) }
    async fn load_sessions(&self, scope: &str) -> Result<Vec<SessionRow>, StorageError> { dispatch!(self, load_sessions, scope) }
    async fn delete_session(&self, scope: &str, key: &str) -> Result<bool, StorageError> { dispatch!(self, delete_session, scope, key) }

    async fn save_daemon(&self, daemon: &DaemonRow) -> Result<(), StorageError> { dispatch!(self, save_daemon, daemon) }
    async fn load_daemons(&self) -> Result<Vec<DaemonRow>, StorageError> { dispatch!(self, load_daemons) }
    async fn delete_daemon(&self, name: &str) -> Result<bool, StorageError> { dispatch!(self, delete_daemon, name) }

    async fn append_audit(&self, entry: &AuditRow) -> Result<(), StorageError> { dispatch!(self, append_audit, entry) }
    async fn query_audit(&self, limit: usize) -> Result<Vec<AuditRow>, StorageError> { dispatch!(self, query_audit, limit) }

    async fn save_axon_store(&self, store: &AxonStoreRow) -> Result<(), StorageError> { dispatch!(self, save_axon_store, store) }
    async fn load_axon_stores(&self) -> Result<Vec<AxonStoreRow>, StorageError> { dispatch!(self, load_axon_stores) }
    async fn delete_axon_store(&self, name: &str) -> Result<bool, StorageError> { dispatch!(self, delete_axon_store, name) }

    async fn save_dataspace(&self, ds: &DataspaceRow) -> Result<(), StorageError> { dispatch!(self, save_dataspace, ds) }
    async fn load_dataspaces(&self) -> Result<Vec<DataspaceRow>, StorageError> { dispatch!(self, load_dataspaces) }
    async fn delete_dataspace(&self, name: &str) -> Result<bool, StorageError> { dispatch!(self, delete_dataspace, name) }

    async fn save_hibernation(&self, session: &HibernationRow) -> Result<(), StorageError> { dispatch!(self, save_hibernation, session) }
    async fn load_hibernations(&self) -> Result<Vec<HibernationRow>, StorageError> { dispatch!(self, load_hibernations) }

    async fn append_event(&self, event: &EventRow) -> Result<(), StorageError> { dispatch!(self, append_event, event) }
    async fn query_events(&self, topic: Option<&str>, limit: usize) -> Result<Vec<EventRow>, StorageError> { dispatch!(self, query_events, topic, limit) }

    async fn save_cache_entry(&self, entry: &CacheRow) -> Result<(), StorageError> { dispatch!(self, save_cache_entry, entry) }
    async fn load_cache_entries(&self) -> Result<Vec<CacheRow>, StorageError> { dispatch!(self, load_cache_entries) }
    async fn evict_expired_cache(&self) -> Result<u64, StorageError> { dispatch!(self, evict_expired_cache) }

    async fn record_cost(&self, cost: &CostRow) -> Result<(), StorageError> { dispatch!(self, record_cost, cost) }
    async fn query_costs(&self, flow: Option<&str>, limit: usize) -> Result<Vec<CostRow>, StorageError> { dispatch!(self, query_costs, flow, limit) }

    async fn save_schedule(&self, schedule: &ScheduleRow) -> Result<(), StorageError> { dispatch!(self, save_schedule, schedule) }
    async fn load_schedules(&self) -> Result<Vec<ScheduleRow>, StorageError> { dispatch!(self, load_schedules) }
    async fn delete_schedule(&self, name: &str) -> Result<bool, StorageError> { dispatch!(self, delete_schedule, name) }

    async fn is_healthy(&self) -> bool {
        match self {
            StorageDispatcher::InMemory(b) => b.is_healthy().await,
            #[cfg(feature = "postgres")]
            StorageDispatcher::Postgres(b) => b.is_healthy().await,
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_in_memory_trace_round_trip() {
        let backend = InMemoryBackend::new();
        let trace = TraceRow {
            tenant_id: "default".into(),
            trace_id: 1,
            flow_name: "test_flow".into(),
            status: "success".into(),
            steps_executed: 3,
            latency_ms: 150,
            tokens_input: 100,
            tokens_output: 50,
            anchor_checks: 2,
            anchor_breaches: 0,
            errors: 0,
            retries: 0,
            source_file: "test.axon".into(),
            backend: "stub".into(),
            client_key: "".into(),
            replay_of: None,
            correlation_id: None,
            events: serde_json::json!([]),
            annotations: serde_json::json!([]),
        };
        assert!(backend.save_trace(&trace).await.is_ok());
        let loaded = backend.load_traces(10, 0).await.unwrap();
        assert!(loaded.is_empty()); // InMemory returns empty
    }

    #[tokio::test]
    async fn test_in_memory_session_ops() {
        let backend = InMemoryBackend::new();
        let session = SessionRow {
            tenant_id: "default".into(),
            scope: "default".into(),
            key: "user_name".into(),
            value: "Alice".into(),
            source_step: "step_1".into(),
        };
        assert!(backend.save_session(&session).await.is_ok());
        assert!(backend.load_sessions("default").await.unwrap().is_empty());
        assert!(!backend.delete_session("default", "user_name").await.unwrap());
    }

    #[tokio::test]
    async fn test_in_memory_daemon_ops() {
        let backend = InMemoryBackend::new();
        let daemon = DaemonRow {
            tenant_id: "default".into(),
            name: "agent_1".into(),
            state: "running".into(),
            source_file: "agent.axon".into(),
            flow_name: "main".into(),
            event_count: 0,
            restart_count: 0,
            trigger_topic: Some("user.input".into()),
            output_topic: None,
            lifecycle_events: serde_json::json!([]),
        };
        assert!(backend.save_daemon(&daemon).await.is_ok());
        assert!(backend.load_daemons().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_in_memory_audit_ops() {
        let backend = InMemoryBackend::new();
        let entry = AuditRow {
            tenant_id: "default".into(),
            action: "deploy".into(),
            actor: "admin".into(),
            target: "flow_1".into(),
            detail: serde_json::json!({"version": "1.0"}),
        };
        assert!(backend.append_audit(&entry).await.is_ok());
        assert!(backend.query_audit(10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_in_memory_hibernation_ops() {
        let backend = InMemoryBackend::new();
        let hib = HibernationRow {
            tenant_id: "default".into(),
            id: "h1".into(),
            name: "example_agent".into(),
            operation: "process_document".into(),
            status: "active".into(),
            checkpoints: serde_json::json!([]),
            resumed_from: None,
            created_at: 1700000000,
            last_status_change: 1700000000,
            next_checkpoint_id: 1,
        };
        assert!(backend.save_hibernation(&hib).await.is_ok());
        assert!(backend.load_hibernations().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_in_memory_cost_ops() {
        let backend = InMemoryBackend::new();
        let cost = CostRow {
            tenant_id: "default".into(),
            flow_name: "analysis".into(),
            backend: "anthropic".into(),
            input_tokens: 1000,
            output_tokens: 500,
            cost_usd: 0.015,
        };
        assert!(backend.record_cost(&cost).await.is_ok());
        assert!(backend.query_costs(None, 10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_in_memory_health() {
        let backend = InMemoryBackend::new();
        assert!(backend.is_healthy().await);
    }

    #[tokio::test]
    async fn test_in_memory_cache_ops() {
        let backend = InMemoryBackend::new();
        let cache = CacheRow {
            tenant_id: "default".into(),
            flow_name: "test".into(),
            cache_key: "k1".into(),
            result: serde_json::json!({"output": "hello"}),
            ttl_secs: Some(300),
            hit_count: 0,
        };
        assert!(backend.save_cache_entry(&cache).await.is_ok());
        assert!(backend.load_cache_entries().await.unwrap().is_empty());
        assert_eq!(backend.evict_expired_cache().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_in_memory_schedule_ops() {
        let backend = InMemoryBackend::new();
        let schedule = ScheduleRow {
            tenant_id: "default".into(),
            name: "daily_report".into(),
            flow_name: "report".into(),
            interval_secs: 86400,
            enabled: true,
            backend: "anthropic".into(),
            last_run: 0,
            next_run: 86400,
            run_count: 0,
            error_count: 0,
        };
        assert!(backend.save_schedule(&schedule).await.is_ok());
        assert!(backend.load_schedules().await.unwrap().is_empty());
        assert!(!backend.delete_schedule("daily_report").await.unwrap());
    }

    #[tokio::test]
    async fn test_in_memory_event_ops() {
        let backend = InMemoryBackend::new();
        let event = EventRow {
            tenant_id: "default".into(),
            topic: "flow.completed".into(),
            source: "executor".into(),
            payload: serde_json::json!({"flow": "test"}),
        };
        assert!(backend.append_event(&event).await.is_ok());
        assert!(backend.query_events(None, 10).await.unwrap().is_empty());
        assert!(backend.query_events(Some("flow.completed"), 10).await.unwrap().is_empty());
    }

    #[test]
    fn test_storage_error_display() {
        assert_eq!(
            format!("{}", StorageError::ConnectionError("timeout".into())),
            "connection error: timeout"
        );
        assert_eq!(
            format!("{}", StorageError::QueryError("syntax".into())),
            "query error: syntax"
        );
        assert_eq!(
            format!("{}", StorageError::NotFound("trace_42".into())),
            "not found: trace_42"
        );
    }
}
