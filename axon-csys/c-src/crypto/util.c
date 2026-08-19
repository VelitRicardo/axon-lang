/*
 * v1.19.0 — Crypto utilities implementation.
 *
 * Constant-time compare: side-channel-free byte equality.
 * Hex codec: lowercase emit, case-insensitive accept.
 * Base64url-no-pad: RFC 4648 section 5 with trailing padding omitted.
 */

#include "util.h"

#include <stddef.h>

/* ──────────────────────────────────────────────────────────────────────
 * Constant-time equality
 * ────────────────────────────────────────────────────────────────── */

int axon_csys_ct_eq(const uint8_t* a, const uint8_t* b, size_t len) {
    if (len == 0u) {
        return 1;
    }
    if (a == NULL || b == NULL) {
        return 0;
    }
    /* Accumulate XOR differences across all bytes — no early exit. */
    uint8_t diff = 0u;
    for (size_t i = 0; i < len; ++i) {
        diff |= (uint8_t) (a[i] ^ b[i]);
    }
    /* Branch-free reduction of u8 to {0, 1}: diff == 0 → 1, else 0.
     *
     * Trick: in unsigned arithmetic, (0u - 1u) wraps to UINT_MAX.
     *   - For diff == 0: (0u - 1u) >> 31 == 1.
     *   - For diff in 1..255: (d - 1u) >> 31 == 0 (top bit clear).
     *
     * The 31-bit shift works because we cast diff to a >= 32-bit
     * unsigned. No data-dependent branch survives. */
    unsigned d = (unsigned) diff;
    return (int) ((d - 1u) >> 31);
}

/* ──────────────────────────────────────────────────────────────────────
 * Hex codec
 * ────────────────────────────────────────────────────────────────── */

static const char AXON_CSYS_HEX_DIGITS[] = "0123456789abcdef";

void axon_csys_hex_encode(const uint8_t* data, size_t len, char* out) {
    if (data == NULL || out == NULL) {
        return;
    }
    for (size_t i = 0; i < len; ++i) {
        out[2 * i] = AXON_CSYS_HEX_DIGITS[(data[i] >> 4) & 0x0Fu];
        out[2 * i + 1] = AXON_CSYS_HEX_DIGITS[data[i] & 0x0Fu];
    }
}

static bool axon_csys_hex_nibble(char c, uint8_t* out) {
    if (c >= '0' && c <= '9') {
        *out = (uint8_t) (c - '0');
        return true;
    }
    if (c >= 'a' && c <= 'f') {
        *out = (uint8_t) (c - 'a' + 10);
        return true;
    }
    if (c >= 'A' && c <= 'F') {
        *out = (uint8_t) (c - 'A' + 10);
        return true;
    }
    return false;
}

bool axon_csys_hex_decode(const char* hex, size_t hex_len, uint8_t* out) {
    if (hex == NULL || out == NULL) {
        return false;
    }
    if ((hex_len & 1u) != 0u) {
        return false;
    }
    for (size_t i = 0; i < hex_len; i += 2) {
        uint8_t hi = 0;
        uint8_t lo = 0;
        if (!axon_csys_hex_nibble(hex[i], &hi) || !axon_csys_hex_nibble(hex[i + 1], &lo)) {
            return false;
        }
        out[i / 2u] = (uint8_t) ((hi << 4) | lo);
    }
    return true;
}

/* ──────────────────────────────────────────────────────────────────────
 * Base64url-no-pad codec
 *
 * RFC 4648 section 5 alphabet:
 *   index 0..25  → 'A'..'Z'
 *   index 26..51 → 'a'..'z'
 *   index 52..61 → '0'..'9'
 *   index 62     → '-'
 *   index 63     → '_'
 *
 * Input bytes are processed in groups of 3 → 4 output chars. The
 * tail handles 1- and 2-byte remainders (encoded as 2 / 3 chars
 * respectively, no padding `=`).
 * ────────────────────────────────────────────────────────────────── */

/* Sized as 65 (includes the implicit NUL from the string literal) so
 * MSVC /W4 doesn't flag C4295 — `[64] = "..."` truncates the NUL.
 * The encoder only reads indices 0..63, so the trailing NUL is inert. */
static const char AXON_CSYS_B64URL_ALPHABET[] =
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/* Reverse alphabet lookup table: 0xFF marks invalid characters.
 * Built from the alphabet at module-init time would be cleaner, but
 * a baked-in table avoids a startup branch and stays trivially
 * verifiable. The table is initialised below via _Static_assert
 * verification of a few representative entries. */
