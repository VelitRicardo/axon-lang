//! AXON CLI — the native command-line front door to the compiler and runtime.
//!
//! All 14 commands handled natively. Python is no longer required.
//!   Active:  version, check, compile, run, trace, repl, inspect, ld, serve, deploy, diff, replay, stats, graph

use axon::audit_cli;
// §Fase 118.b.2 — the HTTP server is behind the `server` feature. The `Serve`
// subcommand below is NOT gated: it stays in `--help` under every profile and
// refuses in writing when the feature is absent. See `run_serve_dispatch`.
#[cfg(feature = "server")]
use axon::axon_server;
use axon::checker;
use axon::cost_estimator;
use axon::graph_export;
use axon::deployer;
use axon::compiler;
use axon::inspect;
use axon::lambda_data;
use axon::plan_diff;
use axon::repl;
use axon::replay;
use axon::runner;
use axon::runner::AXON_VERSION;
use axon::trace_stats;
use axon::tracer;

use clap::{Parser, Subcommand};
use std::process;

// ── Estructura CLI (espejo del CLI Python) ────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "axon",
    about = "AXON — A programming language for AI cognition.",
    // `axon --version` / `-V` print the same line as `axon version`. Both stay:
    // the subcommand is what every earlier release documented, the flag is what
    // every shell script and package manager expects.
    version = AXON_VERSION,
    arg_required_else_help = true,
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Lex, parse, and type-check an .axon file.
    Check {
        file: String,
        #[arg(long)]
        no_color: bool,
        /// Promote warnings to errors (CI gate).
        #[arg(long)]
        strict: bool,
        /// Directory containing axon-schema
        /// manifests (`.axon-schema.json`) for form (b) `manifest_ref`
        /// and form (c) `env_var` compile-time proof. When set,
        /// `axon check` loads + merges every manifest under the path
        /// and feeds it to the type-checker via
        /// `TypeChecker::with_manifest`. T801-T805 + T803 run against
        /// the resolved column sets for non-inline schemas.
        ///
        /// Without this flag, forms (b)/(c) silently skip at compile
        /// time exactly as in v1.38.3 (D5 backwards-compat absolute).
        /// Mirror of `axon serve --schemas-dir`; the
        /// runtime flag stays unchanged.
        #[arg(long, env = "AXON_SCHEMAS_DIR")]
        schemas_dir: Option<String>,
    },
    /// Print the exact lower-level program the `voice` and
    /// `upstream … from Preset@vN` sugar expands to (sugar a
    /// compliance reviewer cannot see through would break the
    /// audit-by-construction property; this is the seeing-through).
    Desugar { file: String },
    /// Compile an .axon file to IR JSON.
    Compile {
        file: String,
        #[arg(short, long, default_value = "anthropic")]
        backend: String,
        #[arg(short, long)]
        output: Option<String>,
        #[arg(long)]
        stdout: bool,
    },
    /// Compile and execute an .axon file.
    Run {
        file: String,
        #[arg(short, long, default_value = "anthropic")]
        backend: String,
        #[arg(long)]
        trace: bool,
        #[arg(long, default_value = "stub")]
        tool_mode: String,
        /// Stream LLM output in real-time (requires tool_mode=real).
        #[arg(long)]
        stream: bool,
        /// Output format: text (default) or json.
        #[arg(long, default_value = "text")]
        output: String,
        /// Export execution plan as JSON without executing.
        #[arg(long)]
        export_plan: bool,
    },
    /// Pretty-print a saved execution trace.
    Trace {
        file: String,
        #[arg(long)]
        no_color: bool,
    },
    /// Show axon-lang version.
    Version,
    /// Start an interactive AXON REPL session.
    Repl,
    /// Introspect the AXON standard library.
    Inspect {
        #[arg(default_value = "anchors")]
        target: String,
        #[arg(long)]
        all: bool,
    },
    /// Start the AxonServer (reactive daemon platform).
    Serve {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 8420)]
        port: u16,
        #[arg(long, default_value = "memory")]
        channel: String,
        #[arg(long, default_value = "")]
        auth_token: String,
        #[arg(long, default_value = "info")]
        log_level: String,
        /// Log output format: "json" (default, structured) or "pretty" (human-readable).
        #[arg(long, default_value = "json")]
        log_format: String,
        /// Optional directory for daily-rotated log files.
        #[arg(long)]
        log_file: Option<String>,
        /// PostgreSQL connection URL (also reads DATABASE_URL env var).
        #[arg(long)]
        database_url: Option<String>,
        /// Type-Driven Wire Inference activation.
        ///
        /// When set, `POST /v1/execute` promotes to SSE for any flow
        /// the type-checker inferred as stream-producing (D1) regardless
        /// of the client's `Accept:` header. Adopters who explicitly
        /// declared `transport: json` retain D3 opt-out semantics.
        ///
        /// Also readable from the env var
        /// `AXON_STRICT_TYPE_DRIVEN_TRANSPORT` (truthy values: "1",
        /// "true", "yes", "on" — case-insensitive). CLI flag wins
        /// when both are set.
        ///
        /// D6 default: false in v1.22.x, flips to true in v2.0.0.
        #[arg(long)]
        strict_type_driven_transport: bool,
        /// Server-wide default execution backend.
        ///
        /// Rung 3 of the Backend Resolution Contract: an `axonendpoint`
        /// that declares no `backend:` of its own inherits this server
        /// default. Valid values are the closed catalog `anthropic |
        /// auto | gemini | glm | kimi | ollama | openai | openrouter |
        /// stub` — an unknown name aborts startup with exit code 1.
        ///
        /// Also readable from the env var `AXON_DEFAULT_BACKEND`; the
        /// CLI flag wins when both are set. Unset ≡ no server default
        /// (resolution falls through to the environment-available
        /// providers).
        #[arg(long)]
        backend: Option<String>,
        /// Directory containing declared
        /// store-schema manifests (`*.axon-schema.json` files at the
        /// project root and/or under a `schemas/` subdirectory).
        ///
        /// When set, `POST /v1/deploy` loads and merges every manifest
        /// under the directory before running the deploy-time store
        /// verification pass. Declared columns (schema forms
        /// a/b/c) become the authoritative shape — any drift between
        /// the manifest and the LIVE Postgres introspection raises
        /// axon-T807 (DeclaredVsLiveDrift) and fails the deploy.
        ///
        /// Also readable from the env var `AXON_SCHEMAS_DIR`; the CLI
        /// flag wins when both are set. Unset ≡ no manifest loading —
        /// the v1.37.0 verify_postgres_schemas behavior is preserved
        /// verbatim (absolute backwards-compat: an adopter who has
        /// not adopted the compile-time schema observes ZERO
        /// behavior change at deploy).
        #[arg(long)]
        schemas_dir: Option<String>,
    },
    /// Lambda Data (ΛD) epistemic codec: encode, decode, inspect.
    Ld {
        /// Action: encode, decode, inspect.
        action: String,
        /// Source file (.axon for encode, .ld for decode/inspect).
        file: String,
    },
    /// Compare two exported execution plans.
    Diff {
        /// First plan JSON file (baseline).
        file_a: String,
        /// Second plan JSON file (changed).
        file_b: String,
        /// Output as JSON instead of human-readable.
        #[arg(long)]
        json: bool,
    },
    /// Replay an execution trace or compare two traces for regression.
    Replay {
        /// Trace JSON file to replay.
        file: String,
        /// Optional second trace file for regression comparison.
        #[arg(long)]
        compare: Option<String>,
        /// Output as JSON instead of human-readable.
        #[arg(long)]
        json: bool,
    },
    /// Compute aggregate statistics across execution traces.
    Stats {
        /// One or more trace JSON files.
        #[arg(required = true)]
        files: Vec<String>,
        /// Output as JSON instead of human-readable.
        #[arg(long)]
        json: bool,
        /// Output format: text (default), json, prometheus, csv.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Export dependency graph as DOT (Graphviz) or Mermaid diagram.
    Graph {
        /// AXON source file to analyze.
        file: String,
        /// Output format: dot (default) or mermaid.
        #[arg(long, default_value = "dot")]
        format: String,
    },
    /// Estimate execution cost (tokens/USD) before running a flow.
    Estimate {
        /// AXON source file to analyze.
        file: String,
        /// Output format: text (default) or json.
        #[arg(long, default_value = "text")]
        format: String,
        /// Pricing model: sonnet (default), opus, or haiku.
        #[arg(long, default_value = "sonnet")]
        model: String,
    },
    /// Deploy .axon file to a running AxonServer.
    Deploy {
        file: String,
        #[arg(long, default_value = "http://localhost:8420")]
        server: String,
        #[arg(short, long, default_value = "anthropic")]
        backend: String,
        #[arg(long, default_value = "")]
        auth_token: String,
    },
    /// Generate a JSON compliance dossier from an .axon file.
    Dossier {
        file: String,
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Generate a JSON Software Bill of Materials from an .axon file.
    Sbom {
        file: String,
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Gap analysis against SOC 2 / ISO 27001 / FIPS 140-3 / CC EAL 4+.
    Audit {
        file: String,
        #[arg(long, default_value = "all")]
        framework: String,
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Assemble a deterministic audit evidence ZIP for external auditors.
    #[command(name = "evidence-package")]
    EvidencePackage {
        file: String,
        #[arg(short, long)]
        output: Option<String>,
        /// Free-form auditor intake note embedded in README.md.
        #[arg(long, default_value = "")]
        note: String,
    },
    /// `axonstore` schema introspection / manifest export.
    Store {
        #[command(subcommand)]
        action: StoreCommands,
    },
    /// Proof-Carrying Code: generate + independently verify
    /// machine-checkable proofs of an apx program's declared contract
    /// (compliance / effects / capability / resources / shields).
    Pcc {
        #[command(subcommand)]
        action: PccCommands,
    },
    /// Multi-file diagnostic aggregator. Walks patterns / dirs,
    /// parses every `.axon` file with recovery, and aggregates
    /// diagnostics across the whole corpus in one pass.
    Parse {
        /// File path, directory (walked recursively), or literal
        /// pattern (multiple allowed).
        #[arg(required = true)]
        patterns: Vec<String>,
        /// Cap the total errors reported across all files (D6,
        /// default unlimited). The CLI prints a truncation footer
        /// when the cap kicks in.
        #[arg(long, value_name = "N")]
        max_errors: Option<usize>,
        /// Ignore pattern (substring match — may repeat). Future
        /// releases extend this to fnmatch glob shapes.
        #[arg(long = "ignore", value_name = "PATTERN")]
        ignore: Vec<String>,
        /// Worker thread count (accepted for Python-parity; current
        /// Rust impl runs single-threaded — honest scope).
        #[arg(long, value_name = "N")]
        jobs: Option<usize>,
        /// Emit machine-readable JSON diagnostics (D5).
        #[arg(long)]
        json: bool,
        /// JSON framing when --json is set: `array` (default) or
        /// `ndjson` (one diagnostic per line, streaming).
        #[arg(long, default_value = "array")]
        format: String,
        /// Opt into legacy fail-on-first behavior (D8). Equivalent
        /// to `AXON_PARSER_STRICT=1` env var (OR semantics).
        #[arg(long)]
        strict: bool,
        #[arg(long)]
        no_color: bool,
    },
    /// Round-trip formatter. Token-level formatter
    /// preserving comments verbatim; cosmetic normalisation only
    /// (trailing whitespace + final newline).
    Fmt {
        file: String,
        /// Exit non-zero if the file is not already formatted (CI
        /// gate). Does not modify the file.
        #[arg(long)]
        check: bool,
        /// Write the formatted output back to the file in place.
        #[arg(long)]
        write: bool,
        #[arg(long)]
        no_color: bool,
    },
    /// `axon fix`: the AuthorizationCoverage migration
    /// (doctrine `every_boundary_is_guarded`). Walks `.axon` files and
    /// inserts `public: true` into every DISPATCHING `axonendpoint` that
    /// declares no coverage (no `requires:` / `shield:` / `compliance:`)
    /// and no `public:` — turning the old "silently uncovered" state
    /// into an explicit, auditable opt-out. This is the one-shot migration
    /// for the axon-T890 hard break; it never touches an already-covered or
    /// already-`public` endpoint (idempotent).
    Fix {
        /// File path or directory (walked recursively for `.axon`).
        #[arg(required = true)]
        patterns: Vec<String>,
        /// Report what WOULD change and exit non-zero if anything would,
        /// WITHOUT modifying files (CI gate).
        #[arg(long)]
        check: bool,
    },
}

#[derive(Subcommand)]
enum StoreCommands {
    /// Introspect one (or more) `postgresql` axonstore's live schema
    /// and emit a canonical `.axon-schema.json` manifest. The manifest
    /// is the durable contract between an adopter's
    /// `schema: "qualified.name"` (form b) / `schema: env:VAR` (form
    /// c) declaration and the columns the type-checker proves against
    /// (38.d / 38.e). Unmappable Postgres types (`enum`, `domain`,
    /// array, `citext`, PostGIS, custom composites) are HONESTLY
    /// omitted with a `# omitted: …` comment line — NEVER silently
    /// lossily mapped (D10 / D6).
    Introspect {
        /// Store names to introspect. At least one required.
        #[arg(required = true)]
        store_names: Vec<String>,
        /// Postgres connection string OR `env:VAR`. (The runtime's
        /// `resolve_dsn` accepts both shapes.)
        #[arg(long)]
        connection: String,
        /// Optional path to write the manifest. When omitted, the
        /// manifest is written to stdout.
        #[arg(long)]
        output: Option<String>,
        /// Optional path to an existing manifest. When set, the CLI
        /// emits a structural diff instead of the new manifest —
        /// added/removed columns + type changes + constraint flips.
        #[arg(long)]
        diff: Option<String>,
        /// Forward-compat — `--json` is the default + only
        /// supported output format today; `--yaml` (a future
        /// `yaml-manifest` Cargo feature) is reserved.
        /// Accepting the flag now so adopters can pin it before YAML
        /// support lands.
        #[arg(long, default_value = "json")]
        format: String,
    },
}

#[derive(Subcommand)]
enum PccCommands {
    /// Compile a `.axon` file and emit a Proof-Carrying Code bundle
    /// (JSON) certifying its declared contract across all five property
    /// classes (compliance / effects / capability / resources /
    /// shields). The bundle travels alongside the artifact; a consumer
    /// runs `axon pcc verify` to check it WITHOUT trusting this
    /// compiler.
    Prove {
        /// The `.axon` source file to prove.
        file: String,
        /// Optional path to write the bundle. Stdout when omitted.
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Independently verify a proof bundle against a `.axon` source.
    /// Recompiles the source (an independent re-derivation of the
    /// artifact) and re-checks every proof; the per-proof `artifact_digest`
    /// binding catches a bundle minted for different source. Exit 0 iff
    /// every proof verifies; exit 1 if any is refuted / mismatched.
    Verify {
        /// The `.axon` source file the bundle claims to be about.
        file: String,
        /// The proof bundle JSON (as emitted by `axon pcc prove`).
        bundle: String,
    },
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    let cli = Cli::parse();

    let exit_code = match cli.command {
        Commands::Version => {
            println!("axon-lang {AXON_VERSION}");
            0
        }
        Commands::Check { file, no_color, strict, schemas_dir } => {
            checker::run_check(&file, no_color, strict, schemas_dir.as_deref())
        }
        Commands::Desugar { file } => run_desugar(&file),
        Commands::Compile {
            file,
            backend,
            output,
            stdout,
        } => compiler::run_compile(&file, &backend, output.as_deref(), stdout),
        Commands::Run {
            file,
            backend,
            trace,
            tool_mode,
            stream,
            output,
            export_plan,
        } => runner::run_run(&file, &backend, trace, &tool_mode, stream, &output, export_plan),
        Commands::Trace { file, no_color } => tracer::run_trace(&file, no_color),
        Commands::Repl => repl::run_repl(),
        Commands::Inspect { target, all } => inspect::run_inspect(&target, all),
        Commands::Ld { action, file } => lambda_data::run_ld(&action, &file),
        Commands::Serve {
            host,
            port,
            channel,
            auth_token,
            log_level,
            log_format,
            log_file,
            database_url,
            strict_type_driven_transport,
            backend,
            schemas_dir,
        } => run_serve_dispatch(
            host,
            port,
            channel,
            auth_token,
            log_level,
            log_format,
            log_file,
            database_url,
            strict_type_driven_transport,
            backend,
            schemas_dir,
        ),
        Commands::Diff {
            file_a,
            file_b,
            json,
        } => plan_diff::run_diff(&file_a, &file_b, json),
        Commands::Replay {
            file,
            compare,
            json,
        } => replay::run_replay(&file, compare.as_deref(), json),
        Commands::Stats { files, json, format } => {
            let effective_format = if json { "json".to_string() } else { format };
            trace_stats::run_stats(&files, &effective_format)
        }
        Commands::Graph { file, format } => graph_export::run_graph(&file, &format),
        Commands::Estimate { file, format, model } => cost_estimator::run_estimate(&file, &format, &model),
        Commands::Deploy {
            file,
            server,
            backend,
            auth_token,
        } => deployer::run_deploy(&deployer::DeployConfig {
            file,
            server,
            backend,
            auth_token,
        }),
        Commands::Dossier { file, output } => audit_cli::run_dossier(&file, output.as_deref()),
        Commands::Sbom { file, output } => audit_cli::run_sbom(&file, output.as_deref()),
        Commands::Audit { file, framework, output } => {
            audit_cli::run_audit(&file, &framework, output.as_deref())
        }
        Commands::EvidencePackage { file, output, note } => {
            audit_cli::run_evidence_package(&file, output.as_deref(), &note)
        }
        Commands::Store { action } => run_store_command(action),
        Commands::Pcc { action } => match action {
            PccCommands::Prove { file, output } => {
                axon::pcc_cli::run_pcc_prove(&file, output.as_deref())
            }
            PccCommands::Verify { file, bundle } => {
                axon::pcc_cli::run_pcc_verify(&file, &bundle)
            }
        },
        Commands::Parse {
            patterns,
            max_errors,
            ignore,
            jobs,
            json,
            format,
            strict,
            no_color,
        } => run_parse_command(
            patterns, max_errors, ignore, jobs, json, format, strict, no_color,
        ),
        Commands::Fmt { file, check, write, no_color } => {
            run_fmt_command(&file, check, write, no_color)
        }
        Commands::Fix { patterns, check } => run_fix_command(&patterns, check),
    };

    process::exit(exit_code);
}

// ── §Fase 89.b.2 — `axon fix` AuthorizationCoverage migration ────────────────

/// One uncovered dispatching endpoint that `axon fix` will annotate.
struct FixSite {
    name: String,
    /// Byte offset of the endpoint block's matching closing `}`.
    close_off: usize,
}

/// Scan `src` from `start` for the first `{`, then return the byte offset of
/// its matching `}` (brace-depth counted, string literals skipped). `None` if
/// unbalanced.
fn matching_block_close(src: &str, start: usize) -> Option<usize> {
    let b = src.as_bytes();
    let mut i = start;
    while i < b.len() && b[i] != b'{' {
        i += 1;
    }
    if i >= b.len() {
        return None;
    }
    let mut depth = 0usize;
    let mut in_str = false;
    let mut esc = false;
    while i < b.len() {
        let c = b[i];
        if in_str {
            if esc {
                esc = false;
            } else if c == b'\\' {
                esc = true;
            } else if c == b'"' {
                in_str = false;
            }
        } else {
            match c {
                b'"' => in_str = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

/// Convert a 1-indexed (line, column) `Loc` into a byte offset in `src`.
fn line_col_to_offset(src: &str, line: u32, col: u32) -> usize {
    let mut off = 0usize;
    for (idx, l) in src.split_inclusive('\n').enumerate() {
        if (idx as u32) + 1 == line {
            return (off + col.saturating_sub(1) as usize).min(src.len());
        }
        off += l.len();
    }
    src.len()
}

/// Rewrite one `.axon` source string, inserting `public: true` into every
/// uncovered dispatching endpoint. Returns `(new_src, fixed_names)`; the source
/// is unchanged when nothing needs fixing (idempotent).
fn fix_source(src: &str) -> (String, Vec<String>) {
    use axon::ast::Declaration;
    let tokens = match axon::lexer::Lexer::new(src, "<fix>").tokenize() {
        Ok(t) => t,
        Err(_) => return (src.to_string(), Vec::new()),
    };
    let program = match axon::parser::Parser::new(tokens).parse() {
        Ok(p) => p,
        Err(_) => return (src.to_string(), Vec::new()),
    };
    let mut sites: Vec<FixSite> = Vec::new();
    for decl in &program.declarations {
        if let Declaration::AxonEndpoint(ep) = decl {
            // Mirror the §89.b rule EXACTLY: a dispatching endpoint that is
            // uncovered and not already `public: true`.
            let dispatches = !ep.execute_flow.is_empty();
            let covered = !ep.requires_capabilities.is_empty()
                || !ep.shield_ref.is_empty()
                || !ep.compliance.is_empty();
            if dispatches && !covered && !ep.public {
                let start = line_col_to_offset(src, ep.loc.line, ep.loc.column);
                if let Some(close) = matching_block_close(src, start) {
                    sites.push(FixSite {
                        name: ep.name.clone(),
                        close_off: close,
                    });
                }
            }
        }
    }
    if sites.is_empty() {
        return (src.to_string(), Vec::new());
    }
    // Apply bottom-up so earlier offsets stay valid as we splice.
    sites.sort_by(|a, b| b.close_off.cmp(&a.close_off));
    let mut out = src.to_string();
    let mut fixed = Vec::new();
    for site in &sites {
        // Insert after the last non-whitespace char before the `}` so the
        // result reads `… execute: F public: true }` regardless of the
        // pre-existing spacing.
        let mut e = site.close_off;
        let bytes = out.as_bytes();
        while e > 0 && bytes[e - 1].is_ascii_whitespace() {
            e -= 1;
        }
        out.insert_str(e, " public: true");
        fixed.push(site.name.clone());
    }
    fixed.reverse(); // report in source order
    (out, fixed)
}

/// Recursively collect `.axon` files under `path` (a file or directory).
fn collect_axon_files(path: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    if path.is_dir() {
        if let Ok(entries) = std::fs::read_dir(path) {
            let mut children: Vec<_> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
            children.sort();
            for child in children {
                collect_axon_files(&child, out);
            }
        }
    } else if path.extension().and_then(|s| s.to_str()) == Some("axon") {
        out.push(path.to_path_buf());
    }
}

/// `axon fix` dispatcher. Walks the patterns, rewrites each `.axon` file
/// in place (or reports under `--check`), and exits non-zero under `--check`
/// when anything would change (a CI gate).
fn run_fix_command(patterns: &[String], check: bool) -> i32 {
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    for p in patterns {
        collect_axon_files(std::path::Path::new(p), &mut files);
    }
    if files.is_empty() {
        eprintln!("axon fix: no `.axon` files found in {patterns:?}");
        return 1;
    }
    let mut changed_files = 0usize;
    let mut total_fixed = 0usize;
    for file in &files {
        let src = match std::fs::read_to_string(file) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("axon fix: cannot read {}: {e}", file.display());
                continue;
            }
        };
        let (new_src, fixed) = fix_source(&src);
        if fixed.is_empty() {
            continue;
        }
        changed_files += 1;
        total_fixed += fixed.len();
        if check {
            println!(
                "would fix {} ({} endpoint{}): {}",
                file.display(),
                fixed.len(),
                if fixed.len() == 1 { "" } else { "s" },
                fixed.join(", ")
            );
        } else if let Err(e) = std::fs::write(file, new_src) {
            eprintln!("axon fix: cannot write {}: {e}", file.display());
            return 1;
        } else {
            println!(
                "fixed {} ({} endpoint{}): {}",
                file.display(),
                fixed.len(),
                if fixed.len() == 1 { "" } else { "s" },
                fixed.join(", ")
            );
        }
    }
    if changed_files == 0 {
        println!("axon fix: nothing to do — every endpoint is already covered or `public`.");
        return 0;
    }
    if check {
        println!(
            "axon fix --check: {total_fixed} endpoint(s) across {changed_files} file(s) need `public: true`."
        );
        return 1; // CI gate: uncovered endpoints remain
    }
    println!("axon fix: annotated {total_fixed} endpoint(s) across {changed_files} file(s).");
    0
}

/// `axon parse` subcommand dispatcher. Delegates the
/// taxonomy + walk to `axon::cli_parse` and emits the report in the
/// requested format (human / JSON array / NDJSON).
fn run_parse_command(
    patterns: Vec<String>,
    max_errors: Option<usize>,
    ignore: Vec<String>,
    jobs: Option<usize>,
    json: bool,
    format: String,
    strict: bool,
    no_color: bool,
) -> i32 {
    let config = axon::cli_parse::ParseConfig {
        patterns,
        max_errors,
        ignore_patterns: ignore,
        jobs,
        json,
        format,
        strict,
        no_color,
    };
    let (diagnostics, io_errors, truncated) = axon::cli_parse::run_parse(&config);
    if config.json {
        print!("{}", axon::cli_parse::render_json(&diagnostics, &config.format));
    } else {
        print!(
            "{}",
            axon::cli_parse::render_human(
                &diagnostics,
                &io_errors,
                truncated,
                config.no_color,
            )
        );
    }
    axon::cli_parse::exit_code(&diagnostics, &io_errors)
}

/// `axon fmt` subcommand dispatcher. Reads the file,
/// runs the token-level round-trip formatter, dispatches to
/// stdout / --check / --write mode.
/// `axon desugar <file>`: print the exact lower-level program
/// the sugar compiled to. For each `voice`, the generated expansion source;
/// for each preset-instantiated `upstream`, the fully-merged declaration.
/// A file with no sugar prints a note and exits 0 (nothing was hidden).
fn run_desugar(file: &str) -> i32 {
    let source = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot read {file}: {e}");
            return 1;
        }
    };
    let tokens = match axon::lexer::Lexer::new(&source, file).tokenize() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error: lex failed: {e:?}");
            return 1;
        }
    };
    // Parser::parse already ran the §80.g voice + §80.f preset expansions —
    // what we print IS what the type-checker checked and the IR carries.
    let program = match axon::parser::Parser::new(tokens).parse() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: parse failed at {}:{}: {}", e.line, e.column, e.message);
            return 1;
        }
    };
    let mut printed_any = false;
    for decl in &program.declarations {
        match decl {
            axon::ast::Declaration::Voice(v) => {
                printed_any = true;
                println!("// ── voice {} expands to ──────────────────────────", v.name);
                // The ots pair prints with the first mulaw voice, exactly as
                // the expansion injected it.
                print!("{}", axon::voice_desugar::expansion_source(v, true));
                println!();
            }
            axon::ast::Declaration::Upstream(u) if u.preset.is_some() => {
                printed_any = true;
                println!("// ── upstream {} (preset-expanded) ────────────────", u.name);
                print!("{}", axon::upstream_presets::render_upstream(u));
                println!();
            }
            // §87.g — `savant` is NOT sugar (it lowers straight to IRSavant), so
            // there is no lower-level program to print. Instead show an honest
            // COMPOSITION view: which existing primitives + engines it
            // orchestrates. Clearly labelled so it never masquerades as an
            // expansion.
            axon::ast::Declaration::Savant(s) => {
                printed_any = true;
                println!("// ── savant {} composes ───────────────────────────", s.name);
                println!("//   (a composition view, not a macro-expansion: `savant`");
                println!("//    lowers directly to IRSavant.)");
                println!("//   domain    → {:?}", s.domain);
                if let Some(c) = &s.cognition {
                    println!(
                        "//   cognition → active-inference engine (depth: {}, divergence: {})",
                        if c.depth.is_empty() { "default" } else { &c.depth },
                        if c.divergence.is_empty() { "default" } else { &c.divergence }
                    );
                }
                if let Some(m) = &s.memory {
                    if !m.backend.is_empty() {
                        println!("//   memory    → composes `memory`/`corpus` {}", m.backend);
                    }
                }
                if let Some(b) = &s.budget {
                    if let Some(n) = b.max_iterations {
                        println!("//   budget    → linear compute budget (max_iterations: {n})");
                    }
                }
                for md in &s.mandates {
                    println!("//   mandate   → {} -> {}", md.name, md.output_type);
                }
                println!("//   engines   → inference (VFE/EFE) · topology (Betti/PHC) · holograph (HRR)");
                println!();
            }
            _ => {}
        }
    }
    if !printed_any {
        println!(
            "// no `voice` / preset-`upstream` / `savant` declarations — nothing to desugar"
        );
    }
    0
}

