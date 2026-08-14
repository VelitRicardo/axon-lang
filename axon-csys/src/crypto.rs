//! §Fase 25.h — Crypto kernels (Rust shim).
//!
//! Safe Rust wrappers around the C23 SHA-256 / HMAC-SHA256 /
//! base64url / hex / continuity-wire primitives in
//! `c-src/crypto/`. Pillar split:
//!
//!   - C side: hash transforms, HMAC composition, branch-free
//!     constant-time compare, pure-arithmetic codec for hex +
//!     base64url. NIST FIPS 180-4 + FIPS 198-1 algorithmically
//!     compliant. No allocation in any entry point.
//!   - Rust side: safe API + error typing. The continuity-wire
//!     wrapper exposes a `ContinuityWire::sign` / `verify` pair
//!     that takes / returns an `i64` for the expiry — leaving
//!     time arithmetic (chrono / now() comparison) to the
//!     consumer (axon-rs::pem::continuity_token).
//!
//! # Why "FIPS-friendly" (not "FIPS-validated")
//!
//! The pure-C SHA-256 / HMAC-SHA256 implementations match the
//! algorithm specs exactly (NIST CAVS reference vectors form the
//! load-bearing drift gate) but the C code is not formally
//! certified by a NIST CAVS lab. Adopters who need formal
//! validation can opt in to a future `fips` cargo feature that
//! swaps the implementation to call into BoringSSL or OpenSSL-FIPS;
//! the wire-format + hash output remain byte-identical. This module
//! is the auditable, dep-free default.

#[cfg(feature = "native")]
use std::os::raw::c_char;

// ──────────────────────────────────────────────────────────────────────
// Raw FFI declarations — must mirror sha256.h / hmac.h / util.h /
// continuity.h byte-for-byte.
// ──────────────────────────────────────────────────────────────────────

pub const SHA256_DIGEST_SIZE: usize = 32;
pub const SHA256_BLOCK_SIZE: usize = 64;

#[repr(C)]
#[cfg(feature = "native")]
struct AxonCsysSha256Ctx {
    h: [u32; 8],
    total_bits: u64,
    buf: [u8; 64],
    buf_len: u8,
}

#[repr(C)]
#[cfg(feature = "native")]
struct AxonCsysHmacSha256Ctx {
    inner_ctx: AxonCsysSha256Ctx,
    opad: [u8; 64],
}

#[cfg(feature = "native")]
extern "C" {
    fn axon_csys_sha256_init(ctx: *mut AxonCsysSha256Ctx);
    fn axon_csys_sha256_update(ctx: *mut AxonCsysSha256Ctx, data: *const u8, len: usize);
    fn axon_csys_sha256_final(ctx: *mut AxonCsysSha256Ctx, out: *mut u8);
    fn axon_csys_sha256(data: *const u8, len: usize, out: *mut u8);

    fn axon_csys_hmac_sha256_init(ctx: *mut AxonCsysHmacSha256Ctx, key: *const u8, key_len: usize);
    fn axon_csys_hmac_sha256_update(ctx: *mut AxonCsysHmacSha256Ctx, data: *const u8, len: usize);
    fn axon_csys_hmac_sha256_final(ctx: *mut AxonCsysHmacSha256Ctx, out: *mut u8);
    fn axon_csys_hmac_sha256(
        key: *const u8,
        key_len: usize,
        data: *const u8,
        data_len: usize,
        out: *mut u8,
    );

    fn axon_csys_ct_eq(a: *const u8, b: *const u8, len: usize) -> i32;

    fn axon_csys_hex_encode(data: *const u8, len: usize, out: *mut c_char);
    fn axon_csys_hex_decode(hex: *const c_char, hex_len: usize, out: *mut u8) -> bool;

    fn axon_csys_b64url_encoded_len(byte_count: usize) -> usize;
    fn axon_csys_b64url_encode(
        data: *const u8,
        len: usize,
        out: *mut c_char,
        out_cap: usize,
        out_len: *mut usize,
    ) -> bool;
    fn axon_csys_b64url_decoded_len(char_count: usize) -> usize;
    fn axon_csys_b64url_decode(
        input: *const c_char,
        len: usize,
        out: *mut u8,
        out_cap: usize,
        out_len: *mut usize,
    ) -> bool;

    fn axon_csys_continuity_sign(
        key: *const u8,
        key_len: usize,
        session_id: *const c_char,
        session_id_len: usize,
        expiry_ms: i64,
        out_wire: *mut c_char,
        out_cap: usize,
        out_len: *mut usize,
    ) -> i32;
    fn axon_csys_continuity_verify(
        key: *const u8,
        key_len: usize,
        wire: *const c_char,
        wire_len: usize,
        out_session_id: *mut c_char,
        session_id_cap: usize,
        out_session_id_len: *mut usize,
        out_expiry_ms: *mut i64,
    ) -> i32;
    fn axon_csys_continuity_max_wire_len(session_id_len: usize) -> usize;
}

