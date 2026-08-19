# `src/` — the ℰMCP OFICIAL

> The official MCP server + knowledge base that lets any coding agent
> (Claude Code, Codex, Cursor, Continue, Cline, …) write **professional
> AXON** from natural-language intent.

## Why this exists

AXON was designed for AI agents, not for humans tapping at a keyboard.
Every primitive — `persona`, `flow`, `shield`, `axonendpoint`, `axonstore`,
`socket`, `session`, the lot — was chosen because *an agent driving a
program* benefits from it: typed dialogue, statically-checked compliance,
audit-chained mutations, epistemically-graded retrieval, deadlock-free
sessions. But until v2.3.0 the **agent-facing interface to the language
itself** was the same as the human-facing one: a README, a paper, and a
folder of examples.

`ℰMCP OFICIAL` closes that loop. It is the **Model Context Protocol
server** AI coding agents connect to so they:

1. **Know every primitive precisely** — grammar, top-level vs. nested,
   what compiles vs. what doesn't, idiomatic patterns.
2. **Validate their output live** — `axon.check(source)` returns
   structured diagnostics; an agent can iterate without leaving the
   conversation.
3. **Compose at the right altitude** — `axon.compose(intent)` returns a
   scaffold from a high-level brief: "a healthcare flow that handles PHI"
   becomes a typed `axonendpoint` + `flow` + `shield` skeleton with the
   correct compliance annotations.
4. **Stay grounded** — every answer the server gives is sourced from the
   canonical knowledge base under `src/knowledge/`, which is itself a
   diff-friendly markdown corpus that humans can review and extend.

The result: a developer **talks** to their AI coding agent in natural
language; the agent **writes** AXON that passes `axon check` at the first
try, uses the right compliance shields by construction, and doesn't have
to guess what's top-level vs. what nests inside a `flow`.

## Architecture

```
src/
├── README.md           ← this file
├── knowledge/          ← the canonical source of truth — diff-reviewed
│   ├── primitives/         one markdown file per primitive (Phase 2 ships 7)
│   │   ├── persona.md     anchor.md   flow.md     reason.md
│ │   ├── step.md tool.md socket.md (v2.3.0 newest)
│   │   └── …
│   ├── grammar/            Phase 3 — top_level + composition + ebnf
│   ├── logic/              Phase 3 — flow_composition + session_duality
│   ├── compliance/         Phase 3 — hipaa/gdpr/pci_dss/sox/soc2/fedramp/gxp/fisma/nist_800_53
│   ├── templates/          Phase 4 — 8 axon-check-clean scaffolds (generic, healthcare, …)
│   └── prompts/            Phase 5 — flow_design + shield_design + session_design
└── axon-emcp/          ← the Rust MCP server (stdio JSON-RPC 2.0)
    ├── Cargo.toml
    ├── tests/              integration tests — phase2 canonical + phase4 template drift
    └── src/
        ├── main.rs              stdio transport + server loop
        ├── lib.rs               re-exports for tests + embedders
        ├── server.rs            MCP protocol handshake + dispatch
        ├── knowledge.rs         loads + indexes src/knowledge/ at startup
        ├── compiler_pipeline.rs lex → parse → type-check via axon-frontend
        ├── tools.rs             axon.primitives / primitive_doc / check / parse / compose
        ├── compose.rs           Phase 4 — domain classifier + template emission
        ├── resources.rs         axon://primitives/* + grammar/* + logic/* + compliance/*
        └── prompts.rs           Phase 5 — prompts/list + prompts/get + {{arg}} renderer
```

**Two layers**, deliberately:

- **`knowledge/`** — the source of truth. Markdown with structured
  frontmatter. Humans edit it; the server reads it. Adding a primitive
  or correcting a rule is a `.md` diff, reviewable by anyone.
- **`axon-emcp/`** — the runtime. A small, focused Rust binary that
  speaks MCP (stdio JSON-RPC 2.0) and projects the knowledge base into
  the protocol surface. Consumes `axon-frontend` for live validation
  (the same lexer/parser/type-checker the `axon` CLI uses).

This separation matters: **the knowledge base outlives the server**.
Tomorrow's MCP variants, today's plain-prompt agents, an LSP server, a
docs site — they all hydrate from the same `knowledge/`.