fn run_fmt_command(file: &str, check: bool, write: bool, no_color: bool) -> i32 {
    use std::fs;
    use std::path::Path;
    let _ = no_color; // ANSI styling is honest scope deferred — the
                     // dispatcher itself doesn't colour today.
    let path = Path::new(file);
    if !path.exists() {
        eprintln!("✗ File not found: {}", file);
        return 2;
    }
    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("✗ Could not read {}: {}", file, e);
            return 2;
        }
    };
    let formatted = match axon::cli_fmt::format_source(&source) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("✗ Lexer error: {}", e);
            return 2;
        }
    };
    if check {
        // CI gate: exit 1 if the file would be reformatted.
        if formatted != source {
            eprintln!(
                "✗ {}: file is not formatted (run `axon fmt --write` to fix)",
                file
            );
            return 1;
        }
        return 0;
    }
    if write {
        if let Err(e) = fs::write(path, &formatted) {
            eprintln!("✗ Could not write {}: {}", file, e);
            return 2;
        }
        return 0;
    }
    // Default mode: emit to stdout.
    print!("{}", formatted);
    0
}

/// `axon store …` subcommand dispatcher. Spins a
/// one-shot Tokio runtime (the CLI runs in a sync `fn main`; the
/// introspection uses `sqlx::PgConnection` which is async).
fn run_store_command(action: StoreCommands) -> i32 {
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("axon-lang: failed to start async runtime: {e}");
            return 2;
        }
    };
    match action {
        StoreCommands::Introspect {
            store_names,
            connection,
            output,
            diff,
            format,
        } => {
            if format != "json" {
                eprintln!(
                    "axon-lang: `--format {format}` is not yet supported. \
                     v1.38.0 ships canonical JSON only; YAML (`--format \
                     yaml`) is committed for a 38.c.2 follow-on behind \
                     the `yaml-manifest` Cargo feature."
                );
                return 2;
            }
            runtime.block_on(async move {
                run_store_introspect(&store_names, &connection, output.as_deref(), diff.as_deref())
                    .await
            })
        }
    }
}