#[cfg(feature = "native")]
const CONT_OK: i32 = 0;
#[cfg(feature = "native")]
const CONT_BAD_BASE64: i32 = -1;
#[cfg(feature = "native")]
const CONT_BAD_FIELD_COUNT: i32 = -2;
#[cfg(feature = "native")]
const CONT_BAD_HEX: i32 = -3;
#[cfg(feature = "native")]
const CONT_BAD_EXPIRY: i32 = -4;
#[cfg(feature = "native")]
const CONT_FORGED_OR_ROTATED: i32 = -5;
#[cfg(feature = "native")]
const CONT_BUFFER_TOO_SMALL: i32 = -6;
#[cfg(feature = "native")]
const CONT_NULL_ARG: i32 = -7;
#[cfg(feature = "native")]
const CONT_PAYLOAD_TOO_LARGE: i32 = -8;

// ──────────────────────────────────────────────────────────────────────
// SHA-256
// ──────────────────────────────────────────────────────────────────────

/// Compute the SHA-256 digest of `data` in one shot.
#[cfg(feature = "native")]
pub fn sha256(data: &[u8]) -> [u8; SHA256_DIGEST_SIZE] {
    let mut out = [0u8; SHA256_DIGEST_SIZE];
    unsafe { axon_csys_sha256(data.as_ptr(), data.len(), out.as_mut_ptr()) };
    out
}

/// Streaming SHA-256 hasher. Equivalent to `Sha256::new()` /
/// `update()` / `finalize()` from the [`sha2`] crate but backed by
/// the in-house C kernel.
#[cfg(feature = "native")]
pub struct Sha256 {
    ctx: AxonCsysSha256Ctx,
}

#[cfg(feature = "native")]
impl Sha256 {
    pub fn new() -> Self {
        let mut ctx = AxonCsysSha256Ctx {
            h: [0u32; 8],
            total_bits: 0,
            buf: [0u8; 64],
            buf_len: 0,
        };
        unsafe { axon_csys_sha256_init(&mut ctx as *mut _) };
        Self { ctx }
    }

    pub fn update(&mut self, data: &[u8]) -> &mut Self {
        unsafe { axon_csys_sha256_update(&mut self.ctx as *mut _, data.as_ptr(), data.len()) };
        self
    }

    pub fn finalize(mut self) -> [u8; SHA256_DIGEST_SIZE] {
        let mut out = [0u8; SHA256_DIGEST_SIZE];
        unsafe { axon_csys_sha256_final(&mut self.ctx as *mut _, out.as_mut_ptr()) };
        out
    }
}

#[cfg(feature = "native")]
impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

// ──────────────────────────────────────────────────────────────────────
// HMAC-SHA256
// ──────────────────────────────────────────────────────────────────────

/// Compute HMAC-SHA256 in one shot. Any key length accepted.
#[cfg(feature = "native")]
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; SHA256_DIGEST_SIZE] {
    let mut out = [0u8; SHA256_DIGEST_SIZE];
    unsafe {
        axon_csys_hmac_sha256(
            key.as_ptr(),
            key.len(),
            data.as_ptr(),
            data.len(),
            out.as_mut_ptr(),
        )
    };
    out
}

/// Streaming HMAC-SHA256 MAC builder.
#[cfg(feature = "native")]
pub struct HmacSha256 {
    ctx: AxonCsysHmacSha256Ctx,
}

#[cfg(feature = "native")]
impl HmacSha256 {
    pub fn new(key: &[u8]) -> Self {
        let mut ctx = AxonCsysHmacSha256Ctx {
            inner_ctx: AxonCsysSha256Ctx {
                h: [0u32; 8],
                total_bits: 0,
                buf: [0u8; 64],
                buf_len: 0,
            },
            opad: [0u8; 64],
        };
        unsafe { axon_csys_hmac_sha256_init(&mut ctx as *mut _, key.as_ptr(), key.len()) };
        Self { ctx }
    }

    pub fn update(&mut self, data: &[u8]) -> &mut Self {
        unsafe { axon_csys_hmac_sha256_update(&mut self.ctx as *mut _, data.as_ptr(), data.len()) };
        self
    }

    pub fn finalize(mut self) -> [u8; SHA256_DIGEST_SIZE] {
        let mut out = [0u8; SHA256_DIGEST_SIZE];
        unsafe { axon_csys_hmac_sha256_final(&mut self.ctx as *mut _, out.as_mut_ptr()) };
        out
    }
}

// ──────────────────────────────────────────────────────────────────────
// Constant-time equality
// ──────────────────────────────────────────────────────────────────────

/// Constant-time byte-slice equality. Returns `true` iff `a` and `b`
/// have the same length AND all bytes match. The implementation runs
/// the same number of memory + arithmetic operations regardless of
/// input contents — timing observation does NOT leak the position of
/// the first byte mismatch.
///
/// Length mismatch returns `false` immediately (a length difference
/// is observable from the wire format itself; constant-time compare
/// is only meaningful for fixed-size MAC outputs).
#[cfg(feature = "native")]
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let r = unsafe { axon_csys_ct_eq(a.as_ptr(), b.as_ptr(), a.len()) };
    r == 1
}

