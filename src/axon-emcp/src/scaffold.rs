//! `axon-emcp scaffold-primitive <name>` — stamp a markdown skeleton
//! for one primitive's documentation, with frontmatter pre-populated
//! from the canonical [`PRIMITIVE_REGISTRY`].
//!
//! [`PRIMITIVE_REGISTRY`]: axon_frontend::PRIMITIVE_REGISTRY
//!
//! # Why this exists
//!
//! The v1.2.0 plan calls for 40 new primitive docs (Tier 1–3) to
//! land in 6.b/c/d. Without tooling, each doc starts from copy-paste
//! drift: a contributor copies `persona.md`, edits the name, the
//! category, the summary, the body — and inevitably forgets to flip
//! a YAML field or rename a heading. Drift bugs from copy-paste are
//! the most common quality regression in any doc corpus.
//!
//! The scaffold CLI removes that surface entirely. The registry is
//! the single source of truth for `name`, `category`, `top_level`,
//! `since`, and `summary`; the CLI reads it and emits a frontmatter
//! block that is correct by construction. The contributor only
//! writes the prose body.
//!
//! # Discipline
//!
//! 1. The registry entry must exist BEFORE running scaffold —
//! "registry first, doc second" is the v1.2.0 discipline.
//! 2. Scaffold refuses to overwrite an existing `.md` (so a
//!    contributor cannot accidentally clobber a documented
//!    primitive by typo-ing a name).
//! 3. After running scaffold and filling the body, the contributor
//!    must flip `doc_status: Pending` to `Documented` in the
//!    registry. The coverage gate test in `knowledge.rs` enforces
//!    this — un-flipped entries fail the gate.

use std::path::{Path, PathBuf};

use axon_frontend::{find_primitive, PrimitiveInfo};

/// Run the `scaffold-primitive <name>` subcommand. Returns an
/// `Ok(message)` to print on success, `Err(message)` on failure.
/// Pure (apart from the single `fs::write` call) so the surface is
/// testable from a temp dir.
pub fn run(name: &str, knowledge_dir: &Path) -> Result<String, String> {
    let info = find_primitive(name).ok_or_else(|| {
        format!(
            "unknown primitive `{name}` — not in axon_frontend::PRIMITIVE_REGISTRY.\n\
             If this is a NEW primitive, the v1.2.0 discipline is registry-first:\n\
             1. Add an entry to `axon-frontend/src/primitive_registry.rs` with \
                `doc_status: Pending`.\n\
             2. Re-run this command — it will stamp the skeleton.\n\
             3. Fill in the body + flip the registry entry to `Documented`.\n\
             4. The coverage gate test then passes."
        )
    })?;

    let primitives_dir = knowledge_dir.join("primitives");
    if !primitives_dir.is_dir() {
        return Err(format!(
            "primitives directory not found at `{}` — run the scaffold from \
             the repo root, or set AXON_EMCP_KNOWLEDGE_DIR to point to the corpus.",
            primitives_dir.display()
        ));
    }

    let target = primitives_dir.join(format!("{name}.md"));
    if target.exists() {
        return Err(format!(
            "doc already exists at `{}` — refusing to overwrite. \
             Edit the existing file in place (and flip its registry entry \
             to `Documented` if it isn't already).",
            target.display()
        ));
    }

    let content = render_skeleton(info);
    std::fs::write(&target, content).map_err(|e| {
        format!("failed to write `{}`: {e}", target.display())
    })?;

    Ok(format!(
        "✓ Scaffolded primitive `{}` at `{}`\n\
         \n\
         Next steps:\n\
         1. Fill in the body sections (Surface / Fields / Runtime behaviour / \
            What this primitive is NOT / See also). The frontmatter is correct \
            by construction — do not edit the YAML.\n\
         2. Author a canonical `.axon` example in a Phase 6 integration test \
            (see `src/axon-emcp/tests/phase2_canonical_programs.rs` for the \
            pattern) so the live `axon-frontend` parser proves the example.\n\
         3. Flip `doc_status: Pending` → `doc_status: Documented` for `{}` \
            in `axon-frontend/src/primitive_registry.rs`.\n\
         4. Run `cargo test --manifest-path src/axon-emcp/Cargo.toml` — the \
            coverage gate should now pass.",
        info.name,
        target.display(),
        info.name,
    ))
}

