//! v2.77.0 — the OAuth token lifecycle engine (paper section 2.5).
//!
//! This is the OSS refresh ENGINE the enterprise v2.4.0 daemon drives; the daemon
//! supplies the custodied app secret + the current refresh token and persists
//! whatever this returns. The engine holds no secrets and no clock — `now_ms`
//! is passed in, so refresh decisions are deterministic and testable.
//!
//! **The per-platform mechanics the paper verified (section 2.5):**
//! - **Facebook Pages** — the long-lived Page token has NO expiry
//!   ([`RefreshMechanism::NeverExpires`]); steady-state operation needs no
//!   refresh at all (the most unattended-friendly platform).
//! - **Instagram** — long-lived tokens (~60d) refreshed by exchange before
//!   expiry ([`RefreshMechanism::LongLivedExchange`]).
//! - **TikTok** — access tokens live 24h, refresh tokens 365d, and **the
//!   refresh token MAY rotate on every use** ([`RefreshMechanism::
//!   RotatingRefreshGrant`]): a returned refresh token that differs from the one
//!   sent MUST be persisted atomically, or access is lost forever. This is the
//! trap v2.77.0 exists to close.
//! - **LinkedIn (member)** — no unattended refresh; token expiry forces the
//!   member to re-consent ([`RefreshMechanism::ReConsent`]). The daemon SURFACES
//!   a re-consent requirement; it never silently refreshes member data (ToS
//! section 4.3/section 5.2). Owned-only posture ⇒ agora targets organization assets, but the
//!   conservative member mechanic is modeled so the daemon never oversteps.

use std::time::Duration;

use crate::platform::Platform;

/// How a platform's primary unattended token is kept alive (paper section 2.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshMechanism {
    /// No expiry — never refreshed (Facebook long-lived Page token).
    NeverExpires,
    /// A long-lived token exchanged for a fresh one before expiry (Facebook /
    /// Instagram User tokens, ~60d). `valid_secs` is the token's lifetime.
    LongLivedExchange { valid_secs: u64 },
    /// A refresh-token grant whose refresh token MAY rotate on use (TikTok).
    /// `access_secs`/`refresh_secs` are the documented lifetimes.
    RotatingRefreshGrant { access_secs: u64, refresh_secs: u64 },
    /// No unattended refresh — the principal must re-consent (LinkedIn member).
    ReConsent,
}

/// The refresh mechanism for a platform's primary owned-asset token (paper section 2.5).
pub fn refresh_mechanism(platform: Platform) -> RefreshMechanism {
    match platform {
        // The long-lived Page token has no expiry — steady-state is truly
        // unattended (paper section 2.5 [FB-TOK]).
        Platform::FacebookPages => RefreshMechanism::NeverExpires,
        // ~60-day long-lived token, refreshed by exchange before expiry.
        Platform::Instagram => RefreshMechanism::LongLivedExchange { valid_secs: 60 * 86_400 },
        // access 24h, refresh 365d, rotating (paper section 2.5 [TT-OAUTH]).
        Platform::TikTok => {
            RefreshMechanism::RotatingRefreshGrant { access_secs: 86_400, refresh_secs: 365 * 86_400 }
        }
        // Member-data refresh is forbidden; expiry ⇒ re-consent (paper section 2.1 section 4.3/section 5.2).
        Platform::LinkedIn => RefreshMechanism::ReConsent,
    }
}

/// Whether a token should be refreshed NOW: `now + skew ≥ expires_at`, and the
/// mechanism actually refreshes. `NeverExpires`/`ReConsent` never trigger an
/// unattended refresh — an expired ReConsent token surfaces re-consent, it is
/// not refreshed. `skew_ms` is the lead margin (refresh BEFORE expiry — an
/// expired token cannot be exchanged, paper section 2.5).
pub fn needs_refresh(
    mechanism: RefreshMechanism,
    access_expires_at_ms: u64,
    now_ms: u64,
    skew_ms: u64,
) -> bool {
    match mechanism {
        RefreshMechanism::NeverExpires | RefreshMechanism::ReConsent => false,
        RefreshMechanism::LongLivedExchange { .. }
        | RefreshMechanism::RotatingRefreshGrant { .. } => {
            now_ms.saturating_add(skew_ms) >= access_expires_at_ms
        }
    }
}

/// The result of a refresh. The daemon persists `access_token` +
/// `access_expires_at_ms` always, and `refresh_token` when [`Self::rotated`]
/// (an unpersisted rotated refresh token is a permanent lockout — the v2.77.0 trap).
#[derive(Clone, PartialEq, Eq)]
pub struct RefreshedTokens {
    pub access_token: String,
    /// The refresh token to store going forward. `Some` and different from the
    /// one sent ⇒ the platform rotated it; persist atomically.
    pub refresh_token: Option<String>,
    pub access_expires_at_ms: u64,
}