## How agents use it

Once installed (see `axon-emcp/README.md` for `mcp.json` snippets per
agent), the agent has access to:

### Tools (it can call these)

| Tool | Phase | What it does |
|---|---|---|
| `axon.primitives(filter?)` | **0 ✅** | List every primitive, optionally filtered by category (`cognition` / `cognitive_io` / `data_plane` / `session_types` / `wire` / `operators`) |
| `axon.primitive_doc(name)` | **0 ✅** | Full reference for one primitive: grammar, top-level status, since-version, complete markdown body (semantic constraints + examples + what-it-is-not + see-also) |
| `axon.check(source)` | **1 ✅** | Validate `.axon` source through the same lex → parse → type-check pipeline `axon check` uses. Returns `{ ok, stage, errors[], warnings[], summary }`. `isError: true` flips on a blocking failure so the agent's "go fix it" reflex fires |
| `axon.parse(source)` | **1 ✅** | Parse to IR (JSON). On success returns `{ ok: true, ir: { node_type: "program", personas, flows, … }, … }`; on failure returns the same diagnostic shape as `axon.check` (uniform parser surface for the agent) |
| `axon.examples(topic)` | 2 | Canonical `.axon` programs by topic (healthcare, banking, chat, multi-agent…) |
| `axon.compose(intent)` | **4 ✅** | Natural-language brief → typed `.axon` scaffold. Closed-domain classifier (generic/healthcare/banking/government/legal/chat/retrieval/multi_agent) drives template selection; every scaffold round-trips through the live `axon-frontend` pipeline before return |
| `axon.validate_pattern(source, pattern)` | 4 | Check whether a source fragment matches an idiomatic pattern (e.g. "is this a well-formed dual session?") |

### Resources (it can read these)

| URI | What it serves |
|---|---|
| `axon://primitives/{name}` | Same content as `axon.primitive_doc` but as a read-only resource the agent can quote |
| `axon://grammar/top_level` | The full top-level vs. nested table |
| `axon://grammar/ebnf` | The EBNF grammar |
| `axon://logic/flow_composition` | When do you nest in `flow` vs. declare top-level? |
| `axon://logic/session_duality` | The v2.3.0 algebra rules for dual sessions |
| `axon://compliance/{framework}` | What `compliance: [...]` annotations cover which framework |

### Prompts (the host surfaces these as slash-commands / chat-menu entries)

| Name | Phase | What it does |
|---|---|---|
| `flow_design` | **5 ✅** | Turn a natural-language flow intent into a typed, anchored, optionally-streaming AXON program. Drives the agent through `axon.compose` → `axon.primitive_doc` → `axon.check`. Arguments: `intent` (required), `domain`, `streaming`, `compliance`. |
| `shield_design` | **5 ✅** | Turn a shield purpose (PHI redaction, jailbreak defence, financial scrubbing) into a typed `shield` declaration with the right scan list, on_breach policy, and compliance tags. Arguments: `purpose` (required), `severity`, `compliance`. |
| `session_design` | **5 ✅** | Turn a dialogue intent (chat, RPC, multiparty) into a v2.3.0 duality-correct `session` + `socket` pair honouring linearity + credit-refined backpressure + multiparty projection. Arguments: `intent` (required), `parties`, `backpressure`, `reconnect`. |

## Install

```bash
# From inside a clone of the axon-lang repo:
cargo install --path src/axon-emcp
# → `axon-emcp` is installed to ~/.cargo/bin (or %USERPROFILE%\.cargo\bin)
```

The installed binary is **fully self-contained** — Phase 1 vendored the
knowledge corpus into the executable via `include_dir!` at compile time.
No `share/`, no env var, no post-install steps. Verify:

```bash
axon-emcp --help  # (or run it under your agent — it speaks MCP on stdio)
```

Then point your agent's MCP config at it:

```jsonc
// Claude Code: ~/.config/claude-code/mcp.json (or platform equivalent)
// Cursor:      ~/.cursor/mcp.json
// Codex:       ~/.codex/mcp.json
// Continue:    ~/.continue/config.json (under "mcp.servers")
{
  "mcpServers": {
    "axon": {
      "command": "axon-emcp"
    }
  }
}
```

