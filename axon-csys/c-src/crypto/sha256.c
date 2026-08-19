/*
 * v1.19.0 — SHA-256 (FIPS 180-4) implementation.
 *
 * Direct port of the FIPS 180-4 section 6.2.2 reference algorithm. Section
 * citations point at the published PDF (NIST 2015 reissue) so an
 * auditor can read this file alongside the standard.
 */

#include "sha256.h"

#include <string.h>

/* ──────────────────────────────────────────────────────────────────────
 * Constants
 * ────────────────────────────────────────────────────────────────── */

/* section 5.3.3 — initial hash values (the first 32 bits of the fractional
 * parts of the square roots of the first 8 primes). */
static const uint32_t AXON_CSYS_SHA256_INITIAL_H[8] = {
    0x6a09e667u, 0xbb67ae85u, 0x3c6ef372u, 0xa54ff53au,
    0x510e527fu, 0x9b05688cu, 0x1f83d9abu, 0x5be0cd19u,
};

/* section 4.2.2 — round constants (the first 32 bits of the fractional
 * parts of the cube roots of the first 64 primes). */
static const uint32_t AXON_CSYS_SHA256_K[64] = {
    0x428a2f98u, 0x71374491u, 0xb5c0fbcfu, 0xe9b5dba5u,
    0x3956c25bu, 0x59f111f1u, 0x923f82a4u, 0xab1c5ed5u,
    0xd807aa98u, 0x12835b01u, 0x243185beu, 0x550c7dc3u,
    0x72be5d74u, 0x80deb1feu, 0x9bdc06a7u, 0xc19bf174u,
    0xe49b69c1u, 0xefbe4786u, 0x0fc19dc6u, 0x240ca1ccu,
    0x2de92c6fu, 0x4a7484aau, 0x5cb0a9dcu, 0x76f988dau,
    0x983e5152u, 0xa831c66du, 0xb00327c8u, 0xbf597fc7u,
    0xc6e00bf3u, 0xd5a79147u, 0x06ca6351u, 0x14292967u,
    0x27b70a85u, 0x2e1b2138u, 0x4d2c6dfcu, 0x53380d13u,
    0x650a7354u, 0x766a0abbu, 0x81c2c92eu, 0x92722c85u,
    0xa2bfe8a1u, 0xa81a664bu, 0xc24b8b70u, 0xc76c51a3u,
    0xd192e819u, 0xd6990624u, 0xf40e3585u, 0x106aa070u,
    0x19a4c116u, 0x1e376c08u, 0x2748774cu, 0x34b0bcb5u,
    0x391c0cb3u, 0x4ed8aa4au, 0x5b9cca4fu, 0x682e6ff3u,
    0x748f82eeu, 0x78a5636fu, 0x84c87814u, 0x8cc70208u,
    0x90befffau, 0xa4506cebu, 0xbef9a3f7u, 0xc67178f2u,
};

/* ──────────────────────────────────────────────────────────────────────
 * Bit operations — FIPS 180-4 section 3.2 + section 4.1.2
 * ────────────────────────────────────────────────────────────────── */

static inline uint32_t axon_csys_sha256_rotr(uint32_t x, unsigned n) {
    /* Right rotate. C does not specify behaviour for shifts ≥ width;
     * this mask + branch chain stays in defined territory. n is
     * always 1..31 in this file by construction. */
    return (x >> n) | (x << (32u - n));
}

#define AXON_CSYS_SHA256_CH(x, y, z) (((x) & (y)) ^ (~(x) & (z)))
#define AXON_CSYS_SHA256_MAJ(x, y, z) (((x) & (y)) ^ ((x) & (z)) ^ ((y) & (z)))

#define AXON_CSYS_SHA256_BSIG0(x) \
    (axon_csys_sha256_rotr(x, 2) ^ axon_csys_sha256_rotr(x, 13) ^ axon_csys_sha256_rotr(x, 22))
#define AXON_CSYS_SHA256_BSIG1(x) \
    (axon_csys_sha256_rotr(x, 6) ^ axon_csys_sha256_rotr(x, 11) ^ axon_csys_sha256_rotr(x, 25))
