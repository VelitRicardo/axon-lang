/*
 * v1.19.0 — HMAC-SHA256 (FIPS 198-1) implementation.
 *
 * See hmac.h for spec citations. Comments below mirror FIPS 198-1
 * sections so an auditor can read this file alongside the standard.
 */

#include "hmac.h"

#include <string.h>

#define AXON_CSYS_HMAC_IPAD 0x36u
#define AXON_CSYS_HMAC_OPAD 0x5Cu

void axon_csys_hmac_sha256_init(
    AxonCsysHmacSha256Ctx* ctx,
    const uint8_t* key,
    size_t key_len)
{
    if (ctx == NULL || (key == NULL && key_len > 0u)) {
        return;
    }

    /* section 5 — derive the block-sized key K'. */
    uint8_t k_prime[AXON_CSYS_SHA256_BLOCK_SIZE];
    if (key_len > AXON_CSYS_SHA256_BLOCK_SIZE) {
        /* section 5 step 1 + section 5 step 2 — long key: pre-hash, then zero-pad. */
        axon_csys_sha256(key, key_len, k_prime);
        memset(k_prime + AXON_CSYS_SHA256_DIGEST_SIZE, 0,
               AXON_CSYS_SHA256_BLOCK_SIZE - AXON_CSYS_SHA256_DIGEST_SIZE);
    } else {
        /* section 5 step 3 — short key: zero-pad in place. The NULL-guard
         * above permits `(key == NULL, key_len == 0)`; standards-
         * compliant memcpy is a no-op at len 0 even with NULL but
         * clang-analyzer's NonNullParamChecker flags the call
         * regardless. Explicit length guard teaches the checker. */
        if (key_len > 0u) {
            memcpy(k_prime, key, key_len);
        }
        memset(k_prime + key_len, 0, AXON_CSYS_SHA256_BLOCK_SIZE - key_len);
    }

    /* section 4 step 1 — derive ipad / opad and seed the inner hash with
     * ipad. Save the opad bytes for the outer hash at finalise. */
    uint8_t ipad[AXON_CSYS_SHA256_BLOCK_SIZE];
    for (size_t i = 0; i < AXON_CSYS_SHA256_BLOCK_SIZE; ++i) {
        ipad[i] = k_prime[i] ^ AXON_CSYS_HMAC_IPAD;
        ctx->opad[i] = k_prime[i] ^ AXON_CSYS_HMAC_OPAD;
    }
    axon_csys_sha256_init(&ctx->inner_ctx);
    axon_csys_sha256_update(&ctx->inner_ctx, ipad, AXON_CSYS_SHA256_BLOCK_SIZE);

    /* Defensive zeroisation: K' carries a transformed copy of the
     * caller's key. Wipe so it does not linger in this stack frame
     * past return. The compiler may legally elide a memset to local
     * memory; the canonical defence is `volatile` writes via a
     * cast, which we use here to be explicit. The cost is negligible
     * (one block). */
    volatile uint8_t* k_wipe = (volatile uint8_t*) k_prime;
    for (size_t i = 0; i < AXON_CSYS_SHA256_BLOCK_SIZE; ++i) {
        k_wipe[i] = 0u;
    }
    /* ipad is non-secret (it's K' XOR public constant) but wipe
     * for symmetry with k_prime — same defensive posture. */
    volatile uint8_t* i_wipe = (volatile uint8_t*) ipad;
    for (size_t i = 0; i < AXON_CSYS_SHA256_BLOCK_SIZE; ++i) {
        i_wipe[i] = 0u;
    }
}

void axon_csys_hmac_sha256_update(
    AxonCsysHmacSha256Ctx* ctx,
    const uint8_t* data,
    size_t len)
{
    if (ctx == NULL) {
        return;
    }
    axon_csys_sha256_update(&ctx->inner_ctx, data, len);
}

void axon_csys_hmac_sha256_final(
    AxonCsysHmacSha256Ctx* ctx,
    uint8_t out[AXON_CSYS_SHA256_DIGEST_SIZE])
{
    if (ctx == NULL || out == NULL) {
        return;
    }

    /* section 4 step 5 — finalise the inner hash. */
    uint8_t inner_digest[AXON_CSYS_SHA256_DIGEST_SIZE];
    axon_csys_sha256_final(&ctx->inner_ctx, inner_digest);

    /* section 4 step 6 — outer hash: SHA256(opad || inner_digest). */
    AxonCsysSha256Ctx outer;
    axon_csys_sha256_init(&outer);
    axon_csys_sha256_update(&outer, ctx->opad, AXON_CSYS_SHA256_BLOCK_SIZE);
    axon_csys_sha256_update(&outer, inner_digest, AXON_CSYS_SHA256_DIGEST_SIZE);
    axon_csys_sha256_final(&outer, out);

    /* Defensive zeroisation: opad + inner_digest could leak partial
     * key material (opad is K' XOR public constant; inner_digest is
     * a hash that depends on K' + the message). */
    volatile uint8_t* opad_wipe = (volatile uint8_t*) ctx->opad;
    for (size_t i = 0; i < AXON_CSYS_SHA256_BLOCK_SIZE; ++i) {
        opad_wipe[i] = 0u;
    }
    volatile uint8_t* inner_wipe = (volatile uint8_t*) inner_digest;
    for (size_t i = 0; i < AXON_CSYS_SHA256_DIGEST_SIZE; ++i) {
        inner_wipe[i] = 0u;
    }
    /* outer ctx h[] holds the result (now in `out`) — wiping would
     * not protect the caller, who already owns `out`. */
}

void axon_csys_hmac_sha256(
    const uint8_t* key,
    size_t key_len,
    const uint8_t* data,
    size_t data_len,
    uint8_t out[AXON_CSYS_SHA256_DIGEST_SIZE])
{
    AxonCsysHmacSha256Ctx ctx;
    axon_csys_hmac_sha256_init(&ctx, key, key_len);
    axon_csys_hmac_sha256_update(&ctx, data, data_len);
    axon_csys_hmac_sha256_final(&ctx, out);
}