// ──────────────────────────────────────────────────────────────────────
// Hex codec
// ──────────────────────────────────────────────────────────────────────

/// Encode `data` to lowercase hex.
#[cfg(feature = "native")]
pub fn hex_encode(data: &[u8]) -> String {
    let len = data.len() * 2;
    let mut buf = vec![0u8; len];
    unsafe {
        axon_csys_hex_encode(data.as_ptr(), data.len(), buf.as_mut_ptr() as *mut c_char);
    }
    // SAFETY: hex emit only writes ASCII digits + lowercase a..f.
    unsafe { String::from_utf8_unchecked(buf) }
}

/// Decode a hex string (case-insensitive). Returns `None` if the
/// input length is odd or any character is not a hex digit.
#[cfg(feature = "native")]
pub fn hex_decode(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    let mut out = vec![0u8; hex.len() / 2];
    let ok =
        unsafe { axon_csys_hex_decode(hex.as_ptr() as *const c_char, hex.len(), out.as_mut_ptr()) };
    if ok {
        Some(out)
    } else {
        None
    }
}

// ──────────────────────────────────────────────────────────────────────
// Base64url-no-pad codec
// ──────────────────────────────────────────────────────────────────────

/// Encode `data` to base64url-no-pad (RFC 4648 §5 with padding stripped).
#[cfg(feature = "native")]
pub fn b64url_encode(data: &[u8]) -> String {
    let cap = unsafe { axon_csys_b64url_encoded_len(data.len()) };
    let mut buf = vec![0u8; cap];
    let mut out_len: usize = 0;
    let ok = unsafe {
        axon_csys_b64url_encode(
            data.as_ptr(),
            data.len(),
            buf.as_mut_ptr() as *mut c_char,
            cap,
            &mut out_len as *mut _,
        )
    };
    debug_assert!(ok && out_len == cap);
    buf.truncate(out_len);
    // SAFETY: encoder only emits ASCII alphabet characters.
    unsafe { String::from_utf8_unchecked(buf) }
}

/// Decode a base64url-no-pad string. Returns `None` if the alphabet
/// is violated or the length is `4k+1` (impossible byte count).
#[cfg(feature = "native")]
pub fn b64url_decode(input: &str) -> Option<Vec<u8>> {
    let cap = unsafe { axon_csys_b64url_decoded_len(input.len()) };
    if cap == usize::MAX {
        return None;
    }
    let mut out = vec![0u8; cap];
    let mut out_len: usize = 0;
    let ok = unsafe {
        axon_csys_b64url_decode(
            input.as_ptr() as *const c_char,
            input.len(),
            out.as_mut_ptr(),
            cap,
            &mut out_len as *mut _,
        )
    };
    if !ok {
        return None;
    }
    out.truncate(out_len);
    Some(out)
}

// ──────────────────────────────────────────────────────────────────────
// Continuity wire format
// ──────────────────────────────────────────────────────────────────────

/// Errors from the continuity-wire primitive. Mapped 1:1 from the
/// C error codes; `axon-rs::pem::continuity_token` adds the
/// `Expired` variant on top after checking the parsed `expiry_ms`
/// against `Utc::now()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContinuityWireError {
    /// base64url decode failed (alphabet violation or invalid length).
    BadBase64,
    /// Decoded text did not contain exactly two record-separator (0x1e)
    /// characters.
    BadFieldCount,
    /// The MAC field was not exactly 64 lowercase hex digits.
    BadHex,
    /// The expiry field was not parseable as a base-10 i64.
    BadExpiry,
    /// HMAC verification failed — token was forged or the signer key
    /// rotated since the token was issued.
    ForgedOrRotated,
    /// Output buffer (or session_id buffer) was too small.
    BufferTooSmall,
    /// FFI received a NULL pointer where one was required.
    NullArg,
    /// session_id exceeded the C kernel's compile-time limit
    /// (`AXON_CSYS_CONT_MAX_SESSION_ID`, currently 1024 bytes) or
    /// contained a forbidden 0x1e byte.
    PayloadTooLarge,
}

impl std::fmt::Display for ContinuityWireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadBase64 => write!(f, "continuity wire: base64url decode failed"),
            Self::BadFieldCount => {
                write!(
                    f,
                    "continuity wire: expected exactly 3 0x1e-separated fields"
                )
            }
            Self::BadHex => {
                write!(f, "continuity wire: MAC field must be 64 hex digits")
            }
            Self::BadExpiry => {
                write!(f, "continuity wire: expiry field not parseable as i64")
            }
            Self::ForgedOrRotated => {
                write!(
                    f,
                    "continuity wire: HMAC mismatch (forged or signer key rotated)"
                )
            }
            Self::BufferTooSmall => {
                write!(f, "continuity wire: output buffer too small")
            }
            Self::NullArg => write!(f, "continuity wire: NULL pointer at FFI"),
            Self::PayloadTooLarge => write!(
                f,
                "continuity wire: session_id exceeds limit or contains 0x1e"
            ),
        }
    }
}

