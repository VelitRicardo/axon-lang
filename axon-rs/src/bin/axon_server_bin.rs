//! `axon-server` — the AXON runtime daemon.
//!
//! v2.81.0, the design decision (RATIFIED 2026-08-04, option (c)). The second binary of
//! the `axon-lang` crate, built only when the `server` feature is on.
//!
//! **This is a ROLE boundary, not the open-core boundary.** `axon` and
//! `axon-server` are both AGPL and both live in this crate; the commercial line
//! lives where it already lives — the `axon-enterprise` repository, its EULA,
//! its privileged layer (multi-tenant custody, audit hash-chain, SSO, per-tenant
//! policy). The split here is the one every engineer already holds from
//! `docker`/`dockerd`, `psql`/`postgres`, `redis-cli`/`redis-server`: the client
//! and the daemon are different artefacts because they are run by different
//! people at different times.
//!
//! | | what it is | who runs it |
//! |---|---|---|
//! | `axon` | the compiler: compliance as a type error | a developer, on every PR |
//! | `axon-server` | the runtime: executes governed flows | a platform team |
//!
//! The flag surface is byte-identical to `axon serve` — same names, same
//! defaults, same env-var fallbacks, same resolution order — because it is the
//! same `ServerConfig` and the same `run_serve`. `axon serve` is NOT deprecated
//! and NOT removed; under a `server` build it keeps working exactly as before,
//! and under a lean build it refuses in writing and points here (see `main.rs`).

use axon::axon_server;
use clap::Parser;
use std::process;

#[derive(Parser)]
#[command(
    name = "axon-server",
    about = "AXON runtime — serves governed flows over HTTP/SSE/WebSocket.",
    long_about = "The AXON runtime daemon: deploys and executes governed flows, \
serving them over HTTP, SSE and WebSocket.\n\n\
The compiler and governance CLI is the separate `axon` binary — use it to \
type-check (`axon check`), compile, and produce audit artefacts. Both binaries \
ship from the `axon-lang` crate under AGPL; this one requires the `server` \
feature (`cargo install axon-lang --features server`).",
    version = axon::version::AXON_VERSION,
)]
struct Cli {
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
    /// v1.22.0 (D6 + D9) — Type-Driven Wire Inference activation.
    ///
    /// When set, `POST /v1/execute` promotes to SSE for any flow the
    /// type-checker inferred as stream-producing (D1) regardless of the
    /// client's `Accept:` header. Adopters who explicitly declared
    /// `transport: json` retain D3 opt-out semantics.
    ///
    /// Also readable from the env var `AXON_STRICT_TYPE_DRIVEN_TRANSPORT`
    /// (truthy values: "1", "true", "yes", "on" — case-insensitive). The CLI
    /// flag wins when both are set.
    #[arg(long)]
    strict_type_driven_transport: bool,
    /// v1.31.0 (D7) — Server-wide default execution backend.
    ///
    /// Rung 3 of the Backend Resolution Contract: an `axonendpoint` that
    /// declares no `backend:` of its own inherits this server default. Valid
    /// values are the closed catalog `anthropic | auto | gemini | glm | kimi |
    /// ollama | openai | openrouter | stub` — an unknown name aborts startup
    /// with exit code 1.
    ///
    /// Also readable from the env var `AXON_DEFAULT_BACKEND`; the CLI flag wins
    /// when both are set.
    #[arg(long)]
    backend: Option<String>,
    /// v1.31.0 (D3 + D7 + D8) — Directory containing declared store-schema
    /// manifests (`*.axon-schema.json`).
    ///
    /// When set, `POST /v1/deploy` loads and merges every manifest under the
    /// directory before running the deploy-time store verification pass. Drift
    /// between a manifest and the LIVE Postgres introspection raises axon-T807
    /// (DeclaredVsLiveDrift) and fails the deploy.
    ///
    /// Also readable from the env var `AXON_SCHEMAS_DIR`; the CLI flag wins when
    /// both are set.
    #[arg(long)]
    schemas_dir: Option<String>,
}

fn main() {
    let cli = Cli::parse();

    // The same three-rung resolution order `axon serve` uses (CLI flag > env var
    // > default), against the same canonical truthy parser the Python CLI shares
    // (v1.22.0 D7). Two entry points, one contract — a second binary that resolved
    // its configuration differently would be a second product.
    let exit_code = axon_server::run_serve(axon_server::ServerConfig {
        host: cli.host,
        port: cli.port,
        channel: cli.channel,
        auth_token: cli.auth_token,
        log_level: cli.log_level,
        log_format: cli.log_format,
        log_file: cli.log_file,
        database_url: cli.database_url.or_else(|| std::env::var("DATABASE_URL").ok()),
        config_path: None,
        strict_type_driven_transport: cli.strict_type_driven_transport
            || axon::env_flags::parse_truthy_env("AXON_STRICT_TYPE_DRIVEN_TRANSPORT"),
        default_backend: cli
            .backend
            .or_else(|| std::env::var("AXON_DEFAULT_BACKEND").ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        schemas_dir: cli
            .schemas_dir
            .or_else(|| std::env::var("AXON_SCHEMAS_DIR").ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
    });

    process::exit(exit_code);
}
