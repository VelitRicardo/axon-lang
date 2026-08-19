//! v2.37.0 — E2E for the `upstream` runtime against a real (local)
//! RFC 6455 server: dial + auth handshake, bidirectional transcoding,
//! vendor-violation surfacing, witnessed reconnect, fail-closed witness
//! refusal, and reconnect-budget exhaustion.
//!
//! The "vendor" here is an in-process tokio-tungstenite server (the same
//! library both sides use since v2.3.0), so the whole loop runs hermetic on
//! plain TCP — no network, no real vendor, deterministic.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::sync::Mutex;

use axon::upstream_runtime::{
    dial_upstream, InboundPayload, OutboundPayload, TracingLifecycleWitness, UpstreamConfigResolver,
    UpstreamError, UpstreamEvent, UpstreamLifecycle, UpstreamLifecycleWitness,
};
use axon_frontend::ir_nodes::{IRUpstream, IRUpstreamMapRule, IRUpstreamReconnect};
use futures::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

// ── Harness ──────────────────────────────────────────────────────────────────

/// Fixed-value resolver — the test stands in for enterprise custody.
struct FixedResolver {
    url: String,
    secret: String,
}

impl UpstreamConfigResolver for FixedResolver {
    fn resolve(&self, _key: &str) -> Option<String> {
        Some(self.url.clone())
    }
    fn reveal_secret(&self, _key: &str) -> Option<String> {
        Some(self.secret.clone())
    }
}

/// Records every lifecycle transition; optionally refuses them all.
struct RecordingWitness {
    seen: Mutex<Vec<UpstreamLifecycle>>,
    refuse: bool,
}

impl RecordingWitness {
    fn new(refuse: bool) -> Arc<Self> {
        Arc::new(RecordingWitness { seen: Mutex::new(Vec::new()), refuse })
    }
}

impl UpstreamLifecycleWitness for RecordingWitness {
    fn witness<'a>(
        &'a self,
        _upstream: &'a str,
        event: &'a UpstreamLifecycle,
    ) -> axon::upstream_runtime::WitnessFuture<'a> {
        self.seen.lock().unwrap().push(event.clone());
        let refuse = self.refuse;
        Box::pin(async move {
            if refuse {
                Err("audit backend unavailable (test)".to_string())
            } else {
                Ok(())
            }
        })
    }
}

fn rule(direction: &str, message: &str, framing: &str) -> IRUpstreamMapRule {
    IRUpstreamMapRule {
        node_type: "upstream_map_rule",
        direction: direction.into(),
        message: message.into(),
        framing: framing.into(),
        tag: None,
        when_field: None,
        when_value: None,
    }
}

/// The canonical cascaded-STT spec: binary audio out, JSON transcripts in.
fn stt_spec(auth_kind: &str, auth_name: Option<&str>, auth_prefix: Option<&str>, max_attempts: i64) -> IRUpstream {
    let mut transcript = rule("receive", "Transcript", "json");
    transcript.when_field = Some("type".into());
    transcript.when_value = Some("Results".into());
    IRUpstream {
        node_type: "upstream",
        source_line: 1,
        source_column: 1,
        name: "TestSTT".into(),
        transport: "websocket".into(),
        protocol: "SttDialogue".into(),
        role: "axon".into(),
        resolve: "upstream.test.url".into(),
        resource_ref: String::new(),
        capacity: None,
        secret: "upstream.test.api_key".into(),
        auth_kind: auth_kind.into(),
        auth_name: auth_name.map(Into::into),
        auth_prefix: auth_prefix.map(Into::into),
        map: vec![rule("send", "AudioChunk", "binary"), transcript],
        reconnect: Some(IRUpstreamReconnect {
            backoff_ms: 1, // fast tests; the doubling law is unit-tested
            max_attempts,
            on_exhausted: "fail".into(),
        }),
        overflow: Some("drop_oldest".into()),
        backpressure_credit: Some(8),
        preset: None,
    }
}

