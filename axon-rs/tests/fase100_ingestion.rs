//! §Fase 118.b — this suite exercises the OOXML surface (reader, editor,
//! path sandbox), which lives behind the `documents` feature. Gated at the
//! file level: the 105 core suites keep running in a `cli`-only build.
#![cfg(feature = "documents")]

//! §Fase 100.a/c/e — the ingestion tool dispatch path: DocumentReader (bounded,
//! born-Untrusted, Parsed), DocumentEditor (surgical + manifest), and the 100.a
//! hallucination-refusal (a declared-but-unimplemented tool typed-refuses).

use axon::tool_executor::{dispatch, dispatch_or_reject};
use base64::Engine;
use std::io::Write;

fn build_docx(document_xml: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let cursor = std::io::Cursor::new(&mut buf);
        let mut zip = zip::ZipWriter::new(cursor);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in [
            ("[Content_Types].xml", b"<Types/>".as_slice()),
            ("_rels/.rels", b"<Relationships/>".as_slice()),
            ("word/document.xml", document_xml.as_bytes()),
        ] {
            zip.start_file(name, opts).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
    }
    buf
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[test]
fn document_reader_reads_born_untrusted_parsed() {
    let docx = build_docx(r#"<w:document><w:body><w:p><w:r><w:t>Confidential total: 250</w:t></w:r></w:p></w:body></w:document>"#);
    let arg = serde_json::json!({ "bytes_base64": b64(&docx) }).to_string();
    let result = dispatch("DocumentReader", &arg).expect("native tool");
    assert!(result.success, "output: {}", result.output);
    let v: serde_json::Value = serde_json::from_str(&result.output).unwrap();
    assert_eq!(v["format"], "docx");
    // Born Untrusted + Parsed — the load-bearing property (D100.1/D100.2).
    assert_eq!(v["taint"], "untrusted");
    assert_eq!(v["provenance"], "parsed");
    assert_eq!(v["epistemic_ceiling"], "know"); // Parsed may reach know via a shield
    assert!(v["full_text"].as_str().unwrap().contains("Confidential total: 250"));
}

#[test]
fn document_reader_refuses_external_relationship() {
    // Build a docx with an external relationship target — must be refused.
    let mut buf = Vec::new();
    {
        let cursor = std::io::Cursor::new(&mut buf);
        let mut zip = zip::ZipWriter::new(cursor);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in [
            ("word/document.xml", b"<w:document/>".as_slice()),
            ("word/_rels/document.xml.rels", br#"<Relationships><Relationship Target="http://evil/x" TargetMode="External"/></Relationships>"#.as_slice()),
        ] {
            zip.start_file(name, opts).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
    }
    let arg = serde_json::json!({ "bytes_base64": b64(&buf) }).to_string();
    let result = dispatch("DocumentReader", &arg).expect("native tool");
    assert!(!result.success);
    assert!(result.output.contains("external relationship"), "output: {}", result.output);
}

#[test]
fn document_editor_surgical_edit_with_manifest() {
    let docx = build_docx(r#"<w:document><w:body><w:p><w:r><w:t>Total: 100</w:t></w:r></w:p></w:body></w:document>"#);
    let arg = serde_json::json!({
        "bytes_base64": b64(&docx),
        "edits": [{ "kind": "replace_text", "part": "word/document.xml", "find": "Total: 100", "replace": "Total: 250" }],
    })
    .to_string();
    let result = dispatch("DocumentEditor", &arg).expect("native tool");
    assert!(result.success, "output: {}", result.output);
    let v: serde_json::Value = serde_json::from_str(&result.output).unwrap();
    // The manifest proves the blast radius is exactly document.xml.
    let touched: Vec<String> = v["touched_parts"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect();
    assert_eq!(touched, vec!["word/document.xml"]);
    // Edit inherits taint (no laundering).
    assert_eq!(v["taint"], "untrusted");
}

#[test]
fn declared_but_unimplemented_tool_typed_refuses_not_hallucinate() {
    // §100.a / D100.12 — `FileReader` is declared in stdlib::TOOLS but has no
    // native executor and no provider (the generic local reader is still
    // unimplemented). It MUST refuse, not fall through to the model.
    // (`PDFExtractor` moved to a REAL §101.b dispatch arm — see fase101_extraction.)
    let result = dispatch_or_reject("FileReader", "some.txt");
    match result {
        Err(msg) => assert!(msg.contains("FileReader") && msg.contains("fabricate")),
        Ok(other) => panic!("FileReader must typed-refuse, got {other:?}"),
    }
    // The real native tools are NOT refused.
    assert!(dispatch_or_reject("DocumentRenderer", "{}").is_ok());
}
