//! Database Pool — PostgreSQL connection pool management for AxonServer.
//!
//! Wraps sqlx::PgPool with production-grade configuration:
//!   - Max connections: 10 (configurable)
//!   - Min connections: 2 (keeps warm connections ready)
//!   - Acquire timeout: 5 seconds
//!   - Idle timeout: 300 seconds
//!   - Health check via SELECT 1
//!
//! Connection URL sourced from `--database-url` CLI arg or `DATABASE_URL` env var.

use crate::storage::StorageError;
use sqlx::postgres::PgPoolOptions;
pub use sqlx::PgPool;

/// Create a PostgreSQL connection pool with production defaults.
pub async fn create_pool(database_url: &str) -> Result<PgPool, StorageError> {
    tracing::info!(url = mask_url(database_url), "db_pool_creating");

    let pool = PgPoolOptions::new()
        .max_connections(10)
        .min_connections(2)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .idle_timeout(std::time::Duration::from_secs(300))
        .connect(database_url)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "db_pool_connection_failed");
            StorageError::ConnectionError(format!("Failed to connect to database: {e}"))
        })?;

    tracing::info!(
        max_connections = 10,
        min_connections = 2,
        "db_pool_created"
    );

    Ok(pool)
}

/// Health check — runs SELECT 1 to verify database connectivity.
pub async fn check_health(pool: &PgPool) -> bool {
    sqlx::query("SELECT 1")
        .execute(pool)
        .await
        .is_ok()
}

/// Mask the password in a database URL for safe logging.
fn mask_url(url: &str) -> String {
    // postgresql://user:password@host:port/db → postgresql://user:***@host:port/db
    if let Some(at_pos) = url.find('@') {
        if let Some(colon_pos) = url[..at_pos].rfind(':') {
            let prefix = &url[..colon_pos + 1];
            let suffix = &url[at_pos..];
            return format!("{prefix}***{suffix}");
        }
    }
    url.to_string()
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_url_with_password() {
        let url = "postgresql://user:secret123@localhost:5432/axon";
        assert_eq!(mask_url(url), "postgresql://user:***@localhost:5432/axon");
    }

    #[test]
    fn test_mask_url_without_password() {
        let url = "postgresql://localhost:5432/axon";
        assert_eq!(mask_url(url), "postgresql://localhost:5432/axon");
    }

    #[test]
    fn test_mask_url_empty() {
        assert_eq!(mask_url(""), "");
    }
}
