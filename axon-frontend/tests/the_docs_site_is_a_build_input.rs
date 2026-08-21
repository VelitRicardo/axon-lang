//! The published documentation site is a BUILD INPUT, exactly like the README.
//!
//! `docs/` is the source of docs.ricardovelit.com. It is the first thing an
//! adopter reads and — increasingly — the thing an AI coding agent scrapes to
//! learn the language. A snippet there that does not compile is not a typo: it
//! is the compiler's own documentation teaching a program the compiler refuses.
//!
//! That failure mode has happened before on every other surface this repository
//! publishes, which is why each of them is now parsed at test time: the README's
//! fenced blocks, the `examples/` tree, the primitive corpus the MCP server
//! serves. This gate extends the same discipline to the site the moment it
//! exists, rather than after it has taught someone something false.
//!
//! Four laws:
//!
//! 1. **`docs.json` is valid and complete** — it parses, and it carries the four
//!    fields Mintlify requires (`name`, `theme`, `colors.primary`, `navigation`).
//!    A malformed one fails the deployment with a message that says nothing
//!    useful, so catching it here is strictly cheaper.
//! 2. **Every asset the manifest declares exists.** A declared favicon or logo
//!    that is not on disk does not fail Mintlify's build — it serves a broken
//!    icon, which is the kind of defect nobody files and everybody notices.
//! 3. **Every page the navigation names exists.** A dangling entry renders as a
//!    404 in the site's own sidebar — the worst kind of broken link, because the
//!    reader assumes the page was deleted on purpose.
//! 4. **Every ` ```axon ` block on the site compiles**, through the same
//!    `run_check` the `axon` CLI runs. No ledger of known-failing blocks: the
//!    site is new, so it starts clean and stays that way. If a block is meant to
//!    fail — showing a diagnostic — write the diagnostic in a plain fence, not
//!    an `axon` one, the way the quickstart's negative case does.

mod common;

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("axon-frontend always has a parent directory")
        .to_path_buf()
}

fn docs_dir() -> PathBuf {
    repo_root().join("docs")
}

/// Every `.mdx` / `.md` page under `docs/`, recursively.
fn site_pages() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for entry in rd {
            let p = entry.expect("dir entry").path();
            if p.is_dir() {
                walk(&p, out);
            } else if p
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e == "mdx" || e == "md")
            {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    walk(&docs_dir(), &mut out);
    out.sort();
    out
}

/// The contents of every ` ```axon ` fence in `markdown`, with the 1-based line
/// each one opens on so a failure points at the source instead of an index.
fn axon_blocks(markdown: &str) -> Vec<(usize, String)> {
    let mut blocks = Vec::new();
    let mut current: Option<(usize, Vec<&str>)> = None;
    for (i, line) in markdown.lines().enumerate() {
        match &mut current {
            None => {
                if line.trim() == "```axon" {
                    current = Some((i + 1, Vec::new()));
                }
            }
            Some((start, buf)) => {
                if line.trim_start().starts_with("```") {
                    blocks.push((*start, buf.join("\n")));
                    current = None;
                } else {
                    buf.push(line);
                }
            }
        }
    }
    blocks
}

/// Run the REAL check — the same entry point `axon check` calls.
fn check_source(src: &str, tag: &str) -> i32 {
    let dir = std::env::temp_dir().join("axon_docs_site_gate");
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let path = dir.join(format!("{tag}.axon"));
    std::fs::write(&path, src).expect("write scratch snippet");
    axon_frontend::checker::run_check(&path.to_string_lossy(), true, false, None)
}

/// A minimal JSON string-value reader — enough to assert on `docs.json`'s
/// required scalars without adding a dependency to a crate whose whole point is
/// that it has none.
fn json_has_key(src: &str, key: &str) -> bool {
    src.contains(&format!("\"{key}\""))
}

// ── 1. the manifest ─────────────────────────────────────────────────────────

