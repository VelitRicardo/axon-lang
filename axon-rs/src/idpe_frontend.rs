//! v2.54.0 — the IDP-E image front-end: the deterministic signal-processing
//! that cleans and analyses a raster BEFORE the recognizer kernel (v2.54.0) reads
//! it.
//!
//! **These are the mathematically substantive, deterministic transforms of the
//! founder's strategy** — anisotropic diffusion + a Gabor phase tensor — and they
//! live in the open core because they are exactly axon's four-pillars identity:
//! pure math, no learning, reproducible bit-for-bit.
//!
//! - **Perona-Malik anisotropic diffusion (Catté-regularised)** removes scan
//!   noise / JPEG artefacts WITHOUT blurring character edges: the conductance is
//!   computed from a Gaussian-smoothed gradient (the Catté regularisation that
//!   makes the scheme well-posed and stable on thin type), so diffusion flows
//!   *along* edges, never across them.
//! - **Gabor orientation energy** isolates the dominant stroke/line orientation —
//!   the signal a layout stage uses to find text lines (horizontal energy) and to
//!   estimate skew, from the sinusoidal phase rather than a heuristic.
//!
//! **Bounded.** Iterations and kernel radius are capped; the transforms
//! allocate O(pixels) and never more. The CVE-prone step is not here — it is the
//! image *decode* (PNG/JPEG/PDF), which is why that lives in the isolated sidecar
//! binary (`src/bin/idpe_sidecar.rs`), feeding this front-end already-decoded
//! grayscale. Hostile bytes never reach the runtime; only numbers do.

use crate::idpe::RasterTile;

/// Max Perona-Malik iterations — a bound so a caller cannot spin the diffusion
/// forever.
pub const MAX_PM_ITERATIONS: u32 = 64;

/// Configuration for Perona-Malik anisotropic diffusion.
#[derive(Debug, Clone, Copy)]
pub struct PeronaMalik {
    /// Number of diffusion steps (clamped to [`MAX_PM_ITERATIONS`]).
    pub iterations: u32,
    /// The conductance edge-stopping parameter `K` — larger keeps more diffusion
    /// across weak gradients. In `[0,255]` gradient units.
    pub kappa: f64,
    /// The time step `λ` — must be ≤ 0.25 for a stable 4-neighbour scheme.
    pub lambda: f64,
    /// The Catté Gaussian pre-smoothing passes applied before measuring the
    /// gradient (0 = raw Perona-Malik; ≥1 = the regularised, well-posed scheme).
    pub catte_passes: u32,
}

impl Default for PeronaMalik {
    fn default() -> Self {
        PeronaMalik { iterations: 8, kappa: 20.0, lambda: 0.2, catte_passes: 1 }
    }
}

/// A 3×3 binomial (Gaussian-approximating) blur — separable, deterministic. The
/// Catté regulariser and a general-purpose denoise both use it.
pub fn gaussian_blur(tile: &RasterTile, passes: u32) -> RasterTile {
    let (w, h) = (tile.width, tile.height);
    let mut cur: Vec<f64> = tile.gray.iter().map(|&g| g as f64).collect();
    for _ in 0..passes {
        // Horizontal [1 2 1]/4.
        let mut tmp = cur.clone();
        for y in 0..h {
            for x in 0..w {
                let l = cur[y * w + x.saturating_sub(1)];
                let c = cur[y * w + x];
                let r = cur[y * w + (x + 1).min(w - 1)];
                tmp[y * w + x] = (l + 2.0 * c + r) / 4.0;
            }
        }
        // Vertical [1 2 1]/4.
        for y in 0..h {
            for x in 0..w {
                let u = tmp[y.saturating_sub(1) * w + x];
                let c = tmp[y * w + x];
                let d = tmp[(y + 1).min(h - 1) * w + x];
                cur[y * w + x] = (u + 2.0 * c + d) / 4.0;
            }
        }
    }
    RasterTile { width: w, height: h, gray: cur.iter().map(|&v| v.round().clamp(0.0, 255.0) as u8).collect() }
}

/// The Perona-Malik conductance `g(∇) = exp(-(∇/K)²)` (Perona-Malik #1) — favours
/// wide, flat regions (diffuse) and stops at strong edges (preserve).
#[inline]
fn conductance(grad: f64, kappa: f64) -> f64 {
    let r = grad / kappa.max(1e-6);
    (-(r * r)).exp()
}