impl RefreshedTokens {
    /// Whether the platform ROTATED the refresh token (a new value different
    /// from the one presented). A `true` here that the daemon fails to persist
    /// atomically loses access forever (TikTok, paper section 2.5).
    pub fn rotated(&self, sent_refresh_token: &str) -> bool {
        matches!(&self.refresh_token, Some(rt) if rt != sent_refresh_token)
    }
}

// The v2.48.0 redacting-Debug discipline: token values never reach a log.
impl std::fmt::Debug for RefreshedTokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RefreshedTokens")
            .field("access_token", &"<redacted>")
            .field("refresh_token", &self.refresh_token.as_ref().map(|_| "<redacted>"))
            .field("access_expires_at_ms", &self.access_expires_at_ms)
            .finish()
    }
}

/// A refresh failure.
#[derive(Debug, Clone)]
pub enum OAuthError {
    /// The platform rejected the refresh (status + message).
    Platform { status: u16, message: String },
    /// The mechanism does not refresh unattended (`NeverExpires`/`ReConsent`) —
    /// a caller asked for a refresh that must not happen.
    NotRefreshable { platform: Platform, mechanism: RefreshMechanism },
    /// Transport / IO failure.
    Transport(String),
    /// The token response was missing a required field.
    Malformed(String),
}

impl std::fmt::Display for OAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OAuthError::Platform { status, message } => {
                write!(f, "token endpoint rejected the refresh ({status}): {message}")
            }
            OAuthError::NotRefreshable { platform, mechanism } => write!(
                f,
                "the {} token mechanism {:?} does not refresh unattended — an expired token \
                 surfaces re-consent, it is never silently refreshed",
                platform.as_str(),
                mechanism
            ),
            OAuthError::Transport(e) => write!(f, "transport failure: {e}"),
            OAuthError::Malformed(e) => write!(f, "malformed token response: {e}"),
        }
    }
}

/// The token-endpoint call configuration for a refresh-grant exchange. The app
/// credentials are supplied by the caller (custodied enterprise-side) and never
/// stored here.
#[derive(Clone)]
pub struct RefreshGrantConfig {
    /// The full token endpoint URL (TikTok: `https://open.tiktokapis.com/v2/oauth/token/`).
    /// Overridable for fixture servers.
    pub token_endpoint: String,
    pub client_key: String,
    pub client_secret: String,
    pub timeout: Duration,
}

// The v2.48.0 redacting-Debug discipline: the client secret never reaches a log.
impl std::fmt::Debug for RefreshGrantConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RefreshGrantConfig")
            .field("token_endpoint", &self.token_endpoint)
            .field("client_key", &self.client_key)
            .field("client_secret", &"<redacted>")
            .field("timeout", &self.timeout)
            .finish()
    }
}

/// Perform a `grant_type=refresh_token` exchange (the TikTok/LinkedIn shape):
/// `POST` form `client_key`/`client_secret`/`grant_type`/`refresh_token`, parse
/// `{access_token, refresh_token, expires_in}`, and compute the absolute expiry
/// from `now_ms`. A rotated refresh token rides back on [`RefreshedTokens`];
/// detect it with [`RefreshedTokens::rotated`] and persist atomically.
pub fn refresh_grant(
    config: &RefreshGrantConfig,
    current_refresh_token: &str,
    now_ms: u64,
) -> Result<RefreshedTokens, OAuthError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(config.timeout)
        .build()
        .map_err(|e| OAuthError::Transport(format!("client build: {e}")))?;
    let resp = client
        .post(&config.token_endpoint)
        .form(&[
            ("client_key", config.client_key.as_str()),
            ("client_secret", config.client_secret.as_str()),
            ("grant_type", "refresh_token"),
            ("refresh_token", current_refresh_token),
        ])
        .send()
        .map_err(|e| OAuthError::Transport(e.to_string()))?;
    let status = resp.status().as_u16();
    let body: serde_json::Value = resp
        .json()
        .map_err(|e| OAuthError::Transport(format!("non-JSON response: {e}")))?;
    if status >= 400 {
        // TikTok surfaces {error, error_description}; be lenient about the shape.
        let message = body
            .pointer("/error_description")
            .or_else(|| body.pointer("/error/message"))
            .or_else(|| body.pointer("/error"))
            .and_then(|v| v.as_str())
            .unwrap_or("(no error description)")
            .to_string();
        return Err(OAuthError::Platform { status, message });
    }
    let access_token = body
        .pointer("/access_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| OAuthError::Malformed("missing access_token".to_string()))?
        .to_string();
    let expires_in = body
        .pointer("/expires_in")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| OAuthError::Malformed("missing expires_in".to_string()))?;
    // The returned refresh token may differ (rotation) or be absent (unchanged).
    let refresh_token = body
        .pointer("/refresh_token")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    Ok(RefreshedTokens {
        access_token,
        refresh_token,
        access_expires_at_ms: now_ms.saturating_add(expires_in.saturating_mul(1000)),
    })
}