/// One-connection vendor: captures the request URI + headers, echoes every
/// binary frame back as a `Results` JSON transcript, then serves until the
/// client closes (or `drop_after` frames, to force a reconnect).
async fn spawn_vendor(drop_after: Option<usize>) -> (SocketAddr, Arc<Mutex<Vec<String>>>, Arc<AtomicU32>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().unwrap();
    let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let connections = Arc::new(AtomicU32::new(0));
    let cap = Arc::clone(&captured);
    let conns = Arc::clone(&connections);
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else { return };
            conns.fetch_add(1, Ordering::SeqCst);
            let cap = Arc::clone(&cap);
            tokio::spawn(async move {
                let mut ws = tokio_tungstenite::accept_hdr_async(
                    stream,
                    |req: &tokio_tungstenite::tungstenite::handshake::server::Request, resp| {
                        let mut lines = vec![format!("uri={}", req.uri())];
                        if let Some(auth) = req.headers().get("Authorization") {
                            lines.push(format!("authorization={}", auth.to_str().unwrap_or("?")));
                        }
                        cap.lock().unwrap().extend(lines);
                        Ok(resp)
                    },
                )
                .await
                .expect("server handshake");
                let mut served = 0usize;
                while let Some(Ok(msg)) = ws.next().await {
                    match msg {
                        Message::Binary(b) => {
                            served += 1;
                            let reply = format!(
                                r#"{{"type":"Results","channel":{{"alternatives":[{{"transcript":"len={}"}}]}}}}"#,
                                b.len()
                            );
                            // v2.81.0 — tungstenite 0.26+ text payloads are `Utf8Bytes`.
                            let _ = ws.send(Message::Text(reply.into())).await;
                            if drop_after.is_some_and(|n| served >= n) {
                                return; // hard drop — no Close frame (vendor "crash")
                            }
                        }
                        // Any text frame mentioning "violate" makes the vendor
                        // reply with a frame OUTSIDE the declared contract.
                        Message::Text(t) if t.contains("violate") => {
                            let _ = ws.send(Message::Text(r#"{"type":"Metadata"}"#.into())).await;
                        }
                        Message::Close(_) => return,
                        _ => {}
                    }
                }
            });
        }
    });
    (addr, captured, connections)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn a_compiled_upstream_dials_and_transcodes_both_directions() {
    // v2.89.0 — the same end-to-end dial, from a program on disk.
    //
    // The gates here hand-build `IRUpstream`, which proves the DIAL. This one
    // takes the declaration an adopter writes, compiles it through the real
    // pipeline, and dials the same real vendor with it. A static fixture works
    // because `resolve:`/`secret:` are CONFIG KEYS by v2.37.0's rule — never
    // literals — so the vendor's ephemeral address is supplied by the resolver
    // and nothing about the program bends to the test.
    const FIXTURE: &str = "tests/fixtures/upstream/stt_dialogue.axon";
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE);
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let tokens = axon_frontend::lexer::Lexer::new(&src, FIXTURE)
        .tokenize()
        .expect("fixture must lex");
    let prog = axon_frontend::parser::Parser::new(tokens)
        .parse()
        .expect("fixture must parse");
    let errors = axon_frontend::type_checker::TypeChecker::new(&prog).check();
    assert!(
        errors.is_empty(),
        "the fixture must TYPE-CHECK: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
    let ir = axon_frontend::ir_generator::IRGenerator::new().generate(&prog);
    let spec = ir
        .upstreams
        .first()
        .expect("the fixture declares an `upstream`; an empty catalog means it no longer lowers")
        .clone();

    // The declaration must have reached the IR as declared, before any dial.
    assert_eq!(
        (spec.auth_kind.as_str(), spec.auth_name.as_deref()),
        ("header", Some("Authorization")),
        "the fixture's `auth: header(\"Authorization\", \"Bearer\")` must lower as \
         header auth; got kind={:?} name={:?} prefix={:?}",
        spec.auth_kind,
        spec.auth_name,
        spec.auth_prefix
    );
    assert_eq!(spec.backpressure_credit, Some(8), "declared credit window");
    assert_eq!(spec.overflow.as_deref(), Some("drop_oldest"));

    let (addr, captured, _) = spawn_vendor(None).await;
    let resolver = FixedResolver {
        url: format!("ws://{addr}/v1/listen?model=nova"),
        secret: "sk-test".into(),
    };
    let witness = RecordingWitness::new(false);

    let mut handle = dial_upstream(&spec, &resolver, witness.clone(), None)
        .await
        .expect("the compiled upstream must dial");
    handle
        .send("AudioChunk", OutboundPayload::Bytes(vec![0u8; 320]))
        .await
        .expect("send audio");

    let ev = handle.recv().await.expect("event");
    match ev {
        UpstreamEvent::Message { message, payload } => {
            assert_eq!(
                message, "Transcript",
                "the inbound frame must classify as the type the FIXTURE's `map:` declares"
            );
            let InboundPayload::Json(v) = payload else {
                panic!("json payload")
            };
            assert_eq!(v["channel"]["alternatives"][0]["transcript"], "len=320");
        }
        other => panic!("expected Transcript, got {other:?}"),
    }

    // The fixture declares `auth: header("Authorization", "Bearer")`, so the
    // secret must ride the HEADER — not the query.
    let cap = captured.lock().unwrap().join("\n");
    assert!(
        cap.contains("authorization="),
        "the declared header auth must be what is sent; captured: {cap}; \
         spec kind={:?} name={:?} prefix={:?}",
        spec.auth_kind,
        spec.auth_name,
        spec.auth_prefix
    );
    assert!(
        cap.contains("sk-test"),
        "the resolved secret must ride the header; captured: {cap}"
    );
    assert!(
        !cap.contains("token=sk-test"),
        "a header-auth upstream must not put the secret on the query; captured: {cap}"
    );

    // The dial was witnessed BEFORE connecting (fail-closed order).
    assert_eq!(
        witness.seen.lock().unwrap()[0],
        UpstreamLifecycle::Connected { attempt: 0 }
    );
    handle.close();
}

