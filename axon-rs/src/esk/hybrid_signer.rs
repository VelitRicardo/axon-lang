//! v4.0.0 — **the hybrid evidence signer: `Ed25519(H) ‖ ML-DSA-65(H)`.**
//!
//! This is the paper's formula (`paper_compliance.md` section 4.2) made runnable —
//! the exact construction the document has been required to describe as
//! *"diseñado y no construido"* until this module existed. `H` is SHA-256 of
//! the message, computed by the module's own C kernel, so the entire pipeline
//! — hash, classical signature, post-quantum signature, and the entropy that
//! feeds both keypairs — lives inside the single cryptographic boundary that
//! the design decision declared in `axon-csys`. This file holds key material and
//! concatenation; it implements no cryptography.
//!
//! # Why hybrid, and why AND
//!
//! The classical half (Ed25519) is mature, tiny, and survivable review by
//! anyone; the post-quantum half (ML-DSA-65, FIPS 204) covers the
//! harvest-now-decrypt-later horizon that NOM-151's 10-year retention forces
//! evidence to survive. A verifier must accept the bundle **only when BOTH
//! halves verify**. The tempting alternative — accept if either half holds —
//! inverts the construction into something *weaker* than either scheme alone,
//! because an attacker then needs to break only their favourite half. The
//! `hybrid_means_both_not_either` test exists to keep that word "and" from
//! quietly becoming "or".
//!
//! # Prehashing, stated plainly
//!
//! Both halves sign `H = SHA-256(message)`, not the message. That is the
//! paper's stated formula, it keeps multi-megabyte evidence artifacts from
//! being streamed twice through two signature schemes, and it makes the two
//! halves provably bind the SAME bytes. The cost, stated rather than hidden:
//! the construction's binding strength is capped by SHA-256's collision
//! resistance. For an evidence bundle whose integrity anchor is already a
//! SHA-256 Merkle chain (`esk::provenance`), that cap was the system-wide
//! bound before this module existed.
//!
//! # Availability
//!
//! Gated on `csys-native`, because the kernels are C and v2.83.0 made the C
//! toolchain opt-in. Without the feature the HMAC-SHA256 baseline in
//! [`super::provenance`] remains the signer — same trait, honestly weaker
//! claim.

use super::provenance::Signer;

use axon_csys::ed25519::{
    keypair_from_seed, Ed25519Keypair, ED25519_PUBLICKEY_BYTES, ED25519_SIGNATURE_BYTES,
};
use axon_csys::mldsa::{MLDSA65_PUBLICKEY_BYTES, MLDSA65_SECRETKEY_BYTES, MLDSA65_SIGNATURE_BYTES};

/// Total bundle length: the fixed lengths of both halves make the
/// concatenation self-describing — no framing bytes, no parse ambiguity.
pub const HYBRID_SIGNATURE_BYTES: usize = ED25519_SIGNATURE_BYTES + MLDSA65_SIGNATURE_BYTES;

/// The two public keys a third party needs to verify a bundle. Everything
/// here is public by definition; it travels WITH the evidence package,
/// because an evidence chain whose verification keys were ephemeral is an
/// evidence chain nobody can re-verify after the process exits.
#[derive(Clone)]
pub struct HybridPublicKeys {
    pub ed25519: [u8; ED25519_PUBLICKEY_BYTES],
    pub ml_dsa_65: [u8; MLDSA65_PUBLICKEY_BYTES],
}

/// The hybrid signer. Holds both secret keys; zeroizes them on drop.
pub struct HybridSigner {
    ed: Ed25519Keypair,
    mldsa_public: [u8; MLDSA65_PUBLICKEY_BYTES],
    mldsa_secret: [u8; MLDSA65_SECRETKEY_BYTES],
}

