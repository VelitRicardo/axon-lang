//! v2.42.0 — the `HolographBackend` port + the OSS reference HRR codec.
//!
//! This is the OSS half of the `savant` memory layer's RUNTIME: the Holographic
//! Reduced Representation (HRR / Vector-Symbolic Architecture) codec that lets a
//! long-horizon research loop compress an entire epistemic graph into a single
//! fixed-dimension vector and decode only the slice it needs — so a mandate
//! started on day 1 keeps exact causal influence on day 15 without the quadratic
//! context-window rot of token sequences (paper section 5).
//!
//! It defines:
//!   - [`HolographBackend`] — the **port** (charter split R1). Enterprise mounts
//! the production SIMD/Q-stable codec (v2.42.0) behind this same trait; the OSS
//!     crate ships only the reference implementation below.
//!   - [`ReferenceHolographCodec`] — a genuinely usable HRR codec over `f64`,
//!     hard-capped at [`HOLOGRAPH_DIM_CAP`] and power-of-two dimensions. It runs
//!     small binding/unbinding on the CPU and is the differential-test ORACLE for
//!     the enterprise engine.
//!
//! **The math (Plate 1995, faithful):**
//!   - *bind* (`⊛`) = circular convolution, computed in the frequency domain as
//!     `x ⊛ y = F⁻¹(F(x) ⊙ F(y))` — `O(n log n)` via a self-contained radix-2 FFT
//!     (no `rustfft`/`num-complex` dependency, mirroring `quant.rs`'s rolled-own
//! complex discipline). This is the paper's section 5.2 identity.
//!   - *unbind* = circular correlation, `y# ⊛ c` where `y#` is the involution
//!     `y#[k] = y[(-k) mod n]`, i.e. `F⁻¹(F(c) ⊙ conj(F(y)))`.
//! - *unitary projection* (`make_unitary`) = the paper's section 5.2 "iterative
//!     projection to a numerically well-behaved point": set every frequency-bin
//!     magnitude to 1 while preserving phase. Binding with a **unitary** key is
//!     then EXACTLY invertible by correlation (recovery cosine = 1), which is the
//!     ">100% retrieval-efficacy" stability the paper cites. Random (non-unitary)
//!     keys recover approximately — the honest HRR property.
//!
//! Honest bound (doctrine `no_unwitnessed_advantage`, v2.23.0): this is a memory
//! *compression* primitive, not a claim of cognitive advantage. Its guarantee is
//! exact, testable algebra (bind∘unbind = identity for unitary keys), nothing more.

use std::f64::consts::PI;

/// The hard dimensionality cap of the OSS reference codec. Above this the
/// enterprise SIMD engine (v2.42.0) is required; the reference returns
/// [`HolographError::CapacityExceeded`] rather than allocate unboundedly.
pub const HOLOGRAPH_DIM_CAP: usize = 4096;

/// A structured HRR error — never a silent wrong-length or OOM result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HolographError {
    /// `dim` exceeds [`HOLOGRAPH_DIM_CAP`] — use the enterprise engine.
    CapacityExceeded { dim: usize, cap: usize },
    /// The FFT reference requires a power-of-two, non-zero dimension.
    NotPowerOfTwo(usize),
    /// An operand's length does not match the codec's dimension.
    DimMismatch { expected: usize, got: usize },
}

impl std::fmt::Display for HolographError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HolographError::CapacityExceeded { dim, cap } => write!(
                f,
                "holograph dimension {dim} exceeds the OSS reference cap {cap} (use the enterprise codec)"
            ),
            HolographError::NotPowerOfTwo(d) => {
                write!(f, "holograph dimension {d} must be a non-zero power of two")
            }
            HolographError::DimMismatch { expected, got } => {
                write!(f, "holograph operand length {got} != codec dimension {expected}")
            }
        }
    }
}

impl std::error::Error for HolographError {}

// ── Minimal complex scalar (no external dep, mirrors quant.rs) ───────────────

#[derive(Debug, Clone, Copy)]
struct Cx {
    re: f64,
    im: f64,
}

