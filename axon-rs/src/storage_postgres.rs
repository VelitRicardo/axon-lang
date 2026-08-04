//! PostgreSQL Storage Backend — full persistent storage for AxonServer.
//!
//! Every data method:
//!   1. Reads the active tenant from `crate::tenant_context::current_tenant_id()` (task-local)
//!   2. Opens a transaction
//!   3. Executes `SET LOCAL axon.current_tenant = '<tenant>'` so Postgres RLS
//!      policies activate for the duration of that transaction
//!   4. Runs the actual DML
//!   5. Commits
//!
//! This means RLS isolation is enforced even if application code forgets to
//! filter by tenant_id — Postgres blocks the query at the row level.

use sqlx::PgPool;
use sqlx::Row;
use crate::storage::*;

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Open a transaction and activate RLS for the current tenant.
/// Every data method must call this instead of `pool.begin()` directly.
macro_rules! begin_tenant_tx {
    ($pool:expr, $tenant:expr, $op:literal) => {{
        let mut tx = $pool.begin().await.map_err(|e| {
            StorageError::ConnectionError(format!("{}: begin tx: {e}", $op))
        })?;
        sqlx::query("SET LOCAL axon.current_tenant = $1")
            .bind($tenant)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::QueryError(format!("{}: set_tenant: {e}", $op)))?;
        tx
    }};
}

macro_rules! commit_tx {
    ($tx:expr, $op:literal) => {
        $tx.commit().await.map_err(|e| {
            StorageError::QueryError(format!("{}: commit: {e}", $op))
        })?
    };
}

// ── PostgresBackend ───────────────────────────────────────────────────────────

/// PostgreSQL implementation of StorageBackend.
pub struct PostgresBackend {
    pub pool: PgPool,
}

impl PostgresBackend {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl StorageBackend for PostgresBackend {

    // ── Traces ────────────────────────────────────────────────────────────────

    async fn save_trace(&self, trace: &TraceRow) -> Result<(), StorageError> {
        let tid = crate::tenant_context::current_tenant_id();
        let mut tx = begin_tenant_tx!(&self.pool, &tid, "save_trace");
        sqlx::query(
            "INSERT INTO traces \
             (tenant_id, trace_id, flow_name, status, steps_executed, latency_ms, \
              tokens_input, tokens_output, anchor_checks, anchor_breaches, errors, retries, \
              source_file, backend, client_key, replay_of, correlation_id, events, annotations) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19) \
             ON CONFLICT (tenant_id, trace_id) DO UPDATE SET \
             status = EXCLUDED.status, steps_executed = EXCLUDED.steps_executed, \
             latency_ms = EXCLUDED.latency_ms, tokens_input = EXCLUDED.tokens_input, \
             tokens_output = EXCLUDED.tokens_output, events = EXCLUDED.events, \
             annotations = EXCLUDED.annotations"
        )
        .bind(&tid)
        .bind(trace.trace_id as i64)
        .bind(&trace.flow_name)
        .bind(&trace.status)
        .bind(trace.steps_executed as i32)
        .bind(trace.latency_ms as i64)
        .bind(trace.tokens_input as i64)
        .bind(trace.tokens_output as i64)
        .bind(trace.anchor_checks as i32)
        .bind(trace.anchor_breaches as i32)
        .bind(trace.errors as i32)
        .bind(trace.retries as i32)
        .bind(&trace.source_file)
        .bind(&trace.backend)
        .bind(&trace.client_key)
        .bind(trace.replay_of.map(|v| v as i64))
        .bind(&trace.correlation_id)
        .bind(&trace.events)
        .bind(&trace.annotations)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::QueryError(format!("save_trace: {e}")))?;
        commit_tx!(tx, "save_trace");
        Ok(())
    }

