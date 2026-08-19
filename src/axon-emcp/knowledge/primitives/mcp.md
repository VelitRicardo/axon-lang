---
name: mcp
summary: Declares an outbound MCP server binding — turns axon into an MCP client of another server.
category: wire
top_level: true
since: v1.24.0+
grammar: |
  # The current `mcp` parser is the generic-declaration form
  # (parser.rs::parse_generic_declaration). Permissive grammar:
  mcp <Name> [(<args>)] [{ ... body ... }]
---

# `mcp`

`mcp` declares **an outbound Model Context Protocol binding** —
the inverse of running an MCP server. Where the ℰMCP runtime
*serves* tools and resources to coding agents, an `mcp`
declaration *consumes* an external MCP server's surface from
within an AXON program.

Use `mcp` when your flow needs to call out to another MCP-
compliant tool catalogue (a specialised retrieval server, a
domain-specific knowledge base, another agent's tool surface)
and have its primitives appear as if declared locally.

## Surface

`mcp` is a **top-level declaration**. It is *not* nested inside
another primitive. The current parser uses the **permissive
generic-declaration form** (`parser.rs::parse_generic_declaration`)
— the syntax accepts a name, optional parens, and an optional
braced block; field constraints are deferred to the v2.0.0 D11
deploy-time validation.

```axon
mcp ClinicalKB {
    server:  "clinical-mcp.internal"
    uri:     "kb://guidelines/2025"
    tools:   [search, get_guideline]
}
```

## Anatomy

### Header — `mcp <Name>`

A **PascalCase identifier** unique within the module. The
runtime resolves the name against its MCP-client registry at
deploy time.

### Body — `{ ... }`

The body is **currently open at the parser level** (the
permissive `parse_generic_declaration` shape). Conventional
fields (validated at deploy time, not at parse time) include:

| Field | Type | Meaning |
|---|---|---|
| `server:` | string | MCP server slug or hostname. |
| `uri:` | string | Resource URI prefix the binding scopes to. |
| `tools:` | bracketed identifiers | Tool subset to import (default: all). |
| `resources:` | bracketed identifiers | Resource subset to import (default: all). |
| `prompts:` | bracketed identifiers | Prompt subset to import (default: all). |
| `transport:` | identifier | `stdio` (subprocess) or `sse` (network). |

A future cycle will tighten the grammar to a typed `MCPDefinition`
AST node; until then, the runtime does the validation.

## Runtime behaviour

At deploy time, the runtime opens an MCP client connection to
the named server, fetches its `tools/list`, `resources/list`,
and `prompts/list`, and exposes them under the binding's
namespace. Inside a flow, an imported MCP tool is invoked the
same way as any local tool — the bridge is transparent.

Audit rows: every outbound MCP call emits
`mcp:<binding>:<method>:<outcome>` carrying `(remote_server,
remote_uri, latency, cost_credits_estimated)`. The audit chain
treats outbound MCP calls as **external dependencies** — they
appear in the dependency graph the v2.0.0 supply-chain check
walks.

## What this primitive is NOT

- **Not the ℰMCP server itself.** This is the **client** side —
  consuming another MCP server's surface. The server side of
  the protocol (what `axon-emcp` itself implements) is the
  AXON runtime's built-in.
- **Not a generic HTTP binding.** MCP is a typed JSON-RPC 2.0
  protocol with `initialize` handshake + capability
  declaration. For arbitrary HTTP egress, declare a `tool`
  with `provider: http`.
- **Not free of compliance gating.** Outbound MCP calls cross
  a trust boundary. A `mcp` binding inside a HIPAA-tagged
  manifest requires the remote server to attest the same
  framework; the v2.0.0 deploy-time check enforces this.

## See also

- `axon://primitives/tool` — the in-flow surface that imported
  MCP tools appear as.
- `axon://primitives/corpus` — the `corpus … from mcp(...)`
  shorthand for RAG-style MCP bindings.
- `axon://primitives/manifest` — outbound MCP bindings appear
  in the manifest's external-dependency graph.
- `axon://compliance/hipaa` — example of cross-server
  compliance attestation.
- The ℰMCP server side: [`src/README.md`](../README.md).