/// The actual introspection + emission. Returns the process exit code.
#[cfg_attr(not(feature = "postgres"), allow(unused_variables))]
async fn run_store_introspect(
    store_names: &[String],
    connection: &str,
    output: Option<&str>,
    diff_path: Option<&str>,
) -> i32 {
    // §Fase 118.b.3 — the REFUSAL. `axon store introspect` reads a LIVE database's
    // catalog, so it is the Postgres analogue of `axon serve`: the subcommand stays
    // in `axon store --help` under every profile and refuses in writing, naming the
    // exact reinstall command. §111 doctrine — a capability that silently vanishes
    // from a build is indistinguishable, to the adopter, from one that never
    // existed. Exit code 2, matching `axon serve` and `axon evidence-package`.
    #[cfg(not(feature = "postgres"))]
    {
        eprintln!(
            "X `axon store introspect` requires the `postgres` feature - this build was compiled without it, so no PostgreSQL driver is linked and there is no database catalog to read.
  Reinstall with: cargo install axon-lang --features postgres
  (`axon check` still type-checks every `axonstore` declaration in this build, including `backend: postgresql` - only reading a LIVE schema needs the driver.)"
        );
        return 2;
    }
    #[cfg(feature = "postgres")]
    {
        use axon::store::introspect_cli::{
            introspect_stores, render_introspection_output,
        };
        use axon::store_introspect::{format_manifest_diff, manifest_diff};
        use axon::store_schema_manifest::Manifest;

        let (manifest, omissions) = match introspect_stores(connection, store_names).await {
            Ok(pair) => pair,
            Err(e) => {
                eprintln!("axon-lang: introspection failed — {e}");
                return 1;
            }
        };

        // — Diff mode: compare against an existing manifest. —
        if let Some(existing_path) = diff_path {
            let existing_src = match std::fs::read_to_string(existing_path) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!(
                        "axon-lang: failed to read existing manifest at \
                         `{existing_path}`: {e}"
                    );
                    return 1;
                }
            };
            let existing = match Manifest::parse_json(&existing_src) {
                Ok(m) => m,
                Err(e) => {
                    eprintln!(
                        "axon-lang: failed to parse existing manifest at \
                         `{existing_path}` — {e}"
                    );
                    return 1;
                }
            };
            let diff = manifest_diff(&existing, &manifest);
            if diff.is_empty() {
                println!("manifest is up to date — no drift between live database and `{existing_path}`.");
                return 0;
            }
            print!("{}", format_manifest_diff(&diff));
            for omission in &omissions {
                println!("{}", omission.as_comment_line());
            }
            return 0;
        }

        // — Default mode: emit the manifest. —
        let rendered = render_introspection_output(&manifest, &omissions);
        match output {
            Some(path) => match std::fs::write(path, &rendered) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("axon-lang: failed to write manifest to `{path}`: {e}");
                    1
                }
            },
            None => {
                println!("{rendered}");
                0
            }
        }
    }
}

