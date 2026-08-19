---
name: voice
summary: "The voice-agent simplicity layer: macro-expands (inspectable via axon desugar) to ots codecs + a carrier session/socket + upstream vendor legs — a blessed-preset phone agent in under 20 lines."
category: session_types
top_level: true
since: v2.37.0
grammar: |
  voice <Name> {
      stt: <Upstream|Preset@vN>        # cascaded: BOTH stt: and tts:
      tts: <Upstream|Preset@vN>
      realtime: <Upstream|Preset@vN>   # fused: exactly one (XOR stt/tts)
      carrier: mulaw8k | pcm16         # default mulaw8k (PSTN)
      interruptible: true | false # barge-in via v2.36.0 interrupt/resume
      legal_basis: <basis>             # REQUIRED when interruptible
      persona: <PersonaName>           # optional
      context: <ContextName>           # optional
  }
---

# `voice`

`voice` is the **simplicity layer** over v2.37.0's `upstream`: a working,
swappable, auditable phone agent in under 20 lines for any blessed
vendor preset — without giving up a single guarantee, because `voice`
is **pure macro-expansion** to the primitives already in the language.

```axon
voice Concierge {
    stt: DeepgramSTT@v1
    tts: ElevenLabsTTS@v1
    interruptible: true
    legal_basis: legitimate_interest
}
```

That declaration expands to — and `axon desugar` prints — exactly:

- the `ots` μ-law↔PCM16 codec pair (carrier `mulaw8k`, the PSTN
  default; `pcm16` skips it),
- a carrier-facing `session ConciergeCarrierTurn` + `socket
  ConciergeCall` with `backpressure: credit(8)` — and, because
  `interruptible: true`, a v2.36.0 `interrupt`/`resume` region for barge-in
  plus `reconnect: cognitive_state` and the declared `legal_basis` (the
  parked mid-utterance residual is a governed data-at-rest surface),
- one `upstream` per vendor leg (`ConciergeSttLink from DeepgramSTT@v1`,
  `ConciergeTtsLink from ElevenLabsTTS@v1`), themselves expanded from
  the v2.37.0 preset catalog.

## One grammar, both architectures

Cascaded (`stt:` + `tts:`) and fused speech-to-speech (`realtime:`) are
equally first-class — the same `voice` grammar, never a special-cased
second path:

```axon
voice Live {
    realtime: OpenAIRealtime@v1
    carrier: pcm16
}
```

## The laws (axon-T852)

- `stt:`+`tts:` XOR `realtime:` — cascaded needs both legs, fused
  exactly one.
- `interruptible: true` **requires** `legal_basis:` — the sugar cannot
  generate a program the `ParkedResidualSoundness` proof refutes.
- Every leg must resolve: a `Preset@vN` from the catalog, or a declared
  `upstream` (which is then referenced, never re-declared).
- The expansion earns **no exemption**: the generated session/socket/
  upstreams are checked by the ordinary v2.3.0/v2.36.0/v2.37.0 laws and carry the
  ordinary PCC proofs (`UpstreamProjectionSoundness`,
  `InterruptibleSessionSoundness`, `ParkedResidualSoundness`).

## Never a black box

`voice` itself never reaches the IR — the deployed artifact IS the
expansion, and `axon desugar <file>` prints it verbatim. A
simplicity layer a compliance reviewer cannot see through would undo
the audit-by-construction property; this one is the seeing-through.
Swapping Deepgram for AssemblyAI is a one-token edit; the seventh
vendor the market ships next quarter is a hand-written `upstream`
declaration away, not a rewrite.
