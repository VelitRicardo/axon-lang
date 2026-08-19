//! §Fase 80.f — the blessed upstream preset catalog + `from Preset@vN`
//! expansion.
//!
//! Each preset is a **versioned, named template** for a vendor the 2026
//! voice market has converged on — and, per D80.5, each is an ORDINARY
//! `.axon` source string (a `session` + an `upstream`), never a black box:
//! `axon desugar` prints the exact expansion, and forking a preset into a
//! local hand-written `upstream` is always available. When a vendor changes
//! its wire contract, a new `@vN` ships; an existing `@vN` never mutates
//! under an adopter (the published-releases-don't-move discipline applied
//! to stdlib artifacts).
//!
//! Expansion runs at the tail of `Parser::parse()` (before type-check, so
//! the §80.c laws see the EXPANDED declaration): the preset's `upstream`
//! fields fill every field the adopter left unwritten (an adopter-written
//! field always wins), and the preset's `session` is injected unless a
//! session of that name is already declared. An unknown preset leaves the
//! declaration unexpanded — the §80.c checker reports it with the
//! available-preset list (accumulating diagnostics beat a parse abort).
//!
//! Message-set scope (v1, deliberate): each preset maps the load-bearing
//! frames of the vendor's wire — audio/text out, transcripts/audio in.
//! Vendor housekeeping frames (Deepgram `Metadata`, OpenAI lifecycle
//! events, …) surface at runtime as explicit `Unmapped` events the
//! consumer may count or ignore; mapping every housekeeping frame is
//! adopter-forkable, not preset-default (narrow projections are
//! legitimate — silence is not).

use crate::ast::{Declaration, DeclarationTrivia, Program, UpstreamDefinition};
use crate::lexer::Lexer;
use crate::parser::Parser;

/// One blessed vendor template. `source` is a complete, compilable `.axon`
/// fragment: the vendor-facing `session` + the full `upstream`.
#[derive(Debug, Clone, Copy)]
pub struct UpstreamPreset {
    /// Preset name — the identifier before `@` in `from DeepgramSTT@v1`.
    pub name: &'static str,
    /// Preset version — the identifier after `@`.
    pub version: &'static str,
    /// `cascaded_stt` | `cascaded_tts` | `fused_realtime` (D80.1: both
    /// architectures are first-class members of ONE catalog).
    pub architecture: &'static str,
    /// The ordinary `.axon` source the reference expands to.
    pub source: &'static str,
}

