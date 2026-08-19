//! v2.77.0 — the agora token-refresh orchestration.
//!
//! The OSS refresh CORE the enterprise v2.4.0 daemon drives: it enumerates a
//! tenant's agora tokens, decides which need refresh (via the v2.77.0 oauth
//! engine), performs the exchange, and **atomically** persists the result to the
//! secret vault — closing the rotating-refresh-token trap (a TikTok refresh
//! token that rotates but is not persisted is a permanent lockout). The
//! enterprise side is a thin background task that calls [`refresh_once`] on a
//! timer with the real [`PgSecretCustody`](crate::secret_custody) and per-tenant
//! enumeration; the logic + its invariants are tested here against an in-memory
//! custody + fixture token endpoints.
//!
//! **Key layout** (per tenant): `agora.<platform>.token` is the ACCESS token
//! (what v2.48.0 injects as the Bearer; its `expires_at` drives the schedule);
//! `agora.<platform>.refresh` is the refresh token (rotating platforms);
//! `agora.<platform>.client_secret` is the custodied app secret. The client key
//! is public config, not custodied.
//!
//! **Discipline** (the tenant-sweeper posture, an earlier release): per-token failures are
//! recorded in the report and the sweep continues; a CAS conflict means another
//! rotator won — the loser NEVER retries with the stale revealed value (custody
//! `VersionConflict`, "never double-spend a refresh credential").

use axon_agora::{oauth, Platform};

use crate::secret_custody::{CustodyError, SecretCustody};

/// The per-platform OAuth token endpoints (config; tests point at fixtures).
#[derive(Clone, Debug)]
pub struct TokenEndpoints {
    pub tiktok: String,
    pub facebook: String,
    pub instagram: String,
    pub linkedin: String,
}

impl TokenEndpoints {
    fn for_platform(&self, p: Platform) -> &str {
        match p {
            Platform::TikTok => &self.tiktok,
            Platform::FacebookPages => &self.facebook,
            Platform::Instagram => &self.instagram,
            Platform::LinkedIn => &self.linkedin,
        }
    }
}

/// The (public) client key per platform. The client SECRET is custodied.
#[derive(Clone, Debug, Default)]
pub struct ClientKeys {
    pub tiktok: String,
    pub facebook: String,
    pub instagram: String,
    pub linkedin: String,
}

impl ClientKeys {
    fn for_platform(&self, p: Platform) -> &str {
        match p {
            Platform::TikTok => &self.tiktok,
            Platform::FacebookPages => &self.facebook,
            Platform::Instagram => &self.instagram,
            Platform::LinkedIn => &self.linkedin,
        }
    }
}

/// The outcome of one refresh sweep for a tenant — counts, so a test can assert
/// the work done without racy log inspection (the an earlier release sweeper posture).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RefreshReport {
    /// Access-token entries examined.
    pub checked: u32,
    /// Access tokens refreshed (exchange or refresh-grant succeeded + committed).
    pub refreshed: u32,
    /// Rotating refresh tokens that the platform rotated AND we persisted
    /// atomically (the TikTok trap, closed).
    pub rotated: u32,
    /// Tokens whose mechanism forbids unattended refresh — a re-consent was
    /// surfaced instead (LinkedIn member data, never silently refreshed).
    pub reconsent_surfaced: u32,
    /// Tokens skipped because the mechanism never expires (Facebook Page).
    pub skipped_never_expires: u32,
    /// Tokens not yet due for refresh.
    pub skipped_not_due: u32,
    /// CAS conflicts — another rotator committed first; we did NOT retry.
    pub cas_conflicts: u32,
    /// Per-token errors (custody/transport), recorded and stepped over.
    pub errors: u32,
}

/// The class prefix under which agora tokens live.
const AGORA_CLASS: &str = "agora.";

/// Parse the platform out of an `agora.<platform>.token` access-token key;
/// `None` for any other key (refresh tokens, client secrets, foreign classes).
fn platform_of_access_key(key: &str) -> Option<Platform> {
    let seg = key.strip_prefix(AGORA_CLASS)?.strip_suffix(".token")?;
    Platform::from_provider(&format!("agora_{seg}"))
}

