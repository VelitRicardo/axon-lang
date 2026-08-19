/*
 * v1.19.0 — HMAC-SHA256 (FIPS 198-1).
 *
 * Pure-C HMAC construction over the in-house SHA-256 transform
 * (`axon_csys_sha256`). FIPS 198-1 section 4 algorithm:
 *
 *   K' = SHA256(K) if len(K) > B, else K padded to B
 *   ipad = K' XOR 0x36 repeated
 *   opad = K' XOR 0x5C repeated
 *   HMAC(K, M) = SHA256(opad || SHA256(ipad || M))
 *
 * Block size B = 64 bytes for SHA-256.
 *
 * Pillar split: identical to sha256.h — C handles the transform,
 * Rust shim wraps for safety + ergonomics. No allocation in any
 * entry point; the streaming context lives on the caller's stack.
 */

#ifndef AXON_CSYS_CRYPTO_HMAC_H
#define AXON_CSYS_CRYPTO_HMAC_H

#include "sha256.h"

#ifdef __cplusplus
extern "C" {
#endif

/* HMAC-SHA256 streaming context. Holds the inner SHA-256 mid-flight
 * + the saved opad bytes used to reseed the outer hash at finalise
 * time. Lay out so the larger inner_ctx comes first — the structure
 * is sized like one SHA-256 ctx + one block, ~136 bytes, fits in
 * any stack frame. */
typedef struct {
    AxonCsysSha256Ctx inner_ctx;
    uint8_t opad[AXON_CSYS_SHA256_BLOCK_SIZE];
} AxonCsysHmacSha256Ctx;

/* Initialise the streaming HMAC with the given key. Any key length
 * is accepted (FIPS 198-1 section 3 requires arbitrary-length support);
 * keys longer than 64 bytes are pre-hashed per section 5. */
void axon_csys_hmac_sha256_init(
    AxonCsysHmacSha256Ctx* ctx,
    const uint8_t* key,
    size_t key_len);

/* Feed `len` bytes of message into the running HMAC. May be called
 * any number of times. */
void axon_csys_hmac_sha256_update(
    AxonCsysHmacSha256Ctx* ctx,
    const uint8_t* data,
    size_t len);

/* Finalise and write the 32-byte MAC to `out`. The ctx is consumed
 * (subsequent updates yield undefined output); call `_init` again
 * before reuse. */
void axon_csys_hmac_sha256_final(
    AxonCsysHmacSha256Ctx* ctx,
    uint8_t out[AXON_CSYS_SHA256_DIGEST_SIZE]);

/* One-shot convenience: equivalent to init → update(data,len) → final.
 * The dominant entry point — streaming exists for callers that compute
 * MACs over multi-segment messages without staging them. */
void axon_csys_hmac_sha256(
    const uint8_t* key,
    size_t key_len,
    const uint8_t* data,
    size_t data_len,
    uint8_t out[AXON_CSYS_SHA256_DIGEST_SIZE]);

#ifdef __cplusplus
}
#endif

#endif /* AXON_CSYS_CRYPTO_HMAC_H */