impl HybridSigner {
    /// Generate both keypairs from the module's own entropy source.
    ///
    /// `None` = OS RNG failure. Surfaced, not panicked: the caller decides
    /// whether an unsigned evidence run is abortable or retryable — this
    /// constructor only refuses to pretend it has keys.
    pub fn generate() -> Option<Self> {
        let mut seed = axon_csys::ed25519::generate_seed()?;
        let ed = keypair_from_seed(&seed);
        // The seed is a second copy of the Ed25519 secret. The first version
        // wrote `drop(seed)` here — a no-op on a `Copy` array (rustc said so;
        // the bytes stay in the frame until it is reused), which would have
        // made "it dies here" one more claim the code does not keep. Wipe it
        // in place instead, same volatile discipline as `Drop`.
        for b in seed.iter_mut() {
            unsafe { core::ptr::write_volatile(b, 0) };
        }
        let (mldsa_public, mldsa_secret) = axon_csys::mldsa::keypair()?;
        Some(HybridSigner { ed, mldsa_public, mldsa_secret })
    }

    /// The verification half of the key material.
    pub fn public_keys(&self) -> HybridPublicKeys {
        HybridPublicKeys {
            ed25519: self.ed.public,
            ml_dsa_65: self.mldsa_public,
        }
    }

    /// Verify a bundle against known public keys — the third-party path,
    /// needing no secret material. The trait's `verify` delegates here.
    ///
    /// Both halves must hold. See the module doc for why "both" is
    /// load-bearing.
    pub fn verify_hybrid(message: &[u8], signature: &[u8], keys: &HybridPublicKeys) -> bool {
        if signature.len() != HYBRID_SIGNATURE_BYTES {
            return false;
        }
        let digest = axon_csys::crypto::sha256(message);
        let (ed_half, mldsa_half) = signature.split_at(ED25519_SIGNATURE_BYTES);
        axon_csys::ed25519::verify(ed_half, &digest, &keys.ed25519)
            && axon_csys::mldsa::verify(mldsa_half, &digest, &keys.ml_dsa_65)
    }
}

impl Signer for HybridSigner {
    fn algorithm(&self) -> &str {
        "Ed25519+ML-DSA-65"
    }

    /// `Ed25519(H) ‖ ML-DSA-65(H)`, `H = SHA-256(message)`.
    ///
    /// # Panics
    ///
    /// If the OS RNG fails mid-signature (ML-DSA's hedged signing draws fresh
    /// randomness per call). The trait has no error channel, and the two
    /// non-panicking alternatives are both worse: an empty return is a
    /// signature that silently never verifies, and an Ed25519-only return is a
    /// bundle that quietly dropped its post-quantum half — the v2.67.0 kernel
    /// defect ("if the evidence is missing, substitute the belief") applied to
    /// the evidence signer itself. Refusing loudly is the only honest exit.
    fn sign(&self, message: &[u8]) -> Vec<u8> {
        let digest = axon_csys::crypto::sha256(message);
        let ed_sig = axon_csys::ed25519::sign(&digest, &self.ed);
        let mldsa_sig = axon_csys::mldsa::sign(&digest, &self.mldsa_secret)
            .expect("OS RNG failed during ML-DSA signing — refusing to emit a partial bundle");
        let mut bundle = Vec::with_capacity(HYBRID_SIGNATURE_BYTES);
        bundle.extend_from_slice(&ed_sig);
        bundle.extend_from_slice(&mldsa_sig);
        bundle
    }

    fn verify(&self, message: &[u8], signature: &[u8]) -> bool {
        Self::verify_hybrid(message, signature, &self.public_keys())
    }
}