/// The v1 catalog: the four cascaded legs + the two fused
/// speech-to-speech APIs the 2026 market converged on (plan §1 survey).
pub const UPSTREAM_PRESETS: &[UpstreamPreset] = &[
    UpstreamPreset {
        name: "DeepgramSTT",
        version: "v1",
        architecture: "cascaded_stt",
        source: r#"
session DeepgramSttDialogue {
    axon:   [ send AudioChunk, receive Transcript, loop ]
    vendor: [ receive AudioChunk, send Transcript, loop ]
}
upstream DeepgramSTT {
    transport: websocket
    protocol: DeepgramSttDialogue
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
"#,
    },
    UpstreamPreset {
        name: "AssemblyAISTT",
        version: "v1",
        architecture: "cascaded_stt",
        source: r#"
session AssemblyAiSttDialogue {
    axon:   [ send AudioChunk, receive Transcript, loop ]
    vendor: [ receive AudioChunk, send Transcript, loop ]
}
upstream AssemblyAISTT {
    transport: websocket
    protocol: AssemblyAiSttDialogue
    role: axon
    resolve: upstream.assemblyai.url
    secret: upstream.assemblyai.api_key
    auth: header("Authorization")
    map: [
        send AudioChunk as binary,
        receive Transcript as json when "type" = "Turn",
    ]
    reconnect: { backoff_ms: 500, max_attempts: 5, on_exhausted: fail }
    overflow: drop_oldest
}
"#,
    },
    UpstreamPreset {
        name: "ElevenLabsTTS",
        version: "v1",
        architecture: "cascaded_tts",
        source: r#"
session ElevenLabsTtsDialogue {
    axon:   [ send TextChunk, receive AudioOut, receive Final, loop ]
    vendor: [ receive TextChunk, send AudioOut, send Final, loop ]
}
upstream ElevenLabsTTS {
    transport: websocket
    protocol: ElevenLabsTtsDialogue
    role: axon
    resolve: upstream.elevenlabs.url
    secret: upstream.elevenlabs.api_key
    auth: header("xi-api-key")
    map: [
        send TextChunk as json,
        receive AudioOut as json when "audio",
        receive Final as json when "isFinal",
    ]
    reconnect: { backoff_ms: 250, max_attempts: 5, on_exhausted: fail }
    overflow: pause_upstream
}
"#,
    },
    UpstreamPreset {
        name: "CartesiaTTS",
        version: "v1",
        architecture: "cascaded_tts",
        source: r#"
session CartesiaTtsDialogue {
    axon:   [ send TextChunk, receive AudioOut, receive Final, loop ]
    vendor: [ receive TextChunk, send AudioOut, send Final, loop ]
}
upstream CartesiaTTS {
    transport: websocket
    protocol: CartesiaTtsDialogue
    role: axon
    resolve: upstream.cartesia.url
    secret: upstream.cartesia.api_key
    auth: query("api_key")
    map: [
        send TextChunk as json,
        receive AudioOut as json when "type" = "chunk",
        receive Final as json when "type" = "done",
    ]
    reconnect: { backoff_ms: 250, max_attempts: 5, on_exhausted: fail }
    overflow: pause_upstream
}
"#,
    },
    UpstreamPreset {
        name: "OpenAIRealtime",
        version: "v1",
        architecture: "fused_realtime",
        source: r#"
session OpenAiRealtimeDialogue {
    axon:   [ send AudioAppend, send ResponseCreate, receive AudioDelta, receive TranscriptDelta, loop ]
    vendor: [ receive AudioAppend, receive ResponseCreate, send AudioDelta, send TranscriptDelta, loop ]
}
upstream OpenAIRealtime {
    transport: websocket
    protocol: OpenAiRealtimeDialogue
    role: axon
    resolve: upstream.openai.realtime_url
    secret: upstream.openai.api_key
    auth: header("Authorization", "Bearer ")
    map: [
        send AudioAppend as json tag "input_audio_buffer.append",
        send ResponseCreate as json tag "response.create",
        receive AudioDelta as json when "type" = "response.audio.delta",
        receive TranscriptDelta as json when "type" = "response.audio_transcript.delta",
    ]
    reconnect: { backoff_ms: 500, max_attempts: 3, on_exhausted: fail }
    overflow: drop_oldest
}
"#,
    },
    UpstreamPreset {
        name: "GeminiLive",
        version: "v1",
        architecture: "fused_realtime",
        source: r#"
session GeminiLiveDialogue {
    axon:   [ send Setup, receive SetupComplete, send AudioInput, receive ServerContent, loop ]
    vendor: [ receive Setup, send SetupComplete, receive AudioInput, send ServerContent, loop ]
}
upstream GeminiLive {
    transport: websocket
    protocol: GeminiLiveDialogue
    role: axon
    resolve: upstream.gemini.live_url
    secret: upstream.gemini.api_key
    auth: query("key")
    map: [
        send Setup as json,
        send AudioInput as json,
        receive SetupComplete as json when "setupComplete",
        receive ServerContent as json when "serverContent",
    ]
    reconnect: { backoff_ms: 500, max_attempts: 3, on_exhausted: fail }
    overflow: drop_oldest
}
"#,
    },
];

/// Look up `"<Name>@<version>"` (the exact `from` reference).
pub fn find(preset_ref: &str) -> Option<&'static UpstreamPreset> {
    let (name, version) = preset_ref.split_once('@')?;
    UPSTREAM_PRESETS.iter().find(|p| p.name == name && p.version == version)
}