// ── §Fase 89.b.2 — `axon fix` codemod unit tests ────────────────────────────
#[cfg(test)]
mod fix_tests {
    use super::fix_source;

    const FLOW: &str = "flow Chat() -> Unit { step S { ask: \"hi\" } }\n";

    /// Re-run the axon-T890 type-checker rule on `src` and report whether T890
    /// fires — the codemod's output MUST clear it (the strongest pin).
    fn fires_t890(src: &str) -> bool {
        let tokens = axon::lexer::Lexer::new(src, "<t>").tokenize().expect("lex");
        let prog = axon::parser::Parser::new(tokens).parse().expect("parse");
        axon::type_checker::TypeChecker::new(&prog)
            .check()
            .into_iter()
            .any(|e| e.message.contains("axon-T890"))
    }

    #[test]
    fn bare_dispatching_endpoint_gets_public_true() {
        let src = format!("{FLOW}axonendpoint E {{ method: POST path: \"/c\" execute: Chat }}");
        assert!(fires_t890(&src), "precondition: bare endpoint fires T890");
        let (out, fixed) = fix_source(&src);
        assert_eq!(fixed, vec!["E".to_string()]);
        assert!(out.contains("public: true"), "codemod inserts public: true. Got: {out}");
        assert!(!fires_t890(&out), "codemod output must clear T890. Got: {out}");
    }