impl Cx {
    const ONE: Cx = Cx { re: 1.0, im: 0.0 };
    #[inline]
    fn new(re: f64, im: f64) -> Cx {
        Cx { re, im }
    }
    #[inline]
    fn add(self, o: Cx) -> Cx {
        Cx::new(self.re + o.re, self.im + o.im)
    }
    #[inline]
    fn sub(self, o: Cx) -> Cx {
        Cx::new(self.re - o.re, self.im - o.im)
    }
    #[inline]
    fn mul(self, o: Cx) -> Cx {
        Cx::new(self.re * o.re - self.im * o.im, self.re * o.im + self.im * o.re)
    }
    #[inline]
    fn conj(self) -> Cx {
        Cx::new(self.re, -self.im)
    }
    #[inline]
    fn abs(self) -> f64 {
        self.re.hypot(self.im)
    }
}

/// In-place iterative radix-2 Cooley–Tukey FFT. `invert` selects the inverse
/// transform (with the `1/n` normalisation). `a.len()` MUST be a power of two.
fn fft(a: &mut [Cx], invert: bool) {
    let n = a.len();
    if n <= 1 {
        return;
    }
    // Bit-reversal permutation.
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            a.swap(i, j);
        }
    }
    // Butterflies.
    let mut len = 2;
    while len <= n {
        let sign = if invert { 1.0 } else { -1.0 };
        let ang = 2.0 * PI / (len as f64) * sign;
        let wlen = Cx::new(ang.cos(), ang.sin());
        let half = len / 2;
        let mut i = 0;
        while i < n {
            let mut w = Cx::ONE;
            for k in 0..half {
                let u = a[i + k];
                let v = a[i + k + half].mul(w);
                a[i + k] = u.add(v);
                a[i + k + half] = u.sub(v);
                w = w.mul(wlen);
            }
            i += len;
        }
        len <<= 1;
    }
    if invert {
        let inv = 1.0 / n as f64;
        for x in a.iter_mut() {
            x.re *= inv;
            x.im *= inv;
        }
    }
}

/// The HRR codec port (charter split R1). Enterprise mounts a SIMD/Q-stable
/// engine behind this trait; OSS ships [`ReferenceHolographCodec`].
pub trait HolographBackend {
    /// The fixed vector dimension this codec binds/unbinds over.
    fn dim(&self) -> usize;
    /// `bind(a, b) = a ⊛ b` — circular convolution (associative, commutative).
    fn bind(&self, a: &[f64], b: &[f64]) -> Result<Vec<f64>, HolographError>;
    /// `unbind(c, key) = key# ⊛ c` — circular correlation. Exact inverse of
    /// `bind` when `key` is unitary (see [`Self::make_unitary`]).
    fn unbind(&self, c: &[f64], key: &[f64]) -> Result<Vec<f64>, HolographError>;
    /// Project `v` onto the unitary manifold: unit magnitude in every frequency
    /// bin, phase preserved (paper section 5.2). Binding with a unitary key is exactly
    /// invertible.
    fn make_unitary(&self, v: &[f64]) -> Result<Vec<f64>, HolographError>;
    /// A deterministic role/filler vector `~ N(0, 1/dim)` seeded by `seed`
    /// (reproducible across runs — no ambient RNG).
    fn role_vector(&self, seed: u64) -> Vec<f64>;
}

/// The OSS reference HRR codec: exact `f64` frequency-domain binding, capped at
/// [`HOLOGRAPH_DIM_CAP`] and power-of-two dimensions.
pub struct ReferenceHolographCodec {
    dim: usize,
}

impl ReferenceHolographCodec {
    /// Build a codec of the given dimension. `dim` must be a non-zero power of
    /// two and `<= HOLOGRAPH_DIM_CAP`.
    pub fn new(dim: usize) -> Result<Self, HolographError> {
        if dim > HOLOGRAPH_DIM_CAP {
            return Err(HolographError::CapacityExceeded {
                dim,
                cap: HOLOGRAPH_DIM_CAP,
            });
        }
        if dim == 0 || !dim.is_power_of_two() {
            return Err(HolographError::NotPowerOfTwo(dim));
        }
        Ok(Self { dim })
    }