impl std::error::Error for ContinuityWireError {}

#[cfg(feature = "native")]
fn map_cont_error(code: i32) -> ContinuityWireError {
    match code {
        CONT_BAD_BASE64 => ContinuityWireError::BadBase64,
        CONT_BAD_FIELD_COUNT => ContinuityWireError::BadFieldCount,
        CONT_BAD_HEX => ContinuityWireError::BadHex,
        CONT_BAD_EXPIRY => ContinuityWireError::BadExpiry,
        CONT_FORGED_OR_ROTATED => ContinuityWireError::ForgedOrRotated,
        CONT_BUFFER_TOO_SMALL => ContinuityWireError::BufferTooSmall,
        CONT_NULL_ARG => ContinuityWireError::NullArg,
        CONT_PAYLOAD_TOO_LARGE => ContinuityWireError::PayloadTooLarge,
        // Defensive — covers future C-side error codes that arrive
        // before the Rust shim is updated.
        _ => ContinuityWireError::BadFieldCount,
    }
}

/// Continuity wire-format primitive — sign and verify operations.
/// This is the time-agnostic surface; consumers (axon-rs's
/// [`ContinuityTokenSigner`]) wrap it with chrono-flavoured
/// expiry checking.
pub struct ContinuityWire;

#[cfg(feature = "native")]
impl ContinuityWire {
    /// Sign `(session_id, expiry_ms)` with `key`. Returns the
    /// base64url-no-pad-encoded wire string.
    pub fn sign(
        key: &[u8],
        session_id: &str,
        expiry_ms: i64,
    ) -> Result<String, ContinuityWireError> {
        let cap = unsafe { axon_csys_continuity_max_wire_len(session_id.len()) };
        let mut buf = vec![0u8; cap];
        let mut out_len: usize = 0;
        let code = unsafe {
            axon_csys_continuity_sign(
                key.as_ptr(),
                key.len(),
                session_id.as_ptr() as *const c_char,
                session_id.len(),
                expiry_ms,
                buf.as_mut_ptr() as *mut c_char,
                cap,
                &mut out_len as *mut _,
            )
        };
        if code != CONT_OK {
            return Err(map_cont_error(code));
        }
        buf.truncate(out_len);
        // SAFETY: signer only emits base64url alphabet characters.
        Ok(unsafe { String::from_utf8_unchecked(buf) })
    }

    /// Verify `wire` against `key`, returning `(session_id, expiry_ms)`
    /// on success. Does NOT check whether the token has expired —
    /// the caller compares `expiry_ms` against the current time
    /// (typically via chrono).
    pub fn verify(key: &[u8], wire: &str) -> Result<(String, i64), ContinuityWireError> {
        // Session-id buffer sized to the C kernel's compile-time max.
        let mut sid_buf = vec![0u8; 1024];
        let mut sid_len: usize = 0;
        let mut expiry_ms: i64 = 0;
        let code = unsafe {
            axon_csys_continuity_verify(
                key.as_ptr(),
                key.len(),
                wire.as_ptr() as *const c_char,
                wire.len(),
                sid_buf.as_mut_ptr() as *mut c_char,
                sid_buf.len(),
                &mut sid_len as *mut _,
                &mut expiry_ms as *mut _,
            )
        };
        if code != CONT_OK {
            return Err(map_cont_error(code));
        }
        sid_buf.truncate(sid_len);
        // The session_id was a UTF-8 string when issued; verify bytes
        // are still valid UTF-8 before exposing as &str. The verify
        // path could hit corrupt bytes if a non-axon signer produced
        // the wire — defensive.
        let session_id =
            String::from_utf8(sid_buf).map_err(|_| ContinuityWireError::BadFieldCount)?;
        Ok((session_id, expiry_ms))
    }
}