/// Apply Catté-regularised Perona-Malik anisotropic diffusion. Denoises while
/// preserving character edges — the read is deterministic (pure float math).
pub fn perona_malik(tile: &RasterTile, cfg: &PeronaMalik) -> RasterTile {
    let (w, h) = (tile.width, tile.height);
    if w == 0 || h == 0 {
        return tile.clone();
    }
    let iters = cfg.iterations.min(MAX_PM_ITERATIONS);
    let lambda = cfg.lambda.clamp(0.0, 0.25);
    let mut img: Vec<f64> = tile.gray.iter().map(|&g| g as f64).collect();

    let at = |v: &[f64], x: usize, y: usize| v[y * w + x];
    for _ in 0..iters {
        // Catté: measure the conductance from a smoothed copy, but diffuse the
        // real image — this is what makes the scheme well-posed on thin strokes.
        let smoothed = if cfg.catte_passes > 0 {
            let t = RasterTile { width: w, height: h, gray: img.iter().map(|&v| v.round().clamp(0.0, 255.0) as u8).collect() };
            gaussian_blur(&t, cfg.catte_passes).gray.iter().map(|&g| g as f64).collect::<Vec<f64>>()
        } else {
            img.clone()
        };
        let mut next = img.clone();
        for y in 0..h {
            for x in 0..w {
                let c = at(&img, x, y);
                let cs = at(&smoothed, x, y);
                // 4-neighbour differences: real image for the flow, smoothed for
                // the conductance.
                let mut acc = 0.0;
                let mut add = |nx: usize, ny: usize| {
                    let g = conductance(at(&smoothed, nx, ny) - cs, cfg.kappa);
                    acc += g * (at(&img, nx, ny) - c);
                };
                if y > 0 {
                    add(x, y - 1);
                }
                if y + 1 < h {
                    add(x, y + 1);
                }
                if x > 0 {
                    add(x - 1, y);
                }
                if x + 1 < w {
                    add(x + 1, y);
                }
                next[y * w + x] = c + lambda * acc;
            }
        }
        img = next;
    }
    RasterTile { width: w, height: h, gray: img.iter().map(|&v| v.round().clamp(0.0, 255.0) as u8).collect() }
}

/// A Gabor filter's parameters. One orientation + scale of the bank.
#[derive(Debug, Clone, Copy)]
pub struct Gabor {
    /// Orientation in radians (0 = horizontal stroke energy → text lines).
    pub theta: f64,
    /// Wavelength of the sinusoid, in pixels.
    pub wavelength: f64,
    /// Gaussian envelope std-dev, in pixels.
    pub sigma: f64,
    /// Spatial aspect ratio (γ) of the envelope.
    pub gamma: f64,
}

impl Default for Gabor {
    fn default() -> Self {
        Gabor { theta: 0.0, wavelength: 6.0, sigma: 3.0, gamma: 0.5 }
    }
}

/// The Gabor response **energy** at a given orientation: `sqrt(even² + odd²)` per
/// pixel (the phase-invariant magnitude), returned as a normalised `[0,255]`
/// map. High where the image has structure at orientation `theta` and scale
/// `wavelength` — e.g. `theta = 0` lights up horizontal text lines. Deterministic;
/// kernel radius is bounded by `⌈3σ⌉` capped at 15.
pub fn gabor_energy(tile: &RasterTile, g: &Gabor) -> RasterTile {
    let (w, h) = (tile.width, tile.height);
    let radius = ((3.0 * g.sigma).ceil() as isize).clamp(1, 15);
    let (ct, st) = (g.theta.cos(), g.theta.sin());
    // Precompute the even (cos) and odd (sin) kernels.
    let mut ke = Vec::new();
    let mut ko = Vec::new();
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let (xf, yf) = (dx as f64, dy as f64);
            // `theta` is the orientation of the detected structure (a text line
            // at theta=0 is horizontal), so the sinusoid varies PERPENDICULAR to
            // it — `xr` is the perpendicular axis (at theta=0, `xr = y`), driving
            // the phase; `yr` runs along the structure.
            let xr = -xf * st + yf * ct;
            let yr = xf * ct + yf * st;
            let env = (-(xr * xr + g.gamma * g.gamma * yr * yr) / (2.0 * g.sigma * g.sigma)).exp();
            let phase = 2.0 * std::f64::consts::PI * xr / g.wavelength.max(1e-6);
            ke.push((dx, dy, env * phase.cos()));
            ko.push((dx, dy, env * phase.sin()));
        }
    }
    let src: Vec<f64> = tile.gray.iter().map(|&v| v as f64).collect();
    let sample = |x: usize, y: usize, dx: isize, dy: isize| -> f64 {
        let nx = (x as isize + dx).clamp(0, w as isize - 1) as usize;
        let ny = (y as isize + dy).clamp(0, h as isize - 1) as usize;
        src[ny * w + nx]
    };
    let mut energy = vec![0.0f64; w * h];
    let mut max_e = 1e-9;
    for y in 0..h {
        for x in 0..w {
            let mut re = 0.0;
            let mut ro = 0.0;
            for (dx, dy, kv) in &ke {
                re += kv * sample(x, y, *dx, *dy);
            }
            for (dx, dy, kv) in &ko {
                ro += kv * sample(x, y, *dx, *dy);
            }
            let e = (re * re + ro * ro).sqrt();
            energy[y * w + x] = e;
            if e > max_e {
                max_e = e;
            }
        }
    }
    RasterTile {
        width: w,
        height: h,
        gray: energy.iter().map(|&e| ((e / max_e) * 255.0).round().clamp(0.0, 255.0) as u8).collect(),
    }
}

