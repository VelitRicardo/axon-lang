//! v2.81.0 — this suite exercises the HTTP server surface (the `axum`
//! router, the tenant extractor, the session carriers, the SSE dialect
//! adapters), which lives behind the `server` feature. Gated at the file level:
//! the core suites keep running in a `cli`-only build.
#![cfg(feature = "server")]

//! v2.67.0 — the OSS server SERVES the session-typed WebSocket.
//!
//! # What was missing
//!
//! **The OSS server had no WebSocket route at all.** Not a stub — none. Meanwhile
//! this repo's public README leads with *"session-typed WebSocket dialogue as a
//! cognitive primitive"*, calls it the first of its kind in any language, and its
//! two-repo note says **this** repo ships "the language + runtime + … + WebSocket
//! session types".
//!
//! Every piece existed: `session_runtime::drive` is a complete, e2e-tested
//! protocol loop; `SessionRuntime` is a real cursor machine; the Honda–Vasconcelos
//! duality proof is genuine; and the enterprise server serves the wire by driving
//! *this same* OSS runtime. **An OSS adopter simply had no door to open**, and so
//! could not evaluate the central claim of the project at all.
//!
//! # The deeper defect this closes
//!
//! The enterprise path — the one that *did* serve — wrote its own situation down
//! under a heading called **"SessionType resolution honesty"**: the IR's
//! `protocol:` string was treated as an *opaque identifier*, and **every deployed
//! socket got a hardcoded canonical chat schema**. So an adopter could declare a
//! protocol, have its duality **proven** at compile time, deploy it — and the
//! runtime would enforce **a different protocol**.
//!
//! A proof about a protocol you do not run is not a proof about anything.
//!
//! v2.67.0 adds the missing SessionType compiler (`session_runtime::compile`), so
//! the schema the runtime enforces is the one the adopter **wrote** — and an
//! unresolvable protocol is **refused**, never substituted.
//!
//! Pins:
//! 1. A declared `socket` is reachable at `GET /ws/{name}` and the handshake
//!    succeeds (the route did not exist).
//! 2. The dialogue follows **the declared protocol** — the server receives what
//!    the adopter said it receives.
//! 3. An **off-protocol** frame is REFUSED by the runtime (the duality proof is
//!    enforced on the wire, not merely at compile time).
//! 4. A socket whose `protocol:` names no declared `session` is **refused** —
//!    never handed a substitute schema.
//! 5. An undeployed socket 404s.

use axum::body::Body;
use axum::http::Request;
use futures::{SinkExt, StreamExt};
use http_body_util::BodyExt;
use tokio_tungstenite::tungstenite::Message;
use tower::util::ServiceExt;

fn server_cfg() -> axon::axon_server::ServerConfig {
    axon::axon_server::ServerConfig {
        host: "127.0.0.1".into(),
        port: 0,
        channel: "memory".into(),
        auth_token: String::new(),
        log_level: "INFO".into(),
        log_format: "json".into(),
        log_file: None,
        database_url: None,
        config_path: None,
        strict_type_driven_transport: false,
        default_backend: None,
        schemas_dir: None,
    }
}

/// `session Trade` — the broker RECEIVES an Order, then SENDS a Fill.
/// `socket Wire` carries it.
const PROGRAM: &str = r#"
type Order { sku: String }
type Fill { id: String }
session Trade {
    broker: [ receive Order, send Fill, end ]
    client: [ send Order, receive Fill, end ]
}
socket Wire { protocol: Trade }
"#;

/// A socket whose protocol does not exist. The upgrade must be REFUSED, not
/// served a canonical shape.
const GHOST: &str = r#"
socket Ghost { protocol: NoSuchProtocol }
"#;