// ══════════════════════════════════════════════════════════════════════
// §Fase 119.h — the toolchain-free twin
// ══════════════════════════════════════════════════════════════════════
//
// Everything below is the SAME surface with no C behind it, for builds
// without the `native` feature. Three properties make this safe to ship
// rather than merely convenient:
//
// 1. **It mirrors the C, it does not reinterpret it.** Each function
//    below was written against its counterpart in `c-src/crypto/`
//    (`sha256.c`, `hmac.c`, `util.c`, `continuity.c`) statement by
//    statement, including the defensive checks the module doc says the
//    C kernel adds over the reference impl — the 0x1e rejection in
//    `sign`, the strict whole-slice i64 parse, the 1024-byte session-id
//    cap. A twin that were merely "correct SHA-256" would still break
//    the wire format; these are byte-compatible in both directions.
//
// 2. **The drift gates judge it too.** The gates in `tests/` call the
//    public API — `crypto::sha256`, `ContinuityWire::sign` — so
//    `cargo test --no-default-features` runs the NIST CAVS vectors and
//    the `sha2`/`hmac`/`base64`/`subtle` cross-checks against these
//    functions instead of against the C. Fidelity is measured, not
//    asserted. (§Fase 119.h added the no-default-features lane so this
//    is not a claim about a suite nobody runs.)
//
// 3. **It costs no dependency.** This module's own doc calls the crate
//    "the auditable, dep-free default"; paying for a toolchain-free
//    build with four third-party crates would trade one broken promise
//    for another.
//
// The single honest asymmetry is `ct_eq`: the C is branch-free by
// construction and compiled as such. Rust has no portable way to forbid
// the optimiser from short-circuiting, so the twin accumulates the
// difference and launders it through `black_box`. That is the standard
// technique and it is what `subtle` does, but it is a hint to the
// compiler rather than a guarantee from it — which is why the native
// build remains the default everywhere the timing matters.

#[cfg(not(feature = "native"))]
mod portable {
    use super::{ContinuityWireError, SHA256_BLOCK_SIZE, SHA256_DIGEST_SIZE};

    // ── SHA-256 — FIPS 180-4, mirroring c-src/crypto/sha256.c ────────

    /// §4.2.2 — the first 32 bits of the fractional parts of the cube
    /// roots of the first 64 primes.
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    /// §5.3.3 — the first 32 bits of the fractional parts of the square
    /// roots of the first 8 primes.
    const INITIAL_H: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    #[derive(Clone)]
    pub struct Sha256 {
        h: [u32; 8],
        total_bits: u64,
        buf: [u8; SHA256_BLOCK_SIZE],
        buf_len: usize,
    }

    impl Sha256 {
        pub fn new() -> Self {
            Self {
                h: INITIAL_H,
                total_bits: 0,
                buf: [0u8; SHA256_BLOCK_SIZE],
                buf_len: 0,
            }
        }

        /// §6.2.2 — compress one 512-bit block into the state.
        fn compress(&mut self, block: &[u8; SHA256_BLOCK_SIZE]) {
            let mut w = [0u32; 64];
            for (i, word) in w.iter_mut().take(16).enumerate() {
                *word = u32::from_be_bytes([
                    block[i * 4],
                    block[i * 4 + 1],
                    block[i * 4 + 2],
                    block[i * 4 + 3],
                ]);
            }
            for i in 16..64 {
                let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
                let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
                w[i] = w[i - 16]
                    .wrapping_add(s0)
                    .wrapping_add(w[i - 7])
                    .wrapping_add(s1);
            }

            let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = self.h;
            for i in 0..64 {
                let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
                let ch = (e & f) ^ (!e & g);
                let t1 = hh
                    .wrapping_add(s1)
                    .wrapping_add(ch)
                    .wrapping_add(K[i])
                    .wrapping_add(w[i]);
                let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
                let maj = (a & b) ^ (a & c) ^ (b & c);
                let t2 = s0.wrapping_add(maj);

                hh = g;
                g = f;
                f = e;
                e = d.wrapping_add(t1);
                d = c;
                c = b;
                b = a;
                a = t1.wrapping_add(t2);
            }
            for (slot, v) in self.h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
                *slot = slot.wrapping_add(v);
            }
        }

        pub fn update(&mut self, mut data: &[u8]) {
            self.total_bits = self.total_bits.wrapping_add((data.len() as u64) * 8);

            if self.buf_len > 0 {
                let take = (SHA256_BLOCK_SIZE - self.buf_len).min(data.len());
                self.buf[self.buf_len..self.buf_len + take].copy_from_slice(&data[..take]);
                self.buf_len += take;
                data = &data[take..];
                if self.buf_len == SHA256_BLOCK_SIZE {
                    let block = self.buf;
                    self.compress(&block);
                    self.buf_len = 0;
                }
            }

            while data.len() >= SHA256_BLOCK_SIZE {
                let mut block = [0u8; SHA256_BLOCK_SIZE];
                block.copy_from_slice(&data[..SHA256_BLOCK_SIZE]);
                self.compress(&block);
                data = &data[SHA256_BLOCK_SIZE..];
            }

            if !data.is_empty() {
                self.buf[..data.len()].copy_from_slice(data);
                self.buf_len = data.len();
            }
        }