impl Drop for HybridSigner {
    /// Best-effort zeroization of both secret keys.
    ///
    /// `write_volatile` because a plain loop of ordinary stores over memory
    /// that is about to be freed is a textbook dead-store: the optimizer may
    /// delete it wholesale, and "we zeroize on drop" would then be one more
    /// claim the code does not keep.
    fn drop(&mut self) {
        for b in self.ed.expanded.iter_mut() {
            unsafe { core::ptr::write_volatile(b, 0) };
        }
        for b in self.mldsa_secret.iter_mut() {
            unsafe { core::ptr::write_volatile(b, 0) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_via_the_signer_trait() {
        let signer = HybridSigner::generate().expect("OS RNG available");
        let msg = b"evidencia bajo firma hibrida";
        let sig = signer.sign(msg);
        assert_eq!(sig.len(), HYBRID_SIGNATURE_BYTES, "64 + 3309 = 3373, fixed");
        assert!(signer.verify(msg, &sig));
        assert!(!signer.verify(b"otra evidencia", &sig));
    }

    /// 🎯 The bundle IS the paper's formula — each half, split at the fixed
    /// boundary, verifies INDEPENDENTLY against `SHA-256(message)` under its
    /// own primitive. A roundtrip cannot prove structure; this does.
    #[test]
    fn the_bundle_is_the_papers_formula() {
        let signer = HybridSigner::generate().expect("rng");
        let keys = signer.public_keys();
        let msg = b"estructura, no solo roundtrip";
        let sig = signer.sign(msg);

        let digest = axon_csys::crypto::sha256(msg);
        let (ed_half, mldsa_half) = sig.split_at(ED25519_SIGNATURE_BYTES);
        assert!(
            axon_csys::ed25519::verify(ed_half, &digest, &keys.ed25519),
            "the first 64 bytes must be Ed25519 over SHA-256(message)"
        );
        assert!(
            axon_csys::mldsa::verify(mldsa_half, &digest, &keys.ml_dsa_65),
            "the remaining 3309 bytes must be ML-DSA-65 over the same digest"
        );
    }

    /// 🎯 Hybrid means BOTH, not EITHER. A bundle with one valid and one
    /// corrupted half must fail — in each direction. This is the test that
    /// keeps the security claim from silently inverting: an either-half
    /// verifier is WEAKER than either scheme alone.
    #[test]
    fn hybrid_means_both_not_either() {
        let signer = HybridSigner::generate().expect("rng");
        let msg = b"ambas mitades o ninguna";
        let sig = signer.sign(msg);

        let mut ed_corrupted = sig.clone();
        ed_corrupted[0] ^= 1; // inside the Ed25519 half
        assert!(
            !signer.verify(msg, &ed_corrupted),
            "a valid ML-DSA half must NOT carry a corrupted Ed25519 half"
        );

        let mut mldsa_corrupted = sig.clone();
        mldsa_corrupted[ED25519_SIGNATURE_BYTES + 10] ^= 1; // inside the ML-DSA half
        assert!(
            !signer.verify(msg, &mldsa_corrupted),
            "a valid Ed25519 half must NOT carry a corrupted ML-DSA half"
        );
    }

    #[test]
    fn wrong_lengths_are_refused_before_any_cryptography() {
        let signer = HybridSigner::generate().expect("rng");
        let msg = b"largo exacto o nada";
        let sig = signer.sign(msg);
        assert!(!signer.verify(msg, &sig[..HYBRID_SIGNATURE_BYTES - 1]));
        assert!(!signer.verify(msg, &[]));
        assert!(!signer.verify(msg, &sig[..ED25519_SIGNATURE_BYTES]));
    }

    /// Key separation: two signers, two key sets, no cross-acceptance.
    #[test]
    fn distinct_signers_reject_each_others_bundles() {
        let a = HybridSigner::generate().expect("rng");
        let b = HybridSigner::generate().expect("rng");
        let msg = b"cada llave firma lo suyo";
        let sig_a = a.sign(msg);
        assert!(a.verify(msg, &sig_a));
        assert!(!b.verify(msg, &sig_a));
    }

    /// The algorithm label is part of the evidence chain's serialized record;
    /// pinned so a rename is a deliberate act, not a drive-by.
    #[test]
    fn the_algorithm_label_is_stable() {
        let signer = HybridSigner::generate().expect("rng");
        assert_eq!(signer.algorithm(), "Ed25519+ML-DSA-65");
    }
}