That's it — restart the agent and it will see **5 tools** (`axon.primitives`,
`axon.primitive_doc`, `axon.check`, `axon.parse`, `axon.compose`),
**14 resources** (`axon://primitives/{name}` + `axon://grammar/*` +
`axon://logic/*` + `axon://compliance/*`), and **3 prompts**
(`flow_design`, `shield_design`, `session_design`) — surfaced by the
host as slash-commands or chat-menu entries — plus the onboarding
instructions on connect.

### Hot-editing the corpus (contributors)

If you're contributing to the corpus (`src/knowledge/primitives/*.md`)
and want edits to take effect WITHOUT recompiling:

```bash
# Option A: run from the repo tree — the binary prefers the in-tree
# dev path over the embedded copy automatically.
cargo run --manifest-path src/axon-emcp/Cargo.toml --release

# Option B: point an installed binary at a checkout via env var.
AXON_EMCP_KNOWLEDGE_DIR=/path/to/axon-lang/src/knowledge axon-emcp
```

The corpus-resolution order (first hit wins): `AXON_EMCP_KNOWLEDGE_DIR`
env var → in-tree dev path (`<crate>/../knowledge`) → embedded corpus.

## Status

| Phase | Surface | Status |
|---|---|---|
| **0** | Server spine (stdio JSON-RPC 2.0), knowledge loader, `axon.primitives` + `axon.primitive_doc`, `axon://primitives/{name}` resources, `socket` primitive documented end-to-end | ✅ |
| **1** | `axon.check` (live validation) + `axon.parse` (IR introspection) + embedded corpus (`cargo install` ships self-contained) | ✅ |
| **2** | The 6 **core cognitive primitives** — `persona`, `flow`, `step`, `anchor`, `tool`, `reason` — each backed by a canonical `.axon` example that round-trips through `axon-frontend` end-to-end. Tier 0 baseline (followed by Tier 1/2/3 in v1.2.0 for full coverage). | ✅ |
| **3** | **Reference resources** — `axon://grammar/{top_level\|composition\|ebnf}`, `axon://logic/{flow_composition\|session_duality}`, `axon://compliance/{hipaa\|gdpr\|pci_dss\|sox\|soc2\|fedramp\|gxp\|fisma\|nist_800_53}`. The Catalog now loads `grammar/`, `logic/`, `compliance/` markdown alongside `primitives/`; the resource dispatcher serves all four URI families with structured errors. | ✅ |
| **4** | **`axon.compose(intent)`** — natural-language brief → typed scaffold. Closed-domain classifier (keyword scoring + explainable scoreboard) over 8 domains (`generic`, `healthcare`, `banking`, `government`, `legal`, `chat`, `retrieval`, `multi_agent`); each scaffold is a hand-authored `.axon` template proven to compile end-to-end through the live `axon-frontend` pipeline. Returns `{scaffold, domain, alternatives, primitives_used, compliance_applied, next_steps, axon_check_verdict}`. | ✅ |
| **5** | **MCP prompts** — `flow_design`, `shield_design`, `session_design` exposed via `prompts/list` + `prompts/get`. Each prompt is a hand-authored markdown body with declared `arguments:` schema; `{{arg}}` placeholders render at `get` time from user-supplied values. The `initialize` handshake now advertises the `prompts` capability so hosts surface the recipes as slash-commands. | ✅ |
| **6.a** | **The fórmula** — `axon_frontend::PRIMITIVE_REGISTRY` is the closed catalogue of every named language construct (47 entries). A coverage gate test enforces the closed pair: every `Documented` registry entry has a `.md`, every `.md` has a `Documented` registry entry. The `axon-emcp scaffold-primitive <name>` subcommand stamps frontmatter-correct skeletons from the registry — drift impossible by construction. | ✅ |
| **6.b** | **Tier 1 — 10 primitives documented** (`context`, `intent`, `memory`, `agent`, `probe`, `validate`, `refine`, `weave`, `type`, `run`) + 12 canonical `.axon` examples drift-gated. The drift gate caught 3 first-draft schema bugs (`memory.store:` is the lifecycle catalog, not store-kind). | ✅ |
| **6.c** | **Tier 2 — 12 primitives documented** (`resource`, `fabric`, `manifest`, `observe`, `reconcile`, `lease`, `ensemble`, `session`, `axonstore`, `dataspace`, `corpus`, `pix`) + 15 canonical examples. **Registry honesty rebaseline**: `taint` removed (47→46 entries) because it has a lexer token but no parser production. Drift gate caught 4 schema bugs (axonstore column types are the closed v1.38.0 catalog, AXON comments are `//` not `#`, session duality on recursive loop+select+branch is computationally heavy → simpler RPC + finite-select shapes for the canonical test). Coverage now **29/46 Documented (63%)**. | ✅ |
| **6.d** | **Tier 3 — 16 primitives documented** (`axonendpoint`, `axpoint`, `daemon`, `mcp`, `listen`, `shield`, `mandate`, `compute`, `lambda`, `forge`, `ots`, `psyche`, `immune`, `reflex`, `heal`, `transact`) + 16 canonical examples. **Second registry honesty rebaseline**: `logic` removed (46→45) — lexer token, no parser production, same as `taint` in 6.c. Drift gate caught 2 psyche-specific bugs (`inference_mode` closed catalog is `{active, passive}`; `safety_constraints` must include `non_diagnostic` per Dependent Type Safety section 4). **🎉 Coverage achieves 45/45 — 100%.** | ✅ |
| **6.e** | Release tag + cross-stack publish (batched with v1.2.0 / 8) | |
| **7.a** | **Verticals — 4 new templates** (`legaltech`, `fintech`, `pharmatech`, `medic_research`) + Domain enum 8→12. Drift gate caught the `severity` reserved-keyword collision. Coverage: 12/33. | ✅ |
| **7.b** | **Agent patterns — 8 new templates** (`chat_research`, `chat_tools`, `chat_skills`, `whatsapp`, `voice`, `dev`, `sales_consultive`, `sales_widget`) + Domain enum 12→20 + classifier curation. Drift gate caught the `on_breach: redact` bug — `redact` is the `redact:` field's name, NOT a value in the closed `on_breach` catalog `{deflect, escalate, halt, quarantine, sanitize_and_retry}`. Coverage: 20/33 (61%). | ✅ |
| **7.c** | **Application patterns — 13 new templates** (`workflow_automation`, `business_intelligence`, `corporate_integration`, `self_learning`, `document_analysis`, `ticket_triage`, `content_moderation`, `knowledge_extraction`, `compliance_monitoring`, `recruitment`, `education`, `financial_advisor`, `data_pipeline`) + Domain enum 20→33. Drift gate caught 3 bugs (`retrieve:` is a flow-step SIBLING not a step body field; `mandate.on_violation` closed catalog is `{coerce, halt, retry}`). **🎉 Coverage 33/33 — v1.2.0 CLOSED.** | ✅ |
| **8** | **Telemetry OTLP-grade + privacy-first**. New [telemetry.rs](src/axon-emcp/src/telemetry.rs) records per-tool/resource/prompt/compose/check events with p50/p95/p99 latency histograms. Always-on in-memory aggregates surfaced via `snapshot()`; opt-in JSONL sink via `AXON_EMCP_TELEMETRY_FILE` (downstream pipelines convert to OTLP wire). **Five hard privacy invariants** enforced by construction (never log source content / intent strings / error messages / unintended egress / unknown deployment-ID). New `axon-emcp telemetry summarize <file>` subcommand for offline aggregation. **🎉 ℰMCP roadmap CLOSED.** | ✅ |

The discipline: every primitive added to `src/knowledge/primitives/` is
backed by a passing `cargo test` that exercises a real `.axon` example
through the live `axon-frontend` parser + type-checker. The knowledge
base does not drift from the language — it is checked against the
implementation on every commit. Phase 1 closed the loop: the agent can
now VALIDATE the code it produces, against the same compiler the `axon`
CLI ships.

Phase 2 proved that discipline in practice: the first draft of three
primitive docs contained inaccuracies (`empathic` → `empathetic`, a
non-existent `<Target>` argument on `reason`, an `effects:` row that
did not match the closed catalog). The integration suite under
`src/axon-emcp/tests/phase2_canonical_programs.rs` rejected all three
on first run, exactly as it will reject any future doc that drifts
from the parser.