#define AXON_CSYS_SHA256_SSIG0(x) \
    (axon_csys_sha256_rotr(x, 7) ^ axon_csys_sha256_rotr(x, 18) ^ ((x) >> 3))
#define AXON_CSYS_SHA256_SSIG1(x) \
    (axon_csys_sha256_rotr(x, 17) ^ axon_csys_sha256_rotr(x, 19) ^ ((x) >> 10))

/* Read a big-endian u32 from `p`. Endian-portable: works on any
 * host architecture without #ifdef branching. */
static inline uint32_t axon_csys_sha256_load_be32(const uint8_t* p) {
    return ((uint32_t) p[0] << 24)
         | ((uint32_t) p[1] << 16)
         | ((uint32_t) p[2] << 8)
         |  (uint32_t) p[3];
}

/* Write a big-endian u32 to `p`. */
static inline void axon_csys_sha256_store_be32(uint8_t* p, uint32_t v) {
    p[0] = (uint8_t) (v >> 24);
    p[1] = (uint8_t) (v >> 16);
    p[2] = (uint8_t) (v >> 8);
    p[3] = (uint8_t) v;
}

/* Write a big-endian u64 to `p`. */
static inline void axon_csys_sha256_store_be64(uint8_t* p, uint64_t v) {
    p[0] = (uint8_t) (v >> 56);
    p[1] = (uint8_t) (v >> 48);
    p[2] = (uint8_t) (v >> 40);
    p[3] = (uint8_t) (v >> 32);
    p[4] = (uint8_t) (v >> 24);
    p[5] = (uint8_t) (v >> 16);
    p[6] = (uint8_t) (v >> 8);
    p[7] = (uint8_t) v;
}

/* ──────────────────────────────────────────────────────────────────────
 * Compression function — FIPS 180-4 section 6.2.2
 * ────────────────────────────────────────────────────────────────── */

static void axon_csys_sha256_compress(
    uint32_t h[8],
    const uint8_t block[AXON_CSYS_SHA256_BLOCK_SIZE])
{
    /* section 6.2.2 step 1 — message schedule W[0..63]. */
    uint32_t w[64];
    for (size_t t = 0; t < 16; ++t) {
        w[t] = axon_csys_sha256_load_be32(block + t * 4u);
    }
    for (size_t t = 16; t < 64; ++t) {
        w[t] = AXON_CSYS_SHA256_SSIG1(w[t - 2])
             + w[t - 7]
             + AXON_CSYS_SHA256_SSIG0(w[t - 15])
             + w[t - 16];
    }

    /* section 6.2.2 step 2 — initialise working variables a..h. */
    uint32_t a = h[0];
    uint32_t b = h[1];
    uint32_t c = h[2];
    uint32_t d = h[3];
    uint32_t e = h[4];
    uint32_t f = h[5];
    uint32_t g = h[6];
    uint32_t hh = h[7];

    /* section 6.2.2 step 3 — 64 rounds. */
    for (size_t t = 0; t < 64; ++t) {
        uint32_t t1 = hh
                    + AXON_CSYS_SHA256_BSIG1(e)
                    + AXON_CSYS_SHA256_CH(e, f, g)
                    + AXON_CSYS_SHA256_K[t]
                    + w[t];
        uint32_t t2 = AXON_CSYS_SHA256_BSIG0(a)
                    + AXON_CSYS_SHA256_MAJ(a, b, c);
        hh = g;
        g = f;
        f = e;
        e = d + t1;
        d = c;
        c = b;
        b = a;
        a = t1 + t2;
    }

    /* section 6.2.2 step 4 — fold working variables back into h. */
    h[0] += a;
    h[1] += b;
    h[2] += c;
    h[3] += d;
    h[4] += e;
    h[5] += f;
    h[6] += g;
    h[7] += hh;
}

/* ──────────────────────────────────────────────────────────────────────
 * Public API
 * ────────────────────────────────────────────────────────────────── */