        /// §5.1.1 — append 0x80, pad with zeros to 56 mod 64, then the
        /// 64-bit big-endian message length in bits.
        pub fn finalize(mut self) -> [u8; SHA256_DIGEST_SIZE] {
            let bits = self.total_bits;
            let len = self.buf_len;
            self.buf[len] = 0x80;
            if len + 1 > SHA256_BLOCK_SIZE - 8 {
                for byte in self.buf[len + 1..].iter_mut() {
                    *byte = 0;
                }
                let block = self.buf;
                self.compress(&block);
                self.buf = [0u8; SHA256_BLOCK_SIZE];
            } else {
                for byte in self.buf[len + 1..].iter_mut() {
                    *byte = 0;
                }
            }
            self.buf[SHA256_BLOCK_SIZE - 8..].copy_from_slice(&bits.to_be_bytes());
            let block = self.buf;
            self.compress(&block);

            let mut out = [0u8; SHA256_DIGEST_SIZE];
            for (i, word) in self.h.iter().enumerate() {
                out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
            }
            out
        }
    }

    pub fn sha256(data: &[u8]) -> [u8; SHA256_DIGEST_SIZE] {
        let mut h = Sha256::new();
        h.update(data);
        h.finalize()
    }

    // ── HMAC-SHA256 — FIPS 198-1, mirroring c-src/crypto/hmac.c ──────

    #[derive(Clone)]
    pub struct HmacSha256 {
        inner: Sha256,
        opad: [u8; SHA256_BLOCK_SIZE],
    }

    impl HmacSha256 {
        pub fn new(key: &[u8]) -> Self {
            // A key longer than the block is replaced by its digest;
            // a shorter one is zero-padded to the block.
            let mut k0 = [0u8; SHA256_BLOCK_SIZE];
            if key.len() > SHA256_BLOCK_SIZE {
                k0[..SHA256_DIGEST_SIZE].copy_from_slice(&sha256(key));
            } else {
                k0[..key.len()].copy_from_slice(key);
            }

            let mut ipad = [0u8; SHA256_BLOCK_SIZE];
            let mut opad = [0u8; SHA256_BLOCK_SIZE];
            for i in 0..SHA256_BLOCK_SIZE {
                ipad[i] = k0[i] ^ 0x36;
                opad[i] = k0[i] ^ 0x5c;
            }

            let mut inner = Sha256::new();
            inner.update(&ipad);
            Self { inner, opad }
        }

        pub fn update(&mut self, data: &[u8]) {
            self.inner.update(data);
        }

        pub fn finalize(self) -> [u8; SHA256_DIGEST_SIZE] {
            let inner_digest = self.inner.finalize();
            let mut outer = Sha256::new();
            outer.update(&self.opad);
            outer.update(&inner_digest);
            outer.finalize()
        }
    }

    pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; SHA256_DIGEST_SIZE] {
        let mut m = HmacSha256::new(key);
        m.update(data);
        m.finalize()
    }

    // ── constant-time compare — mirroring c-src/crypto/util.c ────────

    pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
        if a.len() != b.len() {
            return false;
        }
        let mut diff: u8 = 0;
        for (x, y) in a.iter().zip(b.iter()) {
            diff |= x ^ y;
        }
        // `black_box` denies the optimiser the knowledge that `diff`'s
        // value determines the branch, which is what stops it emitting
        // an early exit. See the asymmetry note above the module.
        std::hint::black_box(diff) == 0
    }

    // ── hex + base64url codecs — mirroring c-src/crypto/util.c ───────

    const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

    pub fn hex_encode(data: &[u8]) -> String {
        let mut out = Vec::with_capacity(data.len() * 2);
        for byte in data {
            out.push(HEX_DIGITS[(byte >> 4) as usize]);
            out.push(HEX_DIGITS[(byte & 0x0f) as usize]);
        }
        // SAFETY: only ASCII digits and lowercase a..f were pushed.
        unsafe { String::from_utf8_unchecked(out) }
    }

    fn hex_val(c: u8) -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    }

    pub fn hex_decode(hex: &[u8]) -> Option<Vec<u8>> {
        if !hex.len().is_multiple_of(2) {
            return None;
        }
        let mut out = Vec::with_capacity(hex.len() / 2);
        for pair in hex.chunks_exact(2) {
            out.push((hex_val(pair[0])? << 4) | hex_val(pair[1])?);
        }
        Some(out)
    }

    /// RFC 4648 §5 with padding stripped.
    const B64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

    pub fn b64url_encoded_len(byte_count: usize) -> usize {
        byte_count * 4 / 3 + usize::from(!(byte_count % 3 == 0)) * (4 - (byte_count % 3) - 1)
    }

    pub fn b64url_encode(data: &[u8]) -> String {
        let mut out = Vec::with_capacity(b64url_encoded_len(data.len()));
        for chunk in data.chunks(3) {
            let b0 = chunk[0] as u32;
            let b1 = *chunk.get(1).unwrap_or(&0) as u32;
            let b2 = *chunk.get(2).unwrap_or(&0) as u32;
            let n = (b0 << 16) | (b1 << 8) | b2;
            out.push(B64URL[(n >> 18) as usize & 0x3f]);
            out.push(B64URL[(n >> 12) as usize & 0x3f]);
            // No padding: a 1-byte tail emits 2 chars, a 2-byte tail 3.
            if chunk.len() > 1 {
                out.push(B64URL[(n >> 6) as usize & 0x3f]);
            }
            if chunk.len() > 2 {
                out.push(B64URL[n as usize & 0x3f]);
            }
        }
        // SAFETY: only the base64url alphabet was pushed.
        unsafe { String::from_utf8_unchecked(out) }
    }

    fn b64url_val(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a' + 26) as u32),
            b'0'..=b'9' => Some((c - b'0' + 52) as u32),
            b'-' => Some(62),
            b'_' => Some(63),
            _ => None,
        }
    }

    pub fn b64url_decode(input: &[u8]) -> Option<Vec<u8>> {
        // A 4k+1 tail cannot encode any whole number of bytes. The C
        // signals this by returning SIZE_MAX from `decoded_len`.
        if input.len() % 4 == 1 {
            return None;
        }
        let mut out = Vec::with_capacity(input.len() / 4 * 3);
        for chunk in input.chunks(4) {
            let mut n: u32 = 0;
            for (i, c) in chunk.iter().enumerate() {
                n |= b64url_val(*c)? << (18 - 6 * i);
            }
            out.push((n >> 16) as u8);
            if chunk.len() > 2 {
                out.push((n >> 8) as u8);
            }
            if chunk.len() > 3 {
                out.push(n as u8);
            }
        }
        Some(out)
    }

    // ── continuity wire — mirroring c-src/crypto/continuity.c ────────

    /// `AXON_CSYS_CONT_MAX_SESSION_ID`.
    const MAX_SESSION_ID: usize = 1024;
    /// `AXON_CSYS_CONT_I64_MAX_CHARS` — sign plus nineteen digits.
    const I64_MAX_CHARS: usize = 20;
    /// `AXON_CSYS_CONT_RECORD_SEPARATOR`.
    const RS: u8 = 0x1e;

    /// The C reads the whole slice or fails: no `strtoll`, so no
    /// trailing whitespace and no `+` sign. Rust's `i64::from_str`
    /// accepts a leading `+`, which would let two distinct wires carry
    /// the same expiry — so the check is written out rather than
    /// delegated.
    fn parse_i64_strict(s: &[u8]) -> Option<i64> {
        if s.is_empty() {
            return None;
        }
        let negative = s[0] == b'-';
        let digits = if negative { &s[1..] } else { s };
        if digits.is_empty() {
            return None;
        }
        let bound: u64 = if negative {
            i64::MAX as u64 + 1
        } else {
            i64::MAX as u64
        };
        let mut acc: u64 = 0;
        for c in digits {
            if !c.is_ascii_digit() {
                return None;
            }
            let digit = u64::from(c - b'0');
            if acc > (bound - digit) / 10 {
                return None;
            }
            acc = acc * 10 + digit;
        }
        Some(if negative {
            if acc == i64::MAX as u64 + 1 {
                i64::MIN
            } else {
                -(acc as i64)
            }
        } else {
            acc as i64
        })
    }

    pub fn continuity_sign(
        key: &[u8],
        session_id: &str,
        expiry_ms: i64,
    ) -> Result<String, ContinuityWireError> {
        if session_id.len() > MAX_SESSION_ID {
            return Err(ContinuityWireError::PayloadTooLarge);
        }
        // Defence in depth the C kernel adds over the reference impl: a
        // separator inside the payload would confuse the verify-side
        // splitter into reading a different session_id than was signed.
        if session_id.as_bytes().contains(&RS) {
            return Err(ContinuityWireError::PayloadTooLarge);
        }

        let expiry_str = expiry_ms.to_string();
        if expiry_str.is_empty() || expiry_str.len() > I64_MAX_CHARS {
            return Err(ContinuityWireError::BadExpiry);
        }

        let mut body = Vec::with_capacity(session_id.len() + 1 + expiry_str.len());
        body.extend_from_slice(session_id.as_bytes());
        body.push(RS);
        body.extend_from_slice(expiry_str.as_bytes());

        let mac = hmac_sha256(key, &body);

        let mut decoded = body;
        decoded.push(RS);
        decoded.extend_from_slice(hex_encode(&mac).as_bytes());

        Ok(b64url_encode(&decoded))
    }

    pub fn continuity_verify(key: &[u8], wire: &str) -> Result<(String, i64), ContinuityWireError> {
        // The C decodes into a fixed buffer sized to the protocol's
        // framing; anything longer is malformed rather than merely
        // large, and both map to BAD_BASE64 there.
        const DECODED_CAP: usize =
            MAX_SESSION_ID + 1 + I64_MAX_CHARS + 1 + SHA256_DIGEST_SIZE * 2 + 4;
        let decoded = b64url_decode(wire.as_bytes()).ok_or(ContinuityWireError::BadBase64)?;
        if decoded.len() > DECODED_CAP {
            return Err(ContinuityWireError::BadBase64);
        }

        let mut first: Option<usize> = None;
        let mut second: Option<usize> = None;
        for (i, byte) in decoded.iter().enumerate() {
            if *byte == RS {
                match (first, second) {
                    (None, _) => first = Some(i),
                    (Some(_), None) => second = Some(i),
                    _ => return Err(ContinuityWireError::BadFieldCount),
                }
            }
        }
        let (first, second) = match (first, second) {
            (Some(a), Some(b)) => (a, b),
            _ => return Err(ContinuityWireError::BadFieldCount),
        };

        let mac_off = second + 1;
        if decoded.len() - mac_off != SHA256_DIGEST_SIZE * 2 {
            return Err(ContinuityWireError::BadHex);
        }

        // The MAC covers everything before the second separator, which
        // is exactly the body that `sign` hashed.
        let expected = hmac_sha256(key, &decoded[..second]);
        let actual = hex_decode(&decoded[mac_off..]).ok_or(ContinuityWireError::BadHex)?;
        if !ct_eq(&expected, &actual) {
            return Err(ContinuityWireError::ForgedOrRotated);
        }

        let expiry_ms =
            parse_i64_strict(&decoded[first + 1..second]).ok_or(ContinuityWireError::BadExpiry)?;

        // Only reached after the MAC verified, so these bytes were UTF-8
        // when signed. Still checked: a non-axon signer holding the key
        // could have produced anything.
        let session_id = String::from_utf8(decoded[..first].to_vec())
            .map_err(|_| ContinuityWireError::BadFieldCount)?;
        Ok((session_id, expiry_ms))
    }
}

