/*
 * v1.19.0 — SHA-256 (FIPS 180-4 section 6.2).
 *
 * Pure-C implementation of the SHA-256 secure hash algorithm,
 * algorithmically compliant with FIPS 180-4. The implementation is
 * endian-portable (uses byte loads rather than word casts), runs in
 * constant time relative to input bytes (no data-dependent branches),
 * and passes the NIST CAVS reference vectors that the drift gate
 * verifies.
 *
 * Pillar split (founder principle):
 *   - C side: hash transform + state machine. No allocation in any
 *     entry point — caller owns the context struct.
 *   - Rust side: high-level API + error type construction. Time
 *     handling stays out of this module entirely (see continuity.h
 *     for the wire-format primitive that does NOT touch system time).
 *
 * Mathematical pillar (preserved verbatim from FIPS 180-4):
 * - 8 hash words h0..h7 initialised per section 5.3.3
 * - 64 round constants K[0..63] per section 4.2.2
 * - Per 64-byte message block: 64-round compression per section 6.2.2
 * - Padding: section 5.1.1 (append 0x80 then 0x00..0x00 then 64-bit BE
 *     length-in-bits)
 *
 * "FIPS-friendly" means algorithmically compliant + drift-gated
 * against NIST vectors. NOT formally validated by NIST CAVS labs —
 * adopters who need that level of certification can opt in to a
 * BoringSSL/OpenSSL-FIPS link via a future cargo feature flag; the
 * default pure-C path is auditable + dep-free.
 */

#ifndef AXON_CSYS_CRYPTO_SHA256_H
#define AXON_CSYS_CRYPTO_SHA256_H

#include <stddef.h>
#include <stdint.h>

/* Pre-C23 GCC short-circuit caveat — see probe.c (nested-#ifdef pattern). */
#ifdef __has_c_attribute
#  if __has_c_attribute(nodiscard)
#    define AXON_CSYS_SHA256_NODISCARD [[nodiscard]]
#  endif
#endif
#ifndef AXON_CSYS_SHA256_NODISCARD
#  define AXON_CSYS_SHA256_NODISCARD
#endif

#ifdef __cplusplus
extern "C" {
#endif

#define AXON_CSYS_SHA256_DIGEST_SIZE 32u
#define AXON_CSYS_SHA256_BLOCK_SIZE  64u

/* Streaming hash context. Caller owns the storage; the C side does
 * not allocate. Initialise with `axon_csys_sha256_init`, feed with
 * `_update`, finalise with `_final`. The buffer field holds at most
 * one partial block of unprocessed bytes; `buf_len` is the number
 * of bytes currently in the buffer (always < AXON_CSYS_SHA256_BLOCK_SIZE
 * between API calls). */
typedef struct {
    uint32_t h[8];          /* running hash state */
    uint64_t total_bits;    /* total message length in bits */
    uint8_t  buf[AXON_CSYS_SHA256_BLOCK_SIZE];
    uint8_t  buf_len;
} AxonCsysSha256Ctx;

/* Initialise to FIPS 180-4 section 5.3.3 starting hash values. */
void axon_csys_sha256_init(AxonCsysSha256Ctx* ctx);

/* Feed `len` bytes into the running hash. May be called any number
 * of times. */
void axon_csys_sha256_update(
    AxonCsysSha256Ctx* ctx,
    const uint8_t* data,
    size_t len);

/* Finalise the hash and write the 32-byte digest to `out`. The ctx
 * is consumed (subsequent updates yield undefined output); call
 * `_init` again before reuse. */
void axon_csys_sha256_final(
    AxonCsysSha256Ctx* ctx,
    uint8_t out[AXON_CSYS_SHA256_DIGEST_SIZE]);

/* One-shot convenience: equivalent to init → update(data,len) → final.
 * The most common entry point — the stream API exists for callers
 * that hash multi-segment messages without staging them. */
void axon_csys_sha256(
    const uint8_t* data,
    size_t len,
    uint8_t out[AXON_CSYS_SHA256_DIGEST_SIZE]);

#ifdef __cplusplus
}
#endif

#endif /* AXON_CSYS_CRYPTO_SHA256_H */