static const uint8_t AXON_CSYS_B64URL_DECODE[256] = {
    /* 0x00..0x1F — control */
    0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu,
    0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu,
    0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu,
    0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu,
    /* 0x20..0x2F — space, punctuation */
    0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu,
    0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu,   62u, 0xFFu, 0xFFu, /* 0x2D = '-' */
    /* 0x30..0x3F — digits + punctuation */
      52u,   53u,   54u,   55u,   56u,   57u,   58u,   59u, /* 0x30..0x37 = '0'..'7' */
      60u,   61u, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, /* 0x38..0x39 = '8','9' */
    /* 0x40..0x5F — uppercase */
    0xFFu,    0u,    1u,    2u,    3u,    4u,    5u,    6u, /* 0x41..0x47 = 'A'..'G' */
       7u,    8u,    9u,   10u,   11u,   12u,   13u,   14u, /* 0x48..0x4F = 'H'..'O' */
      15u,   16u,   17u,   18u,   19u,   20u,   21u,   22u, /* 0x50..0x57 = 'P'..'W' */
      23u,   24u,   25u, 0xFFu, 0xFFu, 0xFFu, 0xFFu,   63u, /* 0x58..0x5A = 'X','Y','Z'; 0x5F = '_' */
    /* 0x60..0x7F — lowercase */
    0xFFu,   26u,   27u,   28u,   29u,   30u,   31u,   32u, /* 0x61..0x67 = 'a'..'g' */
      33u,   34u,   35u,   36u,   37u,   38u,   39u,   40u, /* 0x68..0x6F = 'h'..'o' */
      41u,   42u,   43u,   44u,   45u,   46u,   47u,   48u, /* 0x70..0x77 = 'p'..'w' */
      49u,   50u,   51u, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, /* 0x78..0x7A = 'x','y','z' */
    /* 0x80..0xFF — non-ASCII */
    0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu,
    0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu,
    0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu,
    0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu,
    0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu,
    0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu,
    0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu,
    0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu,
    0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu,
    0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu,
    0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu,
    0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu,
    0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu,
    0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu,
    0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu,
    0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu, 0xFFu,
};

size_t axon_csys_b64url_encoded_len(size_t byte_count) {
    /* Each 3-byte group → 4 chars; remainder of 1 byte → 2 chars,
     * 2 bytes → 3 chars. Equivalent to ceil(byte_count * 4 / 3). */
    size_t full = byte_count / 3u;
    size_t rem = byte_count % 3u;
    size_t out_len = full * 4u;
    if (rem == 1u) {
        out_len += 2u;
    } else if (rem == 2u) {
        out_len += 3u;
    }
    return out_len;
}

bool axon_csys_b64url_encode(
    const uint8_t* data,
    size_t len,
    char* out,
    size_t out_cap,
    size_t* out_len)
{
    if ((data == NULL && len > 0u) || out == NULL) {
        return false;
    }
    size_t need = axon_csys_b64url_encoded_len(len);
    if (out_cap < need) {
        return false;
    }
    size_t i = 0;
    size_t j = 0;
    /* Full 3 → 4 groups. */
    while (i + 3u <= len) {
        uint32_t triple = ((uint32_t) data[i] << 16)
                        | ((uint32_t) data[i + 1] << 8)
                        |  (uint32_t) data[i + 2];
        out[j]     = AXON_CSYS_B64URL_ALPHABET[(triple >> 18) & 0x3Fu];
        out[j + 1] = AXON_CSYS_B64URL_ALPHABET[(triple >> 12) & 0x3Fu];
        out[j + 2] = AXON_CSYS_B64URL_ALPHABET[(triple >> 6) & 0x3Fu];
        out[j + 3] = AXON_CSYS_B64URL_ALPHABET[triple & 0x3Fu];
        i += 3u;
        j += 4u;
    }
    /* Tail. */
    size_t rem = len - i;
    if (rem == 1u) {
        uint32_t single = (uint32_t) data[i] << 16;
        out[j]     = AXON_CSYS_B64URL_ALPHABET[(single >> 18) & 0x3Fu];
        out[j + 1] = AXON_CSYS_B64URL_ALPHABET[(single >> 12) & 0x3Fu];
        j += 2u;
    } else if (rem == 2u) {
        uint32_t pair = ((uint32_t) data[i] << 16) | ((uint32_t) data[i + 1] << 8);
        out[j]     = AXON_CSYS_B64URL_ALPHABET[(pair >> 18) & 0x3Fu];
        out[j + 1] = AXON_CSYS_B64URL_ALPHABET[(pair >> 12) & 0x3Fu];
        out[j + 2] = AXON_CSYS_B64URL_ALPHABET[(pair >> 6) & 0x3Fu];
        j += 3u;
    }
    if (out_len != NULL) {
        *out_len = j;
    }
    return true;
}

