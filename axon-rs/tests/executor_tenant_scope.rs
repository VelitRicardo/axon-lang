//! v2.49.0 — the tenant reaches custody through the REAL executor path.
//!
//! The v2.48.0/v2.49.0 custody runtime tests drive `dispatch_node` with a hand-built
//! `DispatchCtx` whose `tenant_id` is set MANUALLY. The DEPLOYED path is
//! `execute_server_flow`, which — before v2.49.0 — never populated
//! `ctx.tenant_id`, so every explicit-tenant custody call (`rotate`
//! enumeration/reveal, `retrieve` over a `backend: secrets` store, `tool {
//! secret: }` / `secret_partition:` injection) reached the port with an EMPTY
//! tenant and the production `PgSecretCustody` refused it fail-closed
//! ("custody requires an explicit tenant") — on the daemon AND the endpoint.
//!
//! This gate drives `execute_server_flow` end-to-end with a spying custody and
//! asserts the port sees the tenant the caller passed — the exact seam a unit
//! test over `dispatch_node` cannot cover (`reference_enterprise_flow_execution_path`).

use axon::secret_custody::{
    CustodyError, RevealedSecret, SecretCustody, SecretMetadata,
};
use std::sync::{Arc, Mutex};

/// A custody that records the tenant of every call. It never refuses on an
/// empty tenant (that is the ENT `PgSecretCustody` policy) — the point here is
/// to OBSERVE which tenant the executor threaded, so a regression (empty
/// tenant) is caught in OSS CI without a live Postgres.
#[derive(Default)]
struct SpyCustody {
    seen_tenants: Arc<Mutex<Vec<String>>>,
}

#[async_trait::async_trait]
impl SecretCustody for SpyCustody {
    async fn list_metadata(
        &self,
        tenant: &str,
        _class_prefix: &str,
    ) -> Result<Vec<SecretMetadata>, CustodyError> {
        self.seen_tenants.lock().unwrap().push(tenant.to_string());
        Ok(vec![])
    }
    async fn reveal_for_rotation(
        &self,
        tenant: &str,
        key: &str,
    ) -> Result<RevealedSecret, CustodyError> {
        self.seen_tenants.lock().unwrap().push(tenant.to_string());
        Err(CustodyError::NotFound { key: key.to_string() })
    }
    async fn commit_rotation(
        &self,
        tenant: &str,
        key: &str,
        _v: &str,
        _e: Option<i64>,
        _ev: i64,
    ) -> Result<SecretMetadata, CustodyError> {
        self.seen_tenants.lock().unwrap().push(tenant.to_string());
        Err(CustodyError::NotFound { key: key.to_string() })
    }
    async fn reveal_for_dispatch(
        &self,
        tenant: &str,
        key: &str,
    ) -> Result<RevealedSecret, CustodyError> {
        self.seen_tenants.lock().unwrap().push(tenant.to_string());
        Err(CustodyError::NotFound { key: key.to_string() })
    }
}

/// A `retrieve` over a `backend: secrets` store enumerates custody metadata —
/// the cleanest single-call custody trigger (the `rotate` sweep begins with the
/// same `list_metadata`). Driving it through `execute_server_flow` proves the
/// executor threads the tenant to the port.
const ENUM_SRC: &str = r#"
axonstore CrmTokens {
    backend: secrets
    class: crm
}

flow Enumerate() -> Unit {
    retrieve CrmTokens { where: "version > 0" as: rows }
}
"#;

fn run_enumerate_with_tenant(tenant: &str) -> Vec<String> {
    let (_p, ir) =
        axon::flow_plan::compile_source_to_ir(ENUM_SRC, "enum.axon").expect("compile");
    let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let custody = Arc::new(SpyCustody {
        seen_tenants: seen.clone(),
    });
    let empty = std::collections::HashMap::new();
    let _ = axon::runner::execute_server_flow(
        &ir,
        "Enumerate",
        "stub",
        tenant,
        "enum.axon",
        None,
        None,
        &empty,
        &empty,
        None,
        None,
        None,
        None, // budget
        None, // v2.69.0 channel semaphores
        None, // v2.69.0 — tool leases (test: none)
        None, // outbox
        None, // minter
        Some(custody as Arc<dyn SecretCustody>),
        None, // v2.63.0 dataspace_engine (tests: fail closed)
        None, // v2.56.0 scrape_overrides
);
    let out = seen.lock().unwrap().clone();
    out
}

#[test]
fn executor_threads_the_verified_tenant_to_custody() {
    let seen = run_enumerate_with_tenant("acme");
    assert!(
        !seen.is_empty(),
        "custody must be consulted for the secrets-store retrieve"
    );
    assert!(
        seen.iter().all(|t| t == "acme"),
        "the executor must thread the caller's tenant to custody — before v2.49.0 \
         this was empty and the production custody refused it: {seen:?}"
    );
}

#[test]
fn executor_passes_empty_tenant_verbatim_when_unscoped() {
    // A CLI/test with no scope passes "" → custody receives "" and (in prod)
    // fails closed. The regression we guard is the OPPOSITE: a SCOPED caller
    // whose tenant silently became "". Here we pin that "" stays "", not that
    // the spy refuses (that is the ENT policy).
    let seen = run_enumerate_with_tenant("");
    assert!(
        seen.iter().all(|t| t.is_empty()),
        "an unscoped caller's empty tenant must pass through verbatim: {seen:?}"
    );
}