    fn check(&self, v: &[f64]) -> Result<(), HolographError> {
        if v.len() != self.dim {
            Err(HolographError::DimMismatch {
                expected: self.dim,
                got: v.len(),
            })
        } else {
            Ok(())
        }
    }

    fn to_freq(v: &[f64]) -> Vec<Cx> {
        let mut a: Vec<Cx> = v.iter().map(|&x| Cx::new(x, 0.0)).collect();
        fft(&mut a, false);
        a
    }

    fn to_real(mut f: Vec<Cx>) -> Vec<f64> {
        fft(&mut f, true);
        f.into_iter().map(|c| c.re).collect()
    }
}

impl HolographBackend for ReferenceHolographCodec {
    fn dim(&self) -> usize {
        self.dim
    }

    fn bind(&self, a: &[f64], b: &[f64]) -> Result<Vec<f64>, HolographError> {
        self.check(a)?;
        self.check(b)?;
        let fa = Self::to_freq(a);
        let fb = Self::to_freq(b);
        let prod: Vec<Cx> = fa.iter().zip(fb.iter()).map(|(x, y)| x.mul(*y)).collect();
        Ok(Self::to_real(prod))
    }

    fn unbind(&self, c: &[f64], key: &[f64]) -> Result<Vec<f64>, HolographError> {
        self.check(c)?;
        self.check(key)?;
        let fc = Self::to_freq(c);
        let fk = Self::to_freq(key);
        // Correlation: F(c) ⊙ conj(F(key)). For a unitary key (|F(key)|=1) this
        // is the exact inverse of bind; otherwise the standard approximate HRR
        // decode.
        let quot: Vec<Cx> = fc
            .iter()
            .zip(fk.iter())
            .map(|(x, y)| x.mul(y.conj()))
            .collect();
        Ok(Self::to_real(quot))
    }

    fn make_unitary(&self, v: &[f64]) -> Result<Vec<f64>, HolographError> {
        self.check(v)?;
        let mut fv = Self::to_freq(v);
        for bin in fv.iter_mut() {
            let m = bin.abs();
            if m > 1e-12 {
                bin.re /= m;
                bin.im /= m;
            } else {
                *bin = Cx::ONE;
            }
        }
        Ok(Self::to_real(fv))
    }

    fn role_vector(&self, seed: u64) -> Vec<f64> {
        // splitmix64 to decorrelate the seed (so 42 and 43 give unrelated
        // streams, not `seed | 1` which collapses even/odd pairs), then
        // xorshift64 → uniform, Box–Muller → N(0, 1/dim). Deterministic.
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        let mut state = z | 1;
        let mut next_u01 = || -> f64 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            // Map to (0, 1]; avoid exactly 0 for the ln in Box–Muller.
            ((state >> 11) as f64 + 1.0) / ((1u64 << 53) as f64 + 1.0)
        };
        let sigma = 1.0 / (self.dim as f64).sqrt();
        (0..self.dim)
            .map(|_| {
                let u1 = next_u01();
                let u2 = next_u01();
                (-2.0 * u1.ln()).sqrt() * (2.0 * PI * u2).cos() * sigma
            })
            .collect()
    }
}

