# Publishing `axon-emcp`

This crate ships through three independent surfaces. Each is gated by a
different credential set; this doc lists the operator commands for each
in the order they normally run when shipping a new version.

## 1. crates.io

```bash
# Bump the version (Cargo.toml + Cargo.lock — bump-my-version handles both
# if you wire it; otherwise edit Cargo.toml manually and `cargo update -p axon-emcp`).
# Test:
cargo test --manifest-path src/axon-emcp/Cargo.toml

# Publish (requires `cargo login` against a crates.io API token):
cargo publish --manifest-path src/axon-emcp/Cargo.toml
```

## 2. GitHub Release (annotated tag + release notes)

```bash
# After the crates.io publish lands, tag the commit and create the release.
# The tag prefix `axon-emcp-v` distinguishes axon-emcp tags from the
# `v<N>.<N>.<N>` axon-lang tags that share this monorepo.
git tag axon-emcp-v0.4.0 HEAD
git push origin axon-emcp-v0.4.0

gh release create axon-emcp-v0.4.0 \
  --title "axon-emcp 0.4.0 — <one-line summary>" \
  --notes-file release_notes.md
```

## 3. MCP Server Registry (registry.modelcontextprotocol.io)

The official MCP registry hosts metadata about every published MCP
server so clients can discover and (eventually) auto-install them. The
manifest at [`server.json`](./server.json) is the source of truth and
is kept in sync with the crate version.

The publish flow authenticates against the DOMAIN, not a code-hosting
account: an Ed25519 key published as a TXT record at the apex of
`ricardovelit.com` grants the `com.ricardovelit/*` namespace.

That choice is the point. An `io.github.*` namespace is authenticated by
an ACCOUNT NAME — a thing the platform lets you rename and lets someone
else claim afterwards. This project has already lived that once: the
account moved Bemarking -> VelitRicardo, leaving `io.github.Bemarking/*`
unowned. A domain does not rename.

No browser is needed: `mcp-publisher login dns` signs with the private
key, so the flow runs headless and in CI.

```bash
# (a) Install the publisher CLI:
brew install mcp-publisher    # macOS / Linuxbrew
# Or grab the Windows / Linux binary directly from
# https://github.com/modelcontextprotocol/registry/releases

# (b) Validate the manifest before any auth (works offline against
# the live registry's validation endpoint — no credentials needed):
mcp-publisher validate src/axon-emcp/server.json

# (c) Authenticate the `com.ricardovelit/*` namespace with the domain
# key (headless — no browser). The token is cached at
# `~/.config/mcp-publisher/token.json` for subsequent publishes:
mcp-publisher login dns --domain ricardovelit.com --private-key <hex>

# (d) Publish:
mcp-publisher publish src/axon-emcp/server.json
```

After publish, the entry surfaces at
`https://registry.modelcontextprotocol.io/v0/servers?search=axon-emcp`
and on the registry's web UI.

### Version bumps

When you ship a new `axon-emcp` version, bump the `version` field in
`server.json` to match `Cargo.toml`, then run `mcp-publisher publish`
again. The registry treats each version as an immutable record (same
discipline as `cargo publish` — you cannot overwrite a published
version, only deprecate or delete it via `mcp-publisher status`).

### Why no `packages[]` entry?

The registry currently supports `npm | pypi | oci | nuget | mcpb` as
package-source types. `crates.io` is not yet in that enum, so the
manifest is metadata-only and adopters install via `cargo install
axon-emcp` per the description / website link. When the registry adds
crates.io support, we'll add a `packages[]` block with
`registryType: "crates.io"`, `identifier: "axon-emcp"`, and
`transport: { type: "stdio" }` — no other surface change needed.
