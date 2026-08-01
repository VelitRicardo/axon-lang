# Contributing to axon-lang

Thanks for your interest in contributing to **axon-lang**! This guide
walks you through the contribution flow + the legal step (CLA
signature) that the project requires before any PR can land.

## Before you start

axon-lang is licensed under **GNU Affero General Public License v3.0
or later (AGPL-3.0+)** — see [`LICENSE`](./LICENSE). It is authored
and maintained by **Ricardo Velit** as part of a dual-license posture
(AGPL-3.0 for the public + commercial relicensing for the
`axon-enterprise` product). The dual-license architecture is
documented in
[the axon-enterprise `LICENSING.md`](https://github.com/Bemarking/axon-enterprise/blob/master/LICENSING.md).

This means every contributor must sign the
[Contributor License Agreement (CLA)](./CLA.md) before their PR can
be merged. The CLA is the legal instrument that lets the maintainer
ship your contribution under both the AGPL (to the public) AND a
commercial license (to axon-enterprise customers). See [`CLA.md`](./CLA.md)
for the full reasoning + the legal text.

## Quick-start contribution flow

1. **Fork** `Bemarking/axon-lang` to your own GitHub account.
2. **Clone** your fork locally + create a topic branch:
   ```bash
   git clone git@github.com:<YOU>/axon-lang.git
   cd axon-lang
   git checkout -b fix/<short-slug>
   ```
3. **Make your changes** following the repo conventions (see "Style"
   below).
4. **Test locally** — both the Rust + Python suites must pass:
   ```bash
   # Rust
   cd axon-rs && cargo test --workspace

   # Python (from repo root)
   pytest
   ```
5. **Commit + push**:
   ```bash
   git commit -s -m "fix: <one-line summary>"
   git push origin fix/<short-slug>
   ```
   The `-s` flag adds a `Signed-off-by:` line (good practice, even
   though we use the CLA-assistant bot for the formal signature).
6. **Open a pull request** against `Bemarking/axon-lang:master`.
7. **Sign the CLA** when the [cla-assistant.io](https://cla-assistant.io/)
   bot prompts you on your first PR. One-click for individuals; see
   [`CLA.md`](./CLA.md#b-corporate-contributor-license-agreement) for
   corporate contributions.
8. **Respond to review feedback**. Once approved + the CLA is signed
   + CI is green, a maintainer will merge.

## What changes can I contribute?

axon-lang welcomes contributions in these categories:

- **Bug fixes** — failing test cases + the fix.
- **Documentation** — including in-code docstrings + the README +
  tutorials in `docs/`.
- **New tests** — extending coverage of existing surfaces.
- **Performance improvements** — bring benchmarks (criterion or
  `pytest-benchmark`) showing the speedup.
- **Small feature additions** — file a GitHub Issue first to discuss
  scope BEFORE writing the code.
- **Large feature additions / architecture changes** — file a
  `docs/proposal-<slug>.md` PR first to discuss design BEFORE
  implementing. Large unsolicited PRs are unlikely to merge.

Areas where we'd particularly welcome help:

- Additional backend integrations (new LLM provider support).
- Vertical Shield strategies + dictionaries.
- Examples + tutorials in `docs/examples/`.
- Multi-language clients (Go / Java / Ruby / .NET wrappers).
- Editor integrations (LSP improvements, more IDE plugins).

## Style guide

### Rust

- `cargo fmt` clean.
- `cargo clippy --workspace --all-targets -- -D warnings` clean.
- Public APIs documented with `///` doc comments + `# Examples`.
- Test names describe the behavior, not the function: `verify_rejects_alg_none`,
  not `test_verify_1`.

### Python

- Black-formatted (`black .`).
- Ruff-clean (`ruff check .`).
- Type-annotated where it adds clarity.
- Pytest tests in `tests/` mirror the source-tree layout.

### Commit messages

- One-line summary in imperative mood: `fix:`, `feat:`, `docs:`,
  `test:`, `refactor:`, `perf:`, `chore:`.
- Body explains the WHY, not the WHAT (the diff explains the what).
- Reference issues with `Fixes #N` / `Closes #N` / `Refs #N`.

### Pull-request etiquette

- One logical change per PR. Large refactor + bug fix bundled
  together is usually two PRs.
- Keep PRs small + focused. Reviews of < 400 lines land faster than
  reviews of > 2000 lines.
- Update the changelog / release notes if the PR ships a
  user-visible change.

## Where to ask

| Question | Channel |
|---|---|
| General discussion | [GitHub Discussions](https://github.com/Bemarking/axon-lang/discussions) |
| Bug reports | [GitHub Issues](https://github.com/Bemarking/axon-lang/issues) |
| Security vulnerabilities | [security@bemarking.com.co](mailto:security@bemarking.com.co) — see [SECURITY.md](./SECURITY.md) |
| Commercial license inquiries | [licensing@bemarking.com.co](mailto:licensing@bemarking.com.co) |
| CLA / legal | [legal@bemarking.com.co](mailto:legal@bemarking.com.co) |

## Code of conduct

Be excellent to each other. The project maintainers reserve the
right to remove contributors who harass, demean, or otherwise act in
bad faith. Disagreements about technical decisions are welcome;
personal attacks are not.

## Maintainer trust

The maintainer team is led by Ricardo Velit. Decisions on
substantial changes (license, governance, breaking API changes,
release cadence) rest with him as the project's author and copyright
steward. Contributors retain copyright on their contributions; the
maintainer holds the dual-licensing rights via the CLA.

If you contribute substantially over time, you may be invited to
join the maintainer team with commit / merge rights.

---

Thank you again for considering a contribution. The project is
better for every PR.

— Ricardo Velit, on behalf of the axon-lang maintainers
