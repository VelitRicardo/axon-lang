# Development Guide — AXON

## Dual Repository Strategy

This project uses **two GitHub repositories**:

1. **`axon-lang`** (PUBLIC, AGPL-3.0-or-later + commercial dual-license)
   - URL: `git@github.com:Bemarking/axon-lang.git`
   - Contains: Core language, compiler, runtime, 7 LLM backends
   - Visibility: Open Source, GitHub public

2. **`axon-enterprise`** (PRIVATE, Commercial License)
   - URL: `git@github.com:Bemarking/axon-enterprise.git`
   - Contains: Enterprise features (RBAC, SSO, audit logging, metering)
   - Visibility: Private, maintainers only

## Remote Configuration

Your local repository has **two remotes**:

```bash
$ git remote -v
origin      git@github.com:Bemarking/axon-lang.git (fetch/push)
enterprise  git@github.com:Bemarking/axon-enterprise.git (fetch/push)
```

### Setup (one-time)

```bash
# This is already done, but if you need to reconfigure:
git remote set-url origin git@github.com:Bemarking/axon-lang.git
git remote add enterprise git@github.com:Bemarking/axon-enterprise.git
```

## Commit Workflow

### **Type 1: Open Source Commits** (Core Features)
```bash
# Example: bug fixes, new backends, performance improvements
git commit -m "feat: add circuit breaker for resilience"

# Push to PUBLIC only
git push origin master
```

**What goes in axon-lang:**
- ✅ Language features (epistemic directives, forge, agent, shield)
- ✅ Compiler & IR generation
- ✅ LLM backends (Anthropic, OpenAI, Gemini, Kimi, GLM, OpenRouter, Ollama)
- ✅ Core HTTP server (282 routes)
- ✅ PostgreSQL persistence
- ✅ Observability (tracing, logging)
- ✅ Resilience (circuit breaker, retry)
- ✅ Storage abstraction

### **Type 2: Enterprise Commits** (Commercial Features)
```bash
# Example: RBAC, SSO, audit logging
git commit -m "feat(enterprise): add SAML 2.0 SSO integration"

# Push to BOTH repositories
git push origin master && git push enterprise master
```

**What goes in axon-enterprise:**
- 🔒 RBAC (role-based access control)
- 🔒 SSO/SAML integration
- 🔒 Advanced audit logging
- 🔒 Usage metering & billing
- 🔒 Custom tool extensions
- 🔒 Performance optimizations
- 🔒 Studio visual debugger

## Pushing to Both Repositories

### Option 1: Manual Push
```bash
# Push to both in sequence
git push origin master
git push enterprise master

# Or combined
git push origin master enterprise master
```

### Option 2: Use the Helper Script
```bash
# Simple push to both
./scripts/push-both.sh

# Smart push (detects enterprise features)
./scripts/push-smart.sh
```

## Synchronization Strategy

### From `axon-lang` → `axon-enterprise`

Enterprise repository always stays "ahead" of public:
```
axon-lang (public)
    ↓
[sync/merge]
    ↓
axon-enterprise (private)
    ↓
[add enterprise features]
    ↓
axon-enterprise (with private features)
```

In `axon-enterprise`, maintain a sync script:
```bash
#!/bin/bash
# sync-from-upstream.sh
cd axon-core
git remote add upstream git@github.com:Bemarking/axon-lang.git
git fetch upstream
git merge upstream/master
cd ..
git add axon-core
git commit -m "chore: sync core from axon-lang"
git push origin master
```

## Best Practices

### ✅ DO

- Keep commits **atomic and well-described**
- Use commit messages that indicate **scope**: `feat(core):`, `feat(enterprise):`, `chore:`, `docs:`
- Push to `origin` (public) frequently — drives community adoption
- Push to `enterprise` only when it's intentional
- Use tags for releases: `git tag -a v2.75.0 -m "Release v2.75.0"`

### ❌ DON'T

- Push **enterprise features to the public repository**
- Commit **API keys, credentials, or secrets** to either repo
- Force-push to `master` branch on either remote
- Merge without reviewing what's going where

## Commit Message Convention

```
# Open source (axon-lang)
feat(core): add circuit breaker for LLM resilience
fix(backend): handle Kimi API timeout correctly
docs: update README for Phase K
chore: bump version to 1.0.1

# Enterprise (axon-enterprise)
feat(enterprise): add RBAC with role hierarchies
feat(enterprise): add SAML 2.0 SSO integration
fix(enterprise): audit log timezone handling
chore(enterprise): optimize query indexing for multi-tenant
```

## Checking What Goes Where

```bash
# See commits not yet pushed to origin (public)
git log origin/master..HEAD --oneline

# See commits not yet pushed to enterprise (private)
git log enterprise/master..HEAD --oneline

# See which files changed in unpushed commits
git diff origin/master..HEAD --name-only
```

## CI/CD Considerations

- **axon-lang**: Public CI/CD (GitHub Actions) — runs all tests, builds releases
- **axon-enterprise**: Private CI/CD — additional compliance checks, security scans

## Security Notes

1. **Credentials**: Use GitHub secrets for API keys, never commit them
2. **License**: `axon-lang` is AGPL-3.0-or-later (dual-licensed commercially); enterprise code is under a separate commercial license
3. **Access Control**: Only the maintainer team has access to axon-enterprise
4. **Separation**: Enterprise features must be in separate modules/crates when possible

## Questions?

- For **public/open source questions**: File issues on [GitHub/axon-lang](https://github.com/Bemarking/axon-lang)
- For **enterprise questions**: Internal maintainer channel

---
Last updated: July 16, 2026
