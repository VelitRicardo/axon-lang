//! v4.5.0 — the language can say WHAT a field is, not only which regime it
//! falls under.
//!
//! κ answers *which regulation applies*. It does not answer *is this a
//! social-security number or a clinical note*, and without the second answer no
//! claim of de-identification can be checked: a rule that sees only the regime
//! can verify that a class was declared, and nothing at all about what was
//! actually removed. That gap is why the previous cycle could only promise "no
//! field carrying the class survives", which says something about a label and
//! nothing about the content.
//!
//! # Why the catalogue is transversal
//!
//! A social-security number is an identifier under HIPAA, under GDPR and under
//! Ley 1581. What differs between regimes is what each requires be DONE with
//! it. So the kinds describe the data once, and each regime brings its own rule
//! table over them — `HIPAA_SAFE_HARBOR` is the first, and the four LATAM
//! jurisdictions reuse the same seventeen classes rather than restating them.
//!
//! # Why Safe Harbor is decidable at all
//!
//! 45 CFR 164.514(b)(2) is a CLOSED LIST of eighteen identifier classes.
//! Seventeen are enumerable and are encoded here. The eighteenth — *"any other
//! unique identifying number, characteristic, or code"* — is a judgement rather
//! than a class, and the regulation's final clause, that the covered entity
//! have no *actual knowledge* the data could identify someone, is a condition
//! on the ENTITY. Those two are attested, not proved. Everything else is
//! decided.

use axon_frontend::compliance::{
    is_known_identifier, safe_harbor_rule, transitive_identifiers, Deidentify, Generalisation,
    HIPAA_SAFE_HARBOR, IDENTIFIER_KINDS,
};
use axon_frontend::lexer::Lexer;
use axon_frontend::parser::Parser;
use axon_frontend::type_checker::TypeChecker;

fn diagnostics(src: &str) -> Vec<String> {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    let program = Parser::new(tokens).parse().expect("parse");
    TypeChecker::new(&program)
        .check()
        .into_iter()
        .map(|e| e.message)
        .collect()
}

fn program(src: &str) -> axon_frontend::ast::Program {
    let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
    Parser::new(tokens).parse().expect("parse")
}

// ── the clause reaches the declaration ──────────────────────────────────────

#[test]
fn a_type_can_declare_what_it_is() {
    let p = program("type MRN identifier medical_record_number { value: String }\n");
    let t = p
        .declarations
        .iter()
        .find_map(|d| match d {
            axon_frontend::ast::Declaration::Type(t) => Some(t),
            _ => None,
        })
        .expect("the type must parse");
    assert_eq!(t.identifier, "medical_record_number");
}

#[test]
fn the_two_clauses_compose_in_either_order() {
    // `compliance` and `identifier` answer different questions about the same
    // declaration — which regime, and which kind of thing. Requiring a fixed
    // order would be a rule an adopter has to remember for no reason.
    for src in [
        "type SSN identifier ssn compliance [HIPAA] { value: String }\n",
        "type SSN compliance [HIPAA] identifier ssn { value: String }\n",
    ] {
        let p = program(src);
        let t = p
            .declarations
            .iter()
            .find_map(|d| match d {
                axon_frontend::ast::Declaration::Type(t) => Some(t),
                _ => None,
            })
            .expect("parses");
        assert_eq!(t.identifier, "ssn", "in: {src}");
        assert_eq!(t.compliance, vec!["HIPAA".to_string()], "in: {src}");
    }
}

// ── the catalogue is closed ─────────────────────────────────────────────────

#[test]
fn an_identifier_kind_outside_the_catalogue_is_refused() {
    let hits: Vec<String> = diagnostics("type Weird identifier social_security { v: String }\n")
        .into_iter()
        .filter(|m| m.contains("axon-T1225"))
        .collect();
    assert_eq!(hits.len(), 1, "an unknown kind must be refused: {hits:#?}");
    assert!(
        hits[0].contains("ssn"),
        "the diagnostic must suggest the catalogue member — someone who wrote \
         `social_security` needs the spelling, not a rejection: {}",
        hits[0]
    );
}

#[test]
fn an_ordinary_type_pays_nothing() {
    // Most types are not identifiers. A rule that made every declaration answer
    // this question would be a tax, and a tax gets routed around.
    for src in [
        "type Note { text: String }\n",
        "type Record compliance [HIPAA] { note: String }\n",
    ] {
        assert!(
            diagnostics(src).iter().all(|m| !m.contains("axon-T1225")),
            "T1225 fired on a type that declares no identifier: {src}"
        );
    }
}

