# v1.4.0 load tests

Scripts run via [k6](https://k6.io/). Each script declares
`options.thresholds` wired to the GA SLOs enumerated in
`docs/SECURITY_AUDIT_v1_2_0.md`; k6 exits non-zero on any
breach, so these double as CI gates for the v1.2.0 release tag.

## SLO thresholds (enforced per script)

| Surface | p95 | p99 | Error rate | Script |
|---|---|---|---|---|
| WebSocket audio frame (11.a Stream + 11.b buffer + 11.e OTS) | 300ms | 500ms | < 0.5% | `k6_ws_audio_stream.js` |
| ReplayToken emission (11.c) | 1ms | 2ms | < 0.1% | `k6_replay_emission.js` |
| OTS pipeline synthesis cold + cached (11.e) | 5ms cold, 0.05ms cached | 10ms cold, 0.1ms cached | < 0.5% | `k6_ots_synthesis.js` |
| CognitiveState snapshot + restore ≤ 64 KiB (11.d) | 30ms | 50ms | < 0.5% | `k6_pem_snapshot.js` |

## What these DO NOT cover

- Kernel-level attacks on the axon-rs allocator (scoped to the
  fuzzing harness in `tests/security/`).
- Subprocess sandbox escapes (ffmpeg) — those live in the
  external pentest report per `docs/SECURITY_AUDIT_v1_2_0.md`.
- Cross-region data-residency enforcement under load — that's a
  10.l concern, tested in `axon-enterprise/tests/load/`.