/// Run one refresh sweep for `tenant`. `now_ms`/`skew_ms` drive the schedule
/// (refresh BEFORE expiry); the clock is injected for determinism.
pub async fn refresh_once(
    custody: &dyn SecretCustody,
    tenant: &str,
    endpoints: &TokenEndpoints,
    client_keys: &ClientKeys,
    now_ms: u64,
    skew_ms: u64,
) -> RefreshReport {
    let mut report = RefreshReport::default();

    let entries = match custody.list_metadata(tenant, AGORA_CLASS).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(tenant, error = %e, "agora_refresh: list_metadata failed");
            report.errors += 1;
            return report;
        }
    };

    for meta in entries {
        let Some(platform) = platform_of_access_key(&meta.key) else {
            continue; // not an access-token key (refresh token / client secret)
        };
        report.checked += 1;
        let mechanism = oauth::refresh_mechanism(platform);

        match mechanism {
            oauth::RefreshMechanism::NeverExpires => {
                report.skipped_never_expires += 1;
            }
            oauth::RefreshMechanism::ReConsent => {
                // Never auto-refresh member data (LinkedIn ToS section 4.3/section 5.2). If the
                // token is at/near expiry, SURFACE a re-consent requirement.
                let due = meta
                    .expires_at_ms
                    .map(|e| now_ms.saturating_add(skew_ms) >= e as u64)
                    .unwrap_or(false);
                if due {
                    report.reconsent_surfaced += 1;
                    tracing::info!(
                        tenant, key = %meta.key,
                        "agora_refresh: re-consent required (member token — never auto-refreshed)"
                    );
                }
            }
            oauth::RefreshMechanism::LongLivedExchange { .. }
            | oauth::RefreshMechanism::RotatingRefreshGrant { .. } => {
                let due = oauth::needs_refresh(
                    mechanism,
                    meta.expires_at_ms.unwrap_or(0) as u64,
                    now_ms,
                    skew_ms,
                );
                if !due {
                    report.skipped_not_due += 1;
                    continue;
                }
                match refresh_one(custody, tenant, platform, mechanism, &meta, endpoints, client_keys, now_ms).await {
                    Ok(rotated) => {
                        report.refreshed += 1;
                        if rotated {
                            report.rotated += 1;
                        }
                    }
                    Err(RefreshFail::Cas) => report.cas_conflicts += 1,
                    Err(RefreshFail::Other) => report.errors += 1,
                }
            }
        }
    }
    report
}

enum RefreshFail {
    /// A CAS conflict — do NOT retry with the stale revealed value.
    Cas,
    Other,
}