async fn deploy(app: &axum::Router, src: &str) -> serde_json::Value {
    let body = serde_json::json!({ "source": src, "filename": "t.axon", "backend": "stub" });
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/deploy")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

/// Boot the real router on a real TCP port and return its address.
async fn boot(app: axum::Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("ws://{addr}")
}

// ── 1-3. The wire is served, and it speaks the DECLARED protocol ────────────

/// The flagship. Deploy a program, open a real WebSocket, and speak the protocol
/// the adopter wrote.
#[tokio::test]
async fn a_deployed_fixture_is_served_and_follows_its_declared_protocol() {
    // v2.89.0 — the same dialogue, from a program that lives on disk.
    //
    // This gate already deploys through the real path and boots a real TCP
    // listener, so it was always a path proof. Its only Law 4 gap is that the
    // program is a Rust `const`, so nothing on disk holds it and no build can
    // catch the citation going stale.
    const FIXTURE: &str = "tests/fixtures/socket/trade_dialogue.axon";
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE);
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));

    let (app, _state) = axon::axon_server::build_router_with_state(server_cfg());
    let out = deploy(&app, &src).await;
    assert_eq!(out["success"], true, "the fixture must deploy: {out}");

    let base = boot(app).await;
    let (mut ws, resp) = tokio_tungstenite::connect_async(format!("{base}/ws/Wire"))
        .await
        .expect("the declared socket must be SERVED");
    assert_eq!(resp.status(), 101, "the upgrade must be accepted");

    let order = serde_json::json!({
        "v": 1, "kind": "send", "payload_type": "Order", "data": { "sku": "AXN" }
    });
    ws.send(Message::Text(order.to_string().into()))
        .await
        .expect("send Order");

    let reply = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next())
        .await
        .expect("the server must answer within the protocol")
        .expect("a frame")
        .expect("a valid frame");
    let text = match reply {
        Message::Text(t) => t.to_string(),
        other => panic!("expected a text frame, got {other:?}"),
    };
    let frame: serde_json::Value = serde_json::from_str(&text).expect("a JSON frame");
    assert_eq!(
        frame["payload_type"], "Fill",
        "the server must send what the FIXTURE's `session Trade` declares the broker sends. \
         The enterprise path once substituted a canonical chat schema here, so the protocol \
         proven at compile time was not the protocol enforced (v2.67.0 v1.6.0). Got: {frame}"
    );
}

#[tokio::test]
async fn the_declared_socket_is_served_and_follows_its_declared_protocol() {
    let (app, _state) = axon::axon_server::build_router_with_state(server_cfg());
    let out = deploy(&app, PROGRAM).await;
    assert_eq!(out["success"], true, "deploy must succeed: {out}");

    let base = boot(app).await;
    let (mut ws, resp) = tokio_tungstenite::connect_async(format!("{base}/ws/Wire"))
        .await
        .expect(
            "the OSS server must SERVE the session-typed WebSocket — before v2.67.0 there was no \
             route at all, while the README advertised session-typed dialogue as the language's \
             headline feature",
        );
    assert_eq!(resp.status(), 101, "the upgrade must be accepted");

    // The broker's protocol is `?Order.!Fill.end` — so the CLIENT sends Order.
    let order = serde_json::json!({
        "v": 1, "kind": "send", "payload_type": "Order", "data": { "sku": "AXN" }
    });
    ws.send(Message::Text(order.to_string().into()))
        .await
        .expect("send Order");

    // The server, whose cursor is now at `!Fill`, emits the Fill.
    let reply = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next())
        .await
        .expect("the server must answer within the protocol")
        .expect("a frame")
        .expect("a valid frame");

    let text = match reply {
        Message::Text(t) => t.to_string(),
        other => panic!("expected a text frame, got {other:?}"),
    };
    let frame: serde_json::Value = serde_json::from_str(&text).expect("a JSON frame");
    assert_eq!(
        frame["payload_type"], "Fill",
        "the server must send a `Fill` — that is what `session Trade` DECLARES the broker sends. \
         The enterprise path substituted a canonical chat schema here, so the protocol proven at \
         compile time was not the protocol enforced (v2.67.0 v1.6.0). Got: {frame}"
    );
}

