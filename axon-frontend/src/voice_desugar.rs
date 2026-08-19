//! v2.37.0 — `voice` macro-expansion: the simplicity layer lowered to
//! the primitives already in the language, as SOURCE TEXT.
//!
//! The expansion is generated as an ordinary `.axon` source string and
//! parsed through the same pipeline as adopter code — which is exactly what
//! makes the design decision hold: `axon desugar` prints THIS text, the compliance
//! reviewer reads real declarations, and the type-checker + PCC see the
//! expanded program with no special cases. A `voice` declaration itself
//! never reaches the IR: the deployed artifact IS the expansion.
//!
//! What one `voice V` lowers to:
//! - `carrier: mulaw8k` (the default) ⇒ the ots μ-law↔PCM16 codec pair
//!   (`InboundMulawToPcm16` / `OutboundPcm16ToMulaw` — shared, injected
//!   once per program); `carrier: pcm16` ⇒ no transcode.
//! - the carrier-facing `session <V>CarrierTurn` + `socket <V>Call` —
//! v2.36.0-interruptible (barge-in) when `interruptible: true`, in which case
//!   the socket declares `reconnect: cognitive_state` + the voice's
//!   `legal_basis:` (the sugar CANNOT generate a program
//!   `ParkedResidualSoundness` refutes — T852 enforces the input side).
//! - one `upstream <V>SttLink` + `<V>TtsLink` (cascaded) or one
//!   `<V>RealtimeLink` (fused) for each leg given as a `Preset@vN`
//!   reference; a leg naming an already-declared `upstream` is used as-is.
//!
//! Runs at the tail of `Parser::parse()` BEFORE the v2.37.0 preset expansion,
//! so the `from Preset@vN` upstreams this pass emits are themselves
//! expanded in the same parse.

use crate::ast::{Declaration, DeclarationTrivia, Program, VoiceDefinition};
use crate::lexer::Lexer;
use crate::parser::Parser;

/// The shared PSTN codec pair — verbatim from the reference scaffold
/// (`knowledge/templates/voice.axon`), the closed-catalog `ots` surface.
const MULAW_OTS_PAIR: &str = r#"
ots InboundMulawToPcm16 {
    teleology:       "Decode mu-law 8kHz inbound audio for LLM-streaming pipelines"
    homotopy_search: shallow
    loss_function:   "RMSE on reconstructed waveform"
}

ots OutboundPcm16ToMulaw {
    teleology:       "Encode PCM16 LLM output back to mu-law for the carrier"
    homotopy_search: shallow
    loss_function:   "RMSE on reconstructed waveform"
}
"#;

/// Generate the expansion source for one `voice` (pure — this exact text
/// is what `axon desugar` prints). `include_ots` lets the program-level
/// pass share one codec pair across several voices.
pub fn expansion_source(v: &VoiceDefinition, include_ots: bool) -> String {
    let mut out = String::new();
    let name = &v.name;
    let carrier = if v.carrier.is_empty() { "mulaw8k" } else { v.carrier.as_str() };

    if include_ots && carrier == "mulaw8k" {
        out.push_str(MULAW_OTS_PAIR);
    }

    // Carrier-facing session: the caller streams audio in, the agent
    // streams audio out; with barge-in the agent's utterance is a v2.36.0
    // interruptible region whose handler resumes (the abandon exit stays
    // available to the runtime via v2.36.0's two-exit construct).
    // `loop` recurses to the role's start and must be the LAST step of the
    // path it ends (v2.3.0 lowering: everything after a `loop` in sequence is
    // unreachable; a leading `loop` would lower to the unguarded μX.X).
    // The iteration body is credit-BALANCED by construction (one receive
    // per send, Δ = 0) so the generated socket's credit window passes the
    // v2.3.0 Presburger discharge — sugar that generated an unsustainable
    // loop would fail its own compile.
    if v.interruptible {
        out.push_str(&format!(
            "\nsession {name}CarrierTurn {{\n    \
                 agent:  [ receive AudioIn, interrupt {{ send AudioOut, loop }} on CallerSpeech as cause resumable {{ resume }} ]\n    \
                 caller: [ send AudioIn, interrupt {{ receive AudioOut, loop }} on CallerSpeech as cause resumable {{ resume }} ]\n\
             }}\n"
        ));
    } else {
        out.push_str(&format!(
            "\nsession {name}CarrierTurn {{\n    \
                 agent:  [ receive AudioIn, send AudioOut, loop ]\n    \
                 caller: [ send AudioIn, receive AudioOut, loop ]\n\
             }}\n"
        ));
    }

    out.push_str(&format!("\nsocket {name}Call {{\n    protocol: {name}CarrierTurn\n    backpressure: credit(8)\n"));
    if v.interruptible {
        out.push_str("    reconnect: cognitive_state\n");
    }
    if let Some(basis) = &v.legal_basis {
        out.push_str(&format!("    legal_basis: {basis}\n"));
    }
    out.push_str("}\n");

    // Vendor legs — synthesize an upstream per preset-referenced leg; a
    // leg naming a declared upstream is referenced, not re-declared.
    for (leg, suffix) in [(&v.stt, "SttLink"), (&v.tts, "TtsLink"), (&v.realtime, "RealtimeLink")] {
        if let Some(r) = leg {
            if r.contains('@') {
                out.push_str(&format!("\nupstream {name}{suffix} from {r} {{ }}\n"));
            }
        }
    }
    out
}

