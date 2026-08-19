/*
 * v1.19.0 — Continuity-token wire format primitives.
 *
 * Pure-C primitives that produce + verify the byte-format of axon's
 * PEM continuity token (v1.4.0). The wire layout (preserved
 * verbatim from the Rust reference impl in
 * `axon-rs/src/pem/continuity_token.rs`):
 *
 *   plain_text = session_id || 0x1e || expiry_ms_decimal
 *   mac        = HMAC-SHA256(key, plain_text)
 *   decoded    = plain_text || 0x1e || hex_lower(mac)
 *   wire       = base64url_no_pad(decoded)
 *
 * 0x1e is the ASCII record-separator. The hex-then-base64 outer
 * layer is historical (the original Python prototype emitted hex
 * for human inspection and base64-wrapped to make the field URL-
 * safe); the C port preserves this byte-identically so existing
 * tokens issued by Rust signers verify against this kernel and
 * vice versa.
 *
 * Pillar split (founder principle):
 *   - C side: HMAC compute + base64url + hex + constant-time MAC
 *     compare + record-separator parsing. NO time logic — the
 *     primitive returns the parsed `expiry_ms` and the caller
 *     decides if it has expired (Rust shim does that with chrono).
 *   - Rust side: chrono arithmetic for `expires_at <= Utc::now()`,
 *     UTF-8 validation of session_id strings, error-type construction.
 *
 * Mathematical pillar (preserved verbatim from Rust ref impl):
 *   - Sign body = session_id || 0x1e || expiry_ms (UTF-8 decimal,
 *     i64-typed, signed). MUST NOT include the trailing
 *     `0x1e || hex(mac)` in the MAC input (covers a foot-gun in the
 *     original Python prototype).
 *   - Verify: split decoded text on 0x1e into exactly 3 fields;
 *     reconstruct sign body from fields[0] || 0x1e || fields[1];
 *     constant-time compare to hex-decoded fields[2].
 */

#ifndef AXON_CSYS_CRYPTO_CONTINUITY_H
#define AXON_CSYS_CRYPTO_CONTINUITY_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Error codes — negative space leaves room for "successful sign
 * produced N bytes" semantics, mirrored in the Rust shim. */
typedef enum {
    AXON_CSYS_CONT_OK                = 0,
    AXON_CSYS_CONT_BAD_BASE64        = -1,  /* base64url decode failed */
    AXON_CSYS_CONT_BAD_FIELD_COUNT   = -2,  /* not exactly 3 0x1e-separated fields */
    AXON_CSYS_CONT_BAD_HEX           = -3,  /* MAC field not 64 hex chars */
    AXON_CSYS_CONT_BAD_EXPIRY        = -4,  /* expiry field not parseable as i64 */
    AXON_CSYS_CONT_FORGED_OR_ROTATED = -5,  /* MAC mismatch */
    AXON_CSYS_CONT_BUFFER_TOO_SMALL  = -6,  /* output buffer insufficient */
    AXON_CSYS_CONT_NULL_ARG          = -7,  /* required pointer was NULL */
    AXON_CSYS_CONT_PAYLOAD_TOO_LARGE = -8,  /* session_id over compile-time limit */
} AxonCsysContinuityError;

/* Maximum length of the session_id field accepted by the C kernel.
 * Adopters issue UUIDs (~36 chars) or short URL-safe slugs; 1024
 * is generous and keeps the per-call stack budget under 4 KiB. */
#define AXON_CSYS_CONT_MAX_SESSION_ID 1024u

/* Sign a continuity token body.
 *
 * Inputs:
 *   key             — HMAC signing key (any length; tiktoken per
 * FIPS 198-1 section 3 + section 5).
 *   session_id      — UTF-8 session id; MUST NOT contain 0x1e.
 *   expiry_ms       — i64 milliseconds since Unix epoch.
 *
 * Output:
 *   out_wire        — receives base64url-no-pad encoding.
 *   out_cap         — capacity of out_wire in bytes.
 *   out_len         — receives number of bytes written (or NULL to
 *                     skip).
 *
 * Returns AXON_CSYS_CONT_OK on success, error code otherwise. */
AxonCsysContinuityError axon_csys_continuity_sign(
    const uint8_t* key,
    size_t key_len,
    const char* session_id,
    size_t session_id_len,
    int64_t expiry_ms,
    char* out_wire,
    size_t out_cap,
    size_t* out_len);

/* Verify a continuity token wire and parse its fields.
 *
 * Inputs:
 *   key                    — HMAC verification key (must equal the
 *                            signer key; rotated keys produce
 *                            FORGED_OR_ROTATED).
 *   wire                   — base64url-no-pad encoded token.
 *
 * Outputs:
 *   out_session_id         — receives the session_id bytes (no
 *                            NUL terminator).
 *   session_id_cap         — capacity of out_session_id.
 *   out_session_id_len     — receives session_id length (or NULL).
 *   out_expiry_ms          — receives parsed expiry milliseconds.
 *
 * Returns AXON_CSYS_CONT_OK on success, error code otherwise.
 *
 * The Rust shim is responsible for the `expires_at <= now()` check
 * and translates AXON_CSYS_CONT_OK + a stale expiry into the typed
 * `Expired` error variant. The C kernel never reads system time. */
AxonCsysContinuityError axon_csys_continuity_verify(
    const uint8_t* key,
    size_t key_len,
    const char* wire,
    size_t wire_len,
    char* out_session_id,
    size_t session_id_cap,
    size_t* out_session_id_len,
    int64_t* out_expiry_ms);

/* Compute the upper bound on the encoded wire length for a given
 * session_id length. Equal to
 *   ceil((session_id_len + 1 + 20 + 1 + 64) * 4 / 3)
 * where 20 covers the worst-case decimal expansion of i64 (incl. sign).
 * Useful for caller capacity sizing. */
size_t axon_csys_continuity_max_wire_len(size_t session_id_len);

#ifdef __cplusplus
}
#endif

#endif /* AXON_CSYS_CRYPTO_CONTINUITY_H */