    #[test]
    fn covered_endpoint_is_untouched() {
        let src = format!(
            "{FLOW}axonendpoint E {{ method: POST path: \"/c\" execute: Chat requires: [flow.execute] }}"
        );
        let (out, fixed) = fix_source(&src);
        assert!(fixed.is_empty(), "a covered endpoint must not be fixed");
        assert_eq!(out, src, "a covered endpoint's source is unchanged");
    }

    #[test]
    fn already_public_endpoint_is_untouched() {
        let src = format!(
            "{FLOW}axonendpoint E {{ method: POST path: \"/c\" execute: Chat public: true }}"
        );
        let (out, fixed) = fix_source(&src);
        assert!(fixed.is_empty(), "an already-public endpoint must not be re-fixed");
        assert_eq!(out, src);
    }

    #[test]
    fn non_dispatching_endpoint_is_untouched() {
        let src = "axonendpoint E { method: POST path: \"/c\" }".to_string();
        let (_out, fixed) = fix_source(&src);
        assert!(fixed.is_empty(), "an endpoint with no execute crosses no boundary");
    }

    #[test]
    fn codemod_is_idempotent() {
        let src = format!("{FLOW}axonendpoint E {{ method: POST path: \"/c\" execute: Chat }}");
        let (once, _) = fix_source(&src);
        let (twice, fixed2) = fix_source(&once);
        assert!(fixed2.is_empty(), "second run finds nothing to fix");
        assert_eq!(once, twice, "codemod is idempotent");
    }