// ── the toolchain-free surface, identical in shape to the native one ──

/// Compute the SHA-256 digest of `data` in one shot.
#[cfg(not(feature = "native"))]
pub fn sha256(data: &[u8]) -> [u8; SHA256_DIGEST_SIZE] {
    portable::sha256(data)
}

/// Streaming SHA-256 hasher.
#[cfg(not(feature = "native"))]
pub struct Sha256 {
    inner: portable::Sha256,
}

#[cfg(not(feature = "native"))]
impl Sha256 {
    pub fn new() -> Self {
        Self {
            inner: portable::Sha256::new(),
        }
    }
    pub fn update(&mut self, data: &[u8]) -> &mut Self {
        self.inner.update(data);
        self
    }
    pub fn finalize(self) -> [u8; SHA256_DIGEST_SIZE] {
        self.inner.finalize()
    }
}

#[cfg(not(feature = "native"))]
impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute HMAC-SHA256 in one shot. Any key length accepted.
#[cfg(not(feature = "native"))]
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; SHA256_DIGEST_SIZE] {
    portable::hmac_sha256(key, data)
}

/// Streaming HMAC-SHA256 MAC builder.
#[cfg(not(feature = "native"))]
pub struct HmacSha256 {
    inner: portable::HmacSha256,
}

