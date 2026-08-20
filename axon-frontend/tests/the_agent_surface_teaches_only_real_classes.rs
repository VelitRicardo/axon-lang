//! What the ℰMCP tells a coding agent must be true of the compiler.
//!
//! `axon-emcp` is the channel through which an AI coding agent learns AXON: a
//! baked knowledge corpus plus the structured metadata `axon.compose` returns
//! when it scaffolds a project. None of it is compiled. A human reading a stale
//! README suspects; **an agent does not suspect** — it writes what it was told,
//! and the compiler refuses the result while the language takes the blame.
//!
//! That is not hypothetical. The corpus taught `FedRAMP_Moderate` eleven times
//! across three framework guides, and `compose.rs` returned it in the
//! `compliance_applied` array for the Government domain — the field an agent
//! reads to learn which classes its new program asserts. `axon-T1214` refuses
//! that label. Nothing noticed, because the crate has been pinned two majors
//! behind the compiler and sits outside CI.
//!
//! So the check lives HERE, in a crate that builds and a suite that runs, and
//! reads the ℰMCP's files as text. A gate in a crate that does not compile is
//! not a gate.
//!
//! Two laws:
//!
//! 1. **The corpus teaches only real classes** — every label the knowledge
//!    documents put in declarable position is a member of Κ.
//! 2. **The compose tool reports only real classes** — every string in a
//!    `compliance_applied:` array is a member of Κ. This one is stricter: it is
//!    machine-readable output, not prose, so there is no counter-example
//!    exemption. A tool result has no room to say "this one is refused".

mod common;

use axon_frontend::compliance::REGULATORY_CLASSES;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("axon-frontend has a parent")
        .to_path_buf()
}

fn emcp() -> PathBuf {
    repo_root().join("src").join("axon-emcp")
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for entry in rd {
        let p = entry.expect("dir entry").path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().and_then(|s| s.to_str()) == Some("md") {
            out.push(p);
        }
    }
}

// ── 1. the corpus teaches only real classes ─────────────────────────────────

#[test]
fn the_knowledge_corpus_teaches_only_real_regulatory_classes() {
    let knowledge = emcp().join("knowledge");
    assert!(
        knowledge.is_dir(),
        "the ℰMCP knowledge corpus is missing at {} — this gate is checking nothing",
        knowledge.display()
    );

    let mut docs = Vec::new();
    walk(&knowledge, &mut docs);
    assert!(
        docs.len() > 100,
        "only {} knowledge documents found — the corpus moved and this gate is now \
         checking a fraction of it",
        docs.len()
    );

    let mut hits = Vec::new();
    for doc in &docs {
        let Ok(text) = std::fs::read_to_string(doc) else { continue };
        for (line, label) in common::offending_labels(&text, REGULATORY_CLASSES) {
            hits.push(format!(
                "  {}:{} — `{}`",
                doc.strip_prefix(repo_root()).unwrap_or(doc).display(),
                line,
                label
            ));
        }
    }

    assert!(
        hits.is_empty(),
        "the ℰMCP corpus teaches a compliance label the compiler REFUSES. An agent that reads \
         this writes a program `axon-T1214` rejects, and the adopter blames the language.\
         \n\n{}\n\nΚ is: {}\n\nIf the label names a baseline or a criterion level rather than a \
         framework, say so in prose and declare the framework.",
        hits.join("\n"),
        REGULATORY_CLASSES.join(", "),
    );
}

// ── 2. the compose tool reports only real classes ───────────────────────────

/// Every string literal inside a `compliance_applied: &[...]` array.
fn compliance_applied(src: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (n, line) in src.lines().enumerate() {
        let Some(at) = line.find("compliance_applied") else { continue };
        let rest = &line[at..];
        let Some(open) = rest.find('[') else { continue };
        let Some(close) = rest[open..].find(']') else { continue };
        let body = &rest[open + 1..open + close];
        let mut chars = body.chars().peekable();
        let mut current: Option<String> = None;
        while let Some(c) = chars.next() {
            match (c, &mut current) {
                ('"', None) => current = Some(String::new()),
                ('"', Some(_)) => out.push((n + 1, current.take().expect("open literal"))),
                (c, Some(buf)) => buf.push(c),
                _ => {}
            }
        }
    }
    out
}