/// Cosine similarity of two equal-length vectors (0 for a zero vector).
pub fn cosine(a: &[f64], b: &[f64]) -> f64 {
    let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let na: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let nb: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na * nb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_power_of_two_and_oversize() {
        assert!(matches!(
            ReferenceHolographCodec::new(48),
            Err(HolographError::NotPowerOfTwo(48))
        ));
        assert!(matches!(
            ReferenceHolographCodec::new(0),
            Err(HolographError::NotPowerOfTwo(0))
        ));
        assert!(matches!(
            ReferenceHolographCodec::new(HOLOGRAPH_DIM_CAP * 2),
            Err(HolographError::CapacityExceeded { .. })
        ));
        assert!(ReferenceHolographCodec::new(1024).is_ok());
    }

    #[test]
    fn dim_mismatch_is_reported() {
        let c = ReferenceHolographCodec::new(64).unwrap();
        let short = vec![0.0; 32];
        assert!(matches!(
            c.bind(&short, &short),
            Err(HolographError::DimMismatch { expected: 64, got: 32 })
        ));
    }

    #[test]
    fn bind_is_commutative() {
        let c = ReferenceHolographCodec::new(256).unwrap();
        let a = c.role_vector(1);
        let b = c.role_vector(2);
        let ab = c.bind(&a, &b).unwrap();
        let ba = c.bind(&b, &a).unwrap();
        assert!(cosine(&ab, &ba) > 0.999_999);
    }

    #[test]
    fn unitary_bind_unbind_recovers_exactly() {
        // The load-bearing HRR guarantee: with a unitary key, bind∘unbind is the
        // identity (recovery cosine = 1). This is the paper's section 5.2 stability.
        let c = ReferenceHolographCodec::new(512).unwrap();
        let filler = c.role_vector(10);
        let key = c.make_unitary(&c.role_vector(20)).unwrap();
        let bound = c.bind(&filler, &key).unwrap();
        let recovered = c.unbind(&bound, &key).unwrap();
        assert!(
            cosine(&filler, &recovered) > 0.999_999,
            "unitary recovery cosine = {}",
            cosine(&filler, &recovered)
        );
    }

    #[test]
    fn wrong_key_recovers_noise() {
        // Unbinding with an unrelated key must NOT recover the filler — the
        // superposition is opaque without the right key.
        let c = ReferenceHolographCodec::new(512).unwrap();
        let filler = c.role_vector(10);
        let key = c.make_unitary(&c.role_vector(20)).unwrap();
        let wrong = c.make_unitary(&c.role_vector(999)).unwrap();
        let bound = c.bind(&filler, &key).unwrap();
        let recovered = c.unbind(&bound, &wrong).unwrap();
        assert!(
            cosine(&filler, &recovered).abs() < 0.2,
            "wrong-key cosine = {} (should be ~0)",
            cosine(&filler, &recovered)
        );
    }

    #[test]
    fn superposition_decodes_each_pair() {
        // Bind two role→filler pairs, sum them (the HRR "trace"), and decode
        // each filler by its role key — the core of holographic memory.
        let c = ReferenceHolographCodec::new(1024).unwrap();
        let role_a = c.make_unitary(&c.role_vector(1)).unwrap();
        let role_b = c.make_unitary(&c.role_vector(2)).unwrap();
        let fill_a = c.role_vector(100);
        let fill_b = c.role_vector(200);

        let ta = c.bind(&fill_a, &role_a).unwrap();
        let tb = c.bind(&fill_b, &role_b).unwrap();
        let trace: Vec<f64> = ta.iter().zip(tb.iter()).map(|(x, y)| x + y).collect();

        let dec_a = c.unbind(&trace, &role_a).unwrap();
        let dec_b = c.unbind(&trace, &role_b).unwrap();
        // Each filler is recovered above its cross-talk with the other.
        assert!(cosine(&dec_a, &fill_a) > cosine(&dec_a, &fill_b));
        assert!(cosine(&dec_b, &fill_b) > cosine(&dec_b, &fill_a));
        assert!(cosine(&dec_a, &fill_a) > 0.6, "cos={}", cosine(&dec_a, &fill_a));
    }

    #[test]
    fn role_vector_is_deterministic() {
        let c = ReferenceHolographCodec::new(128).unwrap();
        assert_eq!(c.role_vector(42), c.role_vector(42));
        assert_ne!(c.role_vector(42), c.role_vector(43));
    }

    #[test]
    fn make_unitary_is_idempotent() {
        let c = ReferenceHolographCodec::new(256).unwrap();
        let u = c.make_unitary(&c.role_vector(7)).unwrap();
        let uu = c.make_unitary(&u).unwrap();
        assert!(cosine(&u, &uu) > 0.999_99);
    }
}
