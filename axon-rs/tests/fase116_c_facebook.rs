//! §Fase 116.c — the Facebook Pages connector, against a recorded-fixture
//! Graph server, end-to-end through the REAL pipeline.
//!
//! The fixture server enforces the prod invariants a real Graph endpoint
//! carries (`feedback_mock_must_enforce_prod_invariants`): every request MUST
//! present `Authorization: Bearer <token>` — a missing/wrong credential gets
//! the Graph-shaped `OAuthException` 401, never a silent 200. Responses replay
//! the documented Graph shapes (`{"data":[…]}`, `{"id":…}`, `{"error":{…}}`).
//!
//! The final tests drive the FULL production shape: EMS-compile a flow that
//! `import agora.facebook.*`, register the linked IR into the ToolRegistry,
//! register the REAL `FacebookPagesConnector` core, dispatch through the
//! registry — one real HTTP hop to the fixture server — and verify the §94.c
//! custody precedence (`axon_secret` becomes the Bearer header and never
//! reaches the output).

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axon::agora_runtime::{clear_agora_connectors, register_agora_connector};
use axon::tool_registry::ToolRegistry;
use axon_agora::{
    CallContext, ConnectorError, FacebookPagesConfig, FacebookPagesConnector, ModerationAction,
    PublishRequest, SocialConnector,
};
use axon_frontend::ems::{compile_project, EmsOptions};

/// One observed request: (method, path-with-query, authorization header,
/// request body). The body lets §116.c.3 assert the `attached_media` album.
type Seen = Arc<Mutex<Vec<(String, String, String, String)>>>;

/// A minimal deterministic Graph fixture server (std-only, one response per
/// connection, `Connection: close`). Returns its base URL + the request log.
fn spawn_graph_fixture() -> (String, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture");
    let addr = listener.local_addr().unwrap();
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let seen_srv = seen.clone();

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let seen = seen_srv.clone();
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let mut tmp = [0u8; 1024];
                // Read headers (until CRLFCRLF), then the Content-Length body.
                let header_end = loop {
                    match stream.read(&mut tmp) {
                        Ok(0) => return,
                        Ok(n) => buf.extend_from_slice(&tmp[..n]),
                        Err(_) => return,
                    }
                    if let Some(pos) = find_crlfcrlf(&buf) {
                        break pos;
                    }
                    if buf.len() > 65536 {
                        return;
                    }
                };
                let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
                let mut lines = head.lines();
                let request_line = lines.next().unwrap_or_default().to_string();
                let mut auth = String::new();
                let mut content_length = 0usize;
                for line in lines {
                    let lower = line.to_ascii_lowercase();
                    if let Some(v) = lower.strip_prefix("authorization:") {
                        // Preserve original case of the value.
                        auth = line[line.len() - v.trim_start().len()..].trim().to_string();
                    }
                    if let Some(v) = lower.strip_prefix("content-length:") {
                        content_length = v.trim().parse().unwrap_or(0);
                    }
                }
                // Read the full form-encoded body into `buf` so tests can assert
                // what was POSTed (e.g. the §116.c.3 `attached_media` album).
                let body_start = header_end + 4;
                while buf.len() - body_start < content_length {
                    match stream.read(&mut tmp) {
                        Ok(0) => break,
                        Ok(n) => buf.extend_from_slice(&tmp[..n]),
                        Err(_) => break,
                    }
                }
                let body_end = (body_start + content_length).min(buf.len());
                let req_body = String::from_utf8_lossy(&buf[body_start..body_end]).to_string();

                let mut parts = request_line.split_whitespace();
                let method = parts.next().unwrap_or_default().to_string();
                let path = parts.next().unwrap_or_default().to_string();
                seen.lock()
                    .unwrap()
                    .push((method.clone(), path.clone(), auth.clone(), req_body));

                // ── The prod invariant: no valid Bearer, no data. ────────────
                let authorized = auth == "Bearer test-token" || auth == "Bearer custody-token";
                let (status, body) = if !authorized {
                    (
                        "401 Unauthorized",
                        r#"{"error":{"message":"Invalid OAuth access token.","type":"OAuthException","code":190}}"#
                            .to_string(),
                    )
                } else {
                    route(&method, &path)
                };
                let resp = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes());
            });
        }
    });

    (format!("http://{addr}"), seen)
}