/// Refresh one refreshable token: reveal → exchange → commit atomically.
/// Returns `Ok(true)` if the refresh token rotated and was persisted.
#[allow(clippy::too_many_arguments)]
async fn refresh_one(
    custody: &dyn SecretCustody,
    tenant: &str,
    platform: Platform,
    mechanism: oauth::RefreshMechanism,
    access_meta: &crate::secret_custody::SecretMetadata,
    endpoints: &TokenEndpoints,
    client_keys: &ClientKeys,
    now_ms: u64,
) -> Result<bool, RefreshFail> {
    let seg = platform.as_str();
    let refresh_key = format!("{AGORA_CLASS}{seg}.refresh");
    let secret_key = format!("{AGORA_CLASS}{seg}.client_secret");

    // The custodied app secret (server-side only, never agent-visible).
    let client_secret = custody
        .reveal_for_rotation(tenant, &secret_key)
        .await
        .map_err(map_fail)?;

    let cfg = oauth::RefreshGrantConfig {
        token_endpoint: endpoints.for_platform(platform).to_string(),
        client_key: client_keys.for_platform(platform).to_string(),
        client_secret: client_secret.value.clone(),
        timeout: std::time::Duration::from_secs(30),
    };

    // Which credential the exchange consumes: the separate refresh token
    // (RotatingRefreshGrant) or the access token itself (LongLivedExchange).
    let is_rotating = matches!(mechanism, oauth::RefreshMechanism::RotatingRefreshGrant { .. });
    let (consumed, consumed_meta) = if is_rotating {
        let rt = custody.reveal_for_rotation(tenant, &refresh_key).await.map_err(map_fail)?;
        (rt.value.clone(), Some((refresh_key.clone(), rt.version)))
    } else {
        let at = custody.reveal_for_rotation(tenant, &access_meta.key).await.map_err(map_fail)?;
        (at.value.clone(), None)
    };

    // The exchange does blocking HTTP — isolate off the async runtime.
    let cfg2 = cfg.clone();
    let consumed2 = consumed.clone();
    let refreshed = tokio::task::spawn_blocking(move || {
        if is_rotating {
            oauth::refresh_grant(&cfg2, &consumed2, now_ms)
        } else {
            oauth::refresh_long_lived(&cfg2, &consumed2, now_ms)
        }
    })
    .await
    .map_err(|_| RefreshFail::Other)?
    .map_err(|e| {
        tracing::warn!(tenant, platform = seg, error = %e, "agora_refresh: exchange failed");
        RefreshFail::Other
    })?;

    // Persist the new ACCESS token (CAS on its version).
    custody
        .commit_rotation(
            tenant,
            &access_meta.key,
            &refreshed.access_token,
            Some(refreshed.access_expires_at_ms as i64),
            access_meta.version,
        )
        .await
        .map_err(map_fail)?;

    // If the platform rotated the refresh token, persist it ATOMICALLY — the
    // trap. (`consumed_meta` carries the refresh key + its expected version.)
    let mut rotated = false;
    if let Some((rkey, rversion)) = consumed_meta {
        if refreshed.rotated(&consumed) {
            if let Some(new_rt) = &refreshed.refresh_token {
                let refresh_secs = match mechanism {
                    oauth::RefreshMechanism::RotatingRefreshGrant { refresh_secs, .. } => refresh_secs,
                    _ => 0,
                };
                let refresh_exp = now_ms.saturating_add(refresh_secs.saturating_mul(1000)) as i64;
                custody
                    .commit_rotation(tenant, &rkey, new_rt, Some(refresh_exp), rversion)
                    .await
                    .map_err(map_fail)?;
                rotated = true;
            }
        }
    }
    Ok(rotated)
}