/// **The duality proof, enforced on the wire.** The broker's cursor is at
/// `?Order`. Sending it a `Fill` instead is off-protocol, and the runtime must
/// REFUSE it — closing with `1002 protocol error` rather than accepting a frame
/// the type says cannot occur.
#[tokio::test]
async fn an_off_protocol_frame_is_refused_on_the_wire() {
    let (app, _state) = axon::axon_server::build_router_with_state(server_cfg());
    assert_eq!(deploy(&app, PROGRAM).await["success"], true);
    let base = boot(app).await;

    let (mut ws, _) = tokio_tungstenite::connect_async(format!("{base}/ws/Wire"))
        .await
        .expect("upgrade");

    // The cursor is at `?Order`. Send a `Fill` — a message the protocol forbids here.
    let wrong = serde_json::json!({
        "v": 1, "kind": "send", "payload_type": "Fill", "data": { "id": "x" }
    });
    ws.send(Message::Text(wrong.to_string().into()))
        .await
        .expect("send");

    // The runtime must reject it: an `error` frame and/or a close. Either way the
    // dialogue must NOT proceed as if the frame were valid.
    let mut refused = false;
    while let Ok(Some(Ok(msg))) =
        tokio::time::timeout(std::time::Duration::from_secs(5), ws.next()).await
    {
        match msg {
            Message::Text(t) => {
                if t.contains("error") {
                    refused = true;
                    break;
                }
                panic!(
                    "the server ACCEPTED an off-protocol frame and answered `{t}` — the duality \
                     proof must hold on the wire, not just at compile time"
                );
            }
            Message::Close(frame) => {
                refused = true;
                if let Some(f) = frame {
                    assert_eq!(
                        u16::from(f.code),
                        1002,
                        "an off-protocol frame must close with 1002 protocol error"
                    );
                }
                break;
            }
            _ => {}
        }
    }
    assert!(
        refused,
        "an off-protocol frame must be REFUSED — silence would be indistinguishable from \
         acceptance"
    );
}

// ── 4-5. Refusals: we never serve a protocol the adopter did not write ──────

/// A socket whose `protocol:` names no declared `session` is **refused**.
///
/// This refusal is the whole point of v2.67.0. Substituting a default schema here
/// — which is exactly what the enterprise path did for *every* socket — means a
/// protocol can be proven dual at compile time and a different one enforced at
/// runtime. A "safe fallback" would quietly re-introduce the defect.
#[tokio::test]
async fn a_socket_whose_protocol_is_undeclared_is_refused_not_substituted() {
    let (app, _state) = axon::axon_server::build_router_with_state(server_cfg());
    deploy(&app, GHOST).await;
    let base = boot(app).await;

    // A real client, doing exactly what an adopter would do.
    let err = tokio_tungstenite::connect_async(format!("{base}/ws/Ghost"))
        .await
        .err()
        .expect(
            "the upgrade must be REFUSED. Serving a substitute schema here is precisely the \
             defect v2.67.0 v1.6.0 found in the enterprise path: every deployed socket got a hardcoded \
             canonical chat shape, so a protocol proven dual at COMPILE time had a DIFFERENT one \
             enforced at RUNTIME. A 'safe fallback' would quietly re-introduce it.",
        );
    let msg = format!("{err:?}");
    assert!(
        msg.contains("404"),
        "the refusal must be an honest 404, not a hang or a silent accept; got {msg}"
    );
}

#[tokio::test]
async fn an_undeployed_socket_is_not_found() {
    let (app, _state) = axon::axon_server::build_router_with_state(server_cfg());
    let base = boot(app).await;
    let err = tokio_tungstenite::connect_async(format!("{base}/ws/NeverDeployed"))
        .await
        .err()
        .expect("an undeployed socket must refuse the upgrade");
    assert!(format!("{err:?}").contains("404"));
}