#[tokio::test]
async fn dials_with_query_auth_and_transcodes_both_directions() {
    let (addr, captured, _) = spawn_vendor(None).await;
    let spec = stt_spec("query", Some("token"), None, 0);
    let resolver = FixedResolver { url: format!("ws://{addr}/v1/listen?model=nova"), secret: "sk-test".into() };
    let witness = RecordingWitness::new(false);

    let mut handle = dial_upstream(&spec, &resolver, witness.clone(), None).await.expect("dial");
    handle
        .send("AudioChunk", OutboundPayload::Bytes(vec![0u8; 320]))
        .await
        .expect("send audio");

    let ev = handle.recv().await.expect("event");
    match ev {
        UpstreamEvent::Message { message, payload } => {
            assert_eq!(message, "Transcript");
            let InboundPayload::Json(v) = payload else { panic!("json payload") };
            // The WHOLE vendor body is the v2.26.0 Json payload — total navigation.
            assert_eq!(v["channel"]["alternatives"][0]["transcript"], "len=320");
        }
        other => panic!("expected Transcript, got {other:?}"),
    }

    // Auth handshake: the secret rode the query param, composed with the
    // pre-existing query, and no Authorization header was sent.
    let cap = captured.lock().unwrap().join("\n");
    assert!(cap.contains("uri=/v1/listen?model=nova&token=sk-test"), "captured: {cap}");
    assert!(!cap.contains("authorization"), "captured: {cap}");

    // The dial was witnessed BEFORE connecting (fail-closed order).
    assert_eq!(witness.seen.lock().unwrap()[0], UpstreamLifecycle::Connected { attempt: 0 });
    handle.close();
}

#[tokio::test]
async fn dials_with_header_auth_prefix() {
    let (addr, captured, _) = spawn_vendor(None).await;
    let spec = stt_spec("header", Some("Authorization"), Some("Token "), 0);
    let resolver = FixedResolver { url: format!("ws://{addr}/v1"), secret: "dg-key".into() };
    let handle = dial_upstream(&spec, &resolver, RecordingWitness::new(false), None).await.expect("dial");
    // Handshake already completed ⇒ headers captured.
    let cap = captured.lock().unwrap().join("\n");
    assert!(cap.contains("authorization=Token dg-key"), "captured: {cap}");
    handle.close();
}

#[tokio::test]
async fn unclassifiable_vendor_frame_is_an_explicit_unmapped_event() {
    let (addr, _, _) = spawn_vendor(None).await;
    let mut spec = stt_spec("signed_url", None, None, 0);
    // Add a send-json rule so we can poke the vendor into violating.
    let mut poke = rule("send", "Violate", "json");
    poke.tag = Some("violate".into());
    spec.map.push(poke);
    let resolver = FixedResolver { url: format!("ws://{addr}/v1?sig=ok"), secret: String::new() };
    let mut handle = dial_upstream(&spec, &resolver, RecordingWitness::new(false), None).await.expect("dial");

    // Poke the vendor: our projected envelope carries the "violate" tag,
    // and the vendor replies "Metadata" — a frame no receive rule matches.
    // the design decision: the violation is SURFACED as an event, never silently dropped.
    handle.send("Violate", OutboundPayload::Json(serde_json::json!({}))).await.expect("send");
    let ev = handle.recv().await.expect("event");
    match ev {
        UpstreamEvent::Unmapped { detail } => {
            assert!(detail.contains("unclassifiable"), "detail: {detail}");
        }
        other => panic!("expected Unmapped, got {other:?}"),
    }
    handle.close();
}