#[test]
fn the_compose_tool_reports_only_real_regulatory_classes() {
    let path = emcp().join("src").join("compose.rs");
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("compose.rs must be readable at {}: {e}", path.display()));

    let found = compliance_applied(&src);
    assert!(
        found.len() >= 20,
        "only {} labels found across `compliance_applied:` arrays — the field was renamed or \
         reshaped and this gate is now reading nothing",
        found.len()
    );

    let bad: Vec<String> = found
        .iter()
        .filter(|(_, l)| !REGULATORY_CLASSES.contains(&l.as_str()))
        .map(|(n, l)| format!("  compose.rs:{n} — `{l}`"))
        .collect();

    assert!(
        bad.is_empty(),
        "`axon.compose` reports a regulatory class the compiler REFUSES. This is not prose: it \
         is the structured result an agent reads to learn which classes its new program asserts, \
         so the agent writes the label into source and `axon-T1214` rejects it.\n\n{}\n\nΚ is: {}",
        bad.join("\n"),
        REGULATORY_CLASSES.join(", "),
    );
}

#[test]
fn compliance_applied_is_read_from_the_array_and_nothing_else() {
    assert_eq!(
        compliance_applied(r#"    compliance_applied: &["FISMA", "SOC2"],"#),
        vec![(1, "FISMA".into()), (1, "SOC2".into())]
    );
    // a neighbouring field with strings is not this field
    assert!(compliance_applied(r#"    keywords: &["fisma", "fedramp"],"#).is_empty());
    // prose naming the field is not the field
    assert!(compliance_applied("/// the compliance_applied list is returned to the agent").is_empty());
    // the line number is the reader's line
    assert_eq!(
        compliance_applied("a\nb\ncompliance_applied: &[\"GDPR\"]"),
        vec![(3, "GDPR".into())]
    );
}

// ── the shared predicate's own tests ────────────────────────────────────────
//
// `common::compliance_labels` is read by BOTH this gate and the documentation
// site gate. It gets its self-tests here, once, rather than in each caller.

#[test]
fn compliance_labels_are_read_from_declarable_position_only() {
    // declarable — both spellings the grammar accepts
    assert_eq!(
        common::compliance_labels("    compliance: [FISMA, NIST_800_53]"),
        vec![(1, "FISMA".into()), (1, "NIST_800_53".into())]
    );
    assert_eq!(
        common::compliance_labels("type Profile compliance [GDPR] {"),
        vec![(1, "GDPR".into())]
    );

    // prose about a framework is not a declaration and must not be read as one
    assert!(common::compliance_labels("FISMA travels with a FedRAMP baseline.").is_empty());
    assert!(
        common::compliance_labels("| **Moderate** | Serious effect | FedRAMP Moderate |")
            .is_empty()
    );
    assert!(
        common::compliance_labels("the compliance annotation is checked at compile time")
            .is_empty()
    );

    // grammar metavariables are placeholders, not labels
    assert!(common::compliance_labels("    compliance: [<Tag1>, ...]").is_empty());
    assert!(common::compliance_labels("[compliance [<Tag1>, <Tag2>, ...]]").is_empty());
    assert_eq!(
        common::compliance_labels("compliance: [<Tag1>, HIPAA]"),
        vec![(1, "HIPAA".into())]
    );

    // the line number is the reader's line, not an offset
    assert_eq!(
        common::compliance_labels("a\nb\n  compliance: [SOX]"),
        vec![(3, "SOX".into())]
    );
}

#[test]
fn the_refusal_exemption_is_narrow() {
    let kappa = &["HIPAA", "GDPR"];

    // a page that SHOWS the label refused may name it
    let teaching = "axon-T1214 rejects `HIPPA`, which is not a regulatory class.\n\
                    compliance: [HIPPA]";
    assert!(common::offending_labels(teaching, kappa).is_empty());

    // naming the code elsewhere does NOT launder an unrelated label
    let laundering = "The vocabulary is closed (axon-T1214).\n\
                      compliance: [FedRAMP_Moderate]";
    assert_eq!(common::offending_labels(laundering, kappa).len(), 1);

    // and a guide that never mentions the code at all is caught
    let guide = "compliance: [FISMA, NIST_800_53, FedRAMP_Moderate]";
    assert_eq!(common::offending_labels(guide, kappa).len(), 3);
}
