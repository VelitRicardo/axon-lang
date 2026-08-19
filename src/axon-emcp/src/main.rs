//! `axon-emcp` — the official ℰMCP (Epistemic Model Context Protocol)
//! server for AXON.
//!
//! Speaks MCP over stdio + JSON-RPC 2.0. Once an agent (Claude Code,
//! Codex, Cursor, Continue, Cline, …) launches this binary as an MCP
//! subprocess, it can:
//!
//! - Call **tools** (`axon.check`, `axon.parse`, `axon.primitives`,
//!   `axon.primitive_doc`, `axon.compose`) to validate code, look up
//!   grammar, request idiomatic scaffolds.
//! - Read **resources** (`axon://primitives/{name}`,
//!   `axon://grammar/top_level`, …) — the canonical reference
//!   material the agent quotes in replies.
//! - Invoke **prompts** (`flow_design`, `shield_design`,
//!   `session_design`) — host-surfaced design recipes.
//!
//! The wire is **stdio-only**: STDOUT carries JSON-RPC frames (one per
//! line), STDERR carries logs. Anything written to stdout that is not a
//! valid JSON-RPC frame corrupts the agent's parser — so we route every
//! `tracing` event through stderr by default.
//!
//! # Subcommands (v1.2.0)
//!
//! In addition to the default MCP-server mode, this binary supports a
//! handful of contributor-facing subcommands that run, exit, and do
//! NOT enter the MCP loop:
//!
//! - `axon-emcp scaffold-primitive <name>` — stamp a markdown doc
//!   skeleton for one primitive (frontmatter pre-populated from
//!   [`axon_frontend::PRIMITIVE_REGISTRY`]).
//! - `axon-emcp --help` — print usage.
//! - `axon-emcp --version` — print the crate version.
//!
//! No arguments → MCP server mode (the default).

#![forbid(unsafe_code)]
#![deny(missing_docs)]

// All modules live in `src/lib.rs` so the binary and the integration
// test suite (under `tests/`) compile against one copy of each. We
// import only the surfaces the binary entrypoint actually needs.
use axon_emcp::{knowledge, otlp, scaffold, server, telemetry};

use std::path::PathBuf;
use std::process::ExitCode;

use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    // v1.2.0 — subcommand dispatch BEFORE tracing setup. Scaffold
    // and help-style flags are interactive CLI invocations: their
    // stderr is read by humans, and the MCP-server's tracing
    // configuration (compact formatter, info level) is wrong for
    // that audience. The MCP-server lane keeps its own setup below.
    let args: Vec<String> = std::env::args().collect();
    if let Some(sub) = args.get(1) {
        match sub.as_str() {
            "scaffold-primitive" => return run_scaffold_primitive(&args[2..]),
            "telemetry" => return run_telemetry(&args[2..]),
            "--help" | "-h" => return run_help(),
            "--version" | "-V" => return run_version(),
            // Any other first-arg token falls through to MCP server
            // mode — the agent might pass flags we don't yet
            // recognise (e.g. `--protocol-version`), and tolerating
            // them keeps the server forward-compatible.
            _ => {}
        }
    }

    // ── MCP server mode (the default) ─────────────────────────────────

    // Stderr-only tracing. The MCP wire owns stdout — any other writer
    // there is a protocol violation. The `RUST_LOG` env var follows the
    // rest of the workspace (e.g. `RUST_LOG=axon_emcp=debug`).
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with_writer(std::io::stderr)
        .with_target(false)
        .compact()
        .init();

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        "axon-emcp starting — ℰMCP OFICIAL"
    );

    // Load the knowledge base once at startup. The base lives under
    // `src/knowledge/` (relative to the repo root); we resolve it from
    // the binary's compile-time path so an installed binary still finds
    // its corpus — see `knowledge::Catalog::load_default()` for the
    // resolution order.
    let catalog = match knowledge::Catalog::load_default() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "failed to load knowledge base — refusing to start");
            return ExitCode::from(2);
        }
    };
    tracing::info!(
        primitives = catalog.primitive_count(),
        "knowledge base loaded"
    );

    // v1.3.0 — install the per-instance telemetry registry. Reads
    // env vars (`AXON_EMCP_TELEMETRY_FILE`, `AXON_EMCP_DEPLOYMENT_ID`,
    // `AXON_EMCP_TELEMETRY_MAX_SAMPLES`) at startup; missing vars
    // yield defaults (no file sink, no deployment ID, 1k samples).
    let tel_config = telemetry::TelemetryConfig::from_env();
    let jsonl_sink = tel_config.jsonl_sink.clone();
    let deployment_id = tel_config.deployment_id.clone();
    let max_samples = tel_config.max_samples;
    let tel = telemetry::Telemetry::new(tel_config);
    tracing::info!(
        deployment_id = %deployment_id,
        jsonl_sink = ?jsonl_sink,
        max_samples,
        "telemetry installed"
    );

    // v1.4.0 — OTLP/gRPC exporter follow-up. The pusher reads its
    // config from `AXON_EMCP_OTLP_*` env vars; empty endpoint = no
    // task spawned, no socket opened (privacy invariant #6). The
    // background loop snapshots `tel` every `interval` and pushes
    // a translated ResourceMetrics payload to the configured
    // collector. We share `tel` via an `Arc` so the dispatcher and
    // the pusher read the same in-process model.
    let tel_arc = std::sync::Arc::new(tel);
    let otlp_config = otlp::OtlpConfig::from_env();
    otlp::spawn_pusher(otlp_config, tel_arc.clone());

    // Hand off to the stdio MCP loop. Returns when the agent disconnects
    // (clean EOF on stdin) or on a fatal transport error. The OTLP
    // pusher (if active) keeps running in the background and stops
    // when the process exits.
    match server::run_stdio(catalog, tel_arc).await {
        Ok(()) => {
            tracing::info!("axon-emcp shutting down cleanly");
            ExitCode::SUCCESS
        }
        Err(e) => {
            tracing::error!(error = %e, "axon-emcp transport error");
            ExitCode::FAILURE
        }
    }
}

