//! The MCP registry manifest describes the crate that actually ships.
//!
//! `PUBLISHING.md` has always said `server.json` "is the source of truth and is
//! kept in sync with the crate version". **It was not.** The manifest sat at
//! `0.4.0` from the commit that introduced it (`eb40c634`) while the crate went
//! to `0.52.0` — forty-eight minor releases of drift, in the one file whose
//! entire job is to tell MCP clients what this server is. Nothing gated it, so
//! nothing caught it; it surfaced only when someone went to publish.
//!
//! That is exactly v2.81.0's defect in a different registry: crates.io served
//! a README describing v1.0.0 for the 2.80.0 crate, for six releases, behind one
//! ignored warning. The lesson v2.81.0 paid for is that **a published artefact
//! needs a gate, not a convention** — a doc sentence asserting an invariant is
//! not an invariant, it is a wish. This file is that gate.
//!
//! It is deliberately dependency-free (plain string scanning, no `toml` /
//! `serde_json` crate needed) so it can never be the reason the suite fails to
//! build.

const MANIFEST: &str = include_str!("../server.json");
const CARGO: &str = include_str!("../Cargo.toml");

/// Pull `"<key>": "<value>"` out of the flat JSON manifest.
fn json_str(key: &str) -> String {
    let needle = format!("\"{key}\"");
    let start = MANIFEST
        .find(&needle)
        .unwrap_or_else(|| panic!("server.json must declare `{key}`"));
    let after = &MANIFEST[start + needle.len()..];
    let colon = after.find(':').expect("malformed server.json");
    let rest = &after[colon + 1..];
    let open = rest.find('"').expect("malformed server.json");
    let tail = &rest[open + 1..];
    let close = tail.find('"').expect("malformed server.json");
    tail[..close].to_string()
}

/// The `version = "…"` of `[package]` — the FIRST such line, before any
/// dependency's own `version =`.
fn crate_version() -> String {
    CARGO
        .lines()
        .find(|l| l.starts_with("version = \""))
        .and_then(|l| l.split('"').nth(1))
        .expect("Cargo.toml must declare a package version")
        .to_string()
}

#[test]
fn server_json_version_matches_the_crate() {
    let manifest_version = json_str("version");
    let crate_version = crate_version();
    assert_eq!(
        manifest_version, crate_version,
        "server.json declares version {manifest_version} but the crate is {crate_version}.\n\
         \n\
         The MCP registry advertises this manifest to every client that discovers \
         the server; a stale version there tells adopters the AXON MCP server is \
         something other than what `cargo install axon-emcp` gives them. Bump \
         server.json in the SAME commit as Cargo.toml — that is what PUBLISHING.md \
         promises, and this assertion is what makes the promise true.\n\
         \n\
         (Drift history: the manifest sat at 0.4.0 through 0.52.0 because nothing \
         checked it.)"
    );
}

#[test]
fn server_json_names_the_owned_namespace() {
    let name = json_str("name");
    assert_eq!(
        name, "io.github.VelitRicardo/axon-emcp",
        "server.json declares the registry name {name}.\n\
         \n\
         An `io.github.*` namespace is a CLAIM ABOUT WHO WE ARE, authenticated by \
         GitHub account ownership — not a URL that redirects. The account was \
         renamed Bemarking -> VelitRicardo (2026-08-04), so `io.github.Bemarking/*` \
         is no longer ours and `github.com/Bemarking` now 404s, i.e. the old \
         username is unclaimed and re-registrable by anyone.\n\
         \n\
         If this ever needs to change again, change it because the ACCOUNT changed, \
         and remember the old registry entry cannot be renamed — only superseded or \
         deprecated."
    );
}

#[test]
fn server_json_points_at_the_real_repository() {
    // The registry records this URL and adopters follow it. It moved with the
    // account rename too, and a 404 here is a broken front door on the one
    // surface a stranger uses to decide whether this project is real.
    assert_eq!(
        json_str("url"),
        "https://github.com/VelitRicardo/axon-lang",
        "server.json's repository URL must name the current account."
    );
    assert_eq!(
        json_str("subfolder"),
        "src/axon-emcp",
        "server.json's repository subfolder must locate this crate in the monorepo."
    );
}
