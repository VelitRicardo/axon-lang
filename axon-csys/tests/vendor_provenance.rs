//! §Fase 124.b — **vendored cryptography is what its provenance says it is.**
//!
//! # Why this gate exists
//!
//! D124.3 put the entire cryptographic module boundary inside `axon-csys`,
//! because a FIPS 140-3 evaluation needs a declared boundary and splitting
//! crypto between C23 and Rust crates doubles the surface an assessor must
//! examine. D124.6 then settled that the boundary is populated with vetted
//! reference implementations rather than hand-written lattice arithmetic: the
//! boundary argument is about what is *inside* it, not about who typed the
//! code, and a from-scratch ML-DSA is the first thing a CMVP assessor
//! questions.
//!
//! That reasoning rests entirely on the vendored code **being** the upstream it
//! claims to be. Otherwise the argument inverts: the files look like an
//! implementation a reviewer trusts, carry its lineage in their headers, and
//! are something else. A local edit — a debugging `printf` left behind, a
//! "harmless" refactor, a malicious change — would be invisible to review
//! precisely because nobody re-reads vendored crypto.
//!
//! So `c-src/vendor/PROVENANCE.toml` pins a repository, a COMMIT, a license and
//! a SHA-256 per file, and this recomputes them. Vendoring without this gate
//! would open the supply-chain hole that a single boundary exists to close.
//!
//! # Why it parses the manifest instead of hardcoding hashes
//!
//! A hash written in the test and a hash written in the manifest are two copies
//! of one fact, and §122.e paid for a law written three times where the third
//! copy was the one that drifted. The manifest is the single source; this file
//! reads it.

use std::path::{Path, PathBuf};

fn vendor_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("c-src").join("vendor")
}

/// `(path, sha256, bytes)` for every file the manifest pins.
///
/// A deliberately small hand-rolled reader: pulling a TOML parser into
/// `dev-dependencies` to read four keys would add a dependency to the crate
/// whose entire purpose is to keep the cryptographic boundary small and
/// auditable.
fn pinned_files() -> Vec<(String, String, u64)> {
    let manifest = std::fs::read_to_string(vendor_dir().join("PROVENANCE.toml"))
        .expect("c-src/vendor/PROVENANCE.toml must exist — vendored code without recorded provenance is exactly what this gate refuses");
    let manifest = manifest.replace("\r\n", "\n");

    let mut out = Vec::new();
    let (mut path, mut sha, mut bytes) = (None, None, None);
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        if line == "[[upstream.file]]" {
            path = None;
            sha = None;
            bytes = None;
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim().trim_matches('"');
        match key {
            "path" => path = Some(value.to_string()),
            "sha256" => sha = Some(value.to_string()),
            "bytes" => bytes = value.parse::<u64>().ok(),
            _ => {}
        }
        if let (Some(p), Some(s), Some(b)) = (&path, &sha, bytes) {
            out.push((p.clone(), s.clone(), b));
            path = None;
            sha = None;
            bytes = None;
        }
    }
    out
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// 🎯 Every pinned file is byte-for-byte what the manifest says.
#[test]
fn vendored_files_match_their_recorded_hashes() {
    let files = pinned_files();
    assert!(
        !files.is_empty(),
        "PROVENANCE.toml parsed to zero files. The manifest exists but pins nothing, so this \
         gate would pass no matter what is in `c-src/vendor/` — the vacuous-pass shape this \
         project has now been bitten by twice. Either the format changed or the reader broke."
    );

    for (rel, expected, expected_bytes) in files {
        let path = vendor_dir().join(&rel);
        let content = std::fs::read(&path).unwrap_or_else(|e| {
            panic!("PROVENANCE.toml pins `{rel}`, which cannot be read: {e}")
        });

        assert_eq!(
            content.len() as u64,
            expected_bytes,
            "`{rel}` is {} bytes; the manifest records {expected_bytes}",
            content.len()
        );

        let actual = sha256_hex(&content);
        assert_eq!(
            actual, expected,
            "\n\n`{rel}` does not match its recorded SHA-256.\n\n\
             This is vendored cryptography inside the FIPS module boundary. If you edited it, \
             do not update the hash — upstream code is not ours to patch, and an edited copy \
             keeps the lineage in its header while no longer being the implementation that \
             lineage refers to. Send the fix upstream, or vendor a different implementation \
             and record it honestly.\n\n\
             If you REFRESHED the import, update `commit`, `imported` and every hash together, \
             and say in the commit message what moved upstream.\n"
        );
    }
}

/// Every file present under `c-src/vendor/` is accounted for by the manifest.
///
/// The hash check alone is one-directional: it proves the pinned files are
/// unmodified and says nothing about a file that was added and never recorded.
/// An unpinned `.c` inside the boundary is the same defect with the paperwork
/// skipped.
#[test]
fn no_vendored_file_is_unaccounted_for() {
    let pinned: Vec<String> = pinned_files()
        .into_iter()
        .map(|(p, _, _)| p.replace('\\', "/"))
        .collect();

    let mut stack = vec![vendor_dir()];
    let mut orphans = Vec::new();
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("vendor dir readable").flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            // The manifest itself is not vendored code.
            if name == "PROVENANCE.toml" {
                continue;
            }
            let rel = path
                .strip_prefix(vendor_dir())
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if !pinned.contains(&rel) {
                orphans.push(rel);
            }
        }
    }

    assert!(
        orphans.is_empty(),
        "these files live inside the cryptographic module boundary and are not recorded in \
         PROVENANCE.toml: {orphans:?}\n\n\
         Every third-party file in the CMB needs a repository, a commit, a license and a \
         hash. Code whose origin nobody wrote down is code nobody can audit."
    );
}