fn map_fail(e: CustodyError) -> RefreshFail {
    match e {
        CustodyError::VersionConflict { .. } => RefreshFail::Cas,
        _ => RefreshFail::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret_custody::{RevealedSecret, SecretMetadata};
    use std::collections::HashMap;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Mutex;

    /// An in-memory custody with the CAS discipline, for testing the daemon core.
    #[derive(Default)]
    struct MemCustody {
        // key -> (value, version, expires_at_ms)
        rows: Mutex<HashMap<String, (String, i64, Option<i64>)>>,
        /// When set, every `commit_rotation` returns a CAS conflict (simulating
        /// a concurrent rotator that committed first).
        force_cas: std::sync::atomic::AtomicBool,
    }
    impl MemCustody {
        fn seed(&self, key: &str, value: &str, version: i64, expires_at_ms: Option<i64>) {
            self.rows.lock().unwrap().insert(key.into(), (value.into(), version, expires_at_ms));
        }
        fn value_of(&self, key: &str) -> Option<String> {
            self.rows.lock().unwrap().get(key).map(|(v, _, _)| v.clone())
        }
        fn version_of(&self, key: &str) -> Option<i64> {
            self.rows.lock().unwrap().get(key).map(|(_, ver, _)| *ver)
        }
    }

    #[async_trait::async_trait]
    impl SecretCustody for MemCustody {
        async fn list_metadata(
            &self,
            _tenant: &str,
            class_prefix: &str,
        ) -> Result<Vec<SecretMetadata>, CustodyError> {
            let rows = self.rows.lock().unwrap();
            Ok(rows
                .iter()
                .filter(|(k, _)| k.starts_with(class_prefix))
                .map(|(k, (_, ver, exp))| SecretMetadata {
                    key: k.clone(),
                    version: *ver,
                    created_at_ms: 0,
                    expires_at_ms: *exp,
                })
                .collect())
        }
        async fn reveal_for_rotation(
            &self,
            _tenant: &str,
            key: &str,
        ) -> Result<RevealedSecret, CustodyError> {
            let rows = self.rows.lock().unwrap();
            rows.get(key)
                .map(|(v, ver, exp)| RevealedSecret {
                    value: v.clone(),
                    version: *ver,
                    expires_at_ms: *exp,
                })
                .ok_or_else(|| CustodyError::NotFound { key: key.into() })
        }
        async fn commit_rotation(
            &self,
            _tenant: &str,
            key: &str,
            new_value: &str,
            expires_at_ms: Option<i64>,
            expected_version: i64,
        ) -> Result<SecretMetadata, CustodyError> {
            if self.force_cas.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(CustodyError::VersionConflict { key: key.into(), expected: expected_version });
            }
            let mut rows = self.rows.lock().unwrap();
            let entry = rows.get_mut(key).ok_or_else(|| CustodyError::NotFound { key: key.into() })?;
            if entry.1 != expected_version {
                return Err(CustodyError::VersionConflict { key: key.into(), expected: expected_version });
            }
            entry.0 = new_value.to_string();
            entry.1 += 1;
            entry.2 = expires_at_ms;
            Ok(SecretMetadata {
                key: key.into(),
                version: entry.1,
                created_at_ms: 0,
                expires_at_ms,
            })
        }
        async fn reveal_for_dispatch(
            &self,
            tenant: &str,
            key: &str,
        ) -> Result<RevealedSecret, CustodyError> {
            self.reveal_for_rotation(tenant, key).await
        }
    }

    /// A token endpoint that rotates the refresh token (TikTok shape).
    fn spawn_rotating_endpoint() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                std::thread::spawn(move || {
                    let mut tmp = [0u8; 1024];
                    let mut buf = Vec::new();
                    loop {
                        match stream.read(&mut tmp) {
                            Ok(0) => return,
                            Ok(n) => buf.extend_from_slice(&tmp[..n]),
                            Err(_) => return,
                        }
                        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    let body = r#"{"access_token":"new-access","refresh_token":"rt-ROTATED","expires_in":86400}"#;
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(resp.as_bytes());
                });
            }
        });
        format!("http://{addr}/")
    }

    fn endpoints(tiktok: String) -> TokenEndpoints {
        TokenEndpoints {
            tiktok,
            facebook: "http://127.0.0.1:9/".into(),
            instagram: "http://127.0.0.1:9/".into(),
            linkedin: "http://127.0.0.1:9/".into(),
        }
    }

    #[tokio::test]
    async fn tiktok_refresh_rotates_and_persists_both_tokens_atomically() {
        let custody = MemCustody::default();
        // access token expiring soon (now=1000, expires=1500, skew=1000 ⇒ due).
        custody.seed("agora.tiktok.token", "old-access", 5, Some(1500));
        custody.seed("agora.tiktok.refresh", "rt-ORIGINAL", 3, Some(9_999_999_999));
        custody.seed("agora.tiktok.client_secret", "app-secret", 1, None);

        let ep = endpoints(spawn_rotating_endpoint());
        let report = refresh_once(&custody, "acme", &ep, &ClientKeys::default(), 1_000, 1_000).await;

        assert_eq!(report.refreshed, 1, "the access token was refreshed");
        assert_eq!(report.rotated, 1, "the rotated refresh token was persisted");
        assert_eq!(report.cas_conflicts, 0);
        assert_eq!(report.errors, 0);

        // The vault now holds the NEW access token (version bumped)...
        assert_eq!(custody.value_of("agora.tiktok.token").as_deref(), Some("new-access"));
        assert_eq!(custody.version_of("agora.tiktok.token"), Some(6));
        // ...and the ROTATED refresh token was persisted (the trap, closed).
        assert_eq!(custody.value_of("agora.tiktok.refresh").as_deref(), Some("rt-ROTATED"));
        assert_eq!(custody.version_of("agora.tiktok.refresh"), Some(4));
    }

    #[tokio::test]
    async fn facebook_never_expires_is_skipped() {
        let custody = MemCustody::default();
        custody.seed("agora.facebook.token", "page-token", 1, None);
        let ep = endpoints("http://127.0.0.1:9/".into());
        let report = refresh_once(&custody, "acme", &ep, &ClientKeys::default(), 1_000, 1_000).await;
        assert_eq!(report.skipped_never_expires, 1);
        assert_eq!(report.refreshed, 0);
        // The token is untouched.
        assert_eq!(custody.value_of("agora.facebook.token").as_deref(), Some("page-token"));
    }

    #[tokio::test]
    async fn linkedin_member_expiry_surfaces_reconsent_never_refreshes() {
        let custody = MemCustody::default();
        // Expiring member token — must NOT be refreshed, only surfaced.
        custody.seed("agora.linkedin.token", "member-token", 2, Some(1500));
        let ep = endpoints("http://127.0.0.1:9/".into());
        let report = refresh_once(&custody, "acme", &ep, &ClientKeys::default(), 1_000, 1_000).await;
        assert_eq!(report.reconsent_surfaced, 1);
        assert_eq!(report.refreshed, 0);
        // The token was NOT rotated (version unchanged, value unchanged).
        assert_eq!(custody.version_of("agora.linkedin.token"), Some(2));
        assert_eq!(custody.value_of("agora.linkedin.token").as_deref(), Some("member-token"));
    }

    #[tokio::test]
    async fn a_not_yet_due_token_is_left_alone() {
        let custody = MemCustody::default();
        // expires far in the future (now=1000, expires=10_000_000, skew=1000).
        custody.seed("agora.tiktok.token", "old-access", 5, Some(10_000_000));
        custody.seed("agora.tiktok.refresh", "rt", 1, None);
        custody.seed("agora.tiktok.client_secret", "s", 1, None);
        let ep = endpoints("http://127.0.0.1:9/".into());
        let report = refresh_once(&custody, "acme", &ep, &ClientKeys::default(), 1_000, 1_000).await;
        assert_eq!(report.skipped_not_due, 1);
        assert_eq!(report.refreshed, 0);
    }

    #[tokio::test]
    async fn a_cas_conflict_is_recorded_and_the_token_is_left_untouched() {
        let custody = MemCustody::default();
        custody.seed("agora.tiktok.token", "old-access", 5, Some(1500));
        custody.seed("agora.tiktok.refresh", "rt-ORIGINAL", 3, Some(9_999_999_999));
        custody.seed("agora.tiktok.client_secret", "s", 1, None);
        // Another rotator committed first: every commit CAS-conflicts.
        custody.force_cas.store(true, std::sync::atomic::Ordering::SeqCst);

        let ep = endpoints(spawn_rotating_endpoint());
        let report = refresh_once(&custody, "acme", &ep, &ClientKeys::default(), 1_000, 1_000).await;

        assert_eq!(report.cas_conflicts, 1, "the CAS loss is recorded");
        assert_eq!(report.refreshed, 0, "a CAS loser never counts as refreshed");
        assert_eq!(report.rotated, 0);
        // The loser did NOT retry with the stale revealed value — nothing changed.
        assert_eq!(custody.value_of("agora.tiktok.token").as_deref(), Some("old-access"));
        assert_eq!(custody.value_of("agora.tiktok.refresh").as_deref(), Some("rt-ORIGINAL"));
    }

    #[test]
    fn cas_error_classifies_as_cas_others_as_other() {
        assert!(matches!(
            map_fail(CustodyError::VersionConflict { key: "k".into(), expected: 6 }),
            RefreshFail::Cas
        ));
        assert!(matches!(map_fail(CustodyError::NotFound { key: "k".into() }), RefreshFail::Other));
    }

    #[test]
    fn access_key_platform_parsing() {
        assert_eq!(platform_of_access_key("agora.tiktok.token"), Some(Platform::TikTok));
        assert_eq!(platform_of_access_key("agora.facebook.token"), Some(Platform::FacebookPages));
        assert_eq!(platform_of_access_key("agora.tiktok.refresh"), None);
        assert_eq!(platform_of_access_key("agora.tiktok.client_secret"), None);
        assert_eq!(platform_of_access_key("crm.hubspot"), None);
    }
}
