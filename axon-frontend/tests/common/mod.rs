//! Shared predicates for the surfaces that teach AXON without being compiled.
//!
//! Two surfaces make regulatory claims in text a compiler never reads: the
//! documentation site under `docs/`, and the knowledge corpus the ℰMCP serves
//! to a coding agent. Both were found teaching `FedRAMP_Moderate`, a label
//! `axon-T1214` refuses, and the site gate and the agent-surface gate each need
//! the same question answered: *which labels does this text put in declarable
//! position, and are they real?*
//!
//! That question gets ONE definition, here. Two copies would be the failure
//! this repository has paid for before — a rule fixed behind one door and left
//! standing behind the other.

/// Every identifier this text puts in DECLARABLE position: inside a
/// `compliance [...]` or `compliance: [...]` list.
///
/// Scoped to the list on purpose. Prose that discusses FedRAMP as an assessment
/// programme is legitimate — the FISMA guide explains the baselines in exactly
/// those terms — and banning the word would be a gate that fights its own
/// documentation. What may not appear is the label in declarable position.
///
/// `<Tag1>` is a GRAMMAR METAVARIABLE, not a label. Every primitive page opens
/// with a grammar block written in that convention, so reading the placeholder
/// as a class would make the gate fire on the one part of the corpus that is
/// deliberately abstract.
pub fn compliance_labels(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let mut rest = line;
        while let Some(at) = rest.find("compliance") {
            rest = &rest[at + "compliance".len()..];
            let after = rest.trim_start();
            let after = after.strip_prefix(':').unwrap_or(after).trim_start();
            let Some(body) = after.strip_prefix('[') else { continue };
            let Some(close) = body.find(']') else { continue };
            let mut ident = String::new();
            let mut angled = false;
            let mut prev = '[';
            for c in body[..close].chars().chain(std::iter::once(',')) {
                if c.is_ascii_alphanumeric() || c == '_' {
                    if ident.is_empty() {
                        angled = prev == '<';
                    }
                    ident.push(c);
                } else {
                    if !ident.is_empty()
                        && !angled
                        && !ident.chars().next().unwrap().is_ascii_digit()
                    {
                        out.push((n + 1, std::mem::take(&mut ident)));
                    }
                    ident.clear();
                }
                prev = c;
            }
        }
    }
    out
}

/// May this document name `label` even though Κ refuses it?
///
/// Only to show it being REFUSED. `quickstart` teaches that `[HIPPA]` does not
/// compile, and the FedRAMP guide opens by saying `FedRAMP_Moderate` is not an
/// annotation — that teaching is the whole point of a closed vocabulary, and a
/// gate that forbade it would forbid documenting its own guarantee.
///
/// Deliberately narrow: the document must carry a line naming BOTH `axon-T1214`
/// and that exact label. Mentioning the code once somewhere does not launder
/// every label in the file, which is what separates a counter-example from a
/// guide that simply teaches the wrong vocabulary.
pub fn shown_as_refused(text: &str, label: &str) -> bool {
    text.lines()
        .any(|l| l.contains("axon-T1214") && l.contains(label))
}

/// The labels in `text` that Κ does not contain and that `text` never shows
/// being refused. Empty means the text is honest about the vocabulary.
pub fn offending_labels(text: &str, kappa: &[&str]) -> Vec<(usize, String)> {
    compliance_labels(text)
        .into_iter()
        .filter(|(_, label)| {
            !kappa.contains(&label.as_str()) && !shown_as_refused(text, label)
        })
        .collect()
}