/// `"DeepgramSTT@v1, …"` — for the §80.c unknown-preset diagnostic.
pub fn available() -> String {
    UPSTREAM_PRESETS
        .iter()
        .map(|p| format!("{}@{}", p.name, p.version))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Fill every field the adopter left unwritten from the preset base — an
/// adopter-written field always wins (that is what makes a preset forkable
/// in place: override one field, keep the rest).
fn merge_preset(user: &mut UpstreamDefinition, base: UpstreamDefinition) {
    if user.transport.is_empty() {
        user.transport = base.transport;
    }
    if user.protocol.is_empty() {
        user.protocol = base.protocol;
    }
    if user.role.is_empty() {
        user.role = base.role;
    }
    if user.resolve.is_empty() {
        user.resolve = base.resolve;
    }
    if user.secret.is_empty() {
        user.secret = base.secret;
    }
    if user.auth_kind.is_empty() {
        user.auth_kind = base.auth_kind;
        user.auth_name = base.auth_name;
        user.auth_prefix = base.auth_prefix;
    }
    if user.map.is_empty() {
        user.map = base.map;
    }
    if user.reconnect.is_none() {
        user.reconnect = base.reconnect;
    }
    if user.overflow.is_none() {
        user.overflow = base.overflow;
    }
    if user.backpressure_credit.is_none() {
        user.backpressure_credit = base.backpressure_credit;
    }
}

/// §80.g (`axon desugar`) — render one (possibly preset-expanded)
/// `upstream` back to canonical source. This is the D80.6 payload: the
/// compliance reviewer reads the exact declaration the compiler checked
/// and the runtime dials — not the sugar that produced it.
pub fn render_upstream(u: &UpstreamDefinition) -> String {
    let mut s = String::new();
    if let Some(p) = &u.preset {
        s.push_str(&format!("// expanded from preset {p}\n"));
    }
    s.push_str(&format!("upstream {} {{\n", u.name));
    s.push_str(&format!("    transport: {}\n", u.transport));
    s.push_str(&format!("    protocol: {}\n", u.protocol));
    s.push_str(&format!("    role: {}\n", u.role));
    s.push_str(&format!("    resolve: {}\n", u.resolve));
    s.push_str(&format!("    secret: {}\n", u.secret));
    match (u.auth_kind.as_str(), &u.auth_name, &u.auth_prefix) {
        ("signed_url", _, _) => s.push_str("    auth: signed_url\n"),
        (kind, Some(name), Some(prefix)) => {
            s.push_str(&format!("    auth: {kind}(\"{name}\", \"{prefix}\")\n"))
        }
        (kind, Some(name), None) => s.push_str(&format!("    auth: {kind}(\"{name}\")\n")),
        (kind, None, _) => s.push_str(&format!("    auth: {kind}\n")),
    }
    s.push_str("    map: [\n");
    for r in &u.map {
        let mut line = format!("        {} {} as {}", r.direction, r.message, r.framing);
        if let Some(tag) = &r.tag {
            line.push_str(&format!(" tag \"{tag}\""));
        }
        match (&r.when_field, &r.when_value) {
            (Some(f), Some(v)) => line.push_str(&format!(" when \"{f}\" = \"{v}\"")),
            (Some(f), None) => line.push_str(&format!(" when \"{f}\"")),
            _ => {}
        }
        line.push_str(",\n");
        s.push_str(&line);
    }
    s.push_str("    ]\n");
    if let Some(rc) = &u.reconnect {
        s.push_str(&format!(
            "    reconnect: {{ backoff_ms: {}, max_attempts: {}, on_exhausted: {} }}\n",
            rc.backoff_ms, rc.max_attempts, rc.on_exhausted
        ));
    }
    if let Some(o) = &u.overflow {
        s.push_str(&format!("    overflow: {o}\n"));
    }
    if let Some(c) = u.backpressure_credit {
        s.push_str(&format!("    backpressure: credit({c})\n"));
    }
    s.push_str("}\n");
    s
}

/// §80.f — expand every `upstream X from Preset@vN { … }` in the program:
/// fill the declaration from the preset and inject the preset's `session`
/// (unless a session of that name already exists — instantiating the same
/// preset twice shares one session declaration). Runs at the tail of
/// `Parser::parse()`; keeps `declarations` and `declaration_trivia`
/// parallel. Unknown presets are left unexpanded for the §80.c checker.
pub fn expand(program: &mut Program) {
    let mut i = 0;
    while i < program.declarations.len() {
        let preset_ref = match &program.declarations[i] {
            Declaration::Upstream(u) => u.preset.clone(),
            _ => None,
        };
        let Some(ref_str) = preset_ref else {
            i += 1;
            continue;
        };
        let Some(preset) = find(&ref_str) else {
            i += 1;
            continue; // §80.c reports the unknown preset with the catalog.
        };
        // Presets are compile-time constants authored in this file and
        // compile-gated by the tests below — a parse failure here is a
        // defect of the catalog itself, not of the adopter's program.
        let tokens = Lexer::new(preset.source, "<upstream-preset>")
            .tokenize()
            .expect("preset catalog source must lex");
        let template = Parser::new(tokens).parse().expect("preset catalog source must parse");
        let mut base: Option<UpstreamDefinition> = None;
        let mut sessions = Vec::new();
        for d in template.declarations {
            match d {
                Declaration::Session(s) => sessions.push(s),
                Declaration::Upstream(u) => base = Some(u),
                _ => {}
            }
        }
        if let Some(base) = base {
            if let Declaration::Upstream(u) = &mut program.declarations[i] {
                merge_preset(u, base);
            }
            for s in sessions {
                let exists = program
                    .declarations
                    .iter()
                    .any(|d| matches!(d, Declaration::Session(x) if x.name == s.name));
                if !exists {
                    program.declarations.insert(i, Declaration::Session(s));
                    program
                        .declaration_trivia
                        .insert(i, DeclarationTrivia::default());
                    i += 1;
                }
            }
        }
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::type_checker::TypeChecker;

    /// Every preset in the catalog must itself compile + type-check clean —
    /// the published-grammar-must-compile discipline applied to the stdlib.
    #[test]
    fn every_preset_compiles_and_checks_clean() {
        for p in UPSTREAM_PRESETS {
            let tokens = Lexer::new(p.source, "<preset>").tokenize().unwrap_or_else(|e| {
                panic!("preset {}@{} must lex: {e:?}", p.name, p.version)
            });
            let prog = Parser::new(tokens)
                .parse()
                .unwrap_or_else(|e| panic!("preset {}@{} must parse: {e:?}", p.name, p.version));
            let errors = TypeChecker::new(&prog).check();
            assert!(
                errors.is_empty(),
                "preset {}@{} must type-check clean, got: {:?}",
                p.name,
                p.version,
                errors.iter().map(|e| &e.message).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn catalog_covers_both_architectures() {
        // D80.1 — cascaded and fused are BOTH first-class catalog members.
        let archs: std::collections::HashSet<&str> =
            UPSTREAM_PRESETS.iter().map(|p| p.architecture).collect();
        assert!(archs.contains("cascaded_stt"));
        assert!(archs.contains("cascaded_tts"));
        assert!(archs.contains("fused_realtime"));
        assert_eq!(UPSTREAM_PRESETS.len(), 6, "the blessed six (grow on adopter demand)");
    }

    #[test]
    fn from_form_expands_and_checks_clean() {
        let src = r#"
upstream MySTT from DeepgramSTT@v1 {
    secret: upstream.mystt.api_key
}
"#;
        let tokens = Lexer::new(src, "<t>").tokenize().unwrap();
        let prog = Parser::new(tokens).parse().unwrap();
        // The session was injected and the upstream filled.
        let u = prog
            .declarations
            .iter()
            .find_map(|d| match d {
                Declaration::Upstream(u) => Some(u),
                _ => None,
            })
            .expect("upstream");
        assert_eq!(u.name, "MySTT");
        assert_eq!(u.preset.as_deref(), Some("DeepgramSTT@v1"), "provenance kept");
        assert_eq!(u.transport, "websocket", "filled from preset");
        assert_eq!(u.secret, "upstream.mystt.api_key", "adopter override wins");
        assert_eq!(u.resolve, "upstream.deepgram.url", "preset default kept");
        assert!(prog
            .declarations
            .iter()
            .any(|d| matches!(d, Declaration::Session(s) if s.name == "DeepgramSttDialogue")));
        let errors = TypeChecker::new(&prog).check();
        assert!(errors.is_empty(), "expanded program checks clean: {:?}",
            errors.iter().map(|e| &e.message).collect::<Vec<_>>());
    }

    #[test]
    fn two_instantiations_share_one_session() {
        let src = r#"
upstream A from DeepgramSTT@v1 { secret: upstream.a.api_key }
upstream B from DeepgramSTT@v1 { secret: upstream.b.api_key }
"#;
        let tokens = Lexer::new(src, "<t>").tokenize().unwrap();
        let prog = Parser::new(tokens).parse().unwrap();
        let sessions = prog
            .declarations
            .iter()
            .filter(|d| matches!(d, Declaration::Session(_)))
            .count();
        assert_eq!(sessions, 1, "shared session, no duplicate declaration");
    }

    #[test]
    fn unknown_preset_is_a_checker_diagnostic_with_the_catalog() {
        let src = "upstream X from NoSuchVendor@v9 { secret: upstream.x.api_key }";
        let tokens = Lexer::new(src, "<t>").tokenize().unwrap();
        let prog = Parser::new(tokens).parse().unwrap();
        let errors = TypeChecker::new(&prog).check();
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("NoSuchVendor@v9") && e.message.contains("DeepgramSTT@v1")),
            "unknown preset must name the catalog, got: {:?}",
            errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );
    }
}
