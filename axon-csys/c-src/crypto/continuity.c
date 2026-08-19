/*
 * v1.19.0 — Continuity-token wire format implementation.
 *
 * See continuity.h for spec rationale + pillar split.
 */

#include "continuity.h"

#include "hmac.h"
#include "util.h"

#include <stdio.h>
#include <string.h>

/* Worst-case decimal length of an i64 (sign + 19 digits). One byte
 * extra of breathing room for snprintf's NUL terminator. */
#define AXON_CSYS_CONT_I64_MAX_CHARS 20u

/* Wire layout constants. */
#define AXON_CSYS_CONT_RECORD_SEPARATOR ((char) 0x1e)

size_t axon_csys_continuity_max_wire_len(size_t session_id_len) {
    /* decoded = session_id || 0x1e || expiry || 0x1e || mac_hex
     * mac_hex is always 64 chars; expiry up to 20 chars. */
    size_t decoded_max = session_id_len
                       + 1u
                       + AXON_CSYS_CONT_I64_MAX_CHARS
                       + 1u
                       + (AXON_CSYS_SHA256_DIGEST_SIZE * 2u);
    return axon_csys_b64url_encoded_len(decoded_max);
}

/* Format an i64 as decimal into buf (NOT NUL-terminated by the
 * caller's contract — but snprintf does NUL-terminate, so we report
 * the strlen as the length). Returns the number of characters
 * written (excluding NUL). */
static size_t axon_csys_cont_format_i64(int64_t value, char* buf, size_t cap) {
    /* snprintf %lld — universally supported. The cast to long-long
     * is portable: int64_t is at least as wide as long long on every
     * conforming target. */
    int n = snprintf(buf, cap, "%lld", (long long) value);
    if (n < 0) {
        return 0u;
    }
    return (size_t) n;
}

/* Parse an i64 from a base-10 ASCII slice. Accepts an optional
 * leading '-'; rejects '+' and any non-digit character. Returns
 * true on success. We do NOT use strtoll because we want strict
 * "consume the entire slice" semantics — strtoll requires a NUL
 * terminator + accepts trailing whitespace.
 *
 * Overflow handling: detects when `acc * 10 + digit` would exceed
 * INT64_MAX (or for negative numbers, below INT64_MIN). Reports
 * failure. */
static bool axon_csys_cont_parse_i64(const char* s, size_t len, int64_t* out) {
    if (len == 0u) {
        return false;
    }
    bool negative = false;
    size_t i = 0;
    if (s[0] == '-') {
        negative = true;
        i = 1;
        if (len == 1u) {
            return false;
        }
    }
    /* Accumulate as u64 to detect overflow cleanly. */
    uint64_t acc = 0u;
    /* For negative numbers the magnitude can reach |INT64_MIN| =
     * 2^63, which requires u64 to represent without overflow. */
    const uint64_t bound = negative
        ? (uint64_t) INT64_MAX + 1u
        : (uint64_t) INT64_MAX;
    for (; i < len; ++i) {
        char c = s[i];
        if (c < '0' || c > '9') {
            return false;
        }
        uint8_t digit = (uint8_t) (c - '0');
        /* Check `acc * 10 + digit > bound` using division to dodge
         * the multiply-overflow itself. */
        if (acc > (bound - (uint64_t) digit) / 10u) {
            return false;
        }
        acc = acc * 10u + (uint64_t) digit;
    }
    if (negative) {
        if (acc == (uint64_t) INT64_MAX + 1u) {
            *out = INT64_MIN;
        } else {
            *out = -(int64_t) acc;
        }
    } else {
        *out = (int64_t) acc;
    }
    return true;
}