/// Exchange a long-lived token for a fresh one (the Facebook/Instagram
/// `fb_exchange_token` shape): `GET` the token endpoint with the client creds +
/// the current token, parse `{access_token, expires_in?}`. There is no refresh
/// token here (the same token family self-renews), so [`RefreshedTokens::
/// refresh_token`] is `None`. An expired token CANNOT be exchanged (paper section 2.5)
/// — the daemon must call this BEFORE expiry.
pub fn refresh_long_lived(
    config: &RefreshGrantConfig,
    current_token: &str,
    now_ms: u64,
) -> Result<RefreshedTokens, OAuthError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(config.timeout)
        .build()
        .map_err(|e| OAuthError::Transport(format!("client build: {e}")))?;
    let resp = client
        .get(&config.token_endpoint)
        .query(&[
            ("grant_type", "fb_exchange_token"),
            ("client_id", config.client_key.as_str()),
            ("client_secret", config.client_secret.as_str()),
            ("fb_exchange_token", current_token),
        ])
        .send()
        .map_err(|e| OAuthError::Transport(e.to_string()))?;
    let status = resp.status().as_u16();
    let body: serde_json::Value = resp
        .json()
        .map_err(|e| OAuthError::Transport(format!("non-JSON response: {e}")))?;
    if status >= 400 {
        let message = body
            .pointer("/error/message")
            .and_then(|v| v.as_str())
            .unwrap_or("(no error message)")
            .to_string();
        return Err(OAuthError::Platform { status, message });
    }
    let access_token = body
        .pointer("/access_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| OAuthError::Malformed("missing access_token".to_string()))?
        .to_string();
    // A long-lived exchange usually omits expires_in (or reports ~60d); default
    // to 60 days so `needs_refresh` schedules the next exchange well before then.
    let expires_in = body.pointer("/expires_in").and_then(|v| v.as_u64()).unwrap_or(60 * 86_400);
    Ok(RefreshedTokens {
        access_token,
        refresh_token: None,
        access_expires_at_ms: now_ms.saturating_add(expires_in.saturating_mul(1000)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    #[test]
    fn mechanisms_match_the_paper() {
        assert_eq!(refresh_mechanism(Platform::FacebookPages), RefreshMechanism::NeverExpires);
        assert_eq!(refresh_mechanism(Platform::LinkedIn), RefreshMechanism::ReConsent);
        assert!(matches!(
            refresh_mechanism(Platform::Instagram),
            RefreshMechanism::LongLivedExchange { .. }
        ));
        assert_eq!(
            refresh_mechanism(Platform::TikTok),
            RefreshMechanism::RotatingRefreshGrant { access_secs: 86_400, refresh_secs: 365 * 86_400 }
        );
    }

    #[test]
    fn never_expires_and_reconsent_never_auto_refresh() {
        // Even a "past-expiry" timestamp does not trigger a refresh for these.
        assert!(!needs_refresh(RefreshMechanism::NeverExpires, 0, 1_000_000, 0));
        assert!(!needs_refresh(RefreshMechanism::ReConsent, 0, 1_000_000, 0));
    }

    #[test]
    fn refreshable_mechanisms_trigger_with_skew_lead() {
        let m = RefreshMechanism::RotatingRefreshGrant { access_secs: 86_400, refresh_secs: 1 };
        // expires at t=100_000; now=80_000; skew=10_000 ⇒ 90_000 < 100_000 ⇒ not yet.
        assert!(!needs_refresh(m, 100_000, 80_000, 10_000));
        // now=95_000; skew=10_000 ⇒ 105_000 ≥ 100_000 ⇒ refresh (before expiry).
        assert!(needs_refresh(m, 100_000, 95_000, 10_000));
    }

    #[test]
    fn rotated_detects_a_changed_refresh_token() {
        let same = RefreshedTokens {
            access_token: "a".into(),
            refresh_token: Some("rt-1".into()),
            access_expires_at_ms: 0,
        };
        assert!(!same.rotated("rt-1"));
        let rotated = RefreshedTokens {
            access_token: "a".into(),
            refresh_token: Some("rt-2".into()),
            access_expires_at_ms: 0,
        };
        assert!(rotated.rotated("rt-1"));
        let absent = RefreshedTokens {
            access_token: "a".into(),
            refresh_token: None,
            access_expires_at_ms: 0,
        };
        assert!(!absent.rotated("rt-1"));
    }

    #[test]
    fn redacting_debug_hides_every_token_value() {
        let t = RefreshedTokens {
            access_token: "ACCESS-SECRET".into(),
            refresh_token: Some("REFRESH-SECRET".into()),
            access_expires_at_ms: 42,
        };
        let dbg = format!("{t:?}");
        assert!(!dbg.contains("ACCESS-SECRET") && !dbg.contains("REFRESH-SECRET"));
        assert!(dbg.contains("42")); // the non-secret field is visible
        let cfg = RefreshGrantConfig {
            token_endpoint: "http://x/".into(),
            client_key: "ck".into(),
            client_secret: "CLIENT-SECRET".into(),
            timeout: Duration::from_secs(1),
        };
        assert!(!format!("{cfg:?}").contains("CLIENT-SECRET"));
    }

    /// A fixture token endpoint that ROTATES the refresh token — the TikTok
    /// trap. Returns (url, seen-refresh-tokens).
    fn spawn_rotating_token_server() -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let seen_srv = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let seen = seen_srv.clone();
                std::thread::spawn(move || {
                    let mut buf = Vec::new();
                    let mut tmp = [0u8; 1024];
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
                    let text = String::from_utf8_lossy(&buf);
                    // The sent refresh_token rides the form body; capture it.
                    let sent = text
                        .rsplit("refresh_token=")
                        .next()
                        .and_then(|s| s.split('&').next())
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    seen.lock().unwrap().push(sent.clone());
                    // Rotate: hand back a NEW refresh token.
                    let body = r#"{"access_token":"new-access","refresh_token":"rt-ROTATED","expires_in":86400}"#;
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(resp.as_bytes());
                });
            }
        });
        (format!("http://{addr}/"), seen)
    }

    #[test]
    fn refresh_grant_returns_the_rotated_token_flagged() {
        let (endpoint, seen) = spawn_rotating_token_server();
        let cfg = RefreshGrantConfig {
            token_endpoint: endpoint,
            client_key: "ck".into(),
            client_secret: "cs".into(),
            timeout: Duration::from_secs(5),
        };
        let out = refresh_grant(&cfg, "rt-ORIGINAL", 1_000).expect("refresh");
        assert_eq!(out.access_token, "new-access");
        assert_eq!(out.access_expires_at_ms, 1_000 + 86_400_000);
        // The trap: the platform rotated the refresh token — the caller MUST persist it.
        assert!(out.rotated("rt-ORIGINAL"), "a rotated refresh token must be flagged");
        assert_eq!(out.refresh_token.as_deref(), Some("rt-ROTATED"));
        // The endpoint received the ORIGINAL refresh token.
        assert_eq!(seen.lock().unwrap().as_slice(), &["rt-ORIGINAL".to_string()]);
    }

    /// A fixture that returns a fixed JSON body to any request.
    fn spawn_json_server(body: &'static str) -> String {
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

    #[test]
    fn refresh_long_lived_exchanges_with_no_rotation() {
        let endpoint = spawn_json_server(r#"{"access_token":"new-ll","expires_in":5184000}"#);
        let cfg = RefreshGrantConfig {
            token_endpoint: endpoint,
            client_key: "ck".into(),
            client_secret: "cs".into(),
            timeout: Duration::from_secs(5),
        };
        let out = refresh_long_lived(&cfg, "old-ll", 1_000).expect("exchange");
        assert_eq!(out.access_token, "new-ll");
        assert!(out.refresh_token.is_none(), "a long-lived exchange has no refresh token");
        assert_eq!(out.access_expires_at_ms, 1_000 + 5_184_000_000);
    }

    #[test]
    fn a_rejected_refresh_is_a_typed_platform_error() {
        // A dead endpoint ⇒ transport error (no live server).
        let cfg = RefreshGrantConfig {
            token_endpoint: {
                let l = TcpListener::bind("127.0.0.1:0").unwrap();
                format!("http://{}/", l.local_addr().unwrap())
            },
            client_key: "ck".into(),
            client_secret: "cs".into(),
            timeout: Duration::from_secs(2),
        };
        assert!(matches!(
            refresh_grant(&cfg, "rt", 0),
            Err(OAuthError::Transport(_))
        ));
    }
}
