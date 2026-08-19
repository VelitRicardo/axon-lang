//! v2.76.0 — discovery + DAG laws through the REAL driver.

use std::collections::BTreeMap;

use axon_frontend::ems::compile_module_set;
use axon_frontend::lexer::Lexer;
use axon_frontend::module_resolver::{scan_imports, ModuleSet};
use axon_frontend::parser::Parser;

fn set_of(pairs: &[(&str, &str)], entry: &str) -> ModuleSet {
    let files: BTreeMap<String, String> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    ModuleSet::from_memory(&files, entry).expect("module set")
}

#[test]
fn cycle_refusal_travels_through_the_driver() {
    let set = set_of(
        &[
            ("main.axon", "import a.{X}\n"),
            ("a.axon", "import b.{Y}\n"),
            ("b.axon", "import a.{X}\n"),
        ],
        "main.axon",
    );
    let err = compile_module_set(&set, None).err().expect("must refuse");
    let msg = &err.errors[0].message;
    assert!(msg.contains("axon-T955"), "{msg}");
    assert!(msg.contains("→"), "cycle path must be named: {msg}");
}

#[test]
fn diamond_links_the_shared_module_once() {
    let set = set_of(
        &[
            ("main.axon", "import b.{Fast}\nimport c.{Careful}\n"),
            ("b.axon", "import d.{Shared}\npersona Fast { domain: [\"speed\"] }\n"),
            ("c.axon", "import d.{Shared}\npersona Careful { domain: [\"care\"] }\n"),
            ("d.axon", "persona Shared { domain: [\"common\"] }\n"),
        ],
        "main.axon",
    );
    let out = compile_module_set(&set, None).expect("diamond compiles");
    assert_eq!(out.module_count, 4);
    let shared: Vec<_> = out
        .ir
        .personas
        .iter()
        .filter(|p| p.name == "Shared")
        .collect();
    assert_eq!(shared.len(), 1, "the diamond's shared module links exactly once");
}

/// The scanner and the parser agree on every accepted import form — the
/// drift class the retired Python regex scanner invited.
#[test]
fn scan_parity_with_the_parser() {
    let source = "import axon.security.{A, B}\nimport lib.deep.path.{One}\nimport solo.{X} @allow_downgrade\n";
    let scanned = scan_imports(source, "t.axon").expect("scan");

    let tokens = Lexer::new(source, "t.axon").tokenize().expect("lex");
    let program = Parser::new(tokens).parse().expect("parse");
    let parsed: Vec<_> = program
        .declarations
        .iter()
        .filter_map(|d| match d {
            axon_frontend::ast::Declaration::Import(n) => Some(n),
            _ => None,
        })
        .collect();

    assert_eq!(scanned.len(), parsed.len());
    for (s, p) in scanned.iter().zip(parsed.iter()) {
        assert_eq!(s.module_path.0, p.module_path);
        assert_eq!(s.names, p.names);
        assert_eq!(s.allow_downgrade, p.allow_downgrade);
    }
}