    async fn load_traces(&self, limit: usize, offset: usize) -> Result<Vec<TraceRow>, StorageError> {
        let tid = crate::tenant_context::current_tenant_id();
        let mut tx = begin_tenant_tx!(&self.pool, &tid, "load_traces");
        let rows = sqlx::query(
            "SELECT tenant_id, trace_id, flow_name, status, steps_executed, latency_ms, \
             tokens_input, tokens_output, anchor_checks, anchor_breaches, errors, retries, \
             source_file, backend, client_key, replay_of, correlation_id, events, annotations \
             FROM traces ORDER BY timestamp_utc DESC LIMIT $1 OFFSET $2"
        )
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| StorageError::QueryError(format!("load_traces: {e}")))?;
        commit_tx!(tx, "load_traces");
        Ok(rows.iter().map(|r| TraceRow {
            tenant_id: r.get("tenant_id"),
            trace_id: r.get::<i64, _>("trace_id") as u64,
            flow_name: r.get("flow_name"),
            status: r.get("status"),
            steps_executed: r.get::<i32, _>("steps_executed") as u32,
            latency_ms: r.get::<i64, _>("latency_ms") as u64,
            tokens_input: r.get::<i64, _>("tokens_input") as u64,
            tokens_output: r.get::<i64, _>("tokens_output") as u64,
            anchor_checks: r.get::<i32, _>("anchor_checks") as u32,
            anchor_breaches: r.get::<i32, _>("anchor_breaches") as u32,
            errors: r.get::<i32, _>("errors") as u32,
            retries: r.get::<i32, _>("retries") as u32,
            source_file: r.get("source_file"),
            backend: r.get("backend"),
            client_key: r.get("client_key"),
            replay_of: r.get::<Option<i64>, _>("replay_of").map(|v| v as u64),
            correlation_id: r.get("correlation_id"),
            events: r.get("events"),
            annotations: r.get("annotations"),
        }).collect())
    }

    async fn get_trace(&self, trace_id: u64) -> Result<Option<TraceRow>, StorageError> {
        let tid = crate::tenant_context::current_tenant_id();
        let mut tx = begin_tenant_tx!(&self.pool, &tid, "get_trace");
        let row = sqlx::query(
            "SELECT tenant_id, trace_id, flow_name, status, steps_executed, latency_ms, \
             tokens_input, tokens_output, anchor_checks, anchor_breaches, errors, retries, \
             source_file, backend, client_key, replay_of, correlation_id, events, annotations \
             FROM traces WHERE trace_id = $1"
        )
        .bind(trace_id as i64)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| StorageError::QueryError(format!("get_trace: {e}")))?;
        commit_tx!(tx, "get_trace");
        Ok(row.map(|r| TraceRow {
            tenant_id: r.get("tenant_id"),
            trace_id: r.get::<i64, _>("trace_id") as u64,
            flow_name: r.get("flow_name"),
            status: r.get("status"),
            steps_executed: r.get::<i32, _>("steps_executed") as u32,
            latency_ms: r.get::<i64, _>("latency_ms") as u64,
            tokens_input: r.get::<i64, _>("tokens_input") as u64,
            tokens_output: r.get::<i64, _>("tokens_output") as u64,
            anchor_checks: r.get::<i32, _>("anchor_checks") as u32,
            anchor_breaches: r.get::<i32, _>("anchor_breaches") as u32,
            errors: r.get::<i32, _>("errors") as u32,
            retries: r.get::<i32, _>("retries") as u32,
            source_file: r.get("source_file"),
            backend: r.get("backend"),
            client_key: r.get("client_key"),
            replay_of: r.get::<Option<i64>, _>("replay_of").map(|v| v as u64),
            correlation_id: r.get("correlation_id"),
            events: r.get("events"),
            annotations: r.get("annotations"),
        }))
    }

    async fn delete_traces(&self, ids: &[u64]) -> Result<u64, StorageError> {
        let tid = crate::tenant_context::current_tenant_id();
        let ids_i64: Vec<i64> = ids.iter().map(|&id| id as i64).collect();
        let mut tx = begin_tenant_tx!(&self.pool, &tid, "delete_traces");
        let result = sqlx::query("DELETE FROM traces WHERE trace_id = ANY($1)")
            .bind(&ids_i64)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::QueryError(format!("delete_traces: {e}")))?;
        commit_tx!(tx, "delete_traces");
        Ok(result.rows_affected())
    }

    // ── Sessions ──────────────────────────────────────────────────────────────

    async fn save_session(&self, entry: &SessionRow) -> Result<(), StorageError> {
        let tid = crate::tenant_context::current_tenant_id();
        let mut tx = begin_tenant_tx!(&self.pool, &tid, "save_session");
        sqlx::query(
            "INSERT INTO sessions (tenant_id, scope, key, value, source_step) \
             VALUES ($1,$2,$3,$4,$5) \
             ON CONFLICT (tenant_id, scope, key) DO UPDATE SET \
             value = EXCLUDED.value, source_step = EXCLUDED.source_step"
        )
        .bind(&tid)
        .bind(&entry.scope)
        .bind(&entry.key)
        .bind(&entry.value)
        .bind(&entry.source_step)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::QueryError(format!("save_session: {e}")))?;
        commit_tx!(tx, "save_session");
        Ok(())
    }

    async fn load_sessions(&self, scope: &str) -> Result<Vec<SessionRow>, StorageError> {
        let tid = crate::tenant_context::current_tenant_id();
        let mut tx = begin_tenant_tx!(&self.pool, &tid, "load_sessions");
        let rows = sqlx::query(
            "SELECT tenant_id, scope, key, value, source_step FROM sessions WHERE scope = $1"
        )
        .bind(scope)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| StorageError::QueryError(format!("load_sessions: {e}")))?;
        commit_tx!(tx, "load_sessions");
        Ok(rows.iter().map(|r| SessionRow {
            tenant_id: r.get("tenant_id"),
            scope: r.get("scope"),
            key: r.get("key"),
            value: r.get("value"),
            source_step: r.get("source_step"),
        }).collect())
    }

    async fn delete_session(&self, scope: &str, key: &str) -> Result<bool, StorageError> {
        let tid = crate::tenant_context::current_tenant_id();
        let mut tx = begin_tenant_tx!(&self.pool, &tid, "delete_session");
        let result = sqlx::query("DELETE FROM sessions WHERE scope = $1 AND key = $2")
            .bind(scope)
            .bind(key)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::QueryError(format!("delete_session: {e}")))?;
        commit_tx!(tx, "delete_session");
        Ok(result.rows_affected() > 0)
    }

    // ── Daemons ───────────────────────────────────────────────────────────────

    async fn save_daemon(&self, daemon: &DaemonRow) -> Result<(), StorageError> {
        let tid = crate::tenant_context::current_tenant_id();
        let mut tx = begin_tenant_tx!(&self.pool, &tid, "save_daemon");
        sqlx::query(
            "INSERT INTO daemons \
             (tenant_id, name, state, source_file, flow_name, event_count, restart_count, \
              trigger_topic, output_topic, lifecycle_events) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) \
             ON CONFLICT (tenant_id, name) DO UPDATE SET \
             state = EXCLUDED.state, event_count = EXCLUDED.event_count, \
             restart_count = EXCLUDED.restart_count, lifecycle_events = EXCLUDED.lifecycle_events"
        )
        .bind(&tid)
        .bind(&daemon.name)
        .bind(&daemon.state)
        .bind(&daemon.source_file)
        .bind(&daemon.flow_name)
        .bind(daemon.event_count as i64)
        .bind(daemon.restart_count as i32)
        .bind(&daemon.trigger_topic)
        .bind(&daemon.output_topic)
        .bind(&daemon.lifecycle_events)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::QueryError(format!("save_daemon: {e}")))?;
        commit_tx!(tx, "save_daemon");
        Ok(())
    }

    async fn load_daemons(&self) -> Result<Vec<DaemonRow>, StorageError> {
        let tid = crate::tenant_context::current_tenant_id();
        let mut tx = begin_tenant_tx!(&self.pool, &tid, "load_daemons");
        let rows = sqlx::query(
            "SELECT tenant_id, name, state, source_file, flow_name, event_count, \
             restart_count, trigger_topic, output_topic, lifecycle_events FROM daemons"
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| StorageError::QueryError(format!("load_daemons: {e}")))?;
        commit_tx!(tx, "load_daemons");
        Ok(rows.iter().map(|r| DaemonRow {
            tenant_id: r.get("tenant_id"),
            name: r.get("name"),
            state: r.get("state"),
            source_file: r.get("source_file"),
            flow_name: r.get("flow_name"),
            event_count: r.get::<i64, _>("event_count") as u64,
            restart_count: r.get::<i32, _>("restart_count") as u32,
            trigger_topic: r.get("trigger_topic"),
            output_topic: r.get("output_topic"),
            lifecycle_events: r.get("lifecycle_events"),
        }).collect())
    }

    async fn delete_daemon(&self, name: &str) -> Result<bool, StorageError> {
        let tid = crate::tenant_context::current_tenant_id();
        let mut tx = begin_tenant_tx!(&self.pool, &tid, "delete_daemon");
        let result = sqlx::query("DELETE FROM daemons WHERE name = $1")
            .bind(name)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::QueryError(format!("delete_daemon: {e}")))?;
        commit_tx!(tx, "delete_daemon");
        Ok(result.rows_affected() > 0)
    }

    // ── Audit ─────────────────────────────────────────────────────────────────

    async fn append_audit(&self, entry: &AuditRow) -> Result<(), StorageError> {
        let tid = crate::tenant_context::current_tenant_id();
        let mut tx = begin_tenant_tx!(&self.pool, &tid, "append_audit");
        sqlx::query(
            "INSERT INTO audit_log (tenant_id, action, actor, target, detail) \
             VALUES ($1,$2,$3,$4,$5)"
        )
        .bind(&tid)
        .bind(&entry.action)
        .bind(&entry.actor)
        .bind(&entry.target)
        .bind(&entry.detail)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::QueryError(format!("append_audit: {e}")))?;
        commit_tx!(tx, "append_audit");
        Ok(())
    }

    async fn query_audit(&self, limit: usize) -> Result<Vec<AuditRow>, StorageError> {
        let tid = crate::tenant_context::current_tenant_id();
        let mut tx = begin_tenant_tx!(&self.pool, &tid, "query_audit");
        let rows = sqlx::query(
            "SELECT tenant_id, action, actor, target, detail \
             FROM audit_log ORDER BY timestamp_utc DESC LIMIT $1"
        )
        .bind(limit as i64)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| StorageError::QueryError(format!("query_audit: {e}")))?;
        commit_tx!(tx, "query_audit");
        Ok(rows.iter().map(|r| AuditRow {
            tenant_id: r.get("tenant_id"),
            action: r.get("action"),
            actor: r.get("actor"),
            target: r.get("target"),
            detail: r.get("detail"),
        }).collect())
    }

    // ── AxonStores ────────────────────────────────────────────────────────────

    async fn save_axon_store(&self, store: &AxonStoreRow) -> Result<(), StorageError> {
        let tid = crate::tenant_context::current_tenant_id();
        let mut tx = begin_tenant_tx!(&self.pool, &tid, "save_axon_store");
        sqlx::query(
            "INSERT INTO axon_stores (tenant_id, name, ontology, entries, created_at, total_ops) \
             VALUES ($1,$2,$3,$4,$5,$6) \
             ON CONFLICT (tenant_id, name) DO UPDATE SET \
             entries = EXCLUDED.entries, total_ops = EXCLUDED.total_ops"
        )
        .bind(&tid)
        .bind(&store.name)
        .bind(&store.ontology)
        .bind(&store.entries)
        .bind(store.created_at as i64)
        .bind(store.total_ops as i64)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::QueryError(format!("save_axon_store: {e}")))?;
        commit_tx!(tx, "save_axon_store");
        Ok(())
    }

    async fn load_axon_stores(&self) -> Result<Vec<AxonStoreRow>, StorageError> {
        let tid = crate::tenant_context::current_tenant_id();
        let mut tx = begin_tenant_tx!(&self.pool, &tid, "load_axon_stores");
        let rows = sqlx::query(
            "SELECT tenant_id, name, ontology, entries, created_at, total_ops FROM axon_stores"
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| StorageError::QueryError(format!("load_axon_stores: {e}")))?;
        commit_tx!(tx, "load_axon_stores");
        Ok(rows.iter().map(|r| AxonStoreRow {
            tenant_id: r.get("tenant_id"),
            name: r.get("name"),
            ontology: r.get("ontology"),
            entries: r.get("entries"),
            created_at: r.get::<i64, _>("created_at") as u64,
            total_ops: r.get::<i64, _>("total_ops") as u64,
        }).collect())
    }

    async fn delete_axon_store(&self, name: &str) -> Result<bool, StorageError> {
        let tid = crate::tenant_context::current_tenant_id();
        let mut tx = begin_tenant_tx!(&self.pool, &tid, "delete_axon_store");
        let result = sqlx::query("DELETE FROM axon_stores WHERE name = $1")
            .bind(name)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::QueryError(format!("delete_axon_store: {e}")))?;
        commit_tx!(tx, "delete_axon_store");
        Ok(result.rows_affected() > 0)
    }

    // ── Dataspaces ────────────────────────────────────────────────────────────

    async fn save_dataspace(&self, ds: &DataspaceRow) -> Result<(), StorageError> {
        let tid = crate::tenant_context::current_tenant_id();
        let mut tx = begin_tenant_tx!(&self.pool, &tid, "save_dataspace");
        sqlx::query(
            "INSERT INTO dataspaces \
             (tenant_id, name, ontology, entries, associations, created_at, total_ops, next_id) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8) \
             ON CONFLICT (tenant_id, name) DO UPDATE SET \
             entries = EXCLUDED.entries, associations = EXCLUDED.associations, \
             total_ops = EXCLUDED.total_ops, next_id = EXCLUDED.next_id"
        )
        .bind(&tid)
        .bind(&ds.name)
        .bind(&ds.ontology)
        .bind(&ds.entries)
        .bind(&ds.associations)
        .bind(ds.created_at as i64)
        .bind(ds.total_ops as i64)
        .bind(ds.next_id as i64)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::QueryError(format!("save_dataspace: {e}")))?;
        commit_tx!(tx, "save_dataspace");
        Ok(())
    }

    async fn load_dataspaces(&self) -> Result<Vec<DataspaceRow>, StorageError> {
        let tid = crate::tenant_context::current_tenant_id();
        let mut tx = begin_tenant_tx!(&self.pool, &tid, "load_dataspaces");
        let rows = sqlx::query(
            "SELECT tenant_id, name, ontology, entries, associations, \
             created_at, total_ops, next_id FROM dataspaces"
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| StorageError::QueryError(format!("load_dataspaces: {e}")))?;
        commit_tx!(tx, "load_dataspaces");
        Ok(rows.iter().map(|r| DataspaceRow {
            tenant_id: r.get("tenant_id"),
            name: r.get("name"),
            ontology: r.get("ontology"),
            entries: r.get("entries"),
            associations: r.get("associations"),
            created_at: r.get::<i64, _>("created_at") as u64,
            total_ops: r.get::<i64, _>("total_ops") as u64,
            next_id: r.get::<i64, _>("next_id") as u64,
        }).collect())
    }

    async fn delete_dataspace(&self, name: &str) -> Result<bool, StorageError> {
        let tid = crate::tenant_context::current_tenant_id();
        let mut tx = begin_tenant_tx!(&self.pool, &tid, "delete_dataspace");
        let result = sqlx::query("DELETE FROM dataspaces WHERE name = $1")
            .bind(name)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::QueryError(format!("delete_dataspace: {e}")))?;
        commit_tx!(tx, "delete_dataspace");
        Ok(result.rows_affected() > 0)
    }

    // ── Hibernations ──────────────────────────────────────────────────────────

    async fn save_hibernation(&self, session: &HibernationRow) -> Result<(), StorageError> {
        let tid = crate::tenant_context::current_tenant_id();
        let mut tx = begin_tenant_tx!(&self.pool, &tid, "save_hibernation");
        sqlx::query(
            "INSERT INTO hibernations \
             (tenant_id, id, name, operation, status, checkpoints, resumed_from, \
              created_at, last_status_change, next_checkpoint_id) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) \
             ON CONFLICT (tenant_id, id) DO UPDATE SET \
             status = EXCLUDED.status, checkpoints = EXCLUDED.checkpoints, \
             resumed_from = EXCLUDED.resumed_from, \
             last_status_change = EXCLUDED.last_status_change, \
             next_checkpoint_id = EXCLUDED.next_checkpoint_id"
        )
        .bind(&tid)
        .bind(&session.id)
        .bind(&session.name)
        .bind(&session.operation)
        .bind(&session.status)
        .bind(&session.checkpoints)
        .bind(session.resumed_from)
        .bind(session.created_at as i64)
        .bind(session.last_status_change as i64)
        .bind(session.next_checkpoint_id as i32)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::QueryError(format!("save_hibernation: {e}")))?;
        commit_tx!(tx, "save_hibernation");
        Ok(())
    }

    async fn load_hibernations(&self) -> Result<Vec<HibernationRow>, StorageError> {
        let tid = crate::tenant_context::current_tenant_id();
        let mut tx = begin_tenant_tx!(&self.pool, &tid, "load_hibernations");
        let rows = sqlx::query(
            "SELECT tenant_id, id, name, operation, status, checkpoints, resumed_from, \
             created_at, last_status_change, next_checkpoint_id FROM hibernations"
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| StorageError::QueryError(format!("load_hibernations: {e}")))?;
        commit_tx!(tx, "load_hibernations");
        Ok(rows.iter().map(|r| HibernationRow {
            tenant_id: r.get("tenant_id"),
            id: r.get("id"),
            name: r.get("name"),
            operation: r.get("operation"),
            status: r.get("status"),
            checkpoints: r.get("checkpoints"),
            resumed_from: r.get("resumed_from"),
            created_at: r.get::<i64, _>("created_at") as u64,
            last_status_change: r.get::<i64, _>("last_status_change") as u64,
            next_checkpoint_id: r.get::<i32, _>("next_checkpoint_id") as u32,
        }).collect())
    }

    // ── Events ────────────────────────────────────────────────────────────────

    async fn append_event(&self, event: &EventRow) -> Result<(), StorageError> {
        let tid = crate::tenant_context::current_tenant_id();
        let mut tx = begin_tenant_tx!(&self.pool, &tid, "append_event");
        sqlx::query(
            "INSERT INTO event_history (tenant_id, topic, source, payload) VALUES ($1,$2,$3,$4)"
        )
        .bind(&tid)
        .bind(&event.topic)
        .bind(&event.source)
        .bind(&event.payload)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::QueryError(format!("append_event: {e}")))?;
        commit_tx!(tx, "append_event");
        Ok(())
    }

    async fn query_events(&self, topic: Option<&str>, limit: usize) -> Result<Vec<EventRow>, StorageError> {
        let tid = crate::tenant_context::current_tenant_id();
        let mut tx = begin_tenant_tx!(&self.pool, &tid, "query_events");
        let rows = match topic {
            Some(t) => {
                sqlx::query(
                    "SELECT tenant_id, topic, source, payload FROM event_history \
                     WHERE topic = $1 ORDER BY timestamp_utc DESC LIMIT $2"
                )
                .bind(t)
                .bind(limit as i64)
                .fetch_all(&mut *tx)
                .await
            }
            None => {
                sqlx::query(
                    "SELECT tenant_id, topic, source, payload FROM event_history \
                     ORDER BY timestamp_utc DESC LIMIT $1"
                )
                .bind(limit as i64)
                .fetch_all(&mut *tx)
                .await
            }
        }.map_err(|e| StorageError::QueryError(format!("query_events: {e}")))?;
        commit_tx!(tx, "query_events");
        Ok(rows.iter().map(|r| EventRow {
            tenant_id: r.get("tenant_id"),
            topic: r.get("topic"),
            source: r.get("source"),
            payload: r.get("payload"),
        }).collect())
    }

    // ── Cache ─────────────────────────────────────────────────────────────────

    async fn save_cache_entry(&self, entry: &CacheRow) -> Result<(), StorageError> {
        let tid = crate::tenant_context::current_tenant_id();
        let mut tx = begin_tenant_tx!(&self.pool, &tid, "save_cache_entry");
        sqlx::query(
            "INSERT INTO execution_cache \
             (tenant_id, flow_name, cache_key, result, ttl_secs, hit_count) \
             VALUES ($1,$2,$3,$4,$5,$6) \
             ON CONFLICT (tenant_id, cache_key) DO UPDATE SET \
             result = EXCLUDED.result, ttl_secs = EXCLUDED.ttl_secs, \
             hit_count = EXCLUDED.hit_count"
        )
        .bind(&tid)
        .bind(&entry.flow_name)
        .bind(&entry.cache_key)
        .bind(&entry.result)
        .bind(entry.ttl_secs)
        .bind(entry.hit_count as i64)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::QueryError(format!("save_cache_entry: {e}")))?;
        commit_tx!(tx, "save_cache_entry");
        Ok(())
    }

    async fn load_cache_entries(&self) -> Result<Vec<CacheRow>, StorageError> {
        let tid = crate::tenant_context::current_tenant_id();
        let mut tx = begin_tenant_tx!(&self.pool, &tid, "load_cache_entries");
        let rows = sqlx::query(
            "SELECT tenant_id, flow_name, cache_key, result, ttl_secs, hit_count \
             FROM execution_cache"
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| StorageError::QueryError(format!("load_cache_entries: {e}")))?;
        commit_tx!(tx, "load_cache_entries");
        Ok(rows.iter().map(|r| CacheRow {
            tenant_id: r.get("tenant_id"),
            flow_name: r.get("flow_name"),
            cache_key: r.get("cache_key"),
            result: r.get("result"),
            ttl_secs: r.get("ttl_secs"),
            hit_count: r.get::<i64, _>("hit_count") as u64,
        }).collect())
    }

    async fn evict_expired_cache(&self) -> Result<u64, StorageError> {
        let tid = crate::tenant_context::current_tenant_id();
        let mut tx = begin_tenant_tx!(&self.pool, &tid, "evict_expired_cache");
        let result = sqlx::query(
            "DELETE FROM execution_cache WHERE ttl_secs IS NOT NULL AND \
             created_at + (ttl_secs || ' seconds')::interval < NOW()"
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::QueryError(format!("evict_expired_cache: {e}")))?;
        commit_tx!(tx, "evict_expired_cache");
        Ok(result.rows_affected())
    }

    // ── Cost tracking ─────────────────────────────────────────────────────────

    async fn record_cost(&self, cost: &CostRow) -> Result<(), StorageError> {
        let tid = crate::tenant_context::current_tenant_id();
        let mut tx = begin_tenant_tx!(&self.pool, &tid, "record_cost");
        sqlx::query(
            "INSERT INTO cost_tracking \
             (tenant_id, flow_name, backend, input_tokens, output_tokens, cost_usd) \
             VALUES ($1,$2,$3,$4,$5,$6)"
        )
        .bind(&tid)
        .bind(&cost.flow_name)
        .bind(&cost.backend)
        .bind(cost.input_tokens as i64)
        .bind(cost.output_tokens as i64)
        .bind(cost.cost_usd)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::QueryError(format!("record_cost: {e}")))?;
        commit_tx!(tx, "record_cost");
        Ok(())
    }

    async fn query_costs(&self, flow: Option<&str>, limit: usize) -> Result<Vec<CostRow>, StorageError> {
        let tid = crate::tenant_context::current_tenant_id();
        let mut tx = begin_tenant_tx!(&self.pool, &tid, "query_costs");
        let rows = match flow {
            Some(f) => {
                sqlx::query(
                    "SELECT tenant_id, flow_name, backend, input_tokens, output_tokens, cost_usd \
                     FROM cost_tracking WHERE flow_name = $1 \
                     ORDER BY timestamp_utc DESC LIMIT $2"
                )
                .bind(f)
                .bind(limit as i64)
                .fetch_all(&mut *tx)
                .await
            }
            None => {
                sqlx::query(
                    "SELECT tenant_id, flow_name, backend, input_tokens, output_tokens, cost_usd \
                     FROM cost_tracking ORDER BY timestamp_utc DESC LIMIT $1"
                )
                .bind(limit as i64)
                .fetch_all(&mut *tx)
                .await
            }
        }.map_err(|e| StorageError::QueryError(format!("query_costs: {e}")))?;
        commit_tx!(tx, "query_costs");
        Ok(rows.iter().map(|r| CostRow {
            tenant_id: r.get("tenant_id"),
            flow_name: r.get("flow_name"),
            backend: r.get("backend"),
            input_tokens: r.get::<i64, _>("input_tokens") as u64,
            output_tokens: r.get::<i64, _>("output_tokens") as u64,
            cost_usd: r.get("cost_usd"),
        }).collect())
    }

    // ── Schedules ─────────────────────────────────────────────────────────────

    async fn save_schedule(&self, schedule: &ScheduleRow) -> Result<(), StorageError> {
        let tid = crate::tenant_context::current_tenant_id();
        let mut tx = begin_tenant_tx!(&self.pool, &tid, "save_schedule");
        sqlx::query(
            "INSERT INTO schedules \
             (tenant_id, name, flow_name, interval_secs, enabled, backend, \
              last_run, next_run, run_count, error_count) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) \
             ON CONFLICT (tenant_id, name) DO UPDATE SET \
             enabled = EXCLUDED.enabled, last_run = EXCLUDED.last_run, \
             next_run = EXCLUDED.next_run, run_count = EXCLUDED.run_count, \
             error_count = EXCLUDED.error_count"
        )
        .bind(&tid)
        .bind(&schedule.name)
        .bind(&schedule.flow_name)
        .bind(schedule.interval_secs as i64)
        .bind(schedule.enabled)
        .bind(&schedule.backend)
        .bind(schedule.last_run as i64)
        .bind(schedule.next_run as i64)
        .bind(schedule.run_count as i64)
        .bind(schedule.error_count as i64)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::QueryError(format!("save_schedule: {e}")))?;
        commit_tx!(tx, "save_schedule");
        Ok(())
    }

    async fn load_schedules(&self) -> Result<Vec<ScheduleRow>, StorageError> {
        let tid = crate::tenant_context::current_tenant_id();
        let mut tx = begin_tenant_tx!(&self.pool, &tid, "load_schedules");
        let rows = sqlx::query(
            "SELECT tenant_id, name, flow_name, interval_secs, enabled, backend, \
             last_run, next_run, run_count, error_count FROM schedules"
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| StorageError::QueryError(format!("load_schedules: {e}")))?;
        commit_tx!(tx, "load_schedules");
        Ok(rows.iter().map(|r| ScheduleRow {
            tenant_id: r.get("tenant_id"),
            name: r.get("name"),
            flow_name: r.get("flow_name"),
            interval_secs: r.get::<i64, _>("interval_secs") as u64,
            enabled: r.get("enabled"),
            backend: r.get("backend"),
            last_run: r.get::<i64, _>("last_run") as u64,
            next_run: r.get::<i64, _>("next_run") as u64,
            run_count: r.get::<i64, _>("run_count") as u64,
            error_count: r.get::<i64, _>("error_count") as u64,
        }).collect())
    }

    async fn delete_schedule(&self, name: &str) -> Result<bool, StorageError> {
        let tid = crate::tenant_context::current_tenant_id();
        let mut tx = begin_tenant_tx!(&self.pool, &tid, "delete_schedule");
        let result = sqlx::query("DELETE FROM schedules WHERE name = $1")
            .bind(name)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::QueryError(format!("delete_schedule: {e}")))?;
        commit_tx!(tx, "delete_schedule");
        Ok(result.rows_affected() > 0)
    }

    // ── Health ────────────────────────────────────────────────────────────────

    async fn is_healthy(&self) -> bool {
        crate::db_pool::check_health(&self.pool).await
    }
}