/// `scaffold-primitive <name>` — stamp a markdown doc skeleton with
/// frontmatter pre-populated from the canonical registry.
///
/// Exit codes:
/// - `0` success — the file was written.
/// - `1` runtime failure (file already exists, knowledge dir missing,
///   write error). Stderr carries the diagnostic.
/// - `2` usage error (no `<name>` argument). Stderr carries usage hint.
fn run_scaffold_primitive(args: &[String]) -> ExitCode {
    if args.is_empty() {
        eprintln!("usage: axon-emcp scaffold-primitive <name>");
        eprintln!();
        eprintln!("Stamps src/knowledge/primitives/<name>.md with the frontmatter pre-populated");
        eprintln!("from axon_frontend::PRIMITIVE_REGISTRY. The name must already exist in the");
        eprintln!("registry (add it there first if it's a new primitive).");
        return ExitCode::from(2);
    }
    let name = &args[0];

    let knowledge_dir = match resolve_knowledge_dir() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(1);
        }
    };

    match scaffold::run(name, &knowledge_dir) {
        Ok(msg) => {
            // Subcommand output goes to stderr per the MCP discipline
            // (stdout is reserved for JSON-RPC; if a contributor pipes
            // `axon-emcp scaffold-primitive foo | something`, they
            // should not get a CLI message mixed in).
            eprintln!("{msg}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

/// Resolve the corpus root for contributor-facing subcommands.
/// Resolution order:
///
/// 1. `AXON_EMCP_KNOWLEDGE_DIR` env var (operator override).
/// 2. `<cwd>/src/knowledge` (running from the repo root).
/// 3. `<cwd>/../knowledge` (running from inside `src/axon-emcp/`).
///
/// Fails with a structured error message if none resolve — the
/// scaffold CLI is not useful from an installed binary outside the
/// repo, by design.
fn resolve_knowledge_dir() -> Result<PathBuf, String> {
    if let Ok(s) = std::env::var("AXON_EMCP_KNOWLEDGE_DIR") {
        let p = PathBuf::from(&s);
        if p.is_dir() {
            return Ok(p);
        }
        return Err(format!(
            "AXON_EMCP_KNOWLEDGE_DIR points to non-directory: {s}"
        ));
    }
    let cwd = std::env::current_dir()
        .map_err(|e| format!("could not read cwd: {e}"))?;
    // Release 0.2.0 — corpus moved INSIDE the crate
    // (`src/axon-emcp/knowledge/`) so `cargo publish` ships it. Dev
    // paths we try (in order): `<cwd>/src/axon-emcp/knowledge` (from
    // repo root), `<cwd>/knowledge` (from inside the crate dir).
    for candidate in [
        cwd.join("src").join("axon-emcp").join("knowledge"),
        cwd.join("knowledge"),
    ] {
        if candidate.is_dir() {
            return Ok(candidate);
        }
    }
    Err(format!(
        "could not find knowledge directory — run from the repo root, \
         or set AXON_EMCP_KNOWLEDGE_DIR to point to src/axon-emcp/knowledge/. \
         cwd = {}",
        cwd.display()
    ))
}

/// `--help` / `-h` — print usage to stderr and exit 0.
fn run_help() -> ExitCode {
    eprintln!(
        "{name} — the official ℰMCP server for AXON, plus contributor subcommands.\n\
         \n\
         USAGE:\n  \
           {name} [SUBCOMMAND]\n\
         \n\
         SUBCOMMANDS:\n  \
           (none)                 Start the MCP server (default — speaks JSON-RPC 2.0 on stdio)\n  \
           scaffold-primitive <name>\n                                Stamp a markdown doc skeleton for one primitive\n  \
           telemetry summarize <file>\n                                Aggregate a JSONL telemetry log into the snapshot shape\n  \
           --help, -h             Print this help\n  \
           --version, -V          Print the crate version\n\
         \n\
         ENV:\n  \
           AXON_EMCP_KNOWLEDGE_DIR        Override the corpus root (default: in-tree, then embedded)\n  \
           AXON_EMCP_TELEMETRY_FILE      Append JSONL telemetry events to this path (default: disabled)\n  \
           AXON_EMCP_DEPLOYMENT_ID       Correlation tag stamped on every event (default: empty)\n  \
           AXON_EMCP_TELEMETRY_MAX_SAMPLES   Latency histogram window per tool (default: 1000)\n  \
           AXON_EMCP_OTLP_ENDPOINT       OTLP/gRPC collector URL (e.g. http://localhost:4317). Empty = exporter disabled.\n  \
           AXON_EMCP_OTLP_HEADERS        Comma-separated key=value headers stamped on every push (e.g. x-honeycomb-team=KEY)\n  \
           AXON_EMCP_OTLP_INTERVAL_SECS  OTLP push interval in seconds (default: 60)\n  \
           AXON_EMCP_OTLP_TIMEOUT_SECS   OTLP per-RPC timeout in seconds (default: 10)\n  \
           RUST_LOG                       Tracing filter (e.g. axon_emcp=debug)\n",
        name = env!("CARGO_PKG_NAME")
    );
    ExitCode::SUCCESS
}

/// `--version` / `-V` — print `<name> <version>` to stderr and exit 0.
fn run_version() -> ExitCode {
    eprintln!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
    ExitCode::SUCCESS
}

/// `telemetry <subcmd>` — v1.3.0 contributor surface for the
/// telemetry layer. Subcommands:
///
/// - `summarize <file>` — aggregate a JSONL telemetry log into the
///   same snapshot shape `axon-emcp` emits in-process. Prints the
///   summary as pretty-JSON to STDOUT (subcommand output) so the
///   contributor can pipe `jq` / save to a file.
fn run_telemetry(args: &[String]) -> ExitCode {
    let Some(sub) = args.first() else {
        eprintln!("usage: axon-emcp telemetry <summarize> <file>");
        return ExitCode::from(2);
    };
    match sub.as_str() {
        "summarize" => {
            let Some(path) = args.get(1) else {
                eprintln!("usage: axon-emcp telemetry summarize <file>");
                return ExitCode::from(2);
            };
            let path = PathBuf::from(path);
            match telemetry::summarize_jsonl(&path) {
                Ok(v) => {
                    // The summary is the subcommand's PRIMARY output;
                    // route to STDOUT so adopters can pipe it to `jq`
                    // / save to file / forward to OTLP collector.
                    println!("{}", serde_json::to_string_pretty(&v).unwrap());
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("error: failed to summarize `{}`: {e}", path.display());
                    ExitCode::from(1)
                }
            }
        }
        other => {
            eprintln!(
                "unknown telemetry subcommand `{other}` — valid: summarize"
            );
            ExitCode::from(2)
        }
    }
}
