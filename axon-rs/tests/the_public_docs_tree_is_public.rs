//! `docs/` is public by definition. `documents/` is private by definition.
//! Neither is enforced by a `.gitignore` alone, because a `.gitignore` is
//! silent by construction — it never tells you it just hid your file, and it
//! never tells you it just stopped hiding one.
//!
//! # Why this test exists
//!
//! Until 2026-08-20 the internal material — cycle plans, vision drafts,
//! investor prep, adopter upgrade notes — lived under `docs/`, kept out of git
//! by `/docs/*` with an `!/docs/papers/` exception. Then `docs/` was freed for
//! the adopter documentation site and the internal tree moved to `documents/`.
//!
//! That swap **inverts the default**. Before, a new folder under `docs/` was
//! private unless someone whitelisted it. Now it is *published* unless someone
//! notices. And the muscle memory points the wrong way: a hundred-odd cycle
//! plans lived at `docs/fase/` for months, so the natural thing to type for the
//! next one is exactly the path that would publish it.
//!
//! A gitignore cannot warn about that. This test can, and does:
//!
//! 1. **Nothing internal-shaped may live under `docs/`.** Matched by name, on
//!    the shapes the internal tree actually uses.
//! 2. **`documents/` must stay ignored.** If the `/documents/` line is ever
//!    dropped from `.gitignore`, this fails — rather than the next `git add -A`
//!    quietly staging the investor deck.
//! 3. **`papers/` must stay tracked.** It is public on purpose: the README
//!    links it and a compiler gate reads one of its files at compile time
//!    (`include_str!`), so a stray ignore rule there breaks the build in a way
//!    that is confusing to diagnose.
//!
//! Rules 2 and 3 are asked of git itself (`git check-ignore`), not of a parsed
//! `.gitignore` — the question is "would git ignore this path", and only git
//! answers that correctly (negations, precedence, nested rules).

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("axon-rs has a parent")
        .to_path_buf()
}

/// Would git's ignore RULES hide `rel`? Asks git, the only correct oracle for
/// negations, precedence and nested rules.
///
/// `--no-index` is load-bearing and was found the hard way. Without it,
/// `check-ignore` consults the index first and reports any TRACKED path as
/// "not ignored" — so the mutation that adds `papers/` to `.gitignore` while
/// `papers/` is still tracked SURVIVED the first version of this test. The
/// rule would have been silently dead: correct today, blind to the exact
/// regression it exists to catch. The question this test must ask is "what do
/// the rules say", not "what is the index doing right now".
///
/// Exits 0 when the path IS ignored, 1 when it is not, and >1 on a real error —
/// which must not be read as "not ignored".
fn git_ignores(rel: &str) -> bool {
    let out = Command::new("git")
        .args(["check-ignore", "-q", "--no-index", rel])
        .current_dir(repo_root())
        .output()
        .expect("git check-ignore runs");
    match out.status.code() {
        Some(0) => true,
        Some(1) => false,
        other => panic!("git check-ignore failed on `{rel}` (exit {other:?}) — cannot decide, so refusing to pass"),
    }
}

/// Is `rel` tracked in the index?
fn git_tracks(rel: &str) -> bool {
    let out = Command::new("git")
        .args(["ls-files", "--error-unmatch", rel])
        .current_dir(repo_root())
        .output()
        .expect("git ls-files runs");
    out.status.success()
}

/// The shapes the internal tree uses. A file or directory under `docs/` whose
/// name matches any of these is internal material that would be published.
fn internal_shape(name: &str) -> Option<&'static str> {
    let n = name.to_ascii_lowercase();
    if n.starts_with("fase") || n.starts_with("phase_") {
        return Some("a cycle plan");
    }
    if n.starts_with("plan_vivo") || n.contains("_exit_review") || n.contains("_execution_backlog") {
        return Some("a living plan / exit review");
    }
    if n.starts_with("vision") || n.starts_with("presentation_") || n.contains("inversor") || n.contains("investor") {
        return Some("vision or investor material");
    }
    if n.starts_with("infra_debt") {
        return Some("an internal debt note");
    }
    None
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for entry in rd {
        let p = entry.expect("dir entry").path();
        let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();
        if matches!(name.as_str(), "node_modules" | ".git" | "target") {
            continue;
        }
        out.push(p.clone());
        if p.is_dir() {
            walk(&p, out);
        }
    }
}

// ── 1. nothing internal-shaped under the public docs tree ───────────────────

#[test]
fn no_internal_material_lives_under_the_public_docs_tree() {
    let docs = repo_root().join("docs");
    if !docs.exists() {
        // The site tree does not exist yet — nothing to police. The rule still
        // has to hold the day it appears, which is what the other tests pin.
        return;
    }
    let mut entries = Vec::new();
    walk(&docs, &mut entries);

    let mut hits = Vec::new();
    for p in entries {
        let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if let Some(kind) = internal_shape(name) {
            hits.push(format!(
                "  {} — {kind}",
                p.strip_prefix(repo_root()).unwrap_or(&p).display()
            ));
        }
    }

    assert!(
        hits.is_empty(),
        "internal material found under `docs/`, which is PUBLIC — it would be committed and \
         published to the adopter documentation site.\n\n{}\n\nInternal material belongs in \
         `documents/` (ignored). If one of these is genuinely public, rename it so it does not \
         wear an internal shape.",
        hits.join("\n")
    );
}

// ── 2. the private tree stays private ───────────────────────────────────────

