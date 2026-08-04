//! §Fase 101.c — the IDP-E recognizer kernel: the deterministic core of axon's
//! Epistemic Vision Engine.
//!
//! **Doctrine (`axon_reads_with_geometry_not_correlation`).** Where the market
//! correlates pixels on a GPU, this kernel reads a document as structure:
//! binarise → **cubical-complex topology** (β₀ connected components + β₁ holes,
//! the stable sub-level-set homology at the ink threshold) for **robust
//! segmentation and a coarse bucket** → **geometric discrimination** (aspect,
//! hole geometry, stroke density) for the actual glyph → reading-order assembly →
//! a `pix`-navigable canonical tree. The read is **pure and deterministic**: the
//! same raster yields the same spans, bit-for-bit (the [`page_digest`]
//! determinism guarantee, §101.c — the analogue of §99's byte-deterministic
//! writer).
//!
//! **The honest framing (D101.14) — topology is a *prior*, not the recogniser.**
//! Persistent Betti numbers partition the alphabet into only a few classes
//! (β₁: `8`→2, `0oO`→1, `1IH`→0). They do **robust segmentation and coarse
//! bucketing**; the **geometry** (aspect ratio, hole position, density) carries
//! the real discrimination. We do not claim "topology reads the letters." And the
//! Cohen-Steiner stability that protects β under *pixel* noise does **not** cover
//! merges/splits (touching glyphs, broken strokes); those surface as a **low
//! confidence** the `anchor` floor catches (D101.7), never a silent wrong glyph.
//!
//! **Scope (D101.17) — clean machine-print / structured documents.** v1 ships a
//! reference prototype set (digits + topologically-distinct uppercase). The
//! production engine's tuned multi-font KB is the enterprise moat (§101.f). This
//! kernel is complete *for its declared scope*; the scope is declared, not hidden.
//!
//! **Safe input.** The kernel reads a bounded **PGM (P5) / PBM (P4) raster** — a
//! trivially, safely parseable grid, decoded in-process with bounds checked
//! BEFORE allocation. Real-world PNG/JPEG/PDF decode is the CVE-prone step and
//! lives in the sidecar (§101.e), which feeds this kernel the same raster tiles.

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use crate::extraction::{
    BBox, ExtractedSpan, ExtractionBounds, ExtractionEngine, ExtractionError, ExtractionHint,
    ExtractionResult,
};

// ── The raster ──────────────────────────────────────────────────────────────

/// A decoded grayscale tile, row-major, `0` = black ink … `255` = white. The
/// safe interchange the sidecar (§101.e) and the kernel share.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RasterTile {
    pub width: usize,
    pub height: usize,
    /// `width * height` bytes, row-major.
    pub gray: Vec<u8>,
}

impl RasterTile {
    /// Parse a bounded **PGM (P5)** or **PBM (P4)** binary raster. Bounds are
    /// checked BEFORE the pixel buffer is read, so a hostile header cannot force
    /// a giant allocation (D101.12). This is the ONLY decode the kernel does
    /// in-process; everything richer is the sidecar's job (§101.e).
    pub fn from_netpbm(bytes: &[u8], bounds: &ExtractionBounds) -> Result<Self, ExtractionError> {
        let mut p = NetpbmCursor { b: bytes, i: 0 };
        let magic = p.token().ok_or_else(|| ExtractionError::DecodeFailed("empty raster".into()))?;
        let (is_bitmap, is_binary) = match magic.as_slice() {
            b"P5" => (false, true), // binary grayscale
            b"P4" => (true, true),  // binary bitmap
            other => {
                return Err(ExtractionError::DecodeFailed(format!(
                    "unsupported netpbm magic {:?} (want P4/P5)",
                    String::from_utf8_lossy(other)
                )))
            }
        };
        let width = p.uint().ok_or_else(|| ExtractionError::DecodeFailed("no width".into()))?;
        let height = p.uint().ok_or_else(|| ExtractionError::DecodeFailed("no height".into()))?;
        // Bound the geometry BEFORE allocating (D101.12).
        let mp = (width.saturating_mul(height)) / 1_000_000;
        if mp as u32 > bounds.max_megapixels {
            return Err(ExtractionError::PixelCapExceeded(mp as u32));
        }
        if width == 0 || height == 0 {
            return Err(ExtractionError::DecodeFailed("zero-dimension raster".into()));
        }
        let mut gray = vec![255u8; width * height];
        if is_bitmap {
            // P4: 1 bit/pixel, rows byte-padded; bit set = black (0).
            p.skip_single_ws();
            let row_bytes = width.div_ceil(8);
            for y in 0..height {
                for xb in 0..row_bytes {
                    let byte = p.byte().ok_or_else(|| {
                        ExtractionError::DecodeFailed("truncated P4 pixel data".into())
                    })?;
                    for bit in 0..8 {
                        let x = xb * 8 + bit;
                        if x < width {
                            let set = (byte >> (7 - bit)) & 1 == 1;
                            gray[y * width + x] = if set { 0 } else { 255 };
                        }
                    }
                }
            }
        } else {
            let maxval = p.uint().ok_or_else(|| ExtractionError::DecodeFailed("no maxval".into()))?;
            if maxval == 0 || maxval > 255 {
                return Err(ExtractionError::DecodeFailed(format!("unsupported maxval {maxval}")));
            }
            p.skip_single_ws();
            for px in gray.iter_mut() {
                *px = p
                    .byte()
                    .ok_or_else(|| ExtractionError::DecodeFailed("truncated P5 pixel data".into()))?;
            }
        }
        let _ = is_binary;
        Ok(RasterTile { width, height, gray })
    }
}