#[test]
fn docs_json_exists_and_carries_what_mintlify_requires() {
    let manifest = docs_dir().join("docs.json");
    assert!(
        manifest.is_file(),
        "docs/docs.json is missing. Mintlify's deployment fails with 'Unable to find docs.json' \
         — a message that names the file but not the reason, so it is worth catching here."
    );
    let src = std::fs::read_to_string(&manifest).expect("docs.json is readable");

    // Parses at all. Serde is not a dependency of this crate, so the check is
    // structural: balanced braces and the required keys present. A syntax error
    // deeper than that is caught by the deployment, which is acceptable — the
    // expensive failure is a MISSING key, which renders a site with no
    // navigation and no error.
    let opens = src.matches('{').count();
    let closes = src.matches('}').count();
    assert_eq!(
        opens, closes,
        "docs/docs.json has unbalanced braces ({opens} open, {closes} close) — it will not parse."
    );

    for key in ["name", "theme", "colors", "navigation"] {
        assert!(
            json_has_key(&src, key),
            "docs/docs.json is missing the required `{key}` field. Mintlify requires \
             name, theme, colors.primary and navigation; without one the deployment \
             either fails or renders an empty shell."
        );
    }
    assert!(
        json_has_key(&src, "primary"),
        "docs/docs.json needs `colors.primary` — a hex brand colour."
    );
}

// ── 1b. the assets the manifest declares exist ──────────────────────────────

/// A declared asset that is not on disk is the same defect class as a dangling
/// navigation entry, and both were found the same way — by writing the gate and
/// running it. The first version of this rule read only a scalar
/// (`"favicon": "/favicon.svg"`) and skipped a themed object
/// (`"favicon": { "light": …, "dark": … }`) entirely. The web editor then wrote
/// exactly that shape, so the rule passed while checking nothing: a gate that is
/// correct on the syntax nobody uses any more.
///
/// It now collects EVERY path-looking string under each asset key, at any depth.
///
/// Paths are resolved against `docs/`, the documentation root — that is what
/// Mintlify's "docs.json is in a subdirectory" setting makes it. So a leading
/// `/` means "the docs root", not "the repository root": `/favicon.svg` is
/// `docs/favicon.svg`, and `/docs/images/x.png` would be `docs/docs/images/x.png`.
fn asset_paths(src: &str, key: &str) -> Vec<String> {
    let needle = format!("\"{key}\"");
    let Some(idx) = src.find(&needle) else { return Vec::new() };
    let rest = &src[idx + needle.len()..];
    // Scalar form: `"favicon": "…"`. Object form: `"favicon": { … }`.
    let colon = match rest.find(':') {
        Some(c) => c,
        None => return Vec::new(),
    };
    let after = rest[colon + 1..].trim_start();
    let body: &str = if after.starts_with('{') {
        let close = match after.find('}') {
            Some(c) => c,
            None => return Vec::new(),
        };
        &after[..close]
    } else {
        // one quoted scalar
        let Some(open) = after.find('"') else { return Vec::new() };
        let tail = &after[open + 1..];
        let Some(close) = tail.find('"') else { return Vec::new() };
        &tail[..close]
    };

    // Every quoted string in `body` that looks like a path, i.e. carries a file
    // extension. That skips the theme keys (`light`, `dark`) without having to
    // enumerate them, which is what let the previous version go blind.
    body.split('"')
        .filter(|t| {
            t.contains('.')
                && !t.starts_with("http")
                && t.rsplit('.').next().is_some_and(|e| {
                    matches!(e, "svg" | "png" | "jpg" | "jpeg" | "webp" | "ico" | "gif")
                })
        })
        .map(|t| t.to_string())
        .collect()
}

