# `axon-emcp`

> The official ℰMCP (Epistemic Model Context Protocol) server for
> [AXON](https://github.com/VelitRicardo/axon-lang) — a stdio JSON-RPC 2.0
> Model Context Protocol server that exposes the AXON language to AI
> coding agents (Claude Code, Codex, Cursor, Continue, Cline, …).

[![crates.io](https://img.shields.io/crates/v/axon-emcp.svg)](https://crates.io/crates/axon-emcp)
[![docs.rs](https://docs.rs/axon-emcp/badge.svg)](https://docs.rs/axon-emcp)
[![MCP Registry](https://img.shields.io/badge/MCP%20Registry-io.github.Bemarking%2Faxon--emcp-blue)](https://registry.modelcontextprotocol.io/v0/servers?search=axon-emcp)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

## What it does

Any MCP-compatible coding agent that launches `axon-emcp` as a
subprocess gets:

- **6 tools** — `axon.primitives`, `axon.primitive_doc`, `axon.check`,
  `axon.parse`, `axon.compose`, `axon.examples`. Live language
  reference + structured validation through the same `axon-frontend`
  lexer/parser/type-checker the `axon` CLI uses (byte-identical
  diagnostics), plus a curated drift-gated example corpus organised
  by language idea.
- **14+ resources** under `axon://` — full primitive references
  (`primitives/{name}`), grammar maps (`grammar/{top_level|composition|ebnf}`),
  composition logic (`logic/{flow_composition|session_duality}`), and
  per-framework compliance maps (`compliance/{hipaa|gdpr|pci_dss|sox|soc2|fedramp|gxp|fisma|nist_800_53}`).
- **3 prompts** — `flow_design`, `shield_design`, `session_design`.
  Host-surfaced design recipes that drive the agent through a structured
  workflow (`axon.compose` → `axon.primitive_doc` → `axon.check`).

The result: a developer talks to their AI coding agent in natural
language; the agent writes AXON that passes `axon check` on first
try, with the right compliance shields by construction.

## Install

```bash
cargo install axon-emcp
```

The installed binary is **fully self-contained** — the knowledge
corpus (45 primitive docs + 33 templates + 17 idiomatic examples +
grammar/logic/compliance references) is baked into the executable
via `include_dir!` at compile time. No `share/` directory, no env
vars, no post-install steps.

### Listed on the official MCP Server Registry

The server is registered at
[`registry.modelcontextprotocol.io`](https://registry.modelcontextprotocol.io/v0/servers?search=axon-emcp)
under the canonical namespace `io.github.Bemarking/axon-emcp`. Tools
that consume the registry (Claude Desktop's connector picker, Cursor's
MCP browser, the `mcp-publisher` CLI's discovery surface, future
clients that index `registry.modelcontextprotocol.io/v0/servers`)
list the server automatically. Adopters that prefer the registry-
driven install flow can run:

```bash
# Inspect the canonical metadata:
curl -s 'https://registry.modelcontextprotocol.io/v0/servers?search=axon-emcp' | jq .

# Or use the publisher CLI for inspection:
mcp-publisher status io.github.Bemarking/axon-emcp
```

The crate itself stays on `crates.io` (the registry currently lists
`npm | pypi | oci | nuget | mcpb` as package-source types; `crates.io`
is registry-tracked via the `websiteUrl` field until it lands in the
package-type enum).

```bash
axon-emcp --help
```

## Configure your agent

Point the agent's MCP config at the installed binary:

```jsonc
// Claude Code:    ~/.config/claude-code/mcp.json
// Cursor:         ~/.cursor/mcp.json
// Codex:          ~/.codex/mcp.json
// Continue:       ~/.continue/config.json (under "mcp.servers")
{
  "mcpServers": {
    "axon": {
      "command": "axon-emcp"
    }
  }
}
```

Restart the agent. The 6 tools, 14+ resources, and 3 prompts now
appear in the agent's surface — the agent will quote them when you
ask it to write AXON.

## Closed-domain composition

```
You: write me a healthcare flow that handles PHI.

Agent: [calls axon.compose(intent: "healthcare flow PHI")]
       ✓ Scaffold returned (axon_check_verdict: well-formed)
       ✓ Domain: healthcare (matched: patient, phi, hipaa, …)
       ✓ Compliance applied: HIPAA, GDPR, GxP, SOC2
       ✓ Primitives used: type, persona, shield, flow, axonendpoint

       Here is your scaffold: …
```

The classifier picks from a **closed 33-domain catalogue** spanning
verticals (healthcare, banking, government, legal, legaltech, fintech,
pharmatech, medic_research), agent patterns (chat_research, chat_tools,
chat_skills, whatsapp, voice, dev, sales_consultive, sales_widget), and
application patterns (workflow_automation, business_intelligence,
corporate_integration, self_learning, document_analysis, ticket_triage,
content_moderation, knowledge_extraction, compliance_monitoring,
recruitment, education, financial_advisor, data_pipeline). Every
template proven to compile through the live `axon-frontend` pipeline.

## Focused examples library

Where `axon.compose` gives you a full-app scaffold by *domain*,
`axon.examples` returns minimal complete programs (~20–60 LOC)
organised by *idea*:

```
You: show me how to use weave correctly.

Agent: [calls axon.examples(primitive: "weave")]
       ✓ Match: weave_braid — Weave braid — composing multiple sub-derivations
       ✓ Topic: composition
       ✓ Primitives: persona, flow, step, weave
       ✓ Source: (verified to compile through axon-frontend)

       Here is the canonical pattern: …
```

Filter on three independent axes (combine freely):

- `name:` — single-example resolution by slug, returns full `.axon`
  source the agent can paste / `axon.check` directly.
- `topic:` — closed 10-entry taxonomy (`composition`, `session_types`,
  `shields`, `effects`, `streaming`, `data`, `agents`, `endpoints`,
  `memory`, `validation`).
- `primitive:` — every example that exercises a given primitive name.

Every example is drift-gated through the same `axon-frontend`
pipeline `axon.check` uses, so what the agent receives is **guaranteed
to compile**.

## Contributor surface

Beyond the MCP server mode (no arguments), `axon-emcp` ships two
contributor-facing subcommands:

```bash
# Scaffold a new primitive doc with frontmatter pre-populated from the
# canonical PRIMITIVE_REGISTRY.
axon-emcp scaffold-primitive <name>

# Aggregate a JSONL telemetry log into a structured snapshot.
axon-emcp telemetry summarize <file>
```

## Telemetry (privacy-first)

Opt-in only — no network egress without an explicit env var:

| Env var | Effect |
|---|---|
| `AXON_EMCP_TELEMETRY_FILE` | Append JSONL events to this path (default: disabled) |
| `AXON_EMCP_DEPLOYMENT_ID` | Correlation tag stamped on every event |
| `AXON_EMCP_TELEMETRY_MAX_SAMPLES` | Latency histogram window per tool (default: 1000) |
| `AXON_EMCP_OTLP_ENDPOINT` | OTLP/gRPC collector URL — empty = exporter disabled |
| `AXON_EMCP_OTLP_HEADERS` | Comma-separated `key=value` auth/routing headers |
| `AXON_EMCP_OTLP_INTERVAL_SECS` | Push interval (default: 60) |
| `AXON_EMCP_OTLP_TIMEOUT_SECS` | Per-RPC timeout (default: 10) |

Seven privacy invariants are enforced by construction:

1. AXON source content is **never** recorded.
2. `axon.compose` intent strings are **never** recorded.
3. Tool error messages are **never** recorded.
4. No remote egress without explicit opt-in.
5. Deployment ID is operator-supplied (default: empty).
6. OTLP exporter only spawns when `AXON_EMCP_OTLP_ENDPOINT` is set — no socket, no DNS, no task otherwise.
7. The OTLP wire payload is a pure function of the in-process snapshot — every value crossing the wire came through one of the closed-catalog `record_*` recorders.

### OTLP/gRPC exporter

Set `AXON_EMCP_OTLP_ENDPOINT` and the server pushes a metrics snapshot
every 60s to your collector:

```bash
AXON_EMCP_OTLP_ENDPOINT=http://localhost:4317 axon-emcp
# Or against Honeycomb / Grafana Cloud / Datadog / Lightstep:
AXON_EMCP_OTLP_ENDPOINT=https://api.honeycomb.io:443 \
AXON_EMCP_OTLP_HEADERS=x-honeycomb-team=YOUR_KEY,x-honeycomb-dataset=axon \
  axon-emcp
```

The OTLP payload carries **18 metric families** keyed by `service.name=axon-emcp`,
`service.version`, `deployment.environment` (from `AXON_EMCP_DEPLOYMENT_ID`):

- `axon_emcp.tool.{calls,errors}` (Sum, `tool=` label)
- `axon_emcp.tool.duration.{p50,p95,p99}_us` (Gauge, `tool=` label)
- `axon_emcp.resource.reads` (Sum, `uri_family=` label)
- `axon_emcp.prompt.{calls,missing_required_arg}` (Sum, `prompt=` label)
- `axon_emcp.compose.{total,overrides,by_domain}` (Sum, `domain=` label)
- `axon_emcp.check.{pass,fail}_by_stage` (Sum, `stage=` label)
- `axon_emcp.examples.{total,by_name,empty_responses,by_topic,by_primitive}` (Sum, labelled)

The JSONL sink + OTLP exporter compose freely — file-based pipelines
(Vector / Fluent Bit / otel-collector `filelog` receiver) and the
direct gRPC push both consume the same in-process snapshot.

## License

MIT. See [the repository](https://github.com/VelitRicardo/axon-lang) for
the full sources, the knowledge corpus under `src/knowledge/`, and the
contributor guide.