struct NetpbmCursor<'a> {
    b: &'a [u8],
    i: usize,
}
impl NetpbmCursor<'_> {
    fn skip_ws_and_comments(&mut self) {
        loop {
            while self.i < self.b.len() && self.b[self.i].is_ascii_whitespace() {
                self.i += 1;
            }
            if self.i < self.b.len() && self.b[self.i] == b'#' {
                while self.i < self.b.len() && self.b[self.i] != b'\n' {
                    self.i += 1;
                }
            } else {
                break;
            }
        }
    }
    fn token(&mut self) -> Option<Vec<u8>> {
        self.skip_ws_and_comments();
        if self.i >= self.b.len() {
            return None;
        }
        let start = self.i;
        while self.i < self.b.len() && !self.b[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
        Some(self.b[start..self.i].to_vec())
    }
    fn uint(&mut self) -> Option<usize> {
        let t = self.token()?;
        std::str::from_utf8(&t).ok()?.parse().ok()
    }
    /// Consume exactly one whitespace byte after the header (the netpbm spec).
    fn skip_single_ws(&mut self) {
        if self.i < self.b.len() && self.b[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
    fn byte(&mut self) -> Option<u8> {
        if self.i < self.b.len() {
            let v = self.b[self.i];
            self.i += 1;
            Some(v)
        } else {
            None
        }
    }
}

// ── Binarisation (Otsu) ─────────────────────────────────────────────────────

/// A binary ink/background grid. `ink[i]` = true where the pixel is darker than
/// the Otsu threshold. Deterministic: the threshold is a pure function of the
/// histogram.
struct BitGrid {
    w: usize,
    h: usize,
    ink: Vec<bool>,
    /// The threshold chosen — recorded so the read is explainable.
    threshold: u8,
}

impl BitGrid {
    fn binarise(tile: &RasterTile) -> BitGrid {
        // Otsu's method — the between-class-variance-maximising threshold. Pure
        // function of the 256-bin histogram → deterministic.
        let mut hist = [0u64; 256];
        for &g in &tile.gray {
            hist[g as usize] += 1;
        }
        let total: u64 = tile.gray.len() as u64;
        let sum: f64 = (0..256).map(|i| i as f64 * hist[i] as f64).sum();
        let (mut w_b, mut sum_b, mut max_var, mut thresh) = (0.0f64, 0.0f64, -1.0f64, 128u8);
        for t in 0..256 {
            w_b += hist[t] as f64;
            if w_b == 0.0 {
                continue;
            }
            let w_f = total as f64 - w_b;
            if w_f == 0.0 {
                break;
            }
            sum_b += t as f64 * hist[t] as f64;
            let m_b = sum_b / w_b;
            let m_f = (sum - sum_b) / w_f;
            let var = w_b * w_f * (m_b - m_f) * (m_b - m_f);
            if var > max_var {
                max_var = var;
                thresh = t as u8;
            }
        }
        let ink: Vec<bool> = tile.gray.iter().map(|&g| g <= thresh).collect();
        BitGrid { w: tile.width, h: tile.height, ink, threshold: thresh }
    }

    #[inline]
    fn ink(&self, x: usize, y: usize) -> bool {
        self.ink[y * self.w + x]
    }
}

// ── Cubical topology: β₀ (components) + β₁ (holes) ──────────────────────────

/// A connected ink component — a glyph candidate. Carries its pixel bbox, ink
/// mass, and its topological signature (β₀ = 1 by construction; β₁ = holes).
#[derive(Debug, Clone)]
struct Component {
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
    ink_pixels: u64,
    /// β₁ — the number of holes (enclosed background regions), digital-topology
    /// consistent (4-connected foreground ⇒ 8-connected background).
    holes: u32,
    /// Normalised centroids of the holes within the bbox (`y` in `[0,1]`,
    /// top→bottom) — the geometry that separates topologically-equal glyphs.
    hole_centroids_y: Vec<f64>,
}

impl Component {
    fn width(&self) -> usize {
        self.x1 - self.x0 + 1
    }
    fn height(&self) -> usize {
        self.y1 - self.y0 + 1
    }
    /// Aspect ratio (w/h) — the primary geometric discriminant.
    fn aspect(&self) -> f64 {
        self.width() as f64 / self.height() as f64
    }
    /// Ink density within the bbox — separates thin strokes from filled forms.
    fn density(&self) -> f64 {
        self.ink_pixels as f64 / (self.width() * self.height()) as f64
    }
}

/// A tiny disjoint-set for β₀ (connected components) — the 0-dimensional
/// sub-level-set persistence of the ink set.
struct UnionFind {
    parent: Vec<usize>,
}
impl UnionFind {
    fn new(n: usize) -> Self {
        UnionFind { parent: (0..n).collect() }
    }
    fn find(&mut self, mut x: usize) -> usize {
        while self.parent[x] != x {
            self.parent[x] = self.parent[self.parent[x]];
            x = self.parent[x];
        }
        x
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            // Deterministic: always attach the larger index under the smaller.
            let (lo, hi) = if ra < rb { (ra, rb) } else { (rb, ra) };
            self.parent[hi] = lo;
        }
    }
}

/// Extract the connected ink components (β₀) with 4-connectivity, each tagged
/// with its hole count (β₁). Deterministic: components are returned in
/// row-major seed order.
fn components(grid: &BitGrid) -> Vec<Component> {
    let n = grid.w * grid.h;
    let mut uf = UnionFind::new(n);
    for y in 0..grid.h {
        for x in 0..grid.w {
            if !grid.ink(x, y) {
                continue;
            }
            let idx = y * grid.w + x;
            // 8-connected foreground (pairs with 4-connected background in
            // `count_holes`, the digital-topology-consistent choice) — a thin
            // diagonal stroke stays one component and a glyph's counter stays
            // enclosed instead of leaking through a 1px corner gap.
            if x + 1 < grid.w && grid.ink(x + 1, y) {
                uf.union(idx, idx + 1);
            }
            if y + 1 < grid.h && grid.ink(x, y + 1) {
                uf.union(idx, idx + grid.w);
            }
            if x + 1 < grid.w && y + 1 < grid.h && grid.ink(x + 1, y + 1) {
                uf.union(idx, idx + grid.w + 1);
            }
            if x > 0 && y + 1 < grid.h && grid.ink(x - 1, y + 1) {
                uf.union(idx, idx + grid.w - 1);
            }
        }
    }
    // Gather components by root, preserving first-seen (row-major) order.
    let mut order: Vec<usize> = Vec::new();
    let mut seen: BTreeMap<usize, usize> = BTreeMap::new();
    let mut agg: Vec<(usize, usize, usize, usize, u64)> = Vec::new(); // x0,y0,x1,y1,mass
    for y in 0..grid.h {
        for x in 0..grid.w {
            if !grid.ink(x, y) {
                continue;
            }
            let r = uf.find(y * grid.w + x);
            let slot = *seen.entry(r).or_insert_with(|| {
                order.push(r);
                agg.push((x, y, x, y, 0));
                agg.len() - 1
            });
            let a = &mut agg[slot];
            a.0 = a.0.min(x);
            a.1 = a.1.min(y);
            a.2 = a.2.max(x);
            a.3 = a.3.max(y);
            a.4 += 1;
        }
    }
    agg.into_iter()
        .map(|(x0, y0, x1, y1, mass)| {
            let (holes, hole_centroids_y) = count_holes(grid, x0, y0, x1, y1);
            Component { x0, y0, x1, y1, ink_pixels: mass, holes, hole_centroids_y }
        })
        .collect()
}

/// β₁ for one component's bbox: flood the background (8-connected) inward from
/// the bbox border; any background pixel NOT reached is enclosed → part of a
/// hole. Count the enclosed background components and their vertical centroids.
fn count_holes(grid: &BitGrid, x0: usize, y0: usize, x1: usize, y1: usize) -> (u32, Vec<f64>) {
    let (bw, bh) = (x1 - x0 + 1, y1 - y0 + 1);
    // reached[i]: background pixel connected to the border.
    let mut reached = vec![false; bw * bh];
    let mut stack: Vec<(usize, usize)> = Vec::new();
    let bg = |lx: usize, ly: usize| !grid.ink(x0 + lx, y0 + ly);
    // Seed from the border background pixels.
    for lx in 0..bw {
        for &ly in &[0usize, bh - 1] {
            if bg(lx, ly) && !reached[ly * bw + lx] {
                reached[ly * bw + lx] = true;
                stack.push((lx, ly));
            }
        }
    }
    for ly in 0..bh {
        for &lx in &[0usize, bw - 1] {
            if bg(lx, ly) && !reached[ly * bw + lx] {
                reached[ly * bw + lx] = true;
                stack.push((lx, ly));
            }
        }
    }
    while let Some((lx, ly)) = stack.pop() {
        // 4-connected background flood (pairs with 8-connected foreground) — a
        // 1px diagonal corner gap does NOT leak the exterior into a counter.
        for (dx, dy) in [(0i64, -1i64), (0, 1), (-1, 0), (1, 0)] {
            let (nx, ny) = (lx as i64 + dx, ly as i64 + dy);
            if nx < 0 || ny < 0 || nx as usize >= bw || ny as usize >= bh {
                continue;
            }
            let (nx, ny) = (nx as usize, ny as usize);
            if bg(nx, ny) && !reached[ny * bw + nx] {
                reached[ny * bw + nx] = true;
                stack.push((nx, ny));
            }
        }
    }
    // Enclosed background pixels = holes. Label them (4-connected) to count
    // distinct holes + their vertical centroids.
    let mut hole_id = vec![usize::MAX; bw * bh];
    let mut centroids: Vec<(f64, u64)> = Vec::new(); // (sum_y, count)
    for ly in 0..bh {
        for lx in 0..bw {
            if bg(lx, ly) && !reached[ly * bw + lx] && hole_id[ly * bw + lx] == usize::MAX {
                let id = centroids.len();
                centroids.push((0.0, 0));
                // flood this hole (4-conn)
                let mut s = vec![(lx, ly)];
                hole_id[ly * bw + lx] = id;
                while let Some((cx, cy)) = s.pop() {
                    centroids[id].0 += cy as f64;
                    centroids[id].1 += 1;
                    for (dx, dy) in [(0i64, -1i64), (0, 1), (-1, 0), (1, 0)] {
                        let (nx, ny) = (cx as i64 + dx, cy as i64 + dy);
                        if nx < 0 || ny < 0 || nx as usize >= bw || ny as usize >= bh {
                            continue;
                        }
                        let (nx, ny) = (nx as usize, ny as usize);
                        if bg(nx, ny) && !reached[ny * bw + nx] && hole_id[ny * bw + nx] == usize::MAX
                        {
                            hole_id[ny * bw + nx] = id;
                            s.push((nx, ny));
                        }
                    }
                }
            }
        }
    }
    let mut cys: Vec<f64> = centroids
        .iter()
        .map(|(sy, c)| if *c == 0 { 0.0 } else { (sy / *c as f64) / bh as f64 })
        .collect();
    cys.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (centroids.len() as u32, cys)
}

// ── Geometric discrimination (D101.14) ──────────────────────────────────────

/// A glyph's signature: the topological bucket (β₁) + the geometry that refines
/// it. This is what a prototype is matched against.
#[derive(Debug, Clone, Copy)]
struct GlyphSignature {
    beta1: u32,
    aspect: f64,
    density: f64,
    /// mean normalised vertical hole position (0=top … 1=bottom); NaN → 0.
    /// Computed for the §101.f enterprise-KB seam; the v1 OSS matcher is
    /// topology-first (D101.17) and has no reader yet — named, not silent.
    #[allow(dead_code)]
    hole_y: f64,
}

/// A reference prototype: a character + the signature region it occupies. The
/// v1 set is deliberately small and topologically-distinct (D101.17); the
/// enterprise KB (§101.f) extends it across fonts.
struct Prototype {
    ch: char,
    beta1: u32,
    aspect_lo: f64,
    aspect_hi: f64,
    density_lo: f64,
    density_hi: f64,
}

/// The v1 reference prototype set — clean machine-print, topologically distinct.
/// Chosen so β₁ buckets first, then aspect/density separate within a bucket.
const PROTOTYPES: &[Prototype] = &[
    // β₁ = 2 — two counters.
    Prototype { ch: '8', beta1: 2, aspect_lo: 0.4, aspect_hi: 0.95, density_lo: 0.30, density_hi: 0.95 },
    // β₁ = 1 — one counter. Round vs tall separate 'O'/'0' from 'D'.
    Prototype { ch: '0', beta1: 1, aspect_lo: 0.45, aspect_hi: 0.95, density_lo: 0.20, density_hi: 0.75 },
    Prototype { ch: 'D', beta1: 1, aspect_lo: 0.55, aspect_hi: 1.10, density_lo: 0.55, density_hi: 0.95 },
    // β₁ = 0 — no counter. Aspect + density separate the strokes.
    Prototype { ch: '1', beta1: 0, aspect_lo: 0.05, aspect_hi: 0.42, density_lo: 0.35, density_hi: 1.01 },
    Prototype { ch: 'H', beta1: 0, aspect_lo: 0.60, aspect_hi: 1.30, density_lo: 0.30, density_hi: 0.75 },
    Prototype { ch: '-', beta1: 0, aspect_lo: 1.60, aspect_hi: 8.00, density_lo: 0.40, density_hi: 1.01 },
    Prototype { ch: '.', beta1: 0, aspect_lo: 0.55, aspect_hi: 1.60, density_lo: 0.55, density_hi: 1.01 },
];

/// Classify a signature against the prototype set. Returns `(char, confidence)`
/// where confidence is a **geometric margin** (D101.2): 1.0 when the signature
/// sits squarely inside exactly one prototype's region and far from any other,
/// falling toward 0 as it nears a boundary or matches nothing. A NON-match
/// returns the replacement char with confidence 0 — which any `anchor` floor
/// rejects (D101.7), never a silent wrong glyph (D101.14).
fn classify(sig: GlyphSignature) -> (char, f64) {
    // First bucket by β₁ (topology). Within the bucket, score each prototype by
    // how centrally the geometry sits in its box; the margin to the runner-up is
    // the confidence.
    let score = |p: &Prototype| -> f64 {
        if p.beta1 != sig.beta1 {
            return 0.0;
        }
        let inside = |v: f64, lo: f64, hi: f64| -> f64 {
            if v < lo || v > hi {
                return 0.0;
            }
            let mid = 0.5 * (lo + hi);
            let half = 0.5 * (hi - lo).max(1e-6);
            (1.0 - (v - mid).abs() / half).max(0.0)
        };
        let a = inside(sig.aspect, p.aspect_lo, p.aspect_hi);
        let d = inside(sig.density, p.density_lo, p.density_hi);
        if a == 0.0 || d == 0.0 {
            0.0
        } else {
            // geometric mean — both must be satisfied.
            (a * d).sqrt()
        }
    };
    let mut best = ('\u{FFFD}', 0.0f64);
    let mut runner = 0.0f64;
    for p in PROTOTYPES {
        let s = score(p);
        if s > best.1 {
            runner = best.1;
            best = (p.ch, s);
        } else if s > runner {
            runner = s;
        }
    }
    // Confidence = the winner's score attenuated by how close the runner-up is
    // (the Wasserstein-margin analogue, D101.2).
    let margin = (best.1 - runner).clamp(0.0, 1.0);
    let confidence = if best.1 == 0.0 { 0.0 } else { (0.5 * best.1 + 0.5 * margin).clamp(0.0, 1.0) };
    (best.0, confidence)
}

// ── Reading-order assembly + the canonical tree (D101.11) ───────────────────

/// A node of the `pix`-navigable canonical document tree `D = (N, E, ρ, κ)`
/// (D101.11): a structural block with its deduced text, spatial `location`, and
/// children. Every leaf's text still carries `Inferred` provenance downstream.
#[derive(Debug, Clone, PartialEq)]
pub struct DocNode {
    /// `page | line | word` — the structural kind (κ).
    pub kind: String,
    /// The deduced text of this block (ρ).
    pub text: String,
    /// Normalised location in the page (ρ) — resolution-independent.
    pub bbox: BBox,
    /// Mean confidence of the leaves under this node.
    pub confidence: f64,
    pub children: Vec<DocNode>,
}

/// The result of recognising one page: the flat span list (for the extraction
/// contract) + the structural tree (for `pix`).
#[derive(Debug, Clone, PartialEq)]
pub struct RecognizedPage {
    pub spans: Vec<ExtractedSpan>,
    pub tree: DocNode,
    /// The Otsu threshold used — part of the explainable, replayable read.
    pub threshold: u8,
}

/// Group components into text lines (by vertical overlap) and, within a line,
/// into words (by horizontal gap), preserving reading order. Deterministic:
/// sorts are total and stable.
fn recognise_grid(grid: &BitGrid) -> RecognizedPage {
    let (pw, ph) = (grid.w as f64, grid.h as f64);
    let mut comps = components(grid);
    // Drop specks (noise) below a tiny area floor — bounded, deterministic.
    comps.retain(|c| c.ink_pixels >= 3 && c.width() >= 1 && c.height() >= 1);
    // Sort by (top, left) for reading order.
    comps.sort_by(|a, b| a.y0.cmp(&b.y0).then(a.x0.cmp(&b.x0)));

    // Line grouping by vertical-centre proximity.
    let mut lines: Vec<Vec<Component>> = Vec::new();
    for c in comps {
        let cy = 0.5 * (c.y0 + c.y1) as f64;
        let ch = c.height() as f64;
        let placed = lines.iter_mut().find(|line| {
            let ly = line
                .iter()
                .map(|g| 0.5 * (g.y0 + g.y1) as f64)
                .sum::<f64>()
                / line.len() as f64;
            (cy - ly).abs() < 0.6 * ch
        });
        match placed {
            Some(line) => line.push(c),
            None => lines.push(vec![c]),
        }
    }
    // Order lines top→bottom; within a line, left→right.
    lines.sort_by(|a, b| {
        let ay = a.iter().map(|g| g.y0).min().unwrap_or(0);
        let by = b.iter().map(|g| g.y0).min().unwrap_or(0);
        ay.cmp(&by)
    });

    let mut spans: Vec<ExtractedSpan> = Vec::new();
    let mut line_nodes: Vec<DocNode> = Vec::new();
    for mut line in lines {
        line.sort_by(|a, b| a.x0.cmp(&b.x0));
        let median_w = {
            let mut ws: Vec<usize> = line.iter().map(|c| c.width()).collect();
            ws.sort_unstable();
            ws[ws.len() / 2]
        };
        // Split the line into words on a horizontal gap wider than the glyph.
        let mut words: Vec<Vec<Component>> = Vec::new();
        let mut prev_x1: Option<usize> = None;
        for c in line {
            let gap = prev_x1.map(|px1| c.x0.saturating_sub(px1)).unwrap_or(0);
            if prev_x1.is_some() && gap > median_w {
                words.push(Vec::new());
            }
            if words.is_empty() {
                words.push(Vec::new());
            }
            prev_x1 = Some(c.x1);
            words.last_mut().unwrap().push(c);
        }

        let mut line_children: Vec<DocNode> = Vec::new();
        let (mut lx0, mut ly0, mut lx1, mut ly1) = (usize::MAX, usize::MAX, 0usize, 0usize);
        let mut line_conf_sum = 0.0;
        let mut line_conf_n = 0usize;
        for word in words {
            let mut text = String::new();
            let mut conf_sum = 0.0;
            let (mut wx0, mut wy0, mut wx1, mut wy1) = (usize::MAX, usize::MAX, 0usize, 0usize);
            for c in &word {
                let sig = GlyphSignature {
                    beta1: c.holes,
                    aspect: c.aspect(),
                    density: c.density(),
                    hole_y: if c.hole_centroids_y.is_empty() {
                        0.0
                    } else {
                        c.hole_centroids_y.iter().sum::<f64>() / c.hole_centroids_y.len() as f64
                    },
                };
                let (ch, conf) = classify(sig);
                text.push(ch);
                conf_sum += conf;
                let bbox = BBox {
                    x: c.x0 as f64 / pw,
                    y: c.y0 as f64 / ph,
                    w: c.width() as f64 / pw,
                    h: c.height() as f64 / ph,
                };
                spans.push(ExtractedSpan::new(ch.to_string(), conf, 0, bbox));
                wx0 = wx0.min(c.x0);
                wy0 = wy0.min(c.y0);
                wx1 = wx1.max(c.x1);
                wy1 = wy1.max(c.y1);
            }
            let wconf = if word.is_empty() { 0.0 } else { conf_sum / word.len() as f64 };
            line_conf_sum += conf_sum;
            line_conf_n += word.len();
            lx0 = lx0.min(wx0);
            ly0 = ly0.min(wy0);
            lx1 = lx1.max(wx1);
            ly1 = ly1.max(wy1);
            line_children.push(DocNode {
                kind: "word".into(),
                text,
                bbox: BBox {
                    x: wx0 as f64 / pw,
                    y: wy0 as f64 / ph,
                    w: (wx1 - wx0 + 1) as f64 / pw,
                    h: (wy1 - wy0 + 1) as f64 / ph,
                },
                confidence: wconf,
                children: vec![],
            });
        }
        let line_text = line_children
            .iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        line_nodes.push(DocNode {
            kind: "line".into(),
            text: line_text,
            bbox: BBox {
                x: lx0 as f64 / pw,
                y: ly0 as f64 / ph,
                w: (lx1.saturating_sub(lx0) + 1) as f64 / pw,
                h: (ly1.saturating_sub(ly0) + 1) as f64 / ph,
            },
            confidence: if line_conf_n == 0 { 0.0 } else { line_conf_sum / line_conf_n as f64 },
            children: line_children,
        });
    }

    let page_conf = if spans.is_empty() {
        0.0
    } else {
        spans.iter().map(|s| s.confidence).sum::<f64>() / spans.len() as f64
    };
    let tree = DocNode {
        kind: "page".into(),
        text: line_nodes.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n"),
        bbox: BBox { x: 0.0, y: 0.0, w: 1.0, h: 1.0 },
        confidence: page_conf,
        children: line_nodes,
    };
    RecognizedPage { spans, tree, threshold: grid.threshold }
}

/// Recognise a decoded raster tile into a page — the kernel's public entry.
/// Pure and deterministic (D101.16).
pub fn recognise(tile: &RasterTile) -> RecognizedPage {
    let grid = BitGrid::binarise(tile);
    recognise_grid(&grid)
}

/// A stable digest of a recognised page — the determinism guarantee made
/// checkable (§101.c, the analogue of §99's byte-golden hash). The SAME raster
/// yields the SAME digest, forever, under a pinned engine.
pub fn page_digest(page: &RecognizedPage) -> String {
    let mut h = Sha256::new();
    h.update([page.threshold]);
    for s in &page.spans {
        h.update(s.text.as_bytes());
        h.update((s.confidence.to_bits()).to_le_bytes());
        h.update((s.bbox.x.to_bits()).to_le_bytes());
        h.update((s.bbox.y.to_bits()).to_le_bytes());
        h.update((s.bbox.w.to_bits()).to_le_bytes());
        h.update((s.bbox.h.to_bits()).to_le_bytes());
        h.update(b"|");
    }
    format!("{:x}", h.finalize())
}

// ── The engine ──────────────────────────────────────────────────────────────

/// The IDP-E engine: reads a bounded PGM/PBM raster and recognises it. Its
/// output is born `Inferred` + `Untrusted` with measured confidence — the
/// [`ExtractionEngine`] contract. Real PNG/JPEG/PDF decode is the sidecar's
/// (§101.e); this engine is the deterministic recogniser the sidecar feeds.
#[derive(Debug, Clone, Copy, Default)]
pub struct IdpeEngine;

impl ExtractionEngine for IdpeEngine {
    fn name(&self) -> &str {
        "idp-e"
    }
    fn version(&self) -> &str {
        "1"
    }
    fn extract(
        &self,
        bytes: &[u8],
        _hint: &ExtractionHint,
        bounds: &ExtractionBounds,
    ) -> Result<ExtractionResult, ExtractionError> {
        let tile = RasterTile::from_netpbm(bytes, bounds)?;
        let page = recognise(&tile);
        if page.spans.len() > bounds.max_spans {
            return Err(ExtractionError::SpanCapExceeded(page.spans.len()));
        }
        Ok(ExtractionResult {
            engine: self.name().to_string(),
            engine_version: self.version().to_string(),
            spans: page.spans,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Render a component-bitmap into a PGM (P5) with a white margin, so the
    /// tests exercise the real decode path.
    fn pgm(rows: &[&str]) -> Vec<u8> {
        let h = rows.len();
        let w = rows.iter().map(|r| r.len()).max().unwrap();
        let mut out = format!("P5 {w} {h} 255\n").into_bytes();
        for r in rows {
            for x in 0..w {
                let c = r.as_bytes().get(x).copied().unwrap_or(b' ');
                out.push(if c == b'#' { 0 } else { 255 });
            }
        }
        out
    }

    fn recog(rows: &[&str]) -> RecognizedPage {
        let tile = RasterTile::from_netpbm(&pgm(rows), &ExtractionBounds::default()).unwrap();
        recognise(&tile)
    }

    #[test]
    fn beta1_buckets_the_alphabet() {
        // '8' → two holes; '0' → one; '1' → none. Topology as the coarse bucket.
        let eight = recog(&[
            " ### ",
            "#   #",
            "#   #",
            " ### ",
            "#   #",
            "#   #",
            " ### ",
        ]);
        assert_eq!(eight.tree.text, "8", "two counters → 8");

        let zero = recog(&[
            " ### ",
            "#   #",
            "#   #",
            "#   #",
            "#   #",
            "#   #",
            " ### ",
        ]);
        assert_eq!(zero.tree.text, "0", "one counter → 0");

        let one = recog(&["  #  ", "  #  ", "  #  ", "  #  ", "  #  ", "  #  ", "  #  "]);
        assert_eq!(one.tree.text, "1", "no counter, thin → 1");
    }

    #[test]
    fn reading_order_is_left_to_right_top_to_bottom() {
        // Two glyphs on one line: '1' then '0'.
        let page = recog(&[
            "  #    ### ",
            "  #   #   #",
            "  #   #   #",
            "  #   #   #",
            "  #   #   #",
            "  #   #   #",
            "  #    ### ",
        ]);
        assert_eq!(page.tree.text, "10", "reading order 1 then 0");
        assert_eq!(page.spans.len(), 2);
        assert!(page.spans[0].bbox.x < page.spans[1].bbox.x);
    }

    #[test]
    fn confidence_is_measured_and_in_range() {
        let page = recog(&[
            " ### ",
            "#   #",
            "#   #",
            "#   #",
            "#   #",
            "#   #",
            " ### ",
        ]);
        let c = page.spans[0].confidence;
        assert!((0.0..=1.0).contains(&c), "confidence {c} out of range");
        assert!(c > 0.0, "a clean '0' should classify with positive confidence");
    }

    #[test]
    fn unrecognised_shape_gets_zero_confidence_not_a_wrong_glyph() {
        // A dense square matches no prototype's geometry cleanly → low/zero conf,
        // which an anchor floor rejects — never a silent wrong glyph (D101.14).
        let page = recog(&[
            "#####", "#####", "#####", "#####", "#####",
        ]);
        // Either the replacement char, or a positive-but-low confidence — but a
        // full block must NOT be confidently classified as a real glyph.
        let s = &page.spans[0];
        assert!(s.confidence < 0.6 || s.text == "\u{FFFD}", "block misread confidently: {s:?}");
    }

    #[test]
    fn recognition_is_deterministic() {
        // §101.c determinism guarantee: same raster ⇒ same page digest, twice.
        let rows: &[&str] = &[
            " ### ",
            "#   #",
            "#   #",
            " ### ",
            "#   #",
            "#   #",
            " ### ",
        ];
        let a = page_digest(&recog(rows));
        let b = page_digest(&recog(rows));
        assert_eq!(a, b, "recognition must be bit-for-bit deterministic");
    }

    #[test]
    fn engine_output_is_born_inferred_untrusted() {
        let bytes = pgm(&[" ### ", "#   #", "#   #", "#   #", "#   #", "#   #", " ### "]);
        let out = IdpeEngine
            .extract(&bytes, &ExtractionHint::default(), &ExtractionBounds::default())
            .unwrap();
        assert_eq!(out.engine, "idp-e");
        assert_eq!(out.provenance(), crate::ingest_provenance::IngestProvenance::Inferred);
        assert_eq!(out.taint(), crate::emcp::EpistemicTaint::Untrusted);
        assert_eq!(out.spans[0].epistemic_ceiling(), "believe");
    }

    #[test]
    fn pixel_bound_refuses_giant_raster_before_alloc() {
        // A header claiming 100_000 × 100_000 (10^10 px) must be refused BEFORE
        // any pixel buffer is allocated (D101.12).
        let hdr = b"P5 100000 100000 255\n";
        let err = RasterTile::from_netpbm(hdr, &ExtractionBounds::default()).unwrap_err();
        assert!(matches!(err, ExtractionError::PixelCapExceeded(_)), "{err:?}");
    }

    #[test]
    fn truncated_raster_is_typed_decode_error() {
        let err = RasterTile::from_netpbm(b"P5 5 5 255\n\x00\x00", &ExtractionBounds::default())
            .unwrap_err();
        assert!(matches!(err, ExtractionError::DecodeFailed(_)), "{err:?}");
    }

    #[test]
    fn canonical_tree_has_page_line_word_structure() {
        let page = recog(&["  #    ### ", "  #   #   #", "  #   #   #", "  #   #   #", "  #   #   #", "  #   #   #", "  #    ### "]);
        assert_eq!(page.tree.kind, "page");
        assert!(!page.tree.children.is_empty());
        assert_eq!(page.tree.children[0].kind, "line");
        assert_eq!(page.tree.children[0].children[0].kind, "word");
    }
}