#[test]
fn every_asset_the_manifest_declares_exists() {
    let manifest = docs_dir().join("docs.json");
    let src = std::fs::read_to_string(&manifest).expect("docs.json is readable");

    let mut checked = 0usize;
    let mut missing = Vec::new();
    for key in ["favicon", "logo"] {
        for value in asset_paths(&src, key) {
            checked += 1;
            let rel = value.trim_start_matches('/');
            if !docs_dir().join(rel).is_file() {
                // The `/docs/…` shape is mechanically wrong and always the same
                // mistake: the Mintlify web editor writes paths it sees in the
                // REPOSITORY, while the site resolves them from `docs/` as its
                // root. Naming it beats making the next reader re-derive it —
                // this gate has now caught that exact form three times.
                let hint = if value.starts_with("/docs/") {
                    let fixed = value.replacen("/docs", "", 1);
                    format!(" — the `/docs/` prefix is the REPOSITORY path, not the site path. Write \"{fixed}\"")
                } else {
                    format!(" — expected the file at docs/{rel}")
                };
                missing.push(format!("  {key}: \"{value}\"{hint}"));
            }
        }
    }

    assert!(
        checked > 0,
        "docs.json declares no favicon or logo path — either the asset keys were removed, or \
         the parse missed their shape and this gate is checking nothing."
    );
    assert!(
        missing.is_empty(),
        "docs.json declares {} asset path(s) that are not on disk:\n{}\n\nPaths resolve \
         against `docs/` (the documentation root under Mintlify's subdirectory setting), so a \
         leading `/` is the docs root — not the repository root.

If docs.json was last \n         written by the Mintlify web editor, check `git log -1 -- docs/images/` first: \n         that editor's sync has removed asset files while leaving them declared here.",
        missing.len(),
        missing.join("\n")
    );
}

// ── 2. the navigation names pages that exist ────────────────────────────────

#[test]
fn every_page_the_navigation_names_exists() {
    let manifest = docs_dir().join("docs.json");
    let src = std::fs::read_to_string(&manifest).expect("docs.json is readable");

    // Page entries are the quoted strings inside `"pages": [ … ]`. Read them
    // positionally rather than with a JSON parser: this crate has no runtime
    // dependencies and that is a property worth keeping for one gate.
    let mut pages: Vec<String> = Vec::new();
    let mut rest = src.as_str();
    while let Some(idx) = rest.find("\"pages\"") {
        rest = &rest[idx + "\"pages\"".len()..];
        let Some(open) = rest.find('[') else { break };
        let Some(close) = rest[open..].find(']') else { break };
        let body = &rest[open + 1..open + close];
        for raw in body.split(',') {
            let entry = raw.trim().trim_matches('"').trim();
            if !entry.is_empty() && !entry.starts_with('{') {
                pages.push(entry.to_string());
            }
        }
        rest = &rest[open + close..];
    }

    assert!(
        !pages.is_empty(),
        "docs.json declares no pages — the navigation parse found nothing, so this gate would \
         be vacuous. Either the navigation is empty or its shape changed."
    );

    let mut missing = Vec::new();
    for page in &pages {
        let mdx = docs_dir().join(format!("{page}.mdx"));
        let md = docs_dir().join(format!("{page}.md"));
        if !mdx.is_file() && !md.is_file() {
            missing.push(format!("  {page} — expected docs/{page}.mdx"));
        }
    }
    assert!(
        missing.is_empty(),
        "docs.json's navigation names {} page(s) that do not exist. Each renders as a dead \
         entry in the site's own sidebar:\n{}",
        missing.len(),
        missing.join("\n")
    );
}

// ── 3. the snippets compile ─────────────────────────────────────────────────