/// Render the markdown skeleton from a [`PrimitiveInfo`] entry. The
/// frontmatter is correct by construction; the prose body has
/// section headers with `TODO` markers that the contributor fills.
///
/// The skeleton mirrors the canonical structure of every existing
/// primitive doc (`persona.md`, `flow.md`, `socket.md`, …): Surface
/// → Fields → Runtime behaviour → What this primitive is NOT → See
/// also. A contributor who follows the skeleton produces a doc that
/// looks like the rest of the corpus by default.
fn render_skeleton(info: &PrimitiveInfo) -> String {
    let polarity_label = if info.top_level { "top-level" } else { "nested" };
    let polarity_paragraph = if info.top_level {
        format!(
            "`{}` is a **top-level declaration**. It is *not* nested \
             inside another construct.",
            info.name
        )
    } else {
        format!(
            "`{}` is **nested** — it appears inside another construct \
             (see the parent primitive's doc for context). It is *not* \
             a top-level declaration.",
            info.name
        )
    };

    format!(
        r#"---
name: {name}
summary: {summary}
category: {category}
top_level: {top_level}
since: {since}
grammar: |
  # TODO: replace this stub with the precise EBNF fragment for `{name}`.
  # Mirror the style of `persona.md` / `flow.md` / `socket.md` — show the
  # required fields first, optional fields with `# optional` comments.
  {name} <Name> {{
      # TODO: fields
  }}
---

# `{name}`

TODO: 2–3 sentence introduction. What is this primitive? Why does it
exist? What problem does it solve that the surrounding primitives do
not? Reference the introducing cycle (`{since}`) and the paper / cycle
if there is one.

## Surface

{polarity_paragraph}

```axon
# TODO: replace with a minimal canonical example that compiles end-to-
# end through `axon.check`. Add the matching `assert_template_compiles`
# integration test under `src/axon-emcp/tests/`.
{name} Example {{
    # TODO
}}
```

## Fields

TODO: list each field with its type, requiredness, semantic meaning,
and closed-catalogue values where applicable. Mirror the layout of
`anchor.md` / `tool.md` — one `### \`field:\`` subheading per field,
each with a short body explaining what the compiler enforces.

### `<field>:` (required | optional)

TODO.

## Runtime behaviour

TODO: describe what happens at deploy time + at runtime. Quote the
specific IR node type + the runtime hook (e.g. "lowered to
`<Name>IRNode`; the runtime injects it into …"). Mention any audit
hash-chain rows the primitive generates if it has runtime effects.

## What this primitive is NOT

TODO: 2–4 honest anti-pattern statements. Mirror `persona.md` —
explicit "this is not a system prompt", "this is not a model
selector", etc. Helps the agent avoid confusing this primitive with
adjacent ones.

- **Not a TODO.** TODO.
- **Not a TODO.** TODO.

## See also

- `axon://primitives/<related>` — TODO.
- `axon://primitives/<related>` — TODO.
- `axon://logic/<related>` — TODO.
- `axon://compliance/<framework>` — TODO (if the primitive touches a
  compliance surface).
"#,
        name = info.name,
        summary = info.summary,
        category = info.category,
        top_level = info.top_level,
        since = info.since,
        polarity_paragraph = polarity_paragraph,
    )
    .replace("(polarity_label)", polarity_label) // unused — kept for future
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Throwaway temp dir, no `tempfile` dep — keeps the dependency
    /// surface minimal. Same pattern as the catalog test helpers.
    fn tempdir(label: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "axon-emcp-scaffoldtest-{}-{n}-{label}",
            std::process::id(),
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(path.join("primitives")).unwrap();
        path
    }

    #[test]
    fn rejects_unknown_primitive_name() {
        let dir = tempdir("unknown");
        let err = run("does_not_exist", &dir).expect_err("must reject");
        assert!(err.contains("unknown primitive"));
        assert!(err.contains("axon_frontend::PRIMITIVE_REGISTRY"));
    }

    #[test]
    fn refuses_to_overwrite_existing_doc() {
        let dir = tempdir("overwrite");
        // Pre-create the file so the scaffold sees it as existing.
        let existing = dir.join("primitives").join("persona.md");
        fs::write(&existing, "EXISTING CONTENT").unwrap();
        let err = run("persona", &dir).expect_err("must refuse overwrite");
        assert!(err.contains("already exists"));
        assert!(err.contains("refusing to overwrite"));
        // Belt + braces: the existing content was not clobbered.
        assert_eq!(
            fs::read_to_string(&existing).unwrap(),
            "EXISTING CONTENT"
        );
    }

    #[test]
    fn rejects_when_primitives_dir_is_missing() {
        // Build a knowledge dir WITHOUT primitives/. Scaffold should
        // refuse rather than create the directory implicitly — the
        // discipline is "run from the repo root".
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "axon-emcp-scaffoldtest-noprims-{}-{n}",
            std::process::id(),
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let err = run("persona", &dir).expect_err("must reject missing dir");
        assert!(err.contains("primitives directory not found"));
    }

    #[test]
    fn writes_a_skeleton_with_correct_frontmatter() {
        let dir = tempdir("write");
        // Use `axonendpoint` — a Pending entry. Real registry data,
        // so any drift in PrimitiveInfo serialisation surfaces here.
        let msg = run("axonendpoint", &dir).expect("scaffold should succeed");
        let target = dir.join("primitives").join("axonendpoint.md");
        assert!(target.exists(), "scaffold did not create the file");
        assert!(msg.contains("Scaffolded primitive `axonendpoint`"));
        let body = fs::read_to_string(&target).unwrap();

        // Frontmatter carries the registry-truthful fields verbatim.
        assert!(body.contains("name: axonendpoint"));
        assert!(body.contains("category: wire"));
        assert!(body.contains("top_level: true"));
        assert!(body.contains("since: v1.23.0"));
        // The body's H1 header matches the slug.
        assert!(body.contains("# `axonendpoint`"));
        // Skeleton sections are present.
        assert!(body.contains("## Surface"));
        assert!(body.contains("## Fields"));
        assert!(body.contains("## Runtime behaviour"));
        assert!(body.contains("## What this primitive is NOT"));
        assert!(body.contains("## See also"));
        // TODO markers are explicit so the contributor cannot miss them.
        assert!(body.contains("TODO"));
    }

    #[test]
    fn skeleton_for_nested_primitive_uses_nested_polarity_paragraph() {
        // v2.67.0 — this used to scaffold `transact`, which has been RETRACTED
        // (axon-T938: it never opened a transaction) and is therefore no longer
        // in PRIMITIVE_REGISTRY. `forge` is a live nested primitive and serves
        // the same purpose here: the test is about the `top_level: false`
        // polarity paragraph, not about which primitive carries it.
        let dir = tempdir("nested");
        let _ = run("forge", &dir).expect("scaffold should succeed");
        let body = fs::read_to_string(dir.join("primitives").join("forge.md")).unwrap();
        // Polarity paragraph reflects the registry's `top_level: false`.
        // The nested wording is "is **nested**"; the top-level wording is
        // "is a **top-level declaration**". The nested paragraph DOES
        // mention "top-level declaration" once (negated — "It is *not* a
        // top-level declaration"), so we pin the assertion to the
        // primary marker that distinguishes the two arms.
        assert!(body.contains("top_level: false"));
        assert!(body.contains("`forge` is **nested**"));
        assert!(
            !body.contains("is a **top-level declaration**"),
            "nested polarity arm must not use the top-level affirmative wording"
        );
    }

    #[test]
    fn skeleton_for_top_level_primitive_uses_top_level_polarity_paragraph() {
        let dir = tempdir("toplvl");
        let _ = run("axonstore", &dir).expect("scaffold should succeed");
        let body = fs::read_to_string(dir.join("primitives").join("axonstore.md")).unwrap();
        assert!(body.contains("top_level: true"));
        assert!(body.contains("**top-level declaration**"));
    }

    #[test]
    fn render_skeleton_substitutes_every_registry_field() {
        // Pure function exercise — no I/O, no temp dir. Locks the
        // template's placeholder-substitution contract.
        let info = find_primitive("daemon").expect("daemon must be in registry");
        let s = render_skeleton(info);
        assert!(s.contains("name: daemon"));
        assert!(s.contains("category: wire"));
        assert!(s.contains("top_level: true"));
        assert!(s.contains("since: v1.11.0"));
        // The skeleton must NOT carry an unrendered template token.
        assert!(!s.contains("{name}"));
        assert!(!s.contains("{summary}"));
        assert!(!s.contains("{category}"));
    }
}