#[tokio::test]
async fn reconnects_after_vendor_drop_and_witnesses_it() {
    // Vendor hard-drops after each served frame; budget allows redials.
    let (addr, _, connections) = spawn_vendor(Some(1)).await;
    let spec = stt_spec("query", Some("token"), None, 5);
    let resolver = FixedResolver { url: format!("ws://{addr}/v1"), secret: "k".into() };
    let witness = RecordingWitness::new(false);
    let mut handle = dial_upstream(&spec, &resolver, witness.clone(), None).await.expect("dial");

    handle.send("AudioChunk", OutboundPayload::Bytes(vec![1, 2])).await.expect("send 1");
    // First transcript arrives, then the vendor crashes the connection.
    let ev1 = handle.recv().await.expect("first transcript");
    assert!(matches!(ev1, UpstreamEvent::Message { .. }), "got {ev1:?}");

    // The driver redials; the consumer sees the reconnection explicitly.
    let ev2 = handle.recv().await.expect("reconnect event");
    match ev2 {
        UpstreamEvent::Reconnected { attempt } => assert!(attempt >= 1),
        other => panic!("expected Reconnected, got {other:?}"),
    }
    // …and the SECOND connection works end-to-end.
    handle.send("AudioChunk", OutboundPayload::Bytes(vec![3])).await.expect("send 2");
    let ev3 = handle.recv().await.expect("second transcript");
    assert!(matches!(ev3, UpstreamEvent::Message { .. }), "got {ev3:?}");
    assert!(connections.load(Ordering::SeqCst) >= 2, "vendor saw both dials");

    let seen = witness.seen.lock().unwrap();
    assert!(seen.iter().any(|e| matches!(e, UpstreamLifecycle::Reconnected { .. })), "witnessed: {seen:?}");
    handle.close();
}

#[tokio::test]
async fn exhausted_budget_is_a_terminal_witnessed_event() {
    // Vendor accepts exactly one connection then the listener dies with it:
    // bind, dial once, then drop the listener so every redial is refused.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        // Serve one frame then crash; the listener is consumed ⇒ no redial target.
        while let Some(Ok(msg)) = ws.next().await {
            if matches!(msg, Message::Binary(_)) {
                return;
            }
        }
    });

    let spec = stt_spec("query", Some("token"), None, 2);
    let resolver = FixedResolver { url: format!("ws://{addr}/v1"), secret: "k".into() };
    let witness = RecordingWitness::new(false);
    let mut handle = dial_upstream(&spec, &resolver, witness.clone(), None).await.expect("dial");
    handle.send("AudioChunk", OutboundPayload::Bytes(vec![9])).await.expect("send");
    server.await.unwrap();

    // Drain events until the terminal exhaustion (redials fail fast on a
    // dead port; backoff_ms=1 keeps this sub-second).
    let mut saw_exhausted = false;
    while let Some(ev) = handle.recv().await {
        if let UpstreamEvent::Exhausted { attempts } = ev {
            assert_eq!(attempts, 2, "budget was max_attempts=2");
            saw_exhausted = true;
            break;
        }
    }
    assert!(saw_exhausted, "the consumer must SEE the exhaustion (on_exhausted: fail)");
    let seen = witness.seen.lock().unwrap();
    assert!(
        seen.iter().any(|e| matches!(e, UpstreamLifecycle::Exhausted { attempts: 2 })),
        "exhaustion must be witnessed: {seen:?}"
    );
}

#[tokio::test]
async fn refused_witness_blocks_the_dial_fail_closed() {
    // No server needed — the refusal must abort BEFORE any connection.
    let spec = stt_spec("query", Some("token"), None, 0);
    let resolver = FixedResolver { url: "ws://127.0.0.1:9/v1".into(), secret: "k".into() };
    let err = dial_upstream(&spec, &resolver, RecordingWitness::new(true), None).await.expect_err("must refuse");
    assert!(
        matches!(err, UpstreamError::UnwitnessedLifecycle { .. }),
        "an upstream that cannot witness its own lifecycle refuses to dial, got: {err}"
    );
}

#[tokio::test]
async fn missing_config_and_secret_are_immediate_errors() {
    struct EmptyResolver;
    impl UpstreamConfigResolver for EmptyResolver {
        fn resolve(&self, _: &str) -> Option<String> {
            None
        }
        fn reveal_secret(&self, _: &str) -> Option<String> {
            None
        }
    }
    let spec = stt_spec("query", Some("token"), None, 0);
    let err = dial_upstream(&spec, &EmptyResolver, Arc::new(TracingLifecycleWitness), None).await.expect_err("no url");
    assert!(matches!(err, UpstreamError::MissingConfig { .. }), "got: {err}");
}