fn find_crlfcrlf(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

/// The recorded Graph response shapes, per route (all under /v21.0).
fn route(method: &str, path: &str) -> (&'static str, String) {
    let p = path.split('?').next().unwrap_or(path);
    let body = match (method, p) {
        ("GET", "/v21.0/post1/comments") => {
            r#"{"data":[{"id":"c1","from":{"name":"alice"},"message":"great post"},{"id":"c2","from":{"name":"bob"},"message":"needs work"}]}"#
        }
        ("GET", "/v21.0/page1/insights") => {
            r#"{"data":[{"name":"page_impressions","values":[{"value":1000}]},{"name":"page_post_engagements","values":[{"value":50}]},{"name":"page_fans","values":[{"value":200}]}]}"#
        }
        ("POST", "/v21.0/c1/comments") => r#"{"id":"c1_reply1"}"#,
        ("POST", "/v21.0/c1") => r#"{"success":true}"#,
        ("DELETE", "/v21.0/c2") => r#"{"success":true}"#,
        ("POST", "/v21.0/page1/feed") => r#"{"id":"page1_post9"}"#,
        ("POST", "/v21.0/page1/photos") => r#"{"id":"ph1","post_id":"page1_post10"}"#,
        ("DELETE", "/v21.0/page1_post9") => r#"{"success":true}"#,
        _ => {
            return (
                "404 Not Found",
                format!(
                    r#"{{"error":{{"message":"Unknown path: {method} {p}","type":"GraphMethodException","code":100}}}}"#
                ),
            )
        }
    };
    ("200 OK", body.to_string())
}

fn connector_for(base_url: &str, token: Option<&str>) -> FacebookPagesConnector {
    let mut cfg = FacebookPagesConfig::new("page1");
    cfg.base_url = base_url.to_string();
    cfg.access_token = token.map(str::to_string);
    cfg.timeout = Duration::from_secs(5);
    FacebookPagesConnector::new(cfg).expect("build connector")
}

// ════════════════════════════════════════════════════════════════════════════
//  Direct core tests (the recorded fixtures)
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn reads_map_the_recorded_graph_shapes() {
    let (base, _seen) = spawn_graph_fixture();
    let c = connector_for(&base, Some("test-token"));
    let ctx = CallContext::none();

    let comments = c.read_comments(&ctx, "post1").expect("comments");
    assert_eq!(comments.len(), 2);
    assert_eq!(comments[0].author, "alice");
    assert_eq!(comments[1].text, "needs work");

    let m = c.read_metrics(&ctx, "page1").expect("insights");
    assert_eq!((m.impressions, m.engagements, m.followers), (1000, 50, 200));
}

#[test]
fn writes_reply_moderate_publish_delete_against_the_fixture() {
    let (base, seen) = spawn_graph_fixture();
    let c = connector_for(&base, Some("test-token"));
    let ctx = CallContext::none();

    let r = c.reply(&ctx, "c1", "thanks!").expect("reply");
    assert_eq!(r.object_id, "c1_reply1");

    c.moderate(&ctx, "c1", ModerationAction::Hide).expect("hide");
    c.moderate(&ctx, "c2", ModerationAction::Delete).expect("delete comment");

    let text = PublishRequest { body: "hello world".into(), media_urls: vec![] };
    let receipt = c.publish(&ctx, &text).expect("publish text");
    assert_eq!(receipt.object_id, "page1_post9");

    let photo = PublishRequest {
        body: "caption".into(),
        media_urls: vec!["https://img.example/1.png".into()],
    };
    let receipt = c.publish(&ctx, &photo).expect("publish photo");
    assert_eq!(receipt.object_id, "page1_post10", "prefer the feed-visible post_id");

    c.delete(&ctx, "page1_post9").expect("delete post");

    // The fixture saw the right verbs on the right paths.
    let log = seen.lock().unwrap();
    assert!(log.iter().any(|(m, p, _, _)| m == "POST" && p.starts_with("/v21.0/page1/feed")));
    assert!(log.iter().any(|(m, p, _, _)| m == "DELETE" && p.starts_with("/v21.0/page1_post9")));
}

#[test]
fn multi_photo_publish_uploads_unpublished_containers_then_attaches_to_feed() {
    // §116.c.3 — three images become one album: three UNPUBLISHED photo
    // uploads, then a single feed post attaching all three via `attached_media`.
    let (base, seen) = spawn_graph_fixture();
    let c = connector_for(&base, Some("test-token"));
    let ctx = CallContext::none();

    let album = PublishRequest {
        body: "vacation album".into(),
        media_urls: vec![
            "https://img.example/1.png".into(),
            "https://img.example/2.png".into(),
            "https://img.example/3.png".into(),
        ],
    };
    let receipt = c.publish(&ctx, &album).expect("multi-photo publish");
    // The receipt is the FEED post (the album), not any single container.
    assert_eq!(receipt.object_id, "page1_post9");

    let log = seen.lock().unwrap();
    // Exactly one UNPUBLISHED-photo upload per image…
    let uploads: Vec<_> = log
        .iter()
        .filter(|(m, p, _, _)| m == "POST" && p == "/v21.0/page1/photos")
        .collect();
    assert_eq!(uploads.len(), 3, "one container upload per media url");
    for (_, _, _, body) in &uploads {
        // Each container is created unpublished (never a live post per photo).
        assert!(
            body.contains("published=false"),
            "container must be unpublished; body: {body}"
        );
    }
    // …then exactly one feed post attaching every container.
    let feeds: Vec<_> = log
        .iter()
        .filter(|(m, p, _, _)| m == "POST" && p == "/v21.0/page1/feed")
        .collect();
    assert_eq!(feeds.len(), 1, "one feed post carries the album");
    let feed_body = &feeds[0].3;
    // `attached_media[0]` / `media_fbid` survive URL-encoding as substrings
    // (`attached_media%5B0%5D`, `%22media_fbid%22`).
    assert!(
        feed_body.contains("attached_media") && feed_body.contains("media_fbid"),
        "feed post must attach the containers; body: {feed_body}"
    );
    // Three attachment slots (indices 0..2) for three images.
    for i in 0..3 {
        assert!(
            feed_body.contains(&format!("attached_media%5B{i}%5D"))
                || feed_body.contains(&format!("attached_media[{i}]")),
            "attached_media[{i}] must be present; body: {feed_body}"
        );
    }
    // The message rides with the album.
    assert!(feed_body.contains("message="), "the album carries its message");
}

#[test]
fn a_wrong_token_gets_the_graph_401_as_a_typed_platform_error() {
    let (base, _seen) = spawn_graph_fixture();
    let c = connector_for(&base, Some("stale-token"));
    let err = c.read_comments(&CallContext::none(), "post1").unwrap_err();
    match err {
        ConnectorError::Platform { status, message } => {
            assert_eq!(status, 401);
            assert!(message.contains("Invalid OAuth access token"), "got: {message}");
        }
        other => panic!("expected Platform 401, got: {other}"),
    }
}

#[test]
fn no_credential_fails_closed_without_touching_the_network() {
    let (base, seen) = spawn_graph_fixture();
    let c = connector_for(&base, None);
    let err = c.read_comments(&CallContext::none(), "post1").unwrap_err();
    assert!(matches!(err, ConnectorError::MissingCredential { .. }), "got: {err}");
    assert!(seen.lock().unwrap().is_empty(), "fail-closed means ZERO vendor calls");
}

#[test]
fn a_dead_endpoint_is_a_transport_error() {
    // §Fase 119.i — a PRIVILEGED port, not bind-then-drop.
    //
    // This test used to bind an ephemeral port, drop the listener, and dial the
    // now-closed address. Between the drop and the dial the OS is free to hand
    // that same port to another test's fixture server in this binary — and it
    // did, roughly one run in nine: the "dead" endpoint answered with real
    // comments and `unwrap_err()` panicked on an `Ok`. The failure looked like
    // a connector bug and was a port race.
    //
    // Port 1 cannot be bound without privilege, so no sibling test can occupy
    // it, and connecting is unprivileged — the dial refuses immediately. The
    // property under test (a dead endpoint is a Transport error, not a
    // fabricated success) is unchanged; only its determinism is.
    let dead = "http://127.0.0.1:1";
    let c = connector_for(dead, Some("test-token"));
    let err = c.read_comments(&CallContext::none(), "post1").unwrap_err();
    assert!(matches!(err, ConnectorError::Transport(_)), "got: {err}");
}

// ════════════════════════════════════════════════════════════════════════════
//  The production shape: EMS → ToolRegistry → dispatch → real HTTP
// ════════════════════════════════════════════════════════════════════════════

/// Serialises registry-touching tests (process-global connector registry).
static REG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn compile_facebook_flow() -> axon_frontend::ems::EmsSuccess {
    let dir = std::env::temp_dir().join(format!(
        "fase116c-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let entry = dir.join("main.axon");
    std::fs::write(
        &entry,
        r#"import agora.facebook.{ facebook_read_comments, facebook_publish_post }

credential PageAuth { ttl: 1h grants: [pages_read_engagement, pages_manage_posts] }

type Digest { text: String }

flow ModeratePage(post: String) -> Digest {
  use facebook_read_comments(target = "${post}")
  step Summarize { ask: "digest the comments" output: Digest }
}
"#,
    )
    .expect("write entry");
    let opts = EmsOptions {
        modules_root: Some(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../axon-agora/modules"),
        ),
        use_cache: false,
        cache_dir: None,
    };
    match compile_project(&entry, &opts) {
        Ok(s) => s,
        Err(f) => panic!("facebook flow failed to compile: {:?}", f),
    }
}

#[test]
fn ems_to_registry_to_connector_one_real_http_hop() {
    let _g = REG_LOCK.lock().unwrap();
    let (base, _seen) = spawn_graph_fixture();
    let success = compile_facebook_flow();

    let mut registry = ToolRegistry::new();
    registry.register_from_ir(&success.ir.tools);

    clear_agora_connectors();
    register_agora_connector(Arc::new(connector_for(&base, Some("test-token"))));

    let out = registry
        .dispatch("facebook_read_comments", r#"{"target":"post1"}"#)
        .expect("agora providers dispatch locally");
    assert!(out.success, "got: {}", out.output);
    assert!(out.output.contains("great post"), "got: {}", out.output);
    clear_agora_connectors();
}

/// §116.c.4 — the flow-reachability e2e for multi-photo. A flow that binds a
/// `List<String>` argument in a `use` now BOTH type-checks (the surface accepts
/// `media_urls: List<String>?`) AND, once the runner materializes that list into
/// a JSON array (proven by the runner's `build_body_emits_a_list_param_as_a_json_array`
/// unit test), drives the connector's attached_media album through dispatch_agora
/// → build_publish_request. Here we compile such a flow and dispatch the exact
/// array-shaped argument the runner builds, asserting the fixture sees the album.
#[test]
fn multi_photo_flow_typechecks_and_dispatch_materializes_the_album() {
    let _g = REG_LOCK.lock().unwrap();
    let (base, seen) = spawn_graph_fixture();

    // (a) The flow with a List-typed `use` argument compiles.
    let dir = std::env::temp_dir().join(format!(
        "fase116c-album-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let entry = dir.join("main.axon");
    std::fs::write(
        &entry,
        r#"import agora.facebook.{ facebook_publish_post }

credential PageAuth { ttl: 1h grants: [pages_manage_posts] }

type Digest { text: String }

flow PostAlbum(caption: String) -> Digest {
  use facebook_publish_post(body = "${caption}", media_urls = ["https://a/1.png", "https://a/2.png", "https://a/3.png"])
  step Summarize { ask: "confirm the album" output: Digest }
}
"#,
    )
    .expect("write entry");
    let opts = EmsOptions {
        modules_root: Some(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../axon-agora/modules"),
        ),
        use_cache: false,
        cache_dir: None,
    };
    let success = compile_project(&entry, &opts).expect("album flow compiles with a List arg");

    // (b) Dispatch the array-shaped argument the runner assembles for that `use`.
    let mut registry = ToolRegistry::new();
    registry.register_from_ir(&success.ir.tools);
    clear_agora_connectors();
    register_agora_connector(Arc::new(connector_for(&base, Some("test-token"))));

    let out = registry
        .dispatch(
            "facebook_publish_post",
            r#"{"body":"album","media_urls":["https://a/1.png","https://a/2.png","https://a/3.png"]}"#,
        )
        .expect("dispatch multi-photo publish");
    assert!(out.success, "got: {}", out.output);
    clear_agora_connectors();

    // The connector ran the attached_media album: 3 unpublished uploads + 1 feed.
    let log = seen.lock().unwrap();
    let uploads = log
        .iter()
        .filter(|(m, p, _, _)| m == "POST" && p == "/v21.0/page1/photos")
        .count();
    assert_eq!(uploads, 3, "one unpublished container per media url");
    let feed = log
        .iter()
        .find(|(m, p, _, _)| m == "POST" && p == "/v21.0/page1/feed")
        .expect("feed post");
    assert!(
        feed.3.contains("attached_media") && feed.3.contains("media_fbid"),
        "feed attaches the album; body: {}",
        feed.3
    );
}

/// §94.c precedence, end to end: the custody-injected `axon_secret` becomes the
/// Bearer header (overriding the connector's stale config token), and the value
/// never reaches the output.
#[test]
fn custody_injected_secret_takes_precedence_and_never_leaks() {
    let _g = REG_LOCK.lock().unwrap();
    let (base, seen) = spawn_graph_fixture();
    let success = compile_facebook_flow();

    let mut registry = ToolRegistry::new();
    registry.register_from_ir(&success.ir.tools);

    clear_agora_connectors();
    // The connector's own config token is STALE — only the custody token works.
    register_agora_connector(Arc::new(connector_for(&base, Some("stale-token"))));

    let out = registry
        .dispatch(
            "facebook_read_comments",
            r#"{"target":"post1","axon_secret":"custody-token"}"#,
        )
        .expect("dispatched locally");
    assert!(out.success, "custody token must authorize the call — got: {}", out.output);
    assert!(
        !out.output.contains("custody-token"),
        "the custody value leaked into the outcome: {}",
        out.output
    );
    let log = seen.lock().unwrap();
    assert!(
        log.iter().any(|(_, _, a, _)| a == "Bearer custody-token"),
        "the fixture must have seen the custody Bearer; saw: {:?}",
        log.iter().map(|(_, _, a, _)| a.clone()).collect::<Vec<_>>()
    );
    drop(log);
    clear_agora_connectors();
}
