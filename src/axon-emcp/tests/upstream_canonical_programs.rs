//! v2.37.0 — drift gate for the `upstream` primitive doc.
//!
//! The canonical program published in `knowledge/primitives/upstream.md`
//! must round-trip through the same `axon-frontend` pipeline the `axon`
//! CLI uses — the "published grammar MUST compile" discipline, applied to
//! the outbound-vendor-connection primitive.
//!
//! Mirrors the pattern from `phase2/6b/6c/6d/channel_quartet_canonical_programs.rs`.

use axon_emcp::compiler_pipeline::{run, Outcome};

fn must_compile(label: &str, source: &str) {
    match run(source, label) {
        Outcome::Ok { .. } => { /* well-formed — the whole assertion */ }
        Outcome::Err {
            stage,
            errors,
            warnings,
        } => panic!(
            "{label}: expected well-formed program, got {stage:?} failure:\n\
             errors   = {errors:#?}\n\
             warnings = {warnings:#?}\n\
             source   = {source}"
        ),
    }
}

/// The design-doc section 1 shape: a cascaded-STT upstream — binary audio out,
/// JSON transcripts in, header auth, fail-closed reconnect policy.
#[test]
fn upstream_canonical_program_compiles() {
    let src = r#"
session SttDialogue {
    axon:   [ send AudioChunk, receive Transcript, loop ]
    vendor: [ receive AudioChunk, send Transcript, loop ]
}

upstream DeepgramSTT {
    transport: websocket
    protocol: SttDialogue
    role: axon
    resolve: upstream.deepgram.url
    secret: upstream.deepgram.api_key
    auth: header("Authorization", "Token ")
    map: [
        send AudioChunk as binary,
        receive Transcript as json when "type" = "Results",
    ]
    reconnect: { backoff_ms: 500, max_attempts: 5, on_exhausted: fail }
    overflow: drop_oldest
}
"#;
    must_compile("upstream/canonical", src);
}

/// The v2.37.0 preset-instantiation surface: the form every blessed-vendor
/// adopter actually writes — expansion fills the declaration from the
/// catalog and injects the preset's session before type-check.
#[test]
fn upstream_preset_form_compiles() {
    let src = r#"
upstream MySTT from DeepgramSTT@v1 {
    secret: upstream.mystt.api_key
}
"#;
    must_compile("upstream/preset-form", src);
}

/// v2.37.0 — the keystone claim (plan section 7): a barge-in-capable phone agent
/// in under 20 lines, expanding to ordinary checked primitives.
#[test]
fn voice_canonical_program_compiles() {
    let src = r#"
voice Concierge {
    stt: DeepgramSTT@v1
    tts: ElevenLabsTTS@v1
    interruptible: true
    legal_basis: legitimate_interest
}
"#;
    must_compile("voice/canonical", src);
}

/// v2.37.0 — the fused architecture is the SAME grammar.
#[test]
fn voice_fused_realtime_compiles() {
    let src = r#"
voice Live {
    realtime: OpenAIRealtime@v1
    carrier: pcm16
}
"#;
    must_compile("voice/fused", src);
}