// ── the kind travels the way a class does ───────────────────────────────────

#[test]
fn an_identifier_nested_in_a_wrapper_is_still_carried() {
    // The same walk as κ, over the same structure, for the same reason: putting
    // a type inside a request struct must not hide what is in it.
    let p = program(
        "type MRN identifier medical_record_number { value: String }\n\
         type Inner { mrn: MRN }\n\
         type Request { inner: Inner }\n",
    );
    let found = transitive_identifiers(&p, "Request");
    assert!(
        found.contains("medical_record_number"),
        "an identifier two levels deep must still be seen: {found:?}"
    );
}

#[test]
fn a_type_with_no_identifiers_anywhere_carries_none() {
    let p = program("type Note { text: String }\ntype Wrap { n: Note }\n");
    assert!(
        transitive_identifiers(&p, "Wrap").is_empty(),
        "a structure with no identifiers must report none"
    );
}

#[test]
fn the_identifier_walk_terminates_on_a_cycle() {
    let p = program(
        "type SSN identifier ssn { value: String }\n\
         type Node { next: Node  s: SSN }\n",
    );
    assert!(transitive_identifiers(&p, "Node").contains("ssn"));
}

// ── the Safe Harbor encoding ────────────────────────────────────────────────

#[test]
fn safe_harbor_encodes_the_seventeen_enumerable_classes_as_eighteen_rows() {
    // The regulation lists eighteen classes and SEVENTEEN are enumerable. This
    // table has eighteen ROWS, and the difference is not a discrepancy: item
    // (C) covers both dates and ages over 89, and they take DIFFERENT
    // generalisations — a year, and a single capped category. One row could
    // not carry both operations.
    //
    // The eighteenth class — "any other unique
    // identifying number, characteristic, or code" — is a judgement, not a
    // class, and it is deliberately absent. If this number changes, either the
    // encoding gained a class it should not have, or someone read the
    // regulation again and found one we missed. Both deserve a look.
    assert_eq!(
        HIPAA_SAFE_HARBOR.len(),
        18,
        "the Safe Harbor table encodes the enumerable classes; dates and ages \
         are separate rows because they take different generalisations, though \
         the regulation groups them under item (C)"
    );
}

#[test]
fn every_safe_harbor_rule_names_a_catalogued_kind_and_cites_its_source() {
    for rule in HIPAA_SAFE_HARBOR {
        assert!(
            is_known_identifier(rule.kind),
            "Safe Harbor requires something of `{}`, which is not in the identifier \
             catalogue — the rule table and the catalogue have drifted",
            rule.kind
        );
        assert!(
            rule.citation.starts_with("164.514(b)(2)"),
            "`{}` cites {:?}, which is not the Safe Harbor paragraph. Every row cites \
             the item it encodes so the mapping can be audited against the source text \
             rather than trusted.",
            rule.kind,
            rule.citation
        );
    }
}

#[test]
fn the_regulation_lets_some_data_survive_and_the_encoding_says_so() {
    // THE PROPERTY that a masking-only mechanism cannot express. The regulation
    // does not ask you to delete the date; it asks for the year. A system that
    // makes an adopter throw away data the law lets them keep costs more than
    // the law — and gets routed around.
    let kept: Vec<&str> = HIPAA_SAFE_HARBOR
        .iter()
        .filter(|r| matches!(r.operation, Deidentify::Generalise(_)))
        .map(|r| r.kind)
        .collect();

    assert!(
        kept.contains(&"date") && kept.contains(&"geography") && kept.contains(&"age"),
        "dates, geography and ages are generalised rather than suppressed: {kept:?}"
    );
    assert_eq!(
        safe_harbor_rule("date").map(|r| r.operation),
        Some(Deidentify::Generalise(Generalisation::Year)),
        "a date reduces to its year"
    );
    assert_eq!(
        safe_harbor_rule("ssn").map(|r| r.operation),
        Some(Deidentify::Suppress),
        "an SSN has no reduced form that is still not an SSN"
    );
}

#[test]
fn the_catalogue_is_a_set_and_every_kind_is_snake_case() {
    let mut sorted: Vec<&str> = IDENTIFIER_KINDS.to_vec();
    sorted.sort_unstable();
    let before = sorted.len();
    sorted.dedup();
    assert_eq!(before, sorted.len(), "a kind is listed twice");

    for k in IDENTIFIER_KINDS {
        assert!(
            k.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "identifier kind `{k}` is not snake_case — the spelling is what an \
             adopter types and what every rule table joins on"
        );
    }
}