#[test]
fn every_axon_snippet_on_the_site_compiles() {
    let pages = site_pages();
    assert!(
        !pages.is_empty(),
        "no pages found under docs/ — this gate would pass vacuously."
    );

    let mut failures = Vec::new();
    let mut checked = 0usize;
    for page in &pages {
        let src = std::fs::read_to_string(page).expect("page is readable");
        let rel = page.strip_prefix(repo_root()).unwrap_or(page).display().to_string();
        for (line, block) in axon_blocks(&src) {
            checked += 1;
            let tag = format!("{}_{line}", rel.replace(['/', '\\', '.', '-'], "_"));
            if check_source(&block, &tag) != 0 {
                failures.push(format!("  {rel}:{line}\n{}", indent(&block)));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {checked} `axon` block(s) on the documentation site do NOT compile. The site is \
         the first thing an adopter reads and what a coding agent scrapes; a snippet that does \
         not compile teaches a program the compiler refuses.\n\n{}\n\nIf a block is meant to \
         FAIL — showing a diagnostic — put it in a plain fence, not an `axon` one.",
        failures.len(),
        failures.join("\n\n")
    );
}

fn indent(s: &str) -> String {
    s.lines().map(|l| format!("      {l}")).collect::<Vec<_>>().join("\n")
}

// ── 5. every compliance label on the site is a member of Κ ──────────────────
//
// The site teaches `compliance [...]` annotations. Κ — `REGULATORY_CLASSES` —
// is CLOSED: `axon-T1214` rejects any label outside it. So a page that shows
// `compliance: [FedRAMP_Moderate]` is teaching a declaration the compiler will
// not accept, and the adopter finds out at their first `axon check`.
//
// This is not hypothetical. The knowledge corpus this site is built from
// carries four such labels across four of its nine framework guides —
// `FedRAMP_Moderate` (eleven times), `FedRAMP_Low`, `SOC2_A`, `SOC2_P` — all of
// them baseline or criterion levels, which Κ deliberately does not model. Those
// four guides are held back from the site until that is resolved one way or the
// other, and this law is what keeps them held: it fails the build rather than
// letting a later regeneration quietly publish them.
//
// Law 4 does not catch this. Every one of those blocks is a FRAGMENT — a bare
// `compliance: [...]` line is not a top-level declaration — so it carries a
// plain fence, and law 4 only compiles `axon` fences. The teaching is wrong in
// a block no compiler ever sees. Hence a law that reads the text.
//
// Κ is read from `axon_frontend::compliance::REGULATORY_CLASSES` — the array
// the type checker itself consults. There is no second copy of the list here to
// drift away from it.

#[test]
fn every_compliance_label_the_site_teaches_is_a_member_of_kappa() {
    let kappa = axon_frontend::compliance::REGULATORY_CLASSES;
    let mut hits = Vec::new();

    for page in site_pages() {
        let Ok(text) = std::fs::read_to_string(&page) else { continue };
        // A page may name a label OUTSIDE Κ in exactly one circumstance: to show
        // it being refused. `quickstart` teaches that `[HIPPA]` does not compile,
        // and the concepts page shows the same typo with the diagnostic under it
        // — that teaching is the whole point of a closed vocabulary, and a gate
        // that forbade it would forbid documenting its own guarantee.
        //
        // The exemption is deliberately narrow: the page must carry a line that
        // names BOTH `axon-T1214` and that exact label. Mentioning the code once
        // somewhere does not launder every label on the page, which is what
        // separates a counter-example from a guide that simply teaches the wrong
        // vocabulary — the four framework guides held out of `docs/` teach
        // `FedRAMP_Moderate` as valid and never show it refused, so they stay out.
        for (line, label) in common::offending_labels(&text, kappa) {
            {
                hits.push(format!(
                    "  {}:{} — `{}` is not a regulatory class",
                    page.strip_prefix(repo_root()).unwrap_or(&page).display(),
                    line,
                    label
                ));
            }
        }
    }

    assert!(
        hits.is_empty(),
        "the documentation site teaches a compliance label the compiler REFUSES. Κ is closed \
         and `axon-T1214` rejects anything outside it, so an adopter following this page writes \
         a program that does not compile.\n\n{}\n\nΚ is: {}\n\nIf the label names a baseline or \
         a criterion level rather than a framework, say so in prose and declare the framework. \
         If the framework genuinely belongs in Κ, that is a product decision with a written \
         justification behind it — make it there, not by relaxing this gate.",
        hits.join("\n"),
        kappa.join(", "),
    );
}