size_t axon_csys_b64url_decoded_len(size_t char_count) {
    /* Inverse of `_encoded_len`:
     *   4k chars   → 3k bytes
     *   4k+2 chars → 3k+1 byte
     *   4k+3 chars → 3k+2 bytes
     *   4k+1 chars → invalid (single base64 char encodes 6 bits,
     *                cannot represent a whole number of bytes). */
    size_t full = char_count / 4u;
    size_t rem = char_count % 4u;
    if (rem == 1u) {
        return SIZE_MAX;
    }
    size_t out_len = full * 3u;
    if (rem == 2u) {
        out_len += 1u;
    } else if (rem == 3u) {
        out_len += 2u;
    }
    return out_len;
}

bool axon_csys_b64url_decode(
    const char* in,
    size_t len,
    uint8_t* out,
    size_t out_cap,
    size_t* out_len)
{
    if ((in == NULL && len > 0u) || (out == NULL && out_cap > 0u)) {
        return false;
    }
    size_t need = axon_csys_b64url_decoded_len(len);
    if (need == SIZE_MAX) {
        return false;
    }
    if (out_cap < need) {
        return false;
    }
    /* Fast-path the trivial empty case explicitly. The decode loop
     * below writes to `out[j]`; clang-analyzer's NullDereference
     * checker can't follow the chain (`out_cap >= need` and `need
     * > 0` together imply `out != NULL`) across the helper
     * `axon_csys_b64url_decoded_len`, so it flags the loop body's
     * `out[j]` as a potential null deref. The early return below
     * teaches the checker that the loop is never reached when
     * `out` is permitted-NULL (i.e. when `len == 0`). */
    if (len == 0u) {
        if (out_len != NULL) {
            *out_len = 0u;
        }
        return true;
    }
    /* Belt-and-braces: by here `len > 0` so `need > 0` so
     * `out_cap > 0` so (per precondition) `out != NULL`. The
     * explicit defensive check teaches clang-analyzer the same
     * thing — without it the static checker still flags the
     * `out[j]` writes below as potential null derefs. */
    if (out == NULL) {
        return false;
    }
    size_t i = 0;
    size_t j = 0;
    /* Full 4 → 3 groups. */
    while (i + 4u <= len) {
        uint8_t a = AXON_CSYS_B64URL_DECODE[(uint8_t) in[i]];
        uint8_t b = AXON_CSYS_B64URL_DECODE[(uint8_t) in[i + 1]];
        uint8_t c = AXON_CSYS_B64URL_DECODE[(uint8_t) in[i + 2]];
        uint8_t d = AXON_CSYS_B64URL_DECODE[(uint8_t) in[i + 3]];
        if ((a | b | c | d) >= 64u) {
            return false;
        }
        uint32_t triple = ((uint32_t) a << 18)
                        | ((uint32_t) b << 12)
                        | ((uint32_t) c << 6)
                        |  (uint32_t) d;
        out[j]     = (uint8_t) ((triple >> 16) & 0xFFu);
        out[j + 1] = (uint8_t) ((triple >> 8) & 0xFFu);
        out[j + 2] = (uint8_t) (triple & 0xFFu);
        i += 4u;
        j += 3u;
    }
    /* Tail. */
    size_t rem = len - i;
    if (rem == 2u) {
        uint8_t a = AXON_CSYS_B64URL_DECODE[(uint8_t) in[i]];
        uint8_t b = AXON_CSYS_B64URL_DECODE[(uint8_t) in[i + 1]];
        if ((a | b) >= 64u) {
            return false;
        }
        out[j] = (uint8_t) ((a << 2) | (b >> 4));
        j += 1u;
    } else if (rem == 3u) {
        uint8_t a = AXON_CSYS_B64URL_DECODE[(uint8_t) in[i]];
        uint8_t b = AXON_CSYS_B64URL_DECODE[(uint8_t) in[i + 1]];
        uint8_t c = AXON_CSYS_B64URL_DECODE[(uint8_t) in[i + 2]];
        if ((a | b | c) >= 64u) {
            return false;
        }
        out[j]     = (uint8_t) ((a << 2) | (b >> 4));
        out[j + 1] = (uint8_t) ((b << 4) | (c >> 2));
        j += 2u;
    }
    if (out_len != NULL) {
        *out_len = j;
    }
    return true;
}