#[cfg(not(feature = "native"))]
impl HmacSha256 {
    pub fn new(key: &[u8]) -> Self {
        Self {
            inner: portable::HmacSha256::new(key),
        }
    }
    pub fn update(&mut self, data: &[u8]) -> &mut Self {
        self.inner.update(data);
        self
    }
    pub fn finalize(self) -> [u8; SHA256_DIGEST_SIZE] {
        self.inner.finalize()
    }
}

/// Constant-time byte-slice equality — see the asymmetry note above.
#[cfg(not(feature = "native"))]
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    portable::ct_eq(a, b)
}

/// Encode `data` to lowercase hex.
#[cfg(not(feature = "native"))]
pub fn hex_encode(data: &[u8]) -> String {
    portable::hex_encode(data)
}

/// Decode a hex string (case-insensitive). Returns `None` if the input
/// length is odd or any character is not a hex digit.
#[cfg(not(feature = "native"))]
pub fn hex_decode(hex: &str) -> Option<Vec<u8>> {
    portable::hex_decode(hex.as_bytes())
}

/// Encode `data` to base64url-no-pad (RFC 4648 §5 with padding stripped).
#[cfg(not(feature = "native"))]
pub fn b64url_encode(data: &[u8]) -> String {
    portable::b64url_encode(data)
}

/// Decode a base64url-no-pad string. Returns `None` if the alphabet is
/// violated or the length is `4k+1` (impossible byte count).
#[cfg(not(feature = "native"))]
pub fn b64url_decode(input: &str) -> Option<Vec<u8>> {
    portable::b64url_decode(input.as_bytes())
}

#[cfg(not(feature = "native"))]
impl ContinuityWire {
    /// Sign `(session_id, expiry_ms)` with `key`. Returns the
    /// base64url-no-pad-encoded wire string.
    pub fn sign(
        key: &[u8],
        session_id: &str,
        expiry_ms: i64,
    ) -> Result<String, ContinuityWireError> {
        portable::continuity_sign(key, session_id, expiry_ms)
    }

    /// Verify `wire` against `key`, returning `(session_id, expiry_ms)`
    /// on success. Does NOT check whether the token has expired.
    pub fn verify(key: &[u8], wire: &str) -> Result<(String, i64), ContinuityWireError> {
        portable::continuity_verify(key, wire)
    }
}
