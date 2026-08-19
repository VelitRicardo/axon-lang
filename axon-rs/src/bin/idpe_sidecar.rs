//! v2.54.0 — the IDP-E image-decode sidecar.
//!
//! **This binary is the crash domain.** Image decode — PNG / JPEG / BMP
//! / TIFF — is the CVE-prone step, fed untrusted bytes by definition: in-process,
//! one malformed JPEG is the whole runtime. So it runs HERE, in a separate
//! process the runtime spawns, never linked into the main binary (the `image`
//! crate is behind the `idpe-sidecar` feature; the runtime never pulls it).
//!
//! **Protocol.** Read raw image bytes from stdin; on success write a **PGM (P5)**
//! grayscale raster — denoised by the deterministic front-end (Perona-Malik) — to
//! stdout and exit `0`. On any refusal write a typed reason to stderr and exit
//! non-zero. The runtime's extraction engine reads the PGM and hands it to the
//! recognizer kernel (v2.54.0): hostile bytes never cross back, only numbers do.
//!
//! **Bounded before decode.** The input byte cap and the decoder's own
//! dimension limits are set BEFORE a pixel is produced, so a decompression-bomb
//! image is refused, not expanded. The process is meant to be run under an OS
//! sandbox (seccomp / job object / least-priv user) by the enterprise host
//! (v2.54.0); this binary adds the algorithmic bounds.

use std::io::{Read, Write};

use axon::idpe::RasterTile;
use axon::idpe_frontend::{perona_malik, PeronaMalik};

/// Max input bytes — a hard cap before any decode (a 500 MiB "image" is refused).
const MAX_INPUT_BYTES: usize = 64 * 1024 * 1024;
/// Max decoded dimension per side — bounds the pixel product before full decode.
const MAX_SIDE: u32 = 20_000;
/// Max megapixels of the decoded raster (matches the kernel's `ExtractionBounds`).
const MAX_MEGAPIXELS: u32 = 256;

fn fail(reason: &str) -> ! {
    let _ = writeln!(std::io::stderr(), "idpe-sidecar refused: {reason}");
    std::process::exit(2);
}

fn main() {
    // ── Read stdin, byte-capped ─────────────────────────────────────────────
    let mut buf = Vec::new();
    let mut handle = std::io::stdin().lock().take((MAX_INPUT_BYTES as u64) + 1);
    if handle.read_to_end(&mut buf).is_err() {
        fail("could not read stdin");
    }
    if buf.len() > MAX_INPUT_BYTES {
        fail(&format!("input exceeds {MAX_INPUT_BYTES} bytes — refused before decode"));
    }

    // ── Decode to grayscale ─────────────────────────────────────────────────
    // A netpbm raster is already safe — parse it natively. Everything else is a
    // hostile format decoded by `image` under strict limits.
    let tile = if buf.starts_with(b"P5") || buf.starts_with(b"P4") {
        match RasterTile::from_netpbm(&buf, &axon::extraction::ExtractionBounds::default()) {
            Ok(t) => t,
            Err(e) => fail(&format!("netpbm decode failed: {e}")),
        }
    } else {
        decode_hostile(&buf)
    };

    // ── Deterministic front-end: Perona-Malik denoise ───────────────────────
    let cleaned = perona_malik(&tile, &PeronaMalik::default());

    // ── Emit PGM (P5) ───────────────────────────────────────────────────────
    let mut out = std::io::stdout().lock();
    let header = format!("P5\n{} {}\n255\n", cleaned.width, cleaned.height);
    if out.write_all(header.as_bytes()).and_then(|_| out.write_all(&cleaned.gray)).is_err() {
        fail("could not write stdout");
    }
    let _ = out.flush();
}

/// Decode a hostile image format (PNG/JPEG/BMP/TIFF) under strict limits, in this
/// isolated process. Dimensions are bounded BEFORE the full pixel buffer is
/// produced.
fn decode_hostile(bytes: &[u8]) -> RasterTile {
    use image::ImageReader;

    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);

    let cursor = std::io::Cursor::new(bytes);
    let mut reader = match ImageReader::new(cursor).with_guessed_format() {
        Ok(r) => r,
        Err(e) => fail(&format!("format detection failed: {e}")),
    };
    reader.limits(limits);
    let (w, h) = match reader.into_dimensions() {
        Ok(d) => d,
        Err(e) => fail(&format!("dimension read failed: {e}")),
    };
    let mp = (w as u64 * h as u64) / 1_000_000;
    if mp as u32 > MAX_MEGAPIXELS {
        fail(&format!("{mp} megapixels exceeds the {MAX_MEGAPIXELS} cap — refused before full decode"));
    }

    // Re-read for the actual decode (dimensions already vetted).
    let mut limits2 = image::Limits::default();
    limits2.max_image_width = Some(MAX_SIDE);
    limits2.max_image_height = Some(MAX_SIDE);
    let cursor = std::io::Cursor::new(bytes);
    let mut reader = match ImageReader::new(cursor).with_guessed_format() {
        Ok(r) => r,
        Err(e) => fail(&format!("format detection failed: {e}")),
    };
    reader.limits(limits2);
    let img = match reader.decode() {
        Ok(i) => i,
        Err(e) => fail(&format!("decode failed: {e}")),
    };
    let gray = img.to_luma8();
    RasterTile {
        width: gray.width() as usize,
        height: gray.height() as usize,
        gray: gray.into_raw(),
    }
}