    #[test]
    fn multiple_endpoints_all_fixed_in_one_pass() {
        let src = format!(
            "{FLOW}\
             axonendpoint A {{ method: POST path: \"/a\" execute: Chat }}\n\
             axonendpoint B {{ method: GET path: \"/b\" execute: Chat requires: [flow.execute] }}\n\
             axonendpoint C {{ method: PUT path: \"/c\" execute: Chat }}"
        );
        let (out, fixed) = fix_source(&src);
        assert_eq!(fixed, vec!["A".to_string(), "C".to_string()], "A and C fixed, B (covered) skipped");
        assert!(!fires_t890(&out), "all boundaries covered after fix. Got: {out}");
    }
}

// ── `axon serve` — the refusal contract (§Fase 118.b.2) ───────────────────────
//
// The subcommand is declared UNCONDITIONALLY above: it is in `axon --help`, with
// all eleven flags, under every build profile. What changes with the `server`
// feature is what happens when you run it.
//
// This is the §111 doctrine applied to packaging. §111 spent a fase proving that
// every advertised primitive is REAL; the packaging corollary is that the
// advertised surface must stay advertised even when the implementation is
// absent. A subcommand that silently disappears from a build teaches the adopter
// nothing and reads as a broken install; a linker error teaches them less.

/// The `server` build: hand the parsed flags to the HTTP runtime, unchanged.
#[cfg(feature = "server")]
#[allow(clippy::too_many_arguments)]
fn run_serve_dispatch(
    host: String,
    port: u16,
    channel: String,
    auth_token: String,
    log_level: String,
    log_format: String,
    log_file: Option<String>,
    database_url: Option<String>,
    strict_type_driven_transport: bool,
    backend: Option<String>,
    schemas_dir: Option<String>,
) -> i32 {
    axon_server::run_serve(axon_server::ServerConfig {
        host,
        port,
        channel,
        auth_token,
        log_level,
        log_format,
        log_file,
        database_url: database_url.or_else(|| std::env::var("DATABASE_URL").ok()),
        config_path: None,
        // §Fase 31.f (D6 + D7) — Resolution order for the strict
        // flag (highest precedence first):
        //   1. CLI flag `--strict-type-driven-transport` (when
        //      present, always wins — explicit at run-time).
        //   2. Env var `AXON_STRICT_TYPE_DRIVEN_TRANSPORT` (12-
        //      factor app pattern; common in k8s/docker deploys).
        //   3. D6 default `false` (v1.22.x — backwards-compat).
        // D9 ratified — the default flips to `true` in v2.0.0.
        //
        // D7 cross-stack consistency — Python `axon serve`
        // reads the same env var name verbatim. Truthy values
        // are accepted case-insensitively: "1", "true", "yes",
        // "on". Any other value (including unset) is false.
        strict_type_driven_transport: strict_type_driven_transport
            || axon::env_flags::parse_truthy_env(
                "AXON_STRICT_TYPE_DRIVEN_TRANSPORT",
            ),
        // §Fase 36.g (D7) — server default backend (rung 3 of the
        // Backend Resolution Contract). Resolution order, highest
        // precedence first:
        //   1. CLI flag `--backend <name>` (explicit at run-time).
        //   2. Env var `AXON_DEFAULT_BACKEND` (12-factor; common
        //      in k8s/docker deploys).
        //   3. `None` — no server default; the ladder falls
        //      through to the environment-available `auto` rungs.
        // An empty string from either surface collapses to `None`.
        // The value is validated against the closed catalog at
        // `run_serve` startup — an unknown name fails fast.
        default_backend: backend
            .or_else(|| std::env::var("AXON_DEFAULT_BACKEND").ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        // §Fase 38.j (D3 + D7 + D8) — Resolution order for the
        // declared store-schema manifest directory (highest
        // precedence first):
        //   1. CLI flag `--schemas-dir <path>` (explicit at run-
        //      time; common in dev + `docker run` overrides).
        //   2. Env var `AXON_SCHEMAS_DIR` (12-factor app pattern;
        //      common in k8s/docker `Deployment` manifests).
        //   3. `None` — no manifest loading; v1.37.0 deploy-time
        //      verify is preserved verbatim (D5 absolute).
        // An empty string from either surface collapses to `None`.
        // The directory's existence is NOT verified at startup —
        // a missing dir resolves to "no manifest files" the same
        // way an empty dir does (`load_and_merge_manifests` is
        // total). Failures, if any, surface at deploy time as
        // structured T805/T807/duplicate-store errors.
        schemas_dir: schemas_dir
            .or_else(|| std::env::var("AXON_SCHEMAS_DIR").ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
    })
}

/// The lean build: refuse in writing, and say exactly how to fix it.
///
/// Three things this message must do, each a lesson from an earlier release:
///   1. NAME THE REINSTALL COMMAND. The 2.81.0 `aws-secrets` opt-out was worth
///      nothing to an adopter who could not find the way back in.
///   2. NAME THE OTHER BINARY. Since 2.82.0 the server is `axon-server`, not a
///      flag on `axon` — telling someone only to add a feature would leave them
///      running `axon serve` in a build where it will never be the answer.
///   3. SAY WHAT STILL WORKS. A refusal that reads as "this install is broken"
///      is worse than the linker error it replaced; this one says the compiler
///      and governance surface is intact, which is the whole point of the split.
///
/// Exit code 2 — the same code `axon evidence-package` returns for an absent
/// `documents` feature. Distinct from 1 (a real failure) so CI can
/// tell "wrong build profile" from "the flow was rejected".
#[cfg(not(feature = "server"))]
#[allow(clippy::too_many_arguments)]
fn run_serve_dispatch(
    _host: String,
    _port: u16,
    _channel: String,
    _auth_token: String,
    _log_level: String,
    _log_format: String,
    _log_file: Option<String>,
    _database_url: Option<String>,
    _strict_type_driven_transport: bool,
    _backend: Option<String>,
    _schemas_dir: Option<String>,
) -> i32 {
    eprintln!(
        "X `axon serve` requires the `server` feature - this build was compiled without it, so the HTTP runtime is absent.
  The server is a separate binary: install it with
      cargo install axon-lang --features server
  and run `axon-server` (the `axon` you have keeps working for everything else).
  Still available in this build: check, compile, parse, fmt, fix, desugar, inspect, trace, stats, graph, estimate, diff, ld, repl, run, deploy, replay, dossier, sbom, audit, prove, verify."
    );
    2
}
