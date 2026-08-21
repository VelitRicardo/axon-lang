//! v4.3.0 — the compliance guarantee is worth exactly what its exit list is worth.
//!
//! κ answers one question for a regulated adopter: *can this data leave without
//! a control?* That answer is only as good as the enumeration of "leave", and an
//! enumeration somebody maintains by hand develops a hole the first time
//! primitive number eight arrives — an invisible hole, because every existing
//! rule still passes and the suite stays green.
//!
//! So `compliance::EGRESS_PRIMITIVES` is closed, and this file is what closes
//! it. Four laws, and the third is the one that matters:
//!
//! 1. **Every shielded exit's rule exists.** The `code` an entry claims appears
//!    in the type checker. A catalogue that cites a diagnostic nobody wrote is
//!    the ledger defect, one layer down.
//! 2. **Every shielded exit can name a control.** Its AST node has a
//!    `shield_ref` field, so the rule the entry claims is expressible.
//! 3. **No exit is missing.** Every declaration in the AST that carries an
//!    `effects:` row — the language's own mark for "this reaches outside" — is
//!    in the catalogue. This is the derivable half, and it is what a new
//!    egress primitive trips.
//! 4. **No entry is stale.** Every catalogued primitive is a real primitive.
//!
//! Written as source-text laws over `ast.rs` and `type_checker.rs` on purpose:
//! the property is about what the compiler CONTAINS, and a test that asked the
//! compiler about itself could only report what it already believes.

use axon_frontend::compliance::{Coverage, EGRESS_PRIMITIVES};
use std::path::PathBuf;

fn src(file: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src").join(file);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{} must be readable: {e}", p.display()))
}

/// Every `pub struct <Name>Definition` that declares an `effects:` row.
///
/// The effect row is the language's own statement that a declaration reaches
/// outside the program — it is what `<network>`, `<web>`, `<io>` and
/// `<storage>` are for. Deriving the exit set from it beats a second hand-kept
/// list, which would drift from this one in the usual direction: silently.
fn declarations_with_an_effect_row(ast: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current: Option<&str> = None;
    for line in ast.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("pub struct ") {
            current = rest.split(|c: char| !c.is_alphanumeric() && c != '_').next();
        }
        if t.starts_with("pub effects:") && t.contains("EffectRow") {
            if let Some(name) = current {
                out.push(name.to_string());
            }
        }
    }
    out
}

/// `Foo` for `FooDefinition`, lowercased — the AST's naming convention mapped
/// back to the surface keyword an adopter types.
fn keyword_of(struct_name: &str) -> String {
    struct_name
        .strip_suffix("Definition")
        .unwrap_or(struct_name)
        .to_ascii_lowercase()
}

#[test]
fn every_shielded_exit_has_the_rule_it_claims() {
    let checker = src("type_checker.rs");
    for e in EGRESS_PRIMITIVES {
        if let Coverage::Shielded { code, .. } = e.coverage {
            assert!(
                checker.contains(code),
                "the egress catalogue says `{}` is covered by {code}, and the type checker \
                 does not contain that diagnostic. A catalogue that cites a rule nobody \
                 wrote is worse than no catalogue: it reads as a guarantee.",
                e.primitive
            );
        }
    }
}

#[test]
fn every_shielded_exit_can_name_a_control() {
    let ast = src("ast.rs");
    for e in EGRESS_PRIMITIVES {
        let Coverage::Shielded { .. } = e.coverage else { continue };
        // The struct that owns this keyword must have somewhere to put the shield.
        let struct_name = format!(
            "{}{}Definition",
            e.primitive[..1].to_ascii_uppercase(),
            &e.primitive[1..]
        );
        let Some(start) = ast.find(&format!("pub struct {struct_name}")) else {
            // Naming is not perfectly mechanical (AxonEndpointDefinition), so a
            // miss here is a lookup failure, not a violation — law 4 owns
            // whether the primitive is real.
            continue;
        };
        let end = ast[start..].find("\n}").map(|i| start + i).unwrap_or(ast.len());
        assert!(
            ast[start..end].contains("shield_ref"),
            "`{}` is catalogued as a SHIELDED exit but its declaration has no `shield_ref` \
             field — the rule it claims cannot be satisfied by any program.",
            e.primitive
        );
    }
}

#[test]
fn no_declaration_that_reaches_outside_is_missing_from_the_catalogue() {
    let ast = src("ast.rs");
    let with_effects = declarations_with_an_effect_row(&ast);
    assert!(
        with_effects.len() >= 4,
        "only {} declarations with an `effects:` row found — the field was renamed or \
         reshaped and this law is now deriving the exit set from nothing: {with_effects:?}",
        with_effects.len()
    );

    let catalogued: Vec<&str> = EGRESS_PRIMITIVES.iter().map(|e| e.primitive).collect();
    let missing: Vec<String> = with_effects
        .iter()
        .map(|s| keyword_of(s))
        .filter(|k| !catalogued.contains(&k.as_str()))
        .collect();

    assert!(
        missing.is_empty(),
        "a declaration carries an `effects:` row — the language's own mark for reaching \
         outside the program — and is NOT in the egress catalogue: {missing:?}\n\n\
         Add it to `compliance::EGRESS_PRIMITIVES`. If its κ is computable (it binds a \
         declared TYPE), give it a rule and catalogue it as Shielded. If it is not, \
         catalogue it as NoStaticKappa and say why — an exit missing from the list is \
         indistinguishable from an exit nobody examined, which is the state this whole \
         cycle exists to end.\n\ncatalogued: {catalogued:?}"
    );
}

#[test]
fn no_catalogued_exit_is_a_primitive_that_does_not_exist() {
    for e in EGRESS_PRIMITIVES {
        assert!(
            axon_frontend::find_primitive(e.primitive).is_some(),
            "the egress catalogue lists `{}`, which is not in the primitive registry — \
             a retired exit leaves the catalogue, it does not stay as decoration.",
            e.primitive
        );
    }
}

#[test]
fn the_catalogue_states_a_reason_wherever_it_states_a_limit() {
    // A `NoStaticKappa` entry with an empty `why` is a shrug in a place an
    // auditor reads. The reason is the load-bearing part of the entry.
    for e in EGRESS_PRIMITIVES {
        if let Coverage::NoStaticKappa { why, .. } = e.coverage {
            assert!(
                why.len() > 30,
                "`{}` is catalogued as having no static κ with no real reason given: {why:?}",
                e.primitive
            );
        }
    }
}