/// v2.37.0 — expand every `voice` in the program by parsing its generated
/// source and splicing the declarations in after it. The `voice`
/// declaration itself STAYS in the AST (provenance + T852 validation) but
/// is skipped by the IR generator — the deployed artifact is the expansion.
/// Keeps `declarations`/`declaration_trivia` parallel. Idempotent per
/// program: the ots pair and preset sessions inject at most once.
pub fn expand(program: &mut Program) {
    let mut ots_present = program.declarations.iter().any(
        |d| matches!(d, Declaration::Ots(o) if o.name == "InboundMulawToPcm16"),
    );
    let mut i = 0;
    while i < program.declarations.len() {
        let src = match &program.declarations[i] {
            Declaration::Voice(v) => {
                let s = expansion_source(v, !ots_present);
                let carrier_is_mulaw = v.carrier.is_empty() || v.carrier == "mulaw8k";
                if carrier_is_mulaw {
                    ots_present = true;
                }
                s
            }
            _ => {
                i += 1;
                continue;
            }
        };
        // The expansion source is generated from a template in this file —
        // a parse failure is a defect of the template, not of the adopter's
        // program (and the template is pinned by the tests below).
        let tokens = Lexer::new(&src, "<voice-expansion>")
            .tokenize()
            .expect("voice expansion source must lex");
        let expanded = Parser::new(tokens).parse().expect("voice expansion source must parse");
        let mut at = i + 1;
        for d in expanded.declarations {
            program.declarations.insert(at, d);
            program.declaration_trivia.insert(at, DeclarationTrivia::default());
            at += 1;
        }
        i = at;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::type_checker::TypeChecker;

    fn parse(src: &str) -> Program {
        let tokens = Lexer::new(src, "<t>").tokenize().expect("lex");
        Parser::new(tokens).parse().expect("parse")
    }

    /// The keystone claim (section 7 of the plan): a barge-in-capable phone agent
    /// from a blessed preset pair in UNDER 20 LINES, compiling clean.
    #[test]
    fn twenty_line_cascaded_voice_agent_compiles_clean() {
        let src = r#"
voice Concierge {
    stt: DeepgramSTT@v1
    tts: ElevenLabsTTS@v1
    interruptible: true
    legal_basis: legitimate_interest
}
"#;
        assert!(src.lines().filter(|l| !l.trim().is_empty()).count() < 20);
        let prog = parse(src);
        let errors = TypeChecker::new(&prog).check();
        assert!(
            errors.is_empty(),
            "the 20-line agent must check clean, got: {:?}",
            errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );
        // The expansion is REAL declarations in the program:
        let has = |f: &dyn Fn(&Declaration) -> bool| prog.declarations.iter().any(f);
        assert!(has(&|d| matches!(d, Declaration::Ots(o) if o.name == "InboundMulawToPcm16")));
        assert!(has(&|d| matches!(d, Declaration::Session(s) if s.name == "ConciergeCarrierTurn")));
        assert!(has(&|d| matches!(d, Declaration::Socket(s) if s.name == "ConciergeCall"
            && s.reconnect && s.legal_basis.as_deref() == Some("legitimate_interest"))));
        assert!(has(&|d| matches!(d, Declaration::Upstream(u) if u.name == "ConciergeSttLink"
            && u.preset.as_deref() == Some("DeepgramSTT@v1") && !u.map.is_empty())));
        assert!(has(&|d| matches!(d, Declaration::Upstream(u) if u.name == "ConciergeTtsLink")));
        // …and the voice declaration itself stays for provenance.
        assert!(has(&|d| matches!(d, Declaration::Voice(_))));
    }

    #[test]
    fn fused_realtime_voice_expands_one_leg() {
        let prog = parse(
            r#"
voice Live {
    realtime: OpenAIRealtime@v1
    carrier: pcm16
}
"#,
        );
        let errors = TypeChecker::new(&prog).check();
        assert!(errors.is_empty(), "got: {:?}", errors.iter().map(|e| &e.message).collect::<Vec<_>>());
        assert!(prog.declarations.iter().any(|d| matches!(d, Declaration::Upstream(u) if u.name == "LiveRealtimeLink")));
        // pcm16 carrier ⇒ no codec pair injected.
        assert!(!prog.declarations.iter().any(|d| matches!(d, Declaration::Ots(_))));
        // Non-interruptible socket: no cognitive_state parking.
        assert!(prog.declarations.iter().any(|d| matches!(d, Declaration::Socket(s) if s.name == "LiveCall" && !s.reconnect)));
    }

    #[test]
    fn two_mulaw_voices_share_one_codec_pair() {
        let prog = parse(
            r#"
voice A { stt: DeepgramSTT@v1 tts: CartesiaTTS@v1 }
voice B { stt: AssemblyAISTT@v1 tts: ElevenLabsTTS@v1 }
"#,
        );
        let inbound = prog
            .declarations
            .iter()
            .filter(|d| matches!(d, Declaration::Ots(o) if o.name == "InboundMulawToPcm16"))
            .count();
        assert_eq!(inbound, 1, "the codec pair injects once per program");
    }

    #[test]
    fn leg_naming_a_declared_upstream_is_not_redeclared() {
        let prog = parse(
            r#"
upstream MySTT from DeepgramSTT@v1 { secret: upstream.mystt.api_key }
upstream MyTTS from ElevenLabsTTS@v1 { secret: upstream.mytts.api_key }
voice Concierge {
    stt: MySTT
    tts: MyTTS
}
"#,
        );
        let errors = TypeChecker::new(&prog).check();
        assert!(errors.is_empty(), "got: {:?}", errors.iter().map(|e| &e.message).collect::<Vec<_>>());
        assert!(
            !prog.declarations.iter().any(|d| matches!(d, Declaration::Upstream(u) if u.name == "ConciergeSttLink")),
            "a declared-upstream leg is referenced, never re-declared"
        );
    }
}