AxonCsysContinuityError axon_csys_continuity_sign(
    const uint8_t* key,
    size_t key_len,
    const char* session_id,
    size_t session_id_len,
    int64_t expiry_ms,
    char* out_wire,
    size_t out_cap,
    size_t* out_len)
{
    if ((key == NULL && key_len > 0u)
        || (session_id == NULL && session_id_len > 0u)
        || out_wire == NULL)
    {
        return AXON_CSYS_CONT_NULL_ARG;
    }
    if (session_id_len > AXON_CSYS_CONT_MAX_SESSION_ID) {
        return AXON_CSYS_CONT_PAYLOAD_TOO_LARGE;
    }
    /* Reject session_id containing the wire separator — would
     * confuse the verify-side splitter. The Rust ref impl does not
     * defend against this (it trusts callers); the C kernel adds
     * the check as a defence-in-depth measure. */
    for (size_t i = 0; i < session_id_len; ++i) {
        if (session_id[i] == AXON_CSYS_CONT_RECORD_SEPARATOR) {
            return AXON_CSYS_CONT_PAYLOAD_TOO_LARGE;
        }
    }

    /* Build sign body = session_id || 0x1e || decimal(expiry_ms).
     * Guard the memcpy on `session_id_len > 0` because the
     * NULL-arg check above permits `(session_id == NULL,
     * session_id_len == 0)`. The C standard says memcpy with len 0
     * is a no-op even on NULL pointers, but clang-analyzer's
     * `core.NonNullParamChecker` flags the memcpy regardless. The
     * explicit guard teaches the checker AND matches the principle
     * of "no operation when there's nothing to operate on". */
    char body[AXON_CSYS_CONT_MAX_SESSION_ID + 1u + AXON_CSYS_CONT_I64_MAX_CHARS + 1u];
    if (session_id_len > 0u) {
        memcpy(body, session_id, session_id_len);
    }
    body[session_id_len] = AXON_CSYS_CONT_RECORD_SEPARATOR;
    char expiry_str[AXON_CSYS_CONT_I64_MAX_CHARS + 1u];
    size_t expiry_len = axon_csys_cont_format_i64(expiry_ms, expiry_str, sizeof expiry_str);
    if (expiry_len == 0u || expiry_len > AXON_CSYS_CONT_I64_MAX_CHARS) {
        return AXON_CSYS_CONT_BAD_EXPIRY;
    }
    memcpy(body + session_id_len + 1u, expiry_str, expiry_len);
    size_t body_len = session_id_len + 1u + expiry_len;

    /* Compute MAC over body. */
    uint8_t mac[AXON_CSYS_SHA256_DIGEST_SIZE];
    axon_csys_hmac_sha256(key, key_len, (const uint8_t*) body, body_len, mac);

    /* Build decoded wire = body || 0x1e || hex(mac). */
    char decoded[sizeof body + 1u + (AXON_CSYS_SHA256_DIGEST_SIZE * 2u)];
    memcpy(decoded, body, body_len);
    decoded[body_len] = AXON_CSYS_CONT_RECORD_SEPARATOR;
    axon_csys_hex_encode(mac, AXON_CSYS_SHA256_DIGEST_SIZE,
                         decoded + body_len + 1u);
    size_t decoded_len = body_len + 1u + (AXON_CSYS_SHA256_DIGEST_SIZE * 2u);

    /* Base64url encode into out_wire. */
    size_t encoded_len = 0u;
    if (!axon_csys_b64url_encode((const uint8_t*) decoded, decoded_len,
                                 out_wire, out_cap, &encoded_len)) {
        return AXON_CSYS_CONT_BUFFER_TOO_SMALL;
    }
    if (out_len != NULL) {
        *out_len = encoded_len;
    }
    return AXON_CSYS_CONT_OK;
}