/// The mean Gabor energy at an orientation — a scalar "how much structure lies at
/// `theta`". The layout stage compares `orientation_strength(θ=0)` (horizontal,
/// text lines) against other angles to estimate skew / reading direction.
pub fn orientation_strength(tile: &RasterTile, g: &Gabor) -> f64 {
    let e = gabor_energy(tile, g);
    e.gray.iter().map(|&v| v as f64).sum::<f64>() / (e.gray.len().max(1) as f64) / 255.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tile with a sharp vertical edge (left half black, right half white) plus
    /// salt-and-pepper noise, for the edge-preservation test.
    fn noisy_edge(w: usize, h: usize) -> RasterTile {
        let mut gray = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                let base = if x < w / 2 { 30 } else { 220 };
                // deterministic pseudo-noise (no RNG — reproducible)
                let n = (((x * 7 + y * 13) % 11) as i32 - 5) * 6;
                gray[y * w + x] = (base as i32 + n).clamp(0, 255) as u8;
            }
        }
        RasterTile { width: w, height: h, gray }
    }

    fn variance_within_half(t: &RasterTile, left: bool) -> f64 {
        let (w, h) = (t.width, t.height);
        let mut vals = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let in_half = if left { x < w / 2 } else { x >= w / 2 };
                if in_half {
                    vals.push(t.gray[y * w + x] as f64);
                }
            }
        }
        let mean = vals.iter().sum::<f64>() / vals.len() as f64;
        vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / vals.len() as f64
    }

    #[test]
    fn perona_malik_denoises_flats_but_preserves_the_edge() {
        let noisy = noisy_edge(24, 16);
        let clean = perona_malik(&noisy, &PeronaMalik::default());
        // Within each flat half, variance (noise) drops sharply.
        assert!(
            variance_within_half(&clean, true) < variance_within_half(&noisy, true) * 0.6,
            "left-half noise not reduced"
        );
        // The edge survives: the mean gap between halves stays large.
        let mean = |left: bool| {
            let (w, h) = (clean.width, clean.height);
            let mut s = 0.0;
            let mut n = 0.0;
            for y in 0..h {
                for x in 0..w {
                    if (x < w / 2) == left {
                        s += clean.gray[y * w + x] as f64;
                        n += 1.0;
                    }
                }
            }
            s / n
        };
        assert!((mean(false) - mean(true)).abs() > 120.0, "edge was blurred away");
    }

    #[test]
    fn perona_malik_is_deterministic() {
        let noisy = noisy_edge(20, 20);
        let a = perona_malik(&noisy, &PeronaMalik::default());
        let b = perona_malik(&noisy, &PeronaMalik::default());
        assert_eq!(a, b, "diffusion must be bit-for-bit deterministic");
    }

    #[test]
    fn perona_malik_iterations_are_bounded() {
        let t = noisy_edge(8, 8);
        // A huge iteration request is clamped — it must return, bounded.
        let cfg = PeronaMalik { iterations: 10_000, ..Default::default() };
        let _ = perona_malik(&t, &cfg); // completes (clamped to MAX_PM_ITERATIONS)
    }

    #[test]
    fn gabor_lights_up_the_matching_orientation() {
        // Horizontal stripes → strong energy at theta=0 (horizontal), weak at 90°.
        let (w, h) = (32, 32);
        let mut gray = vec![255u8; w * h];
        for y in 0..h {
            if (y / 3) % 2 == 0 {
                for x in 0..w {
                    gray[y * w + x] = 0;
                }
            }
        }
        let tile = RasterTile { width: w, height: h, gray };
        let horiz = orientation_strength(
            &tile,
            &Gabor { theta: 0.0, wavelength: 6.0, sigma: 3.0, gamma: 0.5 },
        );
        let vert = orientation_strength(
            &tile,
            &Gabor { theta: std::f64::consts::FRAC_PI_2, wavelength: 6.0, sigma: 3.0, gamma: 0.5 },
        );
        assert!(horiz > vert, "horizontal stripes: {horiz} should exceed vertical {vert}");
    }

    #[test]
    fn gabor_is_deterministic() {
        let t = noisy_edge(16, 16);
        let g = Gabor::default();
        assert_eq!(gabor_energy(&t, &g), gabor_energy(&t, &g));
    }
}
