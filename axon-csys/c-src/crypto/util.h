/*
 * v1.19.0 — Crypto utilities: constant-time compare + hex codec
 * + base64url-no-pad codec.
 *
 * These primitives complement the SHA-256 / HMAC kernels in this
 * directory and are kept separate to keep each translation unit
 * tightly scoped + auditable. They have NO dependency on the hash
 * primitives — adopters can use them standalone.
 *
 * Pillar split:
 *   - C side: constant-time byte comparison (single side-channel
 *     surface in the audit posture); hex + base64url codecs that
 *     run in O(n) without allocating.
 *   - Rust side: error-typed wrappers that add UTF-8 validation +
 *     length checks.
 *
 * Mathematical pillar (preserved from the relevant standards):
 *   - Constant-time compare: XOR-OR reduction with branch-free
 * final-bit extraction. See section 6 of RFC 4634-bis (informational
 *     description of standard practice).
 *   - Hex: lowercase encoding per common Unix convention; decode
 *     accepts both upper and lower case.
 * - Base64url: RFC 4648 section 5 — alphabet substitution from section 4 with
 *     `+` → `-` and `/` → `_`; this module's variant strips
 *     trailing padding (`=`) per common URL-safe usage.
 */

#ifndef AXON_CSYS_CRYPTO_UTIL_H
#define AXON_CSYS_CRYPTO_UTIL_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ──────────────────────────────────────────────────────────────────────
 * Constant-time equality
 * ────────────────────────────────────────────────────────────────── */

/* Compare `len` bytes from `a` and `b` in constant time. Returns 1
 * iff all bytes are equal, 0 otherwise. The function executes the
 * exact same number of memory accesses + arithmetic operations
 * regardless of where (or whether) a difference appears, so that
 * timing observation cannot reveal byte positions of mismatch.
 *
 * NULL `a` / `b` with `len == 0` returns 1 (vacuously equal); any
 * NULL with `len > 0` returns 0 — defensive, consistent with the
 * Rust shim's safe-API contract. */
int axon_csys_ct_eq(const uint8_t* a, const uint8_t* b, size_t len);

/* ──────────────────────────────────────────────────────────────────────
 * Hex codec (lowercase emit, mixed-case accept)
 * ────────────────────────────────────────────────────────────────── */

/* Encode `len` bytes to lowercase hex. Writes 2*len characters into
 * `out`; the caller is responsible for sizing the buffer. Does NOT
 * NUL-terminate. */
void axon_csys_hex_encode(
    const uint8_t* data,
    size_t len,
    char* out);

/* Decode `hex_len` characters of hex into bytes. Returns true on
 * success (writes hex_len/2 bytes to `out`); false if hex_len is
 * odd or any character is not a hex digit. Output is undefined on
 * failure — the caller should treat partial writes as garbage. */
bool axon_csys_hex_decode(
    const char* hex,
    size_t hex_len,
    uint8_t* out);

/* ──────────────────────────────────────────────────────────────────────
 * Base64url-no-pad codec (RFC 4648 section 5, padding stripped)
 * ────────────────────────────────────────────────────────────────── */

/* Compute the encoded length for a given input byte count. Equal to
 * ceil(byte_count * 4 / 3). Useful for caller-side capacity checks. */
size_t axon_csys_b64url_encoded_len(size_t byte_count);

/* Encode `len` bytes to base64url-no-pad. Writes the encoded
 * characters into `out`; the encoded length is written to `*out_len`
 * (or pass NULL to skip). Returns true if `out_cap` was sufficient,
 * false otherwise (no partial writes — `out` contents undefined
 * on capacity failure). Does NOT NUL-terminate. */
bool axon_csys_b64url_encode(
    const uint8_t* data,
    size_t len,
    char* out,
    size_t out_cap,
    size_t* out_len);

/* Compute the decoded length for a given input character count.
 * Returns SIZE_MAX if the length is not valid for base64url-no-pad
 * (specifically: `len % 4 == 1` is invalid; all other moduli are
 * valid). */
size_t axon_csys_b64url_decoded_len(size_t char_count);

/* Decode `len` base64url-no-pad characters into bytes. Writes the
 * decoded length to `*out_len` (or pass NULL). Returns true on
 * success, false if any character is outside the alphabet, len is
 * `4k+1`, or `out_cap` is insufficient. */
bool axon_csys_b64url_decode(
    const char* in,
    size_t len,
    uint8_t* out,
    size_t out_cap,
    size_t* out_len);

#ifdef __cplusplus
}
#endif

#endif /* AXON_CSYS_CRYPTO_UTIL_H */