AxonCsysContinuityError axon_csys_continuity_verify(
    const uint8_t* key,
    size_t key_len,
    const char* wire,
    size_t wire_len,
    char* out_session_id,
    size_t session_id_cap,
    size_t* out_session_id_len,
    int64_t* out_expiry_ms)
{
    if ((key == NULL && key_len > 0u)
        || wire == NULL
        || (out_session_id == NULL && session_id_cap > 0u)
        || out_expiry_ms == NULL)
    {
        return AXON_CSYS_CONT_NULL_ARG;
    }

    /* Decode base64url. Cap on decoded length: a wire long enough to
     * decode > MAX_SESSION_ID + 1 + 20 + 1 + 64 = 1110 bytes is a
     * malformed token (exceeds the protocol's framing). */
    char decoded[AXON_CSYS_CONT_MAX_SESSION_ID + 1u + AXON_CSYS_CONT_I64_MAX_CHARS
                 + 1u + (AXON_CSYS_SHA256_DIGEST_SIZE * 2u) + 4u];
    size_t decoded_len = 0u;
    if (!axon_csys_b64url_decode(wire, wire_len, (uint8_t*) decoded,
                                 sizeof decoded, &decoded_len)) {
        /* Distinguish "input too long" from "malformed alphabet" by
         * peeking at the b64 length — but for our purposes both map
         * to BAD_BASE64. */
        return AXON_CSYS_CONT_BAD_BASE64;
    }

    /* Locate the two record separators. The wire format guarantees
     * exactly two; any other count is malformed. */
    size_t first = SIZE_MAX;
    size_t second = SIZE_MAX;
    for (size_t i = 0; i < decoded_len; ++i) {
        if (decoded[i] == AXON_CSYS_CONT_RECORD_SEPARATOR) {
            if (first == SIZE_MAX) {
                first = i;
            } else if (second == SIZE_MAX) {
                second = i;
            } else {
                return AXON_CSYS_CONT_BAD_FIELD_COUNT;
            }
        }
    }
    if (first == SIZE_MAX || second == SIZE_MAX) {
        return AXON_CSYS_CONT_BAD_FIELD_COUNT;
    }

    /* fields[0] = decoded[0..first]
     * fields[1] = decoded[first+1..second]
     * fields[2] = decoded[second+1..decoded_len] */
    size_t session_id_len = first;
    size_t expiry_off = first + 1u;
    size_t expiry_len = second - expiry_off;
    size_t mac_off = second + 1u;
    size_t mac_len = decoded_len - mac_off;

    if (mac_len != AXON_CSYS_SHA256_DIGEST_SIZE * 2u) {
        return AXON_CSYS_CONT_BAD_HEX;
    }

    /* Recompute MAC over (session_id || 0x1e || expiry_str). */
    uint8_t expected_mac[AXON_CSYS_SHA256_DIGEST_SIZE];
    axon_csys_hmac_sha256(key, key_len,
                          (const uint8_t*) decoded, second,
                          expected_mac);

    /* Hex-decode the actual MAC. */
    uint8_t actual_mac[AXON_CSYS_SHA256_DIGEST_SIZE];
    if (!axon_csys_hex_decode(decoded + mac_off, mac_len, actual_mac)) {
        return AXON_CSYS_CONT_BAD_HEX;
    }

    /* Constant-time compare. */
    if (axon_csys_ct_eq(expected_mac, actual_mac, AXON_CSYS_SHA256_DIGEST_SIZE) != 1) {
        return AXON_CSYS_CONT_FORGED_OR_ROTATED;
    }

    /* Parse expiry — i64 base-10. */
    int64_t expiry_ms = 0;
    if (!axon_csys_cont_parse_i64(decoded + expiry_off, expiry_len, &expiry_ms)) {
        return AXON_CSYS_CONT_BAD_EXPIRY;
    }

    /* Copy session_id out. */
    if (session_id_len > session_id_cap) {
        return AXON_CSYS_CONT_BUFFER_TOO_SMALL;
    }
    if (session_id_len > 0u) {
        memcpy(out_session_id, decoded, session_id_len);
    }
    if (out_session_id_len != NULL) {
        *out_session_id_len = session_id_len;
    }
    *out_expiry_ms = expiry_ms;
    return AXON_CSYS_CONT_OK;
}