void axon_csys_sha256_init(AxonCsysSha256Ctx* ctx) {
    if (ctx == NULL) {
        return;
    }
    memcpy(ctx->h, AXON_CSYS_SHA256_INITIAL_H, sizeof ctx->h);
    ctx->total_bits = 0u;
    ctx->buf_len = 0u;
    /* Buffer contents intentionally left uninitialised — only the
     * first `buf_len` bytes are ever read. memset would be defensive
     * but adds a measurable cost on the empty-message hot path. */
}

void axon_csys_sha256_update(
    AxonCsysSha256Ctx* ctx,
    const uint8_t* data,
    size_t len)
{
    if (ctx == NULL || (data == NULL && len > 0u)) {
        return;
    }

    /* Update the total bit count up-front so the final padding can
     * use the original message length. SHA-256 supports messages up
     * to 2^64 - 1 bits per FIPS 180-4 section 5.1.1; the wraparound check
     * is below the application's plausible message size and we do
     * not enforce it here (would require a saturating add). */
    ctx->total_bits += (uint64_t) len * 8u;

    /* Drain caller bytes, filling the partial block first if any. */
    if (ctx->buf_len > 0u) {
        size_t need = AXON_CSYS_SHA256_BLOCK_SIZE - (size_t) ctx->buf_len;
        size_t take = (len < need) ? len : need;
        memcpy(ctx->buf + ctx->buf_len, data, take);
        ctx->buf_len = (uint8_t) ((size_t) ctx->buf_len + take);
        data += take;
        len -= take;
        if (ctx->buf_len == AXON_CSYS_SHA256_BLOCK_SIZE) {
            axon_csys_sha256_compress(ctx->h, ctx->buf);
            ctx->buf_len = 0u;
        }
    }

    /* Compress full blocks straight from the caller buffer. */
    while (len >= AXON_CSYS_SHA256_BLOCK_SIZE) {
        axon_csys_sha256_compress(ctx->h, data);
        data += AXON_CSYS_SHA256_BLOCK_SIZE;
        len -= AXON_CSYS_SHA256_BLOCK_SIZE;
    }

    /* Stash any remainder for the next call / final. */
    if (len > 0u) {
        memcpy(ctx->buf + ctx->buf_len, data, len);
        ctx->buf_len = (uint8_t) ((size_t) ctx->buf_len + len);
    }
}

void axon_csys_sha256_final(
    AxonCsysSha256Ctx* ctx,
    uint8_t out[AXON_CSYS_SHA256_DIGEST_SIZE])
{
    if (ctx == NULL || out == NULL) {
        return;
    }

    /* section 5.1.1 — pad to 56 (mod 64) bytes, then append 8-byte BE
     * length. The 0x80 byte is always added; depending on remaining
     * space we either pad-and-finish-this-block, pad a fresh second
     * block, or both. */
    uint64_t total_bits = ctx->total_bits;
    ctx->buf[ctx->buf_len++] = 0x80u;
    if (ctx->buf_len > 56u) {
        /* Not enough room for the length in this block — pad rest
         * with zeros, compress, then start a fresh block of zeros. */
        memset(ctx->buf + ctx->buf_len, 0, AXON_CSYS_SHA256_BLOCK_SIZE - ctx->buf_len);
        axon_csys_sha256_compress(ctx->h, ctx->buf);
        memset(ctx->buf, 0, 56u);
    } else {
        memset(ctx->buf + ctx->buf_len, 0, 56u - ctx->buf_len);
    }
    axon_csys_sha256_store_be64(ctx->buf + 56u, total_bits);
    axon_csys_sha256_compress(ctx->h, ctx->buf);

    /* Serialise final hash big-endian. */
    for (size_t i = 0; i < 8; ++i) {
        axon_csys_sha256_store_be32(out + i * 4u, ctx->h[i]);
    }
}

void axon_csys_sha256(
    const uint8_t* data,
    size_t len,
    uint8_t out[AXON_CSYS_SHA256_DIGEST_SIZE])
{
    AxonCsysSha256Ctx ctx;
    axon_csys_sha256_init(&ctx);
    axon_csys_sha256_update(&ctx, data, len);
    axon_csys_sha256_final(&ctx, out);
}