#[test]
fn the_documents_tree_is_ignored_by_git() {
    // Probed with paths that need not exist: `check-ignore` answers about the
    // RULE, so the test holds even on a checkout where the internal tree is
    // absent (a fresh clone has no `documents/` at all — it is never committed).
    for rel in [
        "documents/fase/fase_999_probe.md",
        "documents/vision/anything.md",
        "documents/presentation_inversor.md",
        "documents/compliance/whatever.md",
    ] {
        assert!(
            git_ignores(rel),
            "`{rel}` is NOT ignored by git. The `/documents/` line in .gitignore is the only \
             thing keeping cycle plans, vision drafts and investor prep out of the repository; \
             without it the next `git add -A` stages all of it."
        );
    }
}

// ── 3. the public papers stay public ────────────────────────────────────────

#[test]
fn the_papers_tree_is_tracked_and_not_ignored() {
    assert!(
        !git_ignores("papers/paper_compliance.md"),
        "`papers/` is ignored by git. It is public on purpose: the README links it and \
         `axon-frontend/tests/the_paper_matches_the_compiler.rs` reads one of its files with \
         `include_str!` at COMPILE time — ignoring it breaks the build with a confusing \
         'couldn't read' error rather than a missing-file one."
    );
    assert!(
        git_tracks("papers/paper_compliance.md"),
        "`papers/paper_compliance.md` is not tracked, but a compiler gate includes it at build time."
    );
}

/// The compile-time include is the reason rule 3 is not cosmetic. If the papers
/// move again, this constant stops compiling — here, next to the rule that
/// explains why.
#[test]
fn the_compliance_paper_is_where_the_gate_expects_it() {
    let p = repo_root().join("papers").join("paper_compliance.md");
    assert!(
        p.is_file(),
        "`papers/paper_compliance.md` is missing; `the_paper_matches_the_compiler.rs` \
         includes it at compile time and will fail to build."
    );
}

// ── 5. no published page names a path on somebody's machine ─────────────────
//
// This law exists because the generator that produced the examples pages leaked
// one. It quoted a compiler diagnostic verbatim, and the diagnostic ended with
// `: searched C:\Users\<name>\AppData\Local\Temp\…\axon\security.axon` — the
// author's home directory, on its way to a public documentation site.
//
// The class is wider than that one generator: any page that pastes tool output,
// a stack trace, a build log or a shell transcript can carry an absolute path
// out of the machine that produced it. A path like that is never useful to a
// reader — it does not exist on their disk — and it discloses a username and a
// directory layout. So the rule is flat: the public tree names no local path.
//
// What is NOT banned: repository-relative paths (`docs/examples/…`), URLs, and
// the `/images/…` site paths the manifest uses. Those are the paths a reader
// can act on.

/// A path rooted on a specific machine, as opposed to a path relative to a
/// repository or a URL. Each pattern is anchored on the ROOT, so a relative
/// mention (`home/config`) does not match.
fn machine_path(line: &str) -> Option<&'static str> {
    // Windows: a drive letter followed by a separator.
    let bytes = line.as_bytes();
    for i in 0..bytes.len() {
        let c = bytes[i] as char;
        if c.is_ascii_alphabetic()
            && i + 2 < bytes.len()
            && bytes[i + 1] == b':'
            && (bytes[i + 2] == b'\\' || bytes[i + 2] == b'/')
        {
            // `C:\Users\…` — but not a URL scheme, which has no single letter.
            let before = if i == 0 { ' ' } else { bytes[i - 1] as char };
            if !before.is_ascii_alphanumeric() {
                return Some("a Windows drive path");
            }
        }
    }
    for root in ["/Users/", "/home/", "/root/", "/var/folders/", "/private/var/"] {
        if line.contains(root) {
            return Some("a Unix home or temp path");
        }
    }
    if line.contains("AppData") || line.contains("%USERPROFILE%") {
        return Some("a Windows profile directory");
    }
    None
}

#[test]
fn no_published_page_names_a_path_on_somebodys_machine() {
    let docs = repo_root().join("docs");
    if !docs.exists() {
        return;
    }
    let mut entries = Vec::new();
    walk(&docs, &mut entries);

    let mut hits = Vec::new();
    for p in entries {
        if p.is_dir() {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&p) else { continue };
        for (n, line) in text.lines().enumerate() {
            if let Some(kind) = machine_path(line) {
                hits.push(format!(
                    "  {}:{} — {kind}\n      {}",
                    p.strip_prefix(repo_root()).unwrap_or(&p).display(),
                    n + 1,
                    line.trim(),
                ));
            }
        }
    }

    assert!(
        hits.is_empty(),
        "a page under `docs/` names a path on the machine that WROTE it. `docs/` is published \
         to the adopter documentation site, so this discloses a username and a directory layout, \
         and the path means nothing on the reader's disk.\n\n{}\n\nUsually the cause is pasted \
         tool output: quote the diagnostic, not the file it searched.",
        hits.join("\n")
    );
}

#[test]
fn machine_paths_are_recognised_and_repository_paths_are_not() {
    // caught
    assert!(machine_path(r"searched C:\Users\home\AppData\Local\Temp\b.axon").is_some());
    assert!(machine_path("/Users/rv/src/axon/main.axon").is_some());
    assert!(machine_path("/home/runner/work/axon/axon").is_some());
    assert!(machine_path("/var/folders/xy/T/tmp123").is_some());

    // allowed — these are the paths a reader can actually use
    assert_eq!(machine_path("see docs/examples/module_imports.mdx"), None);
    assert_eq!(machine_path("![logo](/images/logoaxonlang.svg)"), None);
    assert_eq!(machine_path("https://docs.ricardovelit.com/quickstart"), None);
    assert_eq!(machine_path("run `axon check src/main.axon`"), None);
    assert_eq!(machine_path("the axon/security.axon module"), None);
}
