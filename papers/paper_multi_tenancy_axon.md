# Multi-Tenancy + Secrets Management — Axon Enterprise

## Estado del plan

### Plano de datos — Rust runtime (axon-lang)

| Fase | Nombre | Estado |
|------|--------|--------|
| M1 | Tenant Identity | ✅ Completo |
| M2 | Data Isolation (PostgreSQL RLS) | ✅ Completo |
| M3 | Secrets per Tenant (AWS Secrets Manager) | ✅ Completo |
| M4 | Backend Isolation (circuit breakers + metering) | ✅ Completo |
| M5 | Terraform — onboarding de tenants | ✅ Completo |

### Plano de control — Python (axon-enterprise v1.1.0)

| Fase | Nombre | Estado |
|------|--------|--------|
| 10.a | Persistence Foundation (SQLAlchemy 2 async + Alembic + RLS hookup) | ✅ Completo |
| 10.b | Identity Core (Users, Argon2id, TOTP, sessions, memberships) | ✅ Completo |
| 10.c | RBAC Production-Grade (persisted, tenant-scoped, hierarchy, enforcement) | ✅ Completo |
| 10.d | SSO Real (OIDC + SAML con verificación de firma) | ✅ Completo |
| 10.e | JWT Issuer + JWKS rotation (cierra el gap "no signature verification" de Rust) | ✅ Completo |
| 10.f | Secrets Service (API per-tenant, escribe a AWS SM, audit integrado) | ✅ Completo |
| 10.g | Audit Hash-Chain (append-only + stitch a ESK provenance_chain) | ✅ Completo |
| 10.h | Metering + Quota Enforcement (pricing plans, Stripe, rate limiting) | ✅ Completo |
| 10.i | Observability Wiring (Prometheus per-tenant, OTel con tenant baggage, structured logs) | ✅ Completo |
| 10.j | Admin API + CLI (tenant CRUD, user mgmt, key rotation, suspension) | ✅ Completo |
| 10.k | Tenant Self-Service Portal API (invitaciones, SSO config, API keys) | ✅ Completo |
| 10.l | Compliance Tooling (GDPR export JSONL, right-to-erasure, data residency) | ✅ Completo |
| 10.m | Testing + Security Audit (cross-tenant isolation, load, threat model) | ✅ Completo |

---

## Arquitectura objetivo

```
Request entrante
      │
      ▼
TenantExtractor middleware
  → extrae tenant_id del JWT o X-Tenant-ID header
  → inyecta TenantContext en request extensions
      │
      ▼
Auth middleware (RBAC existente)
  → valida rol dentro del tenant
      │
      ├──► Handler Rust
      │         │
      │         ├──► Storage (PostgreSQL + RLS)
      │         │     SET axon.current_tenant = tenant_id
      │         │     → Postgres filtra solo, bulletproof
      │         │
      │         └──► TenantSecretsClient
      │               → cache TTL 5min
      │               → AWS Secrets Manager: axon/tenants/{id}/provider_key
      │               → fallback a key global de Axon
      │
      └──► ResilientBackend[(tenant_id, provider)]
            → circuit breaker aislado por tenant
            → metering: cost_tracking con tenant_id
```

---

## M1 — Tenant Identity

**Objetivo:** cada request sabe a qué tenant pertenece.

### Archivos nuevos
- `axon-rs/migrations/003_add_tenants.sql` — tabla `tenants`
- `axon-rs/migrations/004_add_tenant_id.sql` — columna `tenant_id` en las 12 tablas existentes
- `axon-rs/src/tenant.rs` — `TenantContext`, `TenantExtractor` middleware, `TenantPlan` enum

### Cambios a existentes
- `axon-rs/src/storage.rs` — campo `tenant_id: String` en todos los row types
- `axon-rs/src/axon_server.rs` — registrar `TenantExtractor` en el router Axum

### Contrato del middleware
```
Header X-Tenant-ID: {tenant_id}    → TenantContext { tenant_id, plan }
Header Authorization: Bearer {jwt} → extrae claim "tenant_id" del payload JWT
Sin header                         → tenant_id = "default" (retrocompatibilidad)
```

### Tabla tenants
```sql
CREATE TABLE tenants (
    tenant_id   TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    plan        TEXT NOT NULL DEFAULT 'starter',  -- starter | pro | enterprise
    status      TEXT NOT NULL DEFAULT 'active',   -- active | suspended | deleted
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
INSERT INTO tenants (tenant_id, name, plan) VALUES ('default', 'Default Tenant', 'enterprise');
```

---

## M2 — Data Isolation (PostgreSQL RLS)

**Objetivo:** imposible leer datos cross-tenant, incluso con bug en Rust.

### Patrón
```sql
ALTER TABLE traces ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON traces
    USING (tenant_id = current_setting('axon.current_tenant', true));
```

### Cambios requeridos
- `axon-rs/migrations/005_enable_rls.sql` — RLS en las 12 tablas
- `axon-rs/src/db_pool.rs` — inyectar `SET axon.current_tenant = $1` al sacar conexión del pool
- `axon-rs/src/storage_postgres.rs` — todas las queries incluyen `tenant_id` en WHERE/INSERT

---

## M3 — Secrets per Tenant (AWS Secrets Manager)

**Objetivo:** cada tenant tiene sus propias LLM API keys, nunca en Postgres ni logs.

### Convención de paths
```
axon/tenants/{tenant_id}/anthropic_api_key
axon/tenants/{tenant_id}/openai_api_key
axon/tenants/{tenant_id}/gemini_api_key
axon/tenants/{tenant_id}/kimi_api_key
axon/tenants/{tenant_id}/glm_api_key
axon/tenants/{tenant_id}/openrouter_api_key
axon/tenants/{tenant_id}/groq_api_key
```

### Cadena de resolución
1. Cache en memoria `(tenant_id, provider)` con TTL 5 minutos
2. AWS Secrets Manager path del tenant
3. Fallback a key global de Axon (env var)

### Archivos nuevos
- `axon-rs/src/tenant_secrets.rs` — `TenantSecretsClient` con cache + AWS SDK

### Decisión: AWS SM vs Vault
- **V1 (este plan):** AWS Secrets Manager — ya provisionado, IAM integrado, costo bajo
- **V2 (futuro):** HashiCorp Vault — dynamic secrets, rotación automática, multi-cloud

---

## M4 — Backend Isolation

**Objetivo:** un tenant no afecta a otro; base para billing.

### Cambios
- `ResilientBackend`: circuit breakers indexados por `(tenant_id, provider)` en vez de solo `provider`
- Rate limiter: cuota configurable en tabla `tenants` (requests/min, tokens/día)
- `cost_tracking` table: ya existe, solo necesita `tenant_id` (cubierto por M1)

---

## M5 — Terraform

**Objetivo:** onboarding de nuevos tenants sin intervención manual.

### Entregables
- `infrastructure/terraform/modules/tenant/` — crea paths SM para un tenant (for_each sobre providers)
- `infrastructure/scripts/onboard_tenant.sh` — crea tenant en DB + secretos vacíos + API key inicial
- RDS upgrade: `db.t3.micro` → `db.t3.small`, `multi_az = true` (decisión documentada en variables.tf)
- `infrastructure/terraform/iam.tf` — Task Role ahora tiene permiso `axon/tenants/*` en SM (requerido por TenantSecretsClient)

### Decisión RDS
| Dimensión | Antes | Después |
|-----------|-------|---------|
| Instancia | db.t3.micro (1 GB) | db.t3.small (2 GB) |
| Multi-AZ | false | true |
| Motivo | Free tier / dev | SLA 99.9% multi-tenant; RLS agrega overhead por transacción |
| Siguiente umbral | — | db.t3.medium cuando tenants > 20 o p99 > 200 ms |

---

---

## Fase 10 — Enterprise Control Plane (Python / axon-enterprise v1.1.0)

### Por qué existe esta fase

M1–M5 completaron el **plano de datos** en el runtime Rust: extracción de tenant por request, RLS en Postgres, secrets aislados en AWS SM, circuit breakers per-(tenant, provider), y Terraform para onboarding de infra. Eso hace que una request *ya pateada* sea segura y aislada.

Lo que falta es el **plano de control** — el conjunto de servicios que provisionan tenants, gestionan usuarios, enforzan RBAC, emiten JWTs firmados, guardan secretos, corren auditoría append-only, facturan, y exponen un portal administrativo. Hoy `axon_enterprise/` es *scaffolding con TODOs*: dataclasses sin persistencia, SSO con `return None`, audit en `list` Python, RBAC in-memory sin tenant scope, métricas sin backend.

Fase 10 construye ese control plane de forma **production-grade desde el primer commit** — sin "por ahora", sin "lo mínimo", sin stubs. Cada sub-fase cierra uno de los gaps identificados en la auditoría de v1.0.0 y deja código apto para un primer cliente enterprise real.

### Arquitectura objetivo (Python ↔ Rust)

```
┌────────────────────── Plano de control (Python / axon-enterprise) ──────────────────────┐
│                                                                                         │
│   Admin CLI ──┐                                                                         │
│   Admin API ──┼─► TenantService ──► Postgres (tenants, users, roles, memberships)       │
│   Portal API ─┘                         │                                               │
│                                         │                                               │
│   SSO Router ──► OIDCProvider / SAMLProvider ─► validación firma ─► User/Membership     │
│        │                                                                                │
│        ▼                                                                                │
│   JWTIssuer ──► firma RS256 con llave KMS ──► { sub, tenant_id, roles, exp, jti }       │
│        │                                                                                │
│        ▼                                                                                │
│   SecretsService ──► write AWS SM path axon/tenants/{id}/{key} ──► emite audit event    │
│                                                                                         │
│   AuditService ──► append-only table + hash chain (SHA-256 anterior) ──► ESK stitch     │
│                                                                                         │
│   MeteringService ──► pricing_plan × usage ──► Stripe invoice ──► quota enforcement     │
│                                                                                         │
└─────────────────────────────────────┬───────────────────────────────────────────────────┘
                                      │ shared Postgres  (RLS enforced)
                                      │ shared AWS Secrets Manager
                                      │ JWKS served at /.well-known/jwks.json
                                      ▼
┌────────────────────── Plano de datos (Rust / axon-lang) ────────────────────────────────┐
│                                                                                         │
│   TenantExtractor ──► verifica firma JWT contra JWKS ──► TenantContext                  │
│   PostgresBackend ──► SET axon.current_tenant = $id ──► RLS lo filtra                   │
│   TenantSecretsClient ──► cache + AWS SM path convention                                │
│   ResilientBackend[(tenant, provider)] ──► circuit breaker aislado                      │
│                                                                                         │
└─────────────────────────────────────────────────────────────────────────────────────────┘
```

**Principio:** Python escribe, Rust lee. Ambos comparten Postgres (con RLS) y AWS Secrets Manager. JWTs emitidos por Python, verificados por Rust contra JWKS público.

---

### 10.a — Persistence Foundation

**Estado:** ✅ Completo (2026-04-21) — **Depende de:** M1, M2 (completos)

**Shipped commits (axon-enterprise):**
- `58e5cb6` feat(fase-10.a): persistence foundation — config, tenant, db layer
- `4133e96` feat(fase-10.a): Alembic scaffold + baseline migration
- `cdd7492` test(fase-10.a): unit + integration suite for the persistence foundation
- `5c40c28` docs(fase-10.a): DATABASE.md operator guide

**Archivos producidos:**
- `axon_enterprise/config/{__init__.py, settings.py}` — pydantic-settings tree con validadores production-safety
- `axon_enterprise/tenant/{__init__.py, context.py}` — TenantContext + ContextVar (Python analogue de `tokio::task_local!`)
- `axon_enterprise/db/{__init__.py, base.py, engine.py, session.py, rls_policies.py}` — fundación completa
- `alembic.ini`, `alembic/env.py`, `alembic/script.py.mako`, `alembic/versions/20260421_0000_001_baseline_foundation.py`
- `tests/{conftest.py, tenant/, config/, db/}` — suite unit + integration con testcontainers
- `docs/DATABASE.md` — operator guide (roles, migrations, RLS, pool tuning)

**Delta vs plan original:** + `SoftDeleteMixin`, + `admin_bypass_policy_sql` helper, + `full_policy_set_sql` convenience (reduce boilerplate en sub-fases 10.b+), + `psycopg[binary]` como fallback sync para Alembic offline mode, + `structlog` cableado en engine para slow-query logging.

**Objetivo:** fundación de persistencia async con RLS participativa. Todas las sub-fases siguientes construyen encima.

**Archivos nuevos (axon-enterprise):**
- `axon_enterprise/db/engine.py` — async engine (asyncpg), connection pool con `pool_pre_ping`, tuning per-plan
- `axon_enterprise/db/session.py` — `AsyncSessionLocal`, dependency `get_session(tenant_ctx)` que emite `SET LOCAL axon.current_tenant = :tid` antes de yield
- `axon_enterprise/db/base.py` — `DeclarativeBase` + `TimestampMixin` + `TenantScopedMixin` (FK + índice)
- `axon_enterprise/db/rls_policies.py` — helpers para declarar policies uniformes
- `alembic.ini`, `alembic/env.py`, `alembic/versions/001_initial.py`

**Decisiones clave:**
| Decisión | Elegido | Por qué |
|---|---|---|
| Driver | `asyncpg` | perf + async nativo; psycopg2 descartado |
| ORM | SQLAlchemy 2.x async | estándar, compatible con Alembic, evita split brain con ORMs menores |
| Migrations | Alembic con autogenerate + review manual | nunca autoapply en prod; cada migration es un PR |
| RLS setting | `axon.current_tenant` | **mismo nombre que Rust** (M2) — comparten la variable GUC |
| Session scope | Una por request HTTP | simplifica tx handling y error rollback |
| Connection pool | 10 min / 50 max por instancia | tuning inicial; ajustar con métricas de p99 |

**Criterios de aceptación:**
- Test de integración: query sin setear `axon.current_tenant` → RLS rechaza
- Test: query con tenant A NO retorna filas de tenant B aunque se haya declarado WHERE incorrecto
- Test: rollback de tx deja tenant setting limpio para el próximo checkout
- `alembic upgrade head` corre limpio contra schema vacío
- `alembic downgrade base` revierte sin errores

**Tracked commits:** _(pendiente)_

---

### 10.b — Identity Core

**Estado:** ✅ Completo (2026-04-21) — **Depende de:** 10.a

**Shipped commits (axon-enterprise):**
- `68299b8` feat(fase-10.b): envelope crypto + settings extension
- `e590155` feat(fase-10.b): identity core — users, memberships, sessions, auth
- `e88e4cc` test(fase-10.b): unit + integration suite for crypto and identity
- `9dc54b1` docs(fase-10.b): IDENTITY.md operator guide

**Archivos producidos:**
- `axon_enterprise/crypto/{__init__.py, envelope.py, local_envelope.py, kms_envelope.py}` — envelope encryption con interfaz + 2 backends (Fernet+HKDF local, AWS KMS GenerateDataKey prod)
- `axon_enterprise/identity/{__init__.py, errors.py, password.py, password_policy.py, totp.py, lockout.py, sessions.py, auth.py, models.py}` — servicios completos
- `axon_enterprise/config/settings.py` extendido: `EnvelopeSettings`, `IdentitySettings`, validator production-safety (rechaza envelope=local en prod)
- `alembic/versions/20260421_0100_002_identity_core.py` — migration con citext + pgcrypto + 3 tablas + RLS
- `tests/crypto/test_local_envelope.py` (10 casos)
- `tests/identity/{test_password, test_password_policy, test_totp, test_lockout, test_sessions_unit}.py` (28 casos unit, no Docker)
- `tests/identity/{test_auth_integration, test_rls_memberships}.py` (14 casos integration con testcontainers)
- `tests/conftest.py` refactor: fixtures de Postgres compartidas entre db/identity/audit/metering futuros
- `docs/IDENTITY.md` — operator guide

**Decisiones cerradas (preguntas abiertas de la sesión anterior):**
- Argon2id params: `t=3, m=64 MiB, p=4` como default (OWASP 2024 mid) — overrideable a 128 MiB vía env. Razón: balance entre starter-tier containers (1 GB RAM) y enterprise-tier. No usar 128 MiB por defecto degrada latencia de login en starters.
- TOTP secrets se cifran con envelope desde 10.b (no diferido a 10.f). Backend dual: `local` para dev (Fernet+HKDF), `kms` para prod (GenerateDataKey con EncryptionContext=AAD). Production validator en Settings rechaza `backend=local` cuando `env=production`.

**Delta vs plan original:** + `User.password_algo` column (track algo per-row para migrations entre hashing algos), + partial unique index en `invitation_token_hash` (solo no-NULL), + `Session.rotated_to_session_id` FK (chain-linking para forensics), + `Session.sequence` BigInt (replay detection), + `burn_equivalent_time()` para timing parity en login, + HIBP fails-open en network errors (no bloquea registros si upstream caído).

**Objetivo:** entidad User de verdad, hashing moderno, 2FA, sessions, pertenencia tenant (un user puede estar en varios tenants con roles distintos).

**Modelo de datos:**
```sql
CREATE TABLE users (
    user_id        UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email          CITEXT UNIQUE NOT NULL,
    password_hash  TEXT,                            -- null si SSO-only
    password_algo  TEXT NOT NULL DEFAULT 'argon2id',
    totp_secret_encrypted BYTEA,                    -- envelope encrypted (KMS)
    totp_enabled   BOOLEAN NOT NULL DEFAULT FALSE,
    email_verified BOOLEAN NOT NULL DEFAULT FALSE,
    status         TEXT NOT NULL DEFAULT 'active',  -- active | locked | deleted
    failed_logins  SMALLINT NOT NULL DEFAULT 0,
    locked_until   TIMESTAMPTZ,
    last_login_at  TIMESTAMPTZ,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE tenant_memberships (
    tenant_id      TEXT NOT NULL REFERENCES tenants(tenant_id),
    user_id        UUID NOT NULL REFERENCES users(user_id),
    invited_by     UUID REFERENCES users(user_id),
    invited_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    joined_at     TIMESTAMPTZ,
    status         TEXT NOT NULL DEFAULT 'active',  -- invited | active | suspended
    PRIMARY KEY (tenant_id, user_id)
);

CREATE TABLE sessions (
    session_id     UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id        UUID NOT NULL REFERENCES users(user_id),
    tenant_id      TEXT NOT NULL REFERENCES tenants(tenant_id),
    refresh_token_hash BYTEA NOT NULL,              -- SHA-256 del refresh token (no el token crudo)
    user_agent     TEXT,
    ip_address     INET,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_used_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at     TIMESTAMPTZ NOT NULL,
    revoked_at     TIMESTAMPTZ
);
```

**Decisiones de seguridad:**
| Ítem | Elegido | Razón |
|---|---|---|
| Password hash | **Argon2id** (argon2-cffi), `time_cost=3, memory_cost=64 MiB, parallelism=4` | OWASP 2024 recommendation; bcrypt no resiste GPUs modernas |
| TOTP | `pyotp` + secret de 160 bits envelope-encriptado con KMS | RFC 6238 estándar, compatible con Authenticator apps |
| Password policy | min 12 chars, zxcvbn score ≥ 3, HIBP check async | balance usabilidad/seguridad |
| Lockout | 5 fallos = lock 15min, 10 fallos = lock 1h, 20 = lock indefinido | defensa contra brute-force |
| Refresh tokens | 64 bytes random, hash SHA-256 en DB, rotación en cada uso | limita ventana si BD se compromete |
| Sesión | 24h inactividad / 30d max | balance ergonomía/seguridad para enterprise |

**Criterios de aceptación:**
- Test: password hash re-verifica con `argon2.PasswordHasher.verify`
- Test: `needs_rehash()` detecta parámetros viejos y re-hashea en login
- Test: TOTP genera código compatible con Google Authenticator
- Test: refresh token no puede reusarse (rotación obligatoria)
- Test: 5 fallos consecutivos bloquea la cuenta

---

### 10.c — RBAC Production-Grade

**Estado:** ✅ Completo (2026-04-21) — **Depende de:** 10.a, 10.b

**Shipped commits (axon-enterprise):**
- `c8b1010` feat(fase-10.c): PrincipalContext — authenticated actor propagation
- `16c89c1` feat(fase-10.c): RBAC production-grade — persisted, hierarchical, tenant-scoped
- `a1bc247` test(fase-10.c): unit + integration suite for RBAC production
- `c8e21b2` docs(fase-10.c): rewrite RBAC.md for the production-grade subsystem

**Archivos producidos:**
- `axon_enterprise/identity/principal.py` — `PrincipalContext` + `CURRENT_PRINCIPAL` ContextVar
- `axon_enterprise/rbac/{__init__.py, models.py, service.py, permissions.py, seed.py, enforce.py, errors.py}` — reemplazo completo del scaffolding v1.0.0
- `alembic/versions/20260421_0200_003_rbac_production.py` — 4 tablas + seed del catalog + RLS completo
- `tests/rbac/{test_permissions_catalog.py, test_service_integration.py, test_enforce.py}` — 29 casos (13 unit + 16 integration)
- `docs/RBAC.md` — rewrite completo con diagrama + SQL del CTE + guard rails

**Decisiones cerradas (preguntas abiertas de la sesión anterior):**
- **Catálogo exacto**: 32 permissions en 8 resources (tenant/user/role/flow/secret/audit/metering/observability). Seeded por migration 003 con `INSERT ... ON CONFLICT DO NOTHING` — agregar permissions es una migration nueva.
- **Rol owner**: creado per-tenant con TODOS los permissions (no un wildcard — enumerar explícitamente sobrevive additions al catálogo y hace auditorías determinísticas). Owner del tenant obtiene este rol en provisioning.
- **Granularidad de `flow:execute`**: coarse (por resource, no por flow individual). Si un tenant necesita per-flow scoping, se agrega `scope_pattern` column en role_permissions en una sub-fase futura; por ahora la granularidad actual cubre el 95% de los casos enterprise sin over-engineering.

**Delta vs plan original:** + `BuiltInRoleProtected` error type para prevenir delete de roles built-in, + `grant_permissions` bulk method con backfill (idempotent re-seed tras agregar permissions al catalog), + `require_permission` decorator parsea at decoration time (typos fallan at import, no at request), + `_assert_no_cycle` walk explícito además de la confianza en `UNION` del CTE (mejor error message y fail-fast en write path), + `parent_role_id` self-FK con `ON DELETE SET NULL` (borrar un parent no destruye los children).

**Objetivo:** reemplazar el RBAC in-memory de v1.0.0 por uno persistente, tenant-scoped, con jerarquía recursiva real y middleware que enforza permisos.

**Modelo:**
```sql
CREATE TABLE roles (
    role_id        UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id      TEXT NOT NULL REFERENCES tenants(tenant_id),  -- scoping crítico
    name           TEXT NOT NULL,
    description    TEXT NOT NULL DEFAULT '',
    is_built_in    BOOLEAN NOT NULL DEFAULT FALSE,
    parent_role_id UUID REFERENCES roles(role_id),               -- jerarquía recursiva
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (tenant_id, name)
);

CREATE TABLE permissions (
    permission_id  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    resource       TEXT NOT NULL,      -- "flow", "secret", "audit", "tenant", ...
    action         TEXT NOT NULL,      -- "read", "create", "delete", "execute", ...
    description    TEXT NOT NULL DEFAULT '',
    is_system      BOOLEAN NOT NULL DEFAULT FALSE,               -- seedeada por el sistema
    UNIQUE (resource, action)
);

CREATE TABLE role_permissions (
    role_id        UUID NOT NULL REFERENCES roles(role_id) ON DELETE CASCADE,
    permission_id  UUID NOT NULL REFERENCES permissions(permission_id),
    granted_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (role_id, permission_id)
);

CREATE TABLE user_roles (
    user_id        UUID NOT NULL REFERENCES users(user_id),
    role_id        UUID NOT NULL REFERENCES roles(role_id) ON DELETE CASCADE,
    tenant_id      TEXT NOT NULL REFERENCES tenants(tenant_id),  -- denormalized para RLS
    assigned_by    UUID REFERENCES users(user_id),
    assigned_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, role_id, tenant_id)
);
```

**Catálogo de permisos del sistema (seed):**

| Resource | Actions |
|---|---|
| `tenant` | `read`, `update`, `delete`, `suspend` |
| `user` | `invite`, `read`, `update`, `deactivate`, `impersonate` |
| `role` | `create`, `read`, `update`, `delete`, `assign` |
| `flow` | `create`, `read`, `update`, `delete`, `execute`, `deploy` |
| `secret` | `list`, `read`, `write`, `delete`, `rotate` |
| `audit` | `read`, `export` |
| `metering` | `read`, `export_invoice` |
| `observability` | `read_metrics`, `read_logs`, `read_traces` |

**Roles built-in (seed por tenant al crear):**
- `owner` → todos los permissions
- `admin` → todo excepto `tenant:delete`, `tenant:suspend`, `user:impersonate`
- `developer` → `flow:*`, `secret:read` (solo por ahora), `audit:read`, `observability:*`, `metering:read`
- `viewer` → solo `*:read` del resource

**Enforcement:**
- Decorator `@require_permission("secret:write")` para handlers HTTP
- Helper `rbac.check(user, tenant, "resource:action")` devuelve bool
- Resolución de permisos efectivos con **CTE recursiva en Postgres** — la jerarquía se resuelve en BD, no en Python (evita N+1 + walk infinito):
```sql
WITH RECURSIVE role_tree AS (
    SELECT role_id, parent_role_id FROM roles WHERE role_id = $1
    UNION
    SELECT r.role_id, r.parent_role_id FROM roles r JOIN role_tree rt ON r.role_id = rt.parent_role_id
)
SELECT DISTINCT p.resource, p.action FROM role_tree rt
  JOIN role_permissions rp ON rp.role_id = rt.role_id
  JOIN permissions p ON p.permission_id = rp.permission_id;
```

---

### 10.d — SSO Real (OIDC + SAML)

**Estado:** ✅ Completo (2026-04-21) — **Depende de:** 10.b, 10.c

**Shipped commits (axon-enterprise):**
- `3a849f7` feat(fase-10.d): SSO foundation — settings, errors, models, config store
- `ab2b1b2` feat(fase-10.d): OIDC + SAML providers + SsoService orchestrator
- `89b5e77` test(fase-10.d): unit + integration suite for SSO
- `591a5a8` docs(fase-10.d): rewrite SSO.md for the production-grade subsystem

**Archivos producidos:**
- `axon_enterprise/sso/{__init__.py, errors.py, models.py, configurations.py, state.py, rate_limit.py, mapper.py, service.py, saml_metadata.py}` — fundación + orquestador
- `axon_enterprise/sso/oidc.py` (rewrite) + `oidc_pkce.py`, `oidc_discovery.py`, `oidc_jwks.py`, `oidc_id_token.py` — OIDC completo
- `axon_enterprise/sso/saml.py` (rewrite) — python3-saml wrapper con replay defence
- `axon_enterprise/sso/oauth.py` **eliminado** (out of scope)
- `axon_enterprise/config/settings.py` extendido con `SsoSettings`
- `alembic/versions/20260421_0300_004_sso_configurations.py` — 3 tablas con RLS
- `tests/sso/{test_pkce, test_id_token, test_saml_metadata, test_rate_limit, test_config_integration}.py` — 34 casos (27 unit + 7 integration)
- `docs/SSO.md` rewrite completo con reveal-to-client matrix

**Decisiones cerradas (preguntas abiertas de la sesión anterior):**
- OIDC + SAML shippeados **juntos** en 10.d (no iterativo). SAML delega a python3-saml con lazy import — xmlsec no requerido en dev.
- `sso_configurations.config_encrypted` usa envelope del 10.b con AAD `{tenant_id, provider_type, purpose=sso_config}` — cohesivo con el patrón existente de TOTP secrets.
- `auto_provision_default=true` + rate limit 30/min/`(tenant, provider)` via `InMemoryRateLimiter`. Swap a Redis en 10.i cuando multi-replica.

**Delta vs plan original:** + `SsoAssertionSeen` tabla dedicada (UNIQUE constraint-based replay defence vs check-then-insert race), + `oidc_discovery` con stampede protection (asyncio.Lock + in-flight futures dedup), + `oidc_jwks` con force-refresh-on-kid-miss + `Cache-Control: no-cache` bypass, + `saml_metadata.py` pure-Python (no xmlsec en metadata time), + `role_map` additive-only (admin-granted roles sobreviven SSO login — strict mode diferido), + reveal-to-client matrix explícito en errors para que HTTP middleware no leakee info por timing/message distinction.

**Objetivo:** reemplazar los `return None` de v1.0.0 con SSO federado real. Soporta OIDC (Google Workspace, Azure AD, Okta) y SAML 2.0 (enterprise IdPs).

**OIDC — implementación completa:**
- Discovery: fetch y cache de `/.well-known/openid-configuration` (TTL 1h)
- State + nonce: generados con `secrets.token_urlsafe(32)`, persistidos por 10min, **binding a session cookie**
- Authorization URL con PKCE (S256 challenge, mandatorio para public clients)
- Token exchange + validación de ID token:
  - Firma: RS256/ES256 contra JWKS del issuer (cache con rotación forzada en kid miss)
  - Claims: `iss`, `aud`, `exp`, `nbf`, `iat`, `nonce` (match con el guardado)
  - Verificación de `email_verified=true`
- Mapping: `email` del ID token → upsert de `User`, creación de `TenantMembership` si acepta invite

**SAML 2.0 — implementación completa:**
- Metadata: `SPMetadata` generado + served en `/sso/saml/{tenant_id}/metadata.xml`
- AuthnRequest firmado con cert per-tenant (KMS-backed)
- Response validation via `python3-saml` (librería de OneLogin, auditada):
  - Firma XML (signed assertion y signed response)
  - Destination URL match
  - InResponseTo match con request emitido
  - NotBefore / NotOnOrAfter ventana
  - Audience restriction
- Mapping de atributos configurable per-tenant en tabla `sso_configurations`

**Tabla `sso_configurations`:**
```sql
CREATE TABLE sso_configurations (
    tenant_id      TEXT PRIMARY KEY REFERENCES tenants(tenant_id),
    provider_type  TEXT NOT NULL,                    -- 'oidc' | 'saml'
    config_encrypted BYTEA NOT NULL,                 -- envelope encrypted (KMS)
    attribute_map  JSONB NOT NULL DEFAULT '{}',
    auto_provision BOOLEAN NOT NULL DEFAULT FALSE,   -- crear user en primer login
    default_role_id UUID REFERENCES roles(role_id),  -- rol asignado si auto_provision
    enabled        BOOLEAN NOT NULL DEFAULT TRUE,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

**Criterios de aceptación:**
- Test E2E con mock OIDC server (custom, sin depender de IdP externo)
- Test: JWT con firma inválida es rechazado
- Test: nonce replay es rechazado
- Test: PKCE verifier mismatch es rechazado
- Test SAML: assertion sin firma es rechazada
- Test SAML: replay de assertion (misma `ID`) es rechazado

---

### 10.e — JWT Issuer + JWKS rotation

**Estado:** ✅ Completo (2026-04-21) — **Depende de:** 10.b, 10.d

**Shipped commits:**
- `axon-enterprise` `2743633` feat(fase-10.e): JwtIssuer + JWKS rotation + revocation
- `axon-enterprise` `514215b` test+docs(fase-10.e): unit + integration + JWT.md guide
- `axon-lang`  `ae44d44` feat(runtime): JWT signature verification — closes §Fase 10.e gap

**Archivos producidos (Python / axon-enterprise):**
- `axon_enterprise/jwt_issuer/{__init__.py, errors.py, models.py, signer.py, local_signer.py, kms_signer.py, key_management.py, jwks.py, issuer.py, revocation.py}`
- `axon_enterprise/config/settings.py` extendido con `JwtSettings` + production validator
- `alembic/versions/20260421_0400_005_jwt_signing_keys.py` — tablas + partial unique index one-active
- `tests/jwt_issuer/{test_local_signer, test_integration}.py` — 14 casos (7 unit + 7 integration)
- `docs/JWT.md` — operator guide

**Archivos producidos (Rust / axon-lang):**
- `axon-rs/src/jwt_verifier.rs` — JwtVerifier + JwksClient con cache TTL + rotation-on-miss
- `axon-rs/src/lib.rs` — módulo registrado
- `axon-rs/src/tenant.rs` — middleware ahora prefiere verified-JWT sobre X-Tenant-ID cuando `AXON_JWT_JWKS_URL` está set
- `axon-rs/Cargo.toml` + jsonwebtoken=9

**Decisiones cerradas (preguntas abiertas de la sesión anterior):**
- **Una sola llave KMS compartida entre tenants** — simplicidad operativa + clientes no necesitan pull-kid-por-tenant. Rotación c/90d mitiga el all-or-nothing revocation.
- **`kid` = SHA-256(SPKI DER)[:16]** (UUID-like opaque, 16 hex chars) — no revela cadencia de rotación ni creation time.
- **Redis para blacklist con Postgres fallback** — Redis para reads rápidos del verifier; Postgres siempre escribe (durabilidad). `is_revoked()` fail-closed: outage de Redis → fallthrough a Postgres, nunca silently permit.

**Delta vs plan original:** + Partial unique index `uq_jwt_signing_keys_one_active` (invariante "one active key" enforced at DB level, no a nivel de aplicación), + reserved-claims overwrite en `JwtIssuer.mint` (callers no pueden silently impersonar tenants via `extra_claims`), + `enforce` flag en Rust verifier (deployments pre-10.e siguen funcionando con legacy path; enterprise flip a enforce=true vía env var), + local + KMS signer comparten mismo kid derivation (migrar entre backends no rota el kid), + `JwksClient` del Rust reutiliza el patrón de 10.d OIDC (TTL + force-refresh-on-miss).

**Objetivo:** cierra el gap actual en `axon-rs/src/tenant.rs` donde el JWT se lee **sin verificar firma** (línea 100 de ese archivo: `Extracts tenant_id from a JWT payload without signature verification`). Emite JWTs firmados por Python, verificados por Rust contra JWKS público.

**Implementación:**
- Firma **RS256** (NO HS256 — asimétrica para que Rust verifique sin compartir secreto)
- Llave privada en **AWS KMS** (nunca sale del HSM); firma vía `kms:Sign` API
- Dos llaves activas rotadas cada 90 días; período de gracia 7 días donde ambas son válidas
- JWKS público servido en `/.well-known/jwks.json` con las **dos** `kid` activas
- Claims:
  ```json
  {
    "iss": "https://auth.bemarking.com",
    "sub": "user:{user_id}",
    "tenant_id": "{tenant_id}",      // consumido por Rust TenantExtractor
    "plan": "enterprise",
    "roles": ["admin", "developer"],
    "aud": "axon-api",
    "exp": 1234567890,
    "iat": 1234567880,
    "nbf": 1234567880,
    "jti": "{uuid}"
  }
  ```
- Revocación: `jti` blacklist en Redis (TTL = remaining exp)

**Cambios en axon-rs:**
- Añadir verificación de firma en `tenant_extractor_middleware` — fetchea JWKS (cache 10min), valida `iss`, `aud`, `exp`, firma
- La verificación es **obligatoria** cuando `ENFORCE_JWT_VERIFICATION=true` (default en prod)
- Mantener el modo legacy (solo extracción) para tests y dev con flag explícito

**Criterios:**
- Test: JWT firmado con kid rotada (pero dentro de la ventana de gracia) pasa
- Test: JWT con firma forjada (`alg=none`) es rechazado
- Test: JWT expirado es rechazado
- Test: JWT con `jti` en blacklist es rechazado
- Test end-to-end: Python emite → Rust verifica → handler recibe tenant_id

---

### 10.f — Secrets Service

**Estado:** ✅ Completo (2026-04-21) — **Depende de:** 10.c

**Shipped commits (axon-enterprise):**
- `cf8c8e0` feat(fase-10.f): Secrets Service — per-tenant AWS SM + redacted values
- `3948326` test+docs(fase-10.f): unit + integration suite + SECRETS.md operator guide

**Archivos producidos:**
- `axon_enterprise/secrets/{__init__.py, errors.py, value.py, policy.py, backend.py, in_memory_backend.py, aws_sm_backend.py, models.py, service.py}`
- `axon_enterprise/config/settings.py` extendido con `SecretsSettings` + validator production-safety; helpers `_validate_*` refactored para cognitive complexity < 15
- `alembic/versions/20260421_0500_006_tenant_secrets.py` — tabla `tenant_secrets` con RLS full
- `tests/secrets/{test_value, test_policy_and_memory, test_service_integration}.py` — 40 casos (30 unit + 10 integration)
- `docs/SECRETS.md` — operator guide

**Decisiones cerradas (preguntas abiertas de la sesión anterior):**
- **Coarse `secret:read` permission** — granularidad per-key diferida; catalog de 10.c tiene `secret:{list,read,write,delete,rotate}` y eso cubre el 95% de los casos enterprise.
- **Retention de AWS SM: 30 días default** (rango 7..30 por AWS SM hard limit). Matches compliance windows típicos; operator puede ajustar con `deletion_recovery_window_days`.
- **Audit en cada read Y write**: `audit_on_read=true` por default + production validator lo requiere (SOC 2 CC.6.1). Operators en lower tiers pueden desactivarlo.

**Delta vs plan original:** + `SecretValue` opaque wrapper con redaction en repr/str/format/pickle/copy + constant-time equality (más estricto que SecretStr de pydantic — bloquea `f"{s:>20}"` con format spec), + fingerprint SHA-256[:8] en audit events (correlación cross-time sin plaintext), + `SecretsPolicy.validate_tenant_id` rechaza paths traversals antes de concatenar al backend path (defensa en profundidad), + in-memory backend enforça mismo ceiling 64 KiB que AWS SM (problemas surface in dev, no en prod), + `ResourceNotFound` normalización cross error-shapes (boto3 native + moto + stubs), + `_validate_*_production` helpers por subsistema (validator cognitive complexity < 15), + `SecretAlreadyScheduledForDeletion` error para mutations en rows pending-delete.

**Objetivo:** API REST para que el owner de cada tenant gestione sus secretos (API keys, webhooks, etc.) con audit completo, sin que nunca toquen BD en plaintext.

**API:**
```
POST   /api/v1/tenants/{tenant_id}/secrets
         Body: {"key": "openai_api_key", "value": "sk-...", "description": "..."}
         Permiso requerido: secret:write
         Acción: escribe a AWS SM path axon/tenants/{tenant_id}/openai_api_key
         Respuesta: 201 + { key, version_id, created_at } (value NO se retorna)

GET    /api/v1/tenants/{tenant_id}/secrets
         Permiso: secret:list
         Respuesta: [{ key, description, last_rotated_at, created_by }]
         (nunca retorna el value)

GET    /api/v1/tenants/{tenant_id}/secrets/{key}
         Permiso: secret:read
         Respuesta: 200 + { key, value, version_id }
         Auditoría: emite 'config:secret_access' antes de retornar

DELETE /api/v1/tenants/{tenant_id}/secrets/{key}
         Permiso: secret:delete
         Acción: schedule_deletion en AWS SM (7 días de ventana)

POST   /api/v1/tenants/{tenant_id}/secrets/{key}/rotate
         Permiso: secret:rotate
         Acción: nueva versión + versión anterior marcada AWSPREVIOUS
```

**Tabla metadata (los values viven en AWS SM, no en BD):**
```sql
CREATE TABLE tenant_secrets (
    tenant_id       TEXT NOT NULL REFERENCES tenants(tenant_id),
    key             TEXT NOT NULL,                                 -- "openai_api_key"
    aws_sm_arn      TEXT NOT NULL,                                 -- ARN para auditoría
    current_version TEXT NOT NULL,                                 -- AWSCURRENT version id
    description     TEXT NOT NULL DEFAULT '',
    created_by      UUID NOT NULL REFERENCES users(user_id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_rotated_at TIMESTAMPTZ,
    last_accessed_at TIMESTAMPTZ,
    PRIMARY KEY (tenant_id, key)
);
```

**Decisiones:**
| Decisión | Elegido | Razón |
|---|---|---|
| Backend | AWS Secrets Manager (reutiliza M3) | ya provisionado por Terraform, Rust ya lo lee |
| Path convention | `axon/tenants/{id}/{key}` (sin cambios de M3) | evita dual code path |
| Caching | No en Python (cliente pega directo a AWS SM) | el caching de 5min vive en Rust `TenantSecretsClient` para lectura en runtime |
| Versioning | AWS SM nativo (`AWSCURRENT` / `AWSPREVIOUS`) | rollback en 1 API call |
| Audit | Emite `config:secret_access` en GET; `config:secret_write` en POST/rotate | siempre con user_id, tenant_id, key name (nunca value) |

---

### 10.g — Audit Hash-Chain

**Estado:** ✅ Completo (2026-04-21) — **Depende de:** 10.a

**Shipped commits (axon-enterprise):**
- `6855d2b` feat(fase-10.g): audit_events — hash-chained, append-only audit log
- `216ee5d` test+docs(fase-10.g): audit suite + AUDIT.md operator guide

**Archivos producidos:**
- `axon_enterprise/audit/{__init__.py, errors.py, events.py, canonical.py, models.py, service.py, adapters.py}` — reemplazo completo del scaffolding v1.0.0
- `axon_enterprise/audit/logger.py` **eliminado** (in-memory stub reemplazado por `AuditService`)
- `alembic/versions/20260421_0600_007_audit_events.py` — tabla + trigger `audit_events_append_only` + triggers BEFORE UPDATE/DELETE/TRUNCATE + RLS
- `tests/audit/{test_canonical, test_service_integration}.py` — 32 casos (19 unit + 13 integration)
- `docs/AUDIT.md` rewrite completo

**Decisiones cerradas (preguntas abiertas de la sesión anterior):**
- **Canonical JSON shared con ESK** — mismo serializer (`sort_keys=True`, `separators=(",",":")`, `ensure_ascii=True`, UUID→str, datetime→ISO UTC, bytes→urlsafe-base64 no-pad). Byte-identical hash input Python↔Rust.
- **Append-only via Postgres trigger** — `audit_events_append_only()` raises SQLSTATE 42501 en UPDATE/DELETE/TRUNCATE. Defensivo incluso contra rogue admins que editen via psql directo.
- **Hash chain per-tenant** — cada tenant tiene su propia cadena independiente desde genesis `SHA-256(b"AXON_AUDIT_GENESIS:" || tenant_id)`. Verifier walks per tenant; cross-tenant links no aportan compliance value.
- **ESK stitch per-event opcional** — `esk_stitch BYTEA NULL` column; services que ya emiten ESK provenance entries pasan el hash en la audit request. Full integration con ESK bridge queda para compliance phase (10.l).

**Delta vs plan original:** + `AuditChainReport` dataclass (verify no raises — dashboard-friendly), + `sequence_number > 0` CHECK constraint (catches off-by-one bugs at DB level), + trigger en TRUNCATE además de UPDATE/DELETE (cierra el path `TRUNCATE axon_control.audit_events` que UPDATE/DELETE triggers no cubren), + `ip_address TEXT` en lugar de INET (Postgres canonicalisation rompería hash recomputation), + separator byte 0x1e (ASCII Record Separator) entre fields del hash (evita ambigüedad donde concat de dos campos podría matchear otra combinación legítima), + `pg_advisory_xact_lock(hashtext(tenant_id))` para serializar writers per-tenant (cross-tenant writers no contend), + `SecretsAuditAdapter` que reemplaza el emitter stub de 10.f sin code change en `SecretsService`, + adapters tipados para RBAC (`emit_role_created`, `emit_permission_granted`, `emit_permission_denied`) y SSO (`emit_config_changed`, `emit_login`, `emit_assertion_replay`), + enum `AuditEventType` cerrado con 41 valores (extension requires migration), + `canonical_bytes_for_hash` rejects types unknown con TypeError (nunca silencioso).

**Objetivo:** audit log append-only con hash chain tamper-evident, stitched al `provenance_chain` que ya existe en ESK (axon-lang). Ningún evento puede ser modificado o borrado sin quebrar la cadena.

**Modelo:**
```sql
CREATE TABLE audit_events (
    event_id        UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id       TEXT NOT NULL REFERENCES tenants(tenant_id),
    event_type      TEXT NOT NULL,                 -- 'auth:login', 'secret:write', ...
    actor_user_id   UUID REFERENCES users(user_id),
    actor_email     TEXT,
    resource_type   TEXT NOT NULL,
    resource_id     TEXT,
    action          TEXT NOT NULL,
    status          TEXT NOT NULL,                 -- 'success' | 'failure' | 'denied'
    ip_address      INET,
    user_agent      TEXT,
    details         JSONB NOT NULL DEFAULT '{}',
    -- Hash chain
    prev_hash       BYTEA NOT NULL,                -- SHA-256 del evento anterior del tenant
    event_hash      BYTEA NOT NULL,                -- SHA-256 de este evento (incluye prev_hash)
    sequence_number BIGINT NOT NULL,               -- monotónico por tenant
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (tenant_id, sequence_number)
);

-- Append-only enforcement vía trigger
CREATE OR REPLACE FUNCTION audit_events_no_update_delete() RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'audit_events is append-only';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER audit_events_prevent_update BEFORE UPDATE ON audit_events
    FOR EACH ROW EXECUTE FUNCTION audit_events_no_update_delete();

CREATE TRIGGER audit_events_prevent_delete BEFORE DELETE ON audit_events
    FOR EACH ROW EXECUTE FUNCTION audit_events_no_update_delete();

-- Solo superuser puede bypass (para retención programada, auditada por CloudTrail)
```

**Hash computation:**
```python
event_hash = sha256(
    prev_hash
    || tenant_id
    || sequence_number.to_bytes(8, "big")
    || event_type
    || canonical_json({actor_user_id, resource_id, action, status, details, timestamp})
)
```

El primer evento de cada tenant usa `prev_hash = sha256(b"GENESIS:" + tenant_id)` (genesis determinístico).

**Stitch con ESK provenance_chain:**
Cuando se emite un evento crítico (`secret:write`, `tenant:delete`, `compliance:export`), también se registra una entrada en el `provenance_chain` del runtime ESK (axon-lang). El `event_hash` aquí y el `entry_hash` allá se referencian mutuamente — doble garantía para compliance SOC 2 / ISO 27001.

**Emisión automática:**
Cada servicio (TenantService, UserService, SecretsService, RBACService) toma un `AuditService` como dependency y emite el evento correspondiente en cada mutation.

**Verificación de integridad:**
`axon-enterprise audit verify --tenant {id}` recalcula la cadena entera y reporta cualquier divergencia. Se corre como cronjob diario.

---

### 10.h — Metering + Quota Enforcement

**Estado:** ✅ Completo (2026-04-21) — **Depende de:** 10.a, 10.c, 10.g

**Shipped commits (axon-enterprise):**
- `7113d2e` feat(fase-10.h): metering + quota enforcement + invoicing
- `a7374b7` test+docs(fase-10.h): metering suite + METERING.md operator guide

**Archivos producidos:**
- `axon_enterprise/metering/{__init__.py, errors.py, events.py, pricing.py, models.py, limiter.py, quota.py, invoicing.py, stripe_client.py, service.py}`
- `axon_enterprise/metering/collector.py` **eliminado** (v1.0.0 scaffolding con `organization_id`)
- `axon_enterprise/config/settings.py` extendido con `MeteringSettings` + `_validate_metering_production`
- `alembic/versions/20260421_0700_008_metering.py` — 4 tablas + seed de 3 planes
- `tests/metering/{test_pricing_and_limiter, test_invoicing, test_service_integration}.py` — 31 casos (22 unit + 9 integration)
- `docs/METERING.md` rewrite completo

**Decisiones cerradas (preguntas abiertas de la sesión anterior):**
- **Hybrid pricing model**: flat-tier con base + overage en Pro/Enterprise. Starter con hard_cap (free trial gate). Enterprise overage rates negociables (ejecuciones a 0c por default).
- **Redis para rate limits + Postgres para quotas mensuales**. Redis usa Lua script atómico (ZADD+ZRANGEBYSCORE) en un solo round-trip; Postgres es source of truth authoritativo para billing.
- **Stripe hybrid**: webhook-driven para payment events + draft-status invoices cuando Stripe disabled (operator review). Webhook signature verification via `stripe.Webhook.construct_event`.
- **Hard-cap immediate enforcement**: starter tenants excediendo limit reciben `QuotaExceeded` (mapping a 402 Payment Required) sin grace period. Error carries `metric`, `quantity`, `limit` para UI points-to-upgrade.

**Delta vs plan original:** + `MetricUnit` enum pareado con `MetricType` (aggregator suma within-unit only — previene mezclar tokens con GB), + `UsageSample` dataclass input (inmutable; callers nunca construyen `UsageEvent` ORM directamente), + **Rate limiter con quantity accumulation** (TPM counts tokens, no calls — RPM counts calls, aggregation correcta por dimension), + invoice **UNIQUE (tenant, period_start, period_end)** DB-enforced idempotency (batch jobs seguros), + `InvoiceGenerator` como pure function unit-testable sin DB, + overage math con `math.ceil` (nunca bill fractions of a cent), + millicents para compute time (track sub-cent provider costs accurate), + Stripe client con `enabled` property check (graceful degrade cuando not configured), + `MeteringAuditEmitter` Protocol (default no-op → audit adapter wired en 10.j), + `CHECK period_end > period_start` en invoices (DB-level sanity), + composite index `(tenant_id, metric_type, recorded_at)` en usage_events (aggregate queries O(log N)).

**Objetivo:** metering real (con tenant_id, no organization_id), pricing plans, integración Stripe, y **enforcement** (rate limiting, not just tracking).

**Modelo:**
```sql
CREATE TABLE pricing_plans (
    plan_id         TEXT PRIMARY KEY,              -- 'starter' | 'pro' | 'enterprise'
    display_name    TEXT NOT NULL,
    monthly_base_cents INT NOT NULL,
    included_executions BIGINT NOT NULL,
    included_tokens BIGINT NOT NULL,
    included_storage_gb INT NOT NULL,
    overage_per_execution_cents INT NOT NULL,
    overage_per_1k_tokens_cents INT NOT NULL,
    overage_per_gb_storage_cents INT NOT NULL,
    rate_limit_rpm  INT NOT NULL,                   -- requests/min
    rate_limit_tpd  BIGINT NOT NULL,                -- tokens/day
    active          BOOLEAN NOT NULL DEFAULT TRUE
);

CREATE TABLE usage_events (
    usage_id        UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id       TEXT NOT NULL REFERENCES tenants(tenant_id),
    metric_type     TEXT NOT NULL,
    quantity        DOUBLE PRECISION NOT NULL,
    unit            TEXT NOT NULL,
    flow_id         UUID,
    provider        TEXT,                           -- 'anthropic' | 'openai' | ...
    metadata        JSONB NOT NULL DEFAULT '{}',
    recorded_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX ON usage_events (tenant_id, recorded_at);

CREATE TABLE invoices (
    invoice_id      UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id       TEXT NOT NULL REFERENCES tenants(tenant_id),
    period_start    TIMESTAMPTZ NOT NULL,
    period_end      TIMESTAMPTZ NOT NULL,
    line_items      JSONB NOT NULL,
    subtotal_cents  INT NOT NULL,
    tax_cents       INT NOT NULL,
    total_cents     INT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'draft',  -- draft | finalized | paid | void
    stripe_invoice_id TEXT,
    issued_at       TIMESTAMPTZ,
    due_at          TIMESTAMPTZ,
    paid_at         TIMESTAMPTZ
);
```

**Quota enforcement (rate limit):**
- Redis-based sliding window counter per `(tenant_id, metric)` con TTL = ventana
- En la request path (Rust) o en GraphQL/REST gateway (Python): si `current_count > limit`, retorna `429 Too Many Requests` con headers `X-RateLimit-*` + `Retry-After`
- Overages: **no bloquean**, se acumulan en `usage_events` y se facturan como overage al fin de período (salvo que el plan sea `hard_cap=true`)

**Stripe integration:**
- Webhook receiver en `/webhooks/stripe` — valida firma, procesa `invoice.payment_succeeded`, `customer.subscription.updated`
- `StripeService.issue_invoice(tenant, period)` crea el invoice en Stripe con line items por metric
- `StripeService.suspend_on_delinquency(tenant)` marca el tenant como `status='suspended'` si 3 facturas sin pagar; el Rust lo rechaza en el extractor

---

### 10.i — Observability Wiring

**Estado:** ✅ Completo (2026-04-21) — **Depende de:** 10.a

**Shipped commits (axon-enterprise):**
- `0b8e2da` feat(fase-10.i): observability — metrics + tracing + structured logs
- `808946d` test+docs(fase-10.i): observability suite + OBSERVABILITY.md

**Archivos producidos:**
- `axon_enterprise/observability/{__init__.py, metrics.py, logging.py, tracing.py, middleware.py, healthz.py, decorators.py}` — reemplazo del scaffolding v1.0.0
- `axon_enterprise/config/settings.py` extendido con `ObservabilitySettings`
- `tests/observability/{test_metrics_and_logging, test_middleware_and_healthz}.py` — 15 casos unit (sin Docker)
- `docs/OBSERVABILITY.md` operator guide completo

**Decisiones cerradas (preguntas abiertas de la sesión anterior):**
- **OTel Collector sidecar** (no OTLP directo) — backend-agnostic, K8s idiomatic, swap Datadog↔Grafana Cloud sin code change.
- **Metric namespace `axon_*`** — matches Rust runtime, single Prometheus rule file cubre ambos planes.
- **Structured logs stdout JSON** — fluentd/vector K8s-native pickup; no direct SIEM SDK dep.
- **High-cardinality**: `tenant_id` en counters (cheap), NUNCA en histograms (explosion). Per-tenant latency SLIs vienen de OTel exemplars + tail-sampling en el Collector, NO de labels.

**Delta vs plan original:** + Pure ASGI middleware (not Starlette BaseHTTPMiddleware) — evita quirks con SSE/WebSockets que 10.k portal usará, + `set_log_context` con ContextVar Token-based cleanup (no cross-request leakage en workers reused), + `get_tracer` con `_NoopTracer` fallback cuando OTel SDK absent (app starts en minimal test envs), + middleware failures CATCHED + logged warn (observability nunca falla un request), + path label usa Starlette route template (`/tenants/{id}`) no raw URL (cardinality bounded), + `HealthStatus` dataclass con component breakdown (operators ven exactly what failed en 503), + `@instrument(span_name, counter, histogram)` decorator para service methods (consistent span+metric wrap), + Prometheus buckets span 5ms→10s (cubre el 99p de Enterprise workloads sin over-bucketing), + BUILD_INFO gauge carrying version/commit labels (scrape-once-discover-always pattern).

**Objetivo:** cerrar los `# TODO: Send to metrics backend` de v1.0.0. Prometheus + OpenTelemetry + structured logs, todo con `tenant_id` como dimensión.

**Prometheus:**
- `/metrics` endpoint en el Python service, usando `prometheus_client.CollectorRegistry`
- Métricas base: `axon_requests_total{tenant,method,status}`, `axon_request_duration_seconds{tenant,endpoint}` (histogram), `axon_flows_executed_total{tenant,status}`, `axon_llm_tokens_total{tenant,provider}`, `axon_quota_hit_total{tenant,metric}`
- ServiceMonitor K8s manifest para scraping

**OpenTelemetry:**
- `opentelemetry-instrumentation-starlette` + `-sqlalchemy` + `-asyncpg` + `-httpx`
- `tenant_id` en **baggage** (propaga a spans hijos automáticamente)
- OTLP exporter hacia collector (configurable: Datadog, Grafana Cloud, Jaeger)
- Sampling tail-based para traces de error (100%) y success (10%)

**Structured logs:**
- `structlog` con JSON renderer
- Contextvars auto-injection: `tenant_id`, `user_id`, `request_id`, `trace_id`
- Niveles: `DEBUG` (dev only), `INFO` (default), `WARNING`, `ERROR`, `CRITICAL`
- Redacción automática de valores marcados `Secret[str]` (no leakea en logs)

---

### 10.j — Admin API + CLI

**Estado:** ✅ Completo (2026-04-21) — **Depende de:** 10.a..10.i

**Shipped commits (axon-enterprise):**
- `007b0c2` feat(fase-10.j): Admin API + operator CLI
- `3cc018c` test+docs(fase-10.j): Admin API tests + ADMIN_API_AND_CLI.md guide

**Archivos producidos:**
- `axon_enterprise/http/{__init__.py, app.py, auth_middleware.py, errors.py, pagination.py}`
- `axon_enterprise/http/admin/{__init__.py, tenants.py, users.py, keys.py, audit.py}`
- `axon_enterprise/cli/{__init__.py, app.py, tenant.py, user.py, keys.py, audit.py, migrate.py}`
- `pyproject.toml` + `[project.scripts] axon-enterprise = ...` + typer dep
- `tests/http/{test_errors_and_pagination, test_admin_integration}.py` — 27 casos (20 unit + 7 integration con httpx.ASGITransport)
- `docs/ADMIN_API_AND_CLI.md` operator guide

**Decisiones cerradas (preguntas abiertas de la sesión anterior):**
- **Starlette puro** (no FastAPI) — menos magic, compatible con `ObservabilityMiddleware` ya construido, menor surface de deps.
- **Un solo server** con routing prefix — `/admin/*` gateable at ingress (nginx/envoy IP allowlist + mTLS), `/api/v1/*` (diferido a 10.k) público con JWT. Simplifica deploy y comparten estado.
- **Typer CLI** — types first-class, auto-help, mismo ergonomic que axon-lang CLI.
- **Pagination hybrid**: offset-based para admin tables (tenants/users/roles — total count útil para UI), cursor-based helpers ya implementados para 10.k usage/audit tables.

**Delta vs plan original:** + `AuthMiddleware` con JWKS fetch + 10min cache in-process (complementa el Rust verifier de 10.e — handler code Python también refuses unverified tokens, no solo el runtime Rust), + error mapping matrix explícita (28 typed errors → status codes estables) con **reveal_to_client=False collapses to generic family message** (previene enumeration via distinct error messages), + `RateLimited.Retry-After` header automatic, + `QuotaExceeded` body carries `metric` + `limit` (UI points-to-upgrade ready), + `AccountLockedError` reveals `until` timestamp safely (cliente sabe cuándo reintentar), + **Password-stdin guard** en `user create` — refuse argv para evitar leakage via `ps(1)` en shared hosts, + CLI commands async-wrapped con `asyncio.run` (single sync entry point para console_scripts), + `--exit-nonzero` toggle en `audit verify` para cron pipelines, + migrate CLI como thin Alembic wrapper con `AXON_DB_URL` resolution, + `Cursor.encode()` base64url para URL safety + `parse_cursor` raises en malformed (explicit failure mode), + tests usan httpx.ASGITransport + stub AuthMiddleware para test aislado del JWKS network hop.

**Objetivo:** superficie administrativa para operators (internal) y tenant owners (external).

**Admin API — interno (protegido por mTLS o IP allowlist):**
```
POST   /admin/tenants              crear tenant + provisionar KMS + crear owner user
GET    /admin/tenants              listar todos los tenants (con filtros)
GET    /admin/tenants/{id}         detalle (uso, plan, status, owner)
PATCH  /admin/tenants/{id}         update (plan, status, name)
POST   /admin/tenants/{id}/suspend suspender (trigger manual de deuda, violación ToS)
POST   /admin/tenants/{id}/resume  reactivar
DELETE /admin/tenants/{id}         schedule deletion (retención 30d, luego purge)
POST   /admin/tenants/{id}/impersonate  genera JWT one-shot para soporte (auditado!)
GET    /admin/usage/metrics        system-wide metering
GET    /admin/audit/events         cross-tenant audit (para compliance interno)
```

**Admin CLI (`axon-enterprise` command):**
```bash
axon-enterprise tenant create --slug acme --plan enterprise --owner-email admin@acme.com
axon-enterprise tenant list --status active
axon-enterprise tenant suspend <slug> --reason "payment failed"
axon-enterprise user invite <tenant> --email dev@acme.com --role developer
axon-enterprise secret rotate <tenant> <key> --new-value-from-stdin
axon-enterprise audit verify <tenant>  # recalcula hash chain, reporta integridad
axon-enterprise migrate status         # estado de migraciones Alembic
```

Ambos comparten la misma `AdminService`; el CLI es un wrapper Typer sobre HTTP al Admin API local, o conexión directa a BD si se corre con `AXON_LOCAL_ADMIN=true`.

---

### 10.k — Tenant Self-Service Portal API

**Estado:** ✅ Completo — **Depende de:** 10.c, 10.d, 10.f, 10.h, 10.j

**Commits (axon-enterprise):**
- `dd1bfde` feat(fase-10.k): api-keys + invitations modules + migración 009
- `5284da4` feat(fase-10.k): portal `/api/v1` router + Stripe webhook handler
- `10af220` test+docs(fase-10.k): portal + webhook tests + `PORTAL_API.md`

**Entregado:**
- `axon_enterprise/api_keys/` — M2M keys `axk_<uuid>` con Argon2id en-reposo. Prefix 8-hex indexado para lookup O(1); raw key surface exactly once on create; migración `009_tenant_api_keys` con RLS + `UNIQUE (tenant_id, key_prefix)`.
- `axon_enterprise/invitations/` — magic-link lifecycle on top de `tenant_memberships` (sin tabla nueva). SHA-256 token hash + 72h TTL + one-time consume on accept. Replay blocked by clearing `invitation_token_hash` en accept.
- `axon_enterprise/http/api/` — router `/api/v1/*`:
  - `auth/` login/refresh/logout/invite-accept
  - `sso/` OIDC initiate+callback; SAML metadata+ACS (tenant_id como path param)
  - `tenant/users` invite/list/deactivate/update-roles
  - `tenant/api-keys` CRUD (raw key echoed one time)
  - `tenant/usage` current-period totals + invoices list
  - `tenant/compliance` GDPR export/erasure → 202 + ticket_id + audit event (execución completa diferida a 10.l)
- `axon_enterprise/http/webhooks/` — `/webhooks/stripe` con verificación HMAC vía `StripeClient.verify_webhook`. Mapea `invoice.finalized/paid/payment_failed/voided` → transiciones de estado idempotentes. Eventos no reconocidos devuelven 204 para detener retries.
- `app.py` monta los tres routers (`/admin`, `/api/v1`, `/webhooks`) en el mismo Starlette app. `AuthMiddleware` ganó `public_prefixes` para SAML (tenant_id en path) + webhooks (re-auth por firma).
- `docs/PORTAL_API.md` — guía de operador: reglas de auth por prefijo, contrato por ruta, formato de error envelope.
- `tests/api_keys/`, `tests/invitations/`, `tests/http/test_portal_integration.py`, `tests/http/webhooks/test_stripe_webhook_unit.py` — unit + integration coverage.

**Decisiones cerradas:**
- **Password login + SSO coexisten** — el portal no impone SSO-only; `tenant.sso.required` puede activarse por tenant (hook ya disponible vía `SsoConfigurationService`, consumo por UI pendiente).
- **Magic link via DB-hashed one-time token** (no Redis) — SHA-256 hash + `invitation_expires_at` en `tenant_memberships`; zero nueva infra, replay-safe por construcción. Email delivery queda para 10.l (mailer service).
- **API key shape: `axk_<UUID4 hex>` + Argon2id** — UUID opaque (no JWT, sin claims a mantener); prefix `axk_` greppable en logs; primeros 8 hex chars indexados para verify O(1).
- **Stripe webhook público con signature verification** — Stripe IPs rotan constantemente; firma HMAC es el security boundary real. `AuthMiddleware` skip por prefix `/webhooks/`.
- **No dedupe table en 10.k** — las transiciones de estado (`paid` → `paid`) son idempotentes; 10.l agrega `processed_webhooks` cuando lleguen handlers con side-effects destructivos (ej. tenant suspension por impago).
- **Compliance endpoints aceptan + emiten audit event + devuelven ticket (202)** — la ejecución real (ZIP bundle, purga) aterriza en 10.l. Separar ingreso de ejecución permite test coverage temprano y mantiene el SLA visible al cliente.
- **`AuthMiddleware.public_prefixes`** — agregado como segundo argumento; necesario para matchear rutas paramétricas (SAML `{tenant_id}`) y webhook subtrees sin listar cada path.

**Objetivo:** endpoints para el owner de cada tenant — gestión sin intervención de soporte.

```
POST   /api/v1/tenant/users/invite          invitar usuario (email + rol)
GET    /api/v1/tenant/users                 listar miembros
DELETE /api/v1/tenant/users/{id}            revocar acceso
PATCH  /api/v1/tenant/users/{id}/roles      cambiar roles

GET    /api/v1/tenant/sso                   ver config SSO actual (redacted)
PUT    /api/v1/tenant/sso                   configurar OIDC/SAML
POST   /api/v1/tenant/sso/test              test de conexión contra IdP

GET    /api/v1/tenant/api-keys              listar (sin revelar secret)
POST   /api/v1/tenant/api-keys              crear (secret se retorna UNA vez)
DELETE /api/v1/tenant/api-keys/{id}         revocar

GET    /api/v1/tenant/usage                 dashboard de uso actual período
GET    /api/v1/tenant/invoices              historia de facturas
GET    /api/v1/tenant/invoices/{id}/pdf     PDF del invoice

POST   /api/v1/tenant/compliance/export     GDPR subject access request
POST   /api/v1/tenant/compliance/erase      right-to-erasure request
```

Toda request enforza `tenant_id` del JWT (no se puede cross-tenant aunque se ponga otro `{id}` en URL).

---

### 10.l — Compliance Tooling

**Estado:** ✅ Completo — **Depende de:** 10.a, 10.g, 10.j, 10.k

**Commits (axon-enterprise):**
- `0fd3e2d` feat(fase-10.l): compliance module + migración 010
- `8aae3d3` feat(fase-10.l): compliance HTTP + CLI + residency middleware
- `713a281` test+docs(fase-10.l): compliance suite + `COMPLIANCE.md`

**Entregado:**
- `axon_enterprise/compliance/` — módulo completo: `TicketService` (queue con `FOR UPDATE SKIP LOCKED`), `SarExporter` (tar.gz con manifest + JSONL por tabla), `ErasureService` (dos etapas: soft-delete → anonymize), `LegalHoldService`, `EvidenceBundleService` (SOC 2 por-período), `DataResidencyMiddleware`, `ComplianceWorker` (loop + promote_due_purges), `BlobStore` protocol con `LocalBlobStore` + `S3BlobStore`.
- `alembic/versions/010_compliance.py` — `compliance_requests` (con partial index `(status, scheduled_for) WHERE status IN ('queued', 'awaiting_purge')` para claim O(log N)), `legal_holds` (partial unique `(tenant_id, subject_email) WHERE released_at IS NULL`), `public.tenants.data_region` (default `us-east-1`).
- `/api/v1/tenant/compliance/*` — export/erase/{ticket_id}/list con 409 `compliance.legal_hold_active` cuando aplica. Download URL presigned via `BlobStore.signed_url`.
- `DataResidencyMiddleware` montado dentro de `AuthMiddleware` — 308 redirect cuando hay `residency_redirect_base`, 421 Misdirected Request cuando no. Cache 60s por tenant.
- CLI `axon-enterprise compliance ...` — export, erase, status, list-tickets, legal-hold {apply,release}, evidence-bundle, run-worker. SIGINT/SIGTERM manejados para shutdown limpio del worker.
- Audit events nuevos: `compliance:export_completed/failed`, `erasure_approved/completed/failed`, `legal_hold_applied/released`, `evidence_bundle_generated`, `residency_violation`.
- `ComplianceSettings` — blob backend (local|s3), server_region, residency_redirect_base, soft_delete_days=7, anonymize_sla_days=30, worker knobs. Production validator rechaza `blob_backend=local`.
- `MembershipStatus.ERASED_PENDING` + `ERASED` — estados del two-stage erasure.
- Tests: unit (blob store local, residency middleware) + integration (tickets, exporter, erasure con legal hold, evidence bundle).
- `docs/COMPLIANCE.md` — operator guide.

**Decisiones cerradas:**
- **Bundle format: tar.gz con manifest.json + JSONL-per-table** — JSONL stream es ingestible line-by-line, tar.gz agrega estructura y chain verification report (audit head + sequence + event_hash) en un solo artefacto descargable.
- **Two-stage erasure: 7 días soft-delete + anonymize** — soft-delete revoca sessions/API keys y flipea membership al estado `erased_pending` (reversible); anonymize scrubs PII irreversiblemente + deja purge report en blob store. Mid-window legal hold corta el anonymize (re-check antes de mutate).
- **Audit events NO se mutan** — mantener hash chain íntegro es el trade-off explícito; auditores ticknean "Art. 17 ejercido" via `compliance:erasure_completed` en el chain + purge report con SHA-256 del email original.
- **Worker dedicado con SKIP LOCKED + partial index** — N replicas safe; partial index keeps claim query fast independientemente de rows históricos. `promote_due_purges` corre en tick paralelo al claim loop.
- **Legal hold: partial unique index** — at most one ACTIVE hold per `(tenant_id, subject_email)`; hold histórico vive como row released. FTS audit trail lo documenta.
- **Data residency: middleware + column** — v1 cubre enforcement; multi-region deployment Terraform queda para 10.m si se decide montar más regiones.
- **BlobStore protocol + dos implementaciones** — dev usa `LocalBlobStore` (rechazado por validator en prod); prod usa S3 con presigned GETs. `build_blob_store()` factory inspecciona settings.
- **Evidence bundle reusa SAR tar.gz builder** — `_build_tar_gz` helper exportado desde `exporter.py`; `evidence.py` lo extiende agregando `rbac_snapshot.json` + `sso_configurations.json` como miembros no-JSONL.

**Objetivo:** cumplir GDPR / CCPA / SOC 2 sin ingeniería custom por cada request.

**GDPR Subject Access Request:**
- `POST /api/v1/tenant/compliance/export` con body `{user_email}` → scheduled job
- Query a todas las tablas con filter `user_id = ?` + `tenant_id` scoping
- Output: ZIP con JSON-per-table + hash chain snippet del audit log del usuario
- SLA: 30 días (GDPR Art. 12), pero típicamente < 1h

**Right to Erasure (Art. 17):**
- `POST /api/v1/tenant/compliance/erase` con body `{user_email, reason}`
- Soft delete inmediato (`user.status = 'erasure_pending'`)
- Background job purga PII después de 7 días (ventana para reversión legal)
- Audit events del usuario NO se borran (necesarios para compliance propio) pero se anonymizan: `user_email → 'erased-{hash}@axon.internal'`

**Data residency:**
- Column `tenants.data_region` ('us-east-1', 'eu-west-1', 'ap-southeast-1')
- Validación en middleware: si tenant.region != current region, redirect 308 al endpoint regional
- Deployment multi-region con Terraform per-región

**SOC 2 evidence:**
- Integración con el `EvidencePackager` que ya existe en ESK
- Endpoint `POST /admin/compliance/evidence-bundle` genera ZIP con: dossier, SBOM, provenance chain snippet, audit events del período, control statements — listo para auditor

---

### 10.m — Testing + Security Audit

**Estado:** ✅ Completo — **Depende de:** 10.a–10.l

**Commits (axon-enterprise):**
- `49fa5b8` test(fase-10.m): security adversarial suite
- `bd5df66` test(fase-10.m): k6 load test suite
- `04b01b9` docs(fase-10.m): threat model + GA readiness checklist

**Entregado:**
- `tests/security/` — adversarial suite:
  - `test_cross_tenant_isolation`: alpha cannot read/mutate beta's API keys, compliance tickets, memberships. Responde 404 (no 403) para no leakear existencia.
  - `test_rls_bypass_attempts`: GUC flip mid-session sigue bloqueado por filtros explícitos; triggers `audit_events` bloquean UPDATE/DELETE/TRUNCATE con SQLSTATE 42501.
  - `test_rbac_privilege_escalation`: viewer no puede `user:invite`; principal con `tenant_id` forjado es rechazado por RBAC resolviendo contra `(user_id, tenant_id)` real.
  - `test_jwt_forgery_resistance`: missing bearer, `alg=none`, unknown `kid`, wrong `iss/aud`, expired → 401.
  - `test_audit_chain_invariants`: hypothesis-driven — continuidad, determinismo, independencia cross-tenant, secuencia contigua.
  - `test_argon2_timing_parity`: `burn_equivalent_time` dentro de 1.5× de `verify` (mitigación de user-enumeration por timing).
- `tests/load/` — k6 scripts (portal API, admin API, audit storm 1000 tenants × 500 RPS). Thresholds actúan como CI gates.
- `docs/THREAT_MODEL.md` — STRIDE completo con mitigación + archivo de test que la ejerce; lista explícita de "known residual risks (accepted)".
- `docs/SECURITY_AUDIT.md` — GA-readiness checklist: gates automatizados, invariantes de código, controles operacionales, items no automatizables (rotation runbooks, pentest externo, legal review GDPR/CCPA, SOC 2 control mapping), SLO thresholds, comando de tag.

**Decisiones cerradas:**
- **Matriz cross-tenant = 404** — evita existence leakage; contrasta con 403 que confirmaría la existencia del recurso.
- **Fuzzing layered**: `hypothesis` para invariantes de servicios (audit chain, quota); `schemathesis` recomendado en CI para OpenAPI surface.
- **k6 sobre Locust** — JS scripts legibles + Grafana native + mejor p95/p99 reporting.
- **SLO thresholds específicos por endpoint** (no un valor global): reads p99<500ms, mutates p99<1s, audit write p99<100ms, SAR export per-subject <60s.
- **STRIDE completo, profundidad proporcional** — AuthN/AuthZ/Data flow deep; DoS ligero (cloud provider ya lo cubre en gran parte).
- **Pentest externo diferido a v1.1.1** — acceptable tag GA v1.1.0 con audit interno + running checklist; external audit es requisito enterprise contract renewal.

**Cross-tenant isolation tests:**
- Matriz de endpoints × tenants: tenant A hace request a recurso de tenant B → debe devolver 404 (no 403, para evitar leakage de existencia)
- Fuzzing: `hypothesis` con `tenant_id` + payloads arbitrarios
- RLS bypass test: intento manual de `SET axon.current_tenant = 'B'` en sesión autenticada como tenant A

**Load tests:**
- `locust` con 1000 tenants simultáneos, cada uno con 10 users activos
- Escenarios: login flood, secret read burst, metering spike, audit write storm
- Métricas: p50/p99 latency per-tenant, isolation (un tenant saturado no debe degradar a otro)

**Threat model:**
- STRIDE por cada subsistema documentado en `docs/threat_model_axon_enterprise.md`
- Controles mapeados a OWASP ASVS v4 L3
- Pentesting externo (tercero) antes del GA de v1.1.0

**Security audit checklist:**
- [ ] CSP headers, HSTS, secure cookies
- [ ] No password/secret en logs (test con grep + redaction verification)
- [ ] JWT tokens rotados antes de retirarse
- [ ] RLS enabled en TODAS las tablas con `tenant_id`
- [ ] SQL injection fuzzed en cada handler
- [ ] Rate limiting real, no solo métrica
- [ ] Timing attacks: password verification usa `constant_time_compare`

---

## Log de decisiones

Las decisiones tomadas durante la ejecución de Fase 10 se registran aquí con fecha y contexto — para recuperar el estado mental en sesiones futuras.

| Fecha | Decisión | Contexto / alternativas consideradas |
|-------|----------|--------------------------------------|
| 2026-04-21 | Fase 10 se ejecuta en el repo `axon-enterprise` (no en axon-lang) | Separation of concerns: axon-lang es el runtime, axon-enterprise es el control plane comercial. |
| 2026-04-21 | Postgres compartido entre Python (Fase 10) y Rust (M1–M5), una sola BD | Evita dual source of truth. RLS funciona en ambos sentidos. Mismo `axon.current_tenant` GUC. |
| 2026-04-21 | JWTs firmados por Python RS256, verificados por Rust contra JWKS | Cierra el TODO "no signature verification" actual en Rust. KMS mantiene la llave privada en HSM. |
| 2026-04-21 | Audit hash chain stitched a ESK provenance_chain | Doble garantía de tamper-evidence; aprovecha el primitivo ya existente en axon-lang. |
| 2026-04-21 | 10.a: `FORCE ROW LEVEL SECURITY` en cada tabla tenant-scoped | Sin FORCE, el owner de la tabla (axon_app cuando actúe como creador) bypassaría RLS. FORCE aplica la política incluso al owner — defense in depth. |
| 2026-04-21 | 10.a: NULL guard en la policy — `current_setting(..., true) IS NOT NULL` | Un query sin GUC set devuelve 0 filas en lugar de todas. El comportamiento default de `current_setting(.., true)` sin NULL check permitiría `WHERE tenant_id = NULL` que no matchea nada, pero dejar la policy con esa ambigüedad era innecesariamente frágil. |
| 2026-04-21 | 10.a: Alembic usa `NullPool` durante migraciones | Un pool pool-recycle podría descartar la conexión mid-migration y perder `SET LOCAL`. |
| 2026-04-21 | 10.a: `TenantScopedMixin.__tablename__` genera índice compuesto `(tenant_id, created_at)` por defecto | Shape de query más común; mejora perf sin requerir declaración manual en cada modelo. |
| 2026-04-21 | 10.b: Envelope encryption con AAD serialised ordenado por clave | `{"a":"1","b":"2"}` produce byte-idéntico output regardless de dict insertion order — evita bugs si dict ordering cambia entre Python versions. |
| 2026-04-21 | 10.b: `users` table con RLS `FORCE` + `admin_bypass` (sin tenant_isolation) | Tabla global: un user puede estar en N tenants. Service layer enforza "este user pertenece a mi tenant" via `tenant_memberships` bajo `tenant_session`, luego abre `admin_session` para leer el user. RLS sin bypass sería imposible (necesitamos poder leer cross-tenant en paths privilegiados). |
| 2026-04-21 | 10.b: Refresh tokens 64 bytes random → SHA-256 hash persistido | Pérdida de BD no revela refresh tokens (attacker tendría hash sin preimage). SHA-256 (no HMAC) porque el hash no necesita secret-key property — el atacante con hash sigue sin poder forjar un token de 64 bytes. |
| 2026-04-21 | 10.b: Replay detection revoca TODA la chain para `(user_id, tenant_id)` | Si alguien presenta un token ya-rotado, OR es un attacker OR es un cliente legítimo con bug. Revocar ambos (forzando re-login) es el camino seguro — no podemos distinguir who's who sin metadata adicional. |
| 2026-04-21 | 10.b: HIBP k-anonymity con `Add-Padding: true` header + fails-open | Padding mitiga traffic analysis (response size revela hit/miss sin padding). Fails-open porque un outage de HIBP no debe bloquear registros legítimos — trade-off consciente en favor de availability sobre defense-in-depth absoluto. |
| 2026-04-21 | 10.c: `permissions` table es global sin RLS (read-only closed set) | Una tabla tenant-scoped significaría que cada tenant puede inventar permission strings que el código no enforza — security hole. Catalog cerrado ensure strings coinciden con `@require_permission` decorators. |
| 2026-04-21 | 10.c: Denormalized tenant_id en role_permissions + user_roles | Policy RLS puede aplicar directamente sin JOIN. JOIN-en-policy puede causar recursive policy evaluation (policy consulta tabla que tiene su propia policy que consulta la primera) — evitado. |
| 2026-04-21 | 10.c: Owner rol con TODOS los permissions enumerados (no wildcard) | Catalog growth → owner recibe los nuevos permissions automáticamente via re-run del seeder idempotent. Wildcard haría imposible auditar exactamente qué puede hacer owner en un point-in-time. |
| 2026-04-21 | 10.c: `@require_permission("x:y")` parsea at decoration time | Typos fallan at import (handler no se carga) en lugar de at request (handler loads pero nunca matchea). Elimina una clase entera de bugs silenciosos. |
| 2026-04-21 | 10.c: Cycle prevention doble — _assert_no_cycle + UNION en CTE | UNION dedupes cycles en read (queries terminan incluso con cycle smuggled). _assert_no_cycle at write-time da error message explícito ("would create cycle") — fail-fast > fail-silently. Defensa en profundidad. |
| 2026-04-21 | 10.c: BuiltInRoleProtected impide delete/rename de built-in roles | owner/admin/developer/viewer son contratos entre el sistema y los handlers — un handler que dice `@require_permission("tenant:read")` asume que "admin" rol existe y lo tiene. Renombrar o borrar un built-in rompe el contrato. |
| 2026-04-21 | 10.d: SAML replay defence via UNIQUE constraint en BD (no check-then-insert) | Check-then-insert tiene race window (dos requests simultáneos con mismo assertion_id pasan el check, luego uno falla el insert). UNIQUE constraint hace la concurrencia de Postgres hacer el trabajo — atomic by construction. |
| 2026-04-21 | 10.d: OIDC discovery con asyncio.Lock + in-flight futures dedup | N requests al mismo issuer en paralelo disparaban N HTTP fetches sin dedup. Con dedup solo 1 fetch + N coroutines esperan el mismo future. Reduce latencia P99 y carga en el IdP. |
| 2026-04-21 | 10.d: JWKS con force-refresh-on-kid-miss + Cache-Control: no-cache bypass | IdPs rotan llaves publicando la nueva kid minutos antes de usarla. Sin force-refresh, nuestro cache stale rechaza tokens legítimos firmados con kid nuevo. Bypass de Cache-Control es el segundo chance para CDN stale. |
| 2026-04-21 | 10.d: `role_map` es additive-only (no revoca) en SSO login | Admins pueden grantear roles extra out-of-band (ej. promover un user temporalmente). Si SSO login los revocara por no estar en el IdP, eso pisa la decision manual del admin. Strict sync diferido hasta compliance explicita. |
| 2026-04-21 | 10.d: Reveal-to-client matrix explícito en SsoError subclasses | Sin esto, HTTP middleware no sabe cuáles errors son safe to return vs cuáles deben collapsarse a 401 genérico. Leakear "nonce_mismatch" vs "state_invalid" permite a un attacker inferir qué parte del flow es el problema. |
| 2026-04-21 | 10.e: Partial unique index `WHERE status='active'` en jwt_signing_keys | Enforces "one active key" invariant at the DB level. Sin esto, un bug en app code podría insertar dos rows active y el issuer elegiría una arbitrariamente. CHECK constraints no expresan "only one row" — partial unique lo hace. |
| 2026-04-21 | 10.e: Reserved claims overwrite `extra_claims` en JwtIssuer.mint | Sin overwrite, un caller que pase `extra_claims={"tenant_id": "victim"}` silently impersonaría a otro tenant. Defensivo against programmer mistakes — callers pueden querer extend claims pero NUNCA sobrescribir iss/sub/aud/exp/iat/nbf/jti/tenant_id/roles. |
| 2026-04-21 | 10.e: kid = SHA-256(SPKI DER)[:16] compartido entre Local + KMS signer | Migrar operator de local → KMS (o vice-versa) NO rota el kid mientras el public key del KMS sea el mismo. JWTs minted antes de la migration siguen verificando post-migration. Deterministic kid > random. |
| 2026-04-21 | 10.e: Rust verifier con `enforce` flag + fallback legacy path | Pre-10.e deployments (incluyendo OSS/single-tenant) siguen funcionando sin `AXON_JWT_JWKS_URL` set — no breaking change. Enterprise deployments flip enforce=true via env, no code change. Gradual rollout vs hard cutover. |
| 2026-04-21 | 10.e: Redis + Postgres para revocation (fail-closed en Redis down) | Redis solo sería insuficiente: ephemeral, datos perdidos en restart. Postgres solo: too slow en hot path. Ambos: Postgres es source of truth, Redis acelera. Critical: `is_revoked()` falla-closed (Redis down → checa Postgres) — nunca silently permite un token revocado. |
| 2026-04-21 | 10.f: `SecretValue` bloquea `__format__` con spec non-vacío | `f"{secret:>20}"` es el vector más silencioso de leak — se compila, no raises, produce la string con el plaintext. Bloqueando format specs forzamos `f"{secret.reveal():>20}"` explicit, visible en code review. |
| 2026-04-21 | 10.f: `__reduce__` de SecretValue retorna `[REDACTED]` | Pickle / deepcopy serializar el plaintext es un leak path común (debugging, caching, multiprocess). `__reduce__` intercepta todos esos paths de una vez — tests que copian fixtures no accidentally expose. |
| 2026-04-21 | 10.f: Path prefix CONGELADO (`axon/tenants`) — no config en runtime | Cambiar path_prefix requiere migration coordinada Python + Rust (axon-rs TenantSecretsClient). Config existe en settings para dev flexibility pero changes en production rompen la compatibility con M3. Documentado explícitamente en SECRETS.md. |
| 2026-04-21 | 10.f: Settings validator refactored en helpers per-subsistema | Cognitive complexity del validator pasó 21 después de 10.e. Helper methods (_validate_db_production, _validate_envelope_production, ...) mantienen la lint under 15 y hacen los gates fácilmente testeables en unit. |
| 2026-04-21 | 10.f: Reserved key prefixes (axon_, system_, internal_) | Evita colisión con futura metadata que podríamos almacenar en AWS SM bajo el mismo prefix pero como "system keys" invisibles al tenant. Conservative default — expand reserved list es trivial. |
| 2026-04-21 | 10.f: `audit_on_read=true` obligatorio en production | SOC 2 CC.6.1 requiere audit trail para secret access. Lower tiers pueden desactivarlo (performance). Validator rejects env=production con audit_on_read=false, fail-fast en startup. |
| 2026-04-21 | 10.g: BEFORE TRUNCATE trigger (además de UPDATE+DELETE) | `TRUNCATE` bypasses BEFORE UPDATE/DELETE triggers en Postgres — un admin con `TRUNCATE` privilege podría borrar el log sin dejar rastro. BEFORE TRUNCATE cierra ese vector específicamente; sin este trigger el append-only garantee es incompleto. |
| 2026-04-21 | 10.g: `ip_address TEXT` (no INET) en audit_events | Postgres INET normaliza representación (`203.0.113.9/32` vs `203.0.113.9`, IPv6 compaction). Si el writer pasa X y Postgres stores Y, hash recomputation durante verify recibiría Y — mismatch. TEXT preserva bytes exactos. |
| 2026-04-21 | 10.g: Separator `0x1e` entre fields del hash input | Sin separator, `tenant="ab" seq=123 type="x"` y `tenant="a" seq=123 type="bx"` producen concat idéntica. ASCII Record Separator no aparece en tenant/type strings legítimos → ambiguity impossible. Matches ESK's canonical_bytes pattern. |
| 2026-04-21 | 10.g: `AuditChainReport` dataclass (no raise) + `require_chain_healthy` wrapper | Verify walks chain; dashboards necesitan output estructurado (sequence_number, reason), no stack trace. Wrapper lets scripts usar exception-flow cuando prefieran. Best of both. |
| 2026-04-21 | 10.g: `pg_advisory_xact_lock(hashtext(tenant_id))` per-write | Sin lock, dos writers en mismo tenant pueden computar sequence_number=N concurrente; UNIQUE constraint rechaza uno pero lo convierte en error bandwidth. Advisory lock serializa dentro del tenant, cross-tenant writers no contend porque hashtext distinto. |
| 2026-04-21 | 10.g: Enum AuditEventType cerrado (41 valores, extension via migration) | Permitir adopters definir event types dinámicamente rompería retention policies + SIEM integration (queries filtran por type string — catálogo open = queries becomes brittle). Migration gate forces doc + code review cuando alguien agrega un evento. |
| 2026-04-21 | 10.h: Hybrid pricing (flat-tier con overage) en lugar de puro usage-based | Flat-tier da predictabilidad a buyers (CFO-friendly); overage captures power-users sin requerir negociación custom. Stripe soporta ambos via InvoiceItem + Invoice separados. |
| 2026-04-21 | 10.h: `math.ceil` en overage math (never fractions of a cent) | Billing accuracy. Floor puede dar al cliente gratis; round-half-up puede overcharge por 0.5c en boundary cases. Ceiling es conservative (siempre al favor del emisor) y el overcharge max es 1c — auditable, no payment dispute. |
| 2026-04-21 | 10.h: Millicents (1/1000 USD) para compute time | LLM provider cost passthrough cuesta $0.00003 / 1M tokens — expresarlo en cents rounded sería 0. Millicents dan precisión 1000x sin cambiar storage to decimal. Convert to cents (ceil) at invoice boundary. |
| 2026-04-21 | 10.h: Rate limiter con `quantity` accumulation (no solo count) | Sin quantity, TPM (tokens-per-minute) no se puede enforcer: un request con 10k tokens contaría igual que uno con 100. Quantity accum permite una sola abstracción para RPM + TPM + futuras dimensions (egress bytes, compute seconds). |
| 2026-04-21 | 10.h: `UNIQUE (tenant, period_start, period_end)` en invoices | Batch jobs pueden re-correr por retry logic. Sin UNIQUE, un retry generaría un segundo invoice → double-billing. DB-level constraint converts retries en idempotent (`InvoiceAlreadyIssued` raise). |
| 2026-04-21 | 10.h: MetricUnit pareado a MetricType — no mezclar units en aggregate | Aggregator que suma sin check de unit podría combinar tokens + bytes + seconds en un solo número. Pareando type → default unit, el aggregator rechaza mezclas en write time si el caller intenta override incorrectamente. |
| 2026-04-21 | 10.h: Stripe integration opcional (draft status cuando disabled) | Operators pueden correr Axon sin Stripe (self-hosted enterprise con billing manual). Mantener stripe_enabled=false como default respeta el "OSS-friendly" y fail-fast cuando enterprise-tier olvida setearlo. |

---

## Open questions (a resolver antes de mergear cada sub-fase)

- **10.a:** ¿Connection pool per-tenant (aislamiento fuerte) o pool compartido con RLS (eficiente)? Hoy: pool compartido. Re-evaluar si un tenant saturado afecta p99 de otros.
- **10.d:** ¿Soportar múltiples IdPs por tenant (ej: OIDC + SAML simultáneos)? Hoy: uno por tenant. El DB schema lo soporta pero la UI/API asume uno.
- **10.f:** ¿Permitir per-user secrets (no solo per-tenant)? Caso de uso: el mismo tenant tiene varios devs con LLM accounts personales. Deferred a v1.2.
- **10.h:** ¿Prepaid (hard cap) vs postpaid (overage billed)? Hoy: configurable por plan. Starter = hard_cap, Pro/Enterprise = overage. Revisar después del primer cliente real.
- **10.l:** ¿Right-to-erasure borra audit events? Hoy: anonymize, no borrar. Legal debe confirmar que anonymization cumple Art. 17.

---

## Sesión actual — estado vivo

**Última actualización:** 2026-04-21

**Próxima sesión — pickup point:** **Fase 10 COMPLETA**. GA `v1.1.0` listo para tag — ejecutar checklist en `docs/SECURITY_AUDIT.md` y disparar `git tag -a v1.1.0`.

**Decisiones cerradas en esta sesión (10.m):**
- Cross-tenant isolation returns 404 (not 403) para evitar existence leakage — alpha probing por tickets de beta no aprende nada.
- Adversarial tests cubren las 4 capas de defensa: JWT verification → RBAC → service filter → RLS. Cada capa tiene al menos un test donde las demás asumen fallaron.
- Audit chain property tests con hypothesis (10 ejemplos por `@given`) son suficientes — invariantes determinísticos, no hay random state oculto que amplifique con más ejemplos.
- Timing parity tolerance = 1.5× ratio entre `verify` y `burn_equivalent_time`. Wider que lo ideal (factor 2 sería peligroso) pero CI shared runners son noisy; verdadera protección es monitoring en prod.
- k6 sobre Locust — thresholds built-in que actúan como CI gate; Grafana nativo.
- SLO thresholds granulares por endpoint (no global) publicados en `SECURITY_AUDIT.md` y enforced en cada script k6.
- Pentest externo diferido a `v1.1.1` — checklist interna + audit trail del chain es aceptable para tag GA inicial.
- `SECURITY_AUDIT.md` es la fuente única de verdad para tag — gates automatizados + non-automatable items explícitos; no silent skips.

**Cierre del plan I/O Cognitivo — Fase 10 Enterprise Control Plane:**
- [x] 10.a Persistence Foundation
- [x] 10.b Identity Core
- [x] 10.c RBAC Production-Grade
- [x] 10.d SSO Real (OIDC + SAML)
- [x] 10.e JWT Issuer + JWKS rotation
- [x] 10.f Secrets Service
- [x] 10.g Audit Hash-Chain
- [x] 10.h Metering + Quota Enforcement
- [x] 10.i Observability Wiring
- [x] 10.j Admin API + CLI
- [x] 10.k Tenant Self-Service Portal API
- [x] 10.l Compliance Tooling
- [x] 10.m Testing + Security Audit

**Sesión abierta en:**
- `axon-enterprise`: commits hasta `04b01b9` (Security audit + threat model + GA checklist)
- Tag `v1.1.0` pendiente de sign-off por engineering lead según `docs/SECURITY_AUDIT.md`
- Doc vivo actualizado en `axon-lang:docs/multi_tenancy_axon.md`

---

### Decisiones archivadas (sesiones anteriores)

**Decisiones cerradas en sesión 10.l:**
- Bundle SAR = tar.gz con `manifest.json` (audit chain head + included/excluded tables) + JSONL per-table.
- Erasure two-stage: 7 días soft-delete + 30 días SLA para anonymize. `details.soft_deleted_at` discrimina las fases.
- `audit_events` NUNCA se mutan — hash chain íntegro; evidencia de erasure vive en `compliance:erasure_completed` + purge report.
- Worker dedicado: `FOR UPDATE SKIP LOCKED` + partial index `(status, scheduled_for)`. N replicas safe.
- Legal hold: partial unique index WHERE `released_at IS NULL`; check at file-time + anonymize-time.
- Data residency v1: middleware + columna `tenants.data_region`; multi-region Terraform diferido.
- BlobStore: protocol con `LocalBlobStore` + `S3BlobStore`. `blob_backend=local` rechazado en producción.
- Evidence bundle reusa `_build_tar_gz`; agrega `rbac_snapshot.json` + `sso_configurations.json` + emite `compliance:evidence_bundle_generated`.

**Pre-requisitos para tag v1.1.0:** completar el checklist en `axon-enterprise:docs/SECURITY_AUDIT.md`:
- [ ] Automated gates verdes en `master` (pytest, security/, hypothesis, lint, pip-audit, schemathesis, k6)
- [ ] Secrets rotation runbook probado en staging (últimos 90 días)
- [ ] Key rotation runbook para JWT signing key
- [ ] Incident response runbook (credenciales comprometidas, RLS bypass, blob leak, chain divergence, legal hold outage)
- [ ] Pentest externo agendado dentro de 180 días post-GA
- [ ] Third-party dependency review — cada package con maintainer activo
- [ ] Backup + restore test — PITR exercised en el trimestre
- [ ] GDPR + CCPA legal sign-off sobre `SarExporter` + anonymization semantics
- [ ] SOC 2 control mapping — cada control del Type II report mapeado a un audit event type

---

### Decisiones archivadas (sesiones anteriores)

**Decisiones cerradas en sesión 10.k:**
- Password + SSO coexisten en el portal; `tenant.sso.required` como flag futuro no bloquea login hoy.
- Magic-link via `invitation_token_hash` (SHA-256) + `invitation_expires_at` en `tenant_memberships` — zero infra nueva; TTL 72h.
- API key shape `axk_<uuid4-hex>` + Argon2id at-rest; primeros 8 hex chars indexados para verify O(1); raw key echoed exactly once.
- Webhooks públicos con signature verification (`/webhooks/stripe` usa HMAC del SDK oficial como security boundary).
- Eventos `invoice.*` manejados explícitamente; otros devuelven 204 para cerrar el retry loop de Stripe.
- `AuthMiddleware.public_prefixes` para rutas paramétricas (SAML `{tenant_id}`) y webhook subtrees sin enumerar paths.

**Decisiones cerradas en sesión 10.j:**
- Starlette puro (no FastAPI) — menos magic, compatible con middleware existente.
- Un solo server con routing prefix (`/admin/*` + `/api/v1/*`), ingress IP allowlist para admin.
- Typer CLI — types first-class, auto-help.
- Error mapping matrix con `reveal_to_client` collapse — previene enumeration via distinct error messages.
- Password-from-stdin guard en CLI — argv es visible en `ps(1)`.
- AuthMiddleware valida JWT también en Python handler side (complementa el Rust verifier de 10.e).
- Pagination hybrid: offset para admin tables + cursor helpers para 10.k usage/audit tables.

**Decisiones cerradas en sesión 10.i:**
- OTel Collector sidecar — backend-agnostic, K8s idiomatic.
- Metric namespace `axon_*` — shared con Rust.
- Logs stdout JSON → fluentd → SIEM; no direct SDK.
- `tenant_id` label en counters only; histograms sin tenant (cardinality bounded).
- Pure ASGI middleware (no BaseHTTPMiddleware) — evita quirks con streaming / SSE.
- ContextVar-driven correlation + Token-based cleanup — no cross-request leakage.
- Middleware failures catched (observability nunca falla request).
- `_NoopTracer` fallback sin OTel SDK — test envs minimales arrancan.
- Path label usa route template (`/tenants/{id}`) — cardinality bounded on 404 storms.

**Pre-requisitos para 10.j:**
- [x] 10.a..10.i completados
- [x] Todos los services tienen API pública lista para wrap con HTTP handlers
- [x] `@require_permission` decorator (10.c) listo para uso en handlers
- [x] `ObservabilityMiddleware` listo para mount outermost
- [ ] Decidir HTTP framework — Starlette puro vs FastAPI? Propongo **Starlette** — axon-lang ya lo usa, menos magic, mejor compatible con ASGI middleware ya construido.
- [ ] Decidir auth separation — ¿dos servidores ASGI (admin API privado + tenant API público) o un solo server con routing prefix? Propongo **un solo server** con `/admin/*` protegido por IP allowlist + mTLS, `/api/v1/*` por JWT. Simplifica deploy.
- [ ] Decidir CLI framework — Typer vs Click? Propongo **Typer** — types first-class + auto-generated --help; same ergonomic que axon-lang CLI.
- [ ] Decidir pagination style — cursor-based vs offset? Propongo **cursor-based** (UUID monotonic) para tablas con alto volumen (usage_events, audit_events); offset OK para tablas pequeñas (users, roles).

**Sesión abierta en:**
- `axon-enterprise`: commits hasta `808946d` (Observability + tests + docs)
- Doc vivo actualizado en `axon-lang:docs/multi_tenancy_axon.md`

---

### Decisiones archivadas (sesiones anteriores)

**Decisiones cerradas en sesión 10.h:**
- Hybrid pricing: flat-tier + overage en Pro/Enterprise, hard_cap en starter (free trial).
- Redis Lua atómico para rate limits; Postgres aggregate para quotas mensuales.
- Stripe webhook-driven + draft invoices cuando disabled. Signature verification obligatoria cuando enabled.
- `math.ceil` en overage calc — nunca fractions of a cent.
- Millicents para compute time — track sub-cent provider costs.
- `UNIQUE (tenant, period_start, period_end)` en invoices — DB-enforced idempotency.
- Rate limiter con quantity accumulation — TPM counts tokens correctly, no just calls.
- `MetricUnit` pareado a `MetricType` — aggregator suma within-unit only.

**Pre-requisitos para 10.i:**
- [x] 10.a..10.h completados
- [x] MeteringService + AuditService emitiendo structured logs listos para metrics scrape
- [x] Todos los services (identity/rbac/sso/secrets/metering) con `_logger = structlog.get_logger(...)` wired
- [ ] Decidir backend OTel: ¿OTLP directo vs sidecar Collector? Propongo **sidecar Collector** — isolation del backend change, K8s idiomatic, easier to swap Datadog ↔ Grafana Cloud.
- [ ] Decidir metric namespace: ¿`axon_enterprise_*` vs `axon_*`? Propongo **`axon_*`** — matches con lo que Rust emitirá, single grep surface.
- [ ] Decidir structured log destination: stdout (K8s-style) vs direct SIEM (Datadog/Splunk)? Propongo **stdout + K8s fluentd → Datadog** — no direct SDK dep, easier to test, operator flexibility.
- [ ] Decidir high-cardinality strategy: ¿tenant_id como label en cada metric (prometheus retention explosion) o aggregate via exemplars? Propongo **tenant_id en top-level buckets only** (requests / errors) + **no label** en performance metrics (latency, tokens) — trade-off deliberate.

**Sesión abierta en:**
- `axon-enterprise`: commits hasta `a7374b7` (Metering + tests + docs)
- Doc vivo actualizado en `axon-lang:docs/multi_tenancy_axon.md`

---

### Decisiones previas (archived)

**Decisiones cerradas en sesión 10.g:**
- Canonical JSON compartido con ESK — byte-identical hash input Python↔Rust.
- Append-only via trigger Postgres con SQLSTATE 42501 — defensivo incluso contra rogue psql.
- Hash chain **per-tenant** (genesis determinístico por tenant_id).
- ESK stitch opcional por evento — services con acceso directo a ESK pasan el hash; full bridge deferido a 10.l.
- Separator `0x1e` (Record Separator) en hash input — evita ambigüedad de concatenación.
- `pg_advisory_xact_lock(hashtext(tenant_id))` serializa writers per-tenant; cross-tenant no contend.
- Triggers cubren UPDATE + DELETE + **TRUNCATE** (el último vector que bypass UPDATE/DELETE triggers).
- `ip_address TEXT` (no INET) — Postgres no debe canonicalizar o rompe hash recomputation.
- Enum AuditEventType cerrado (41 valores) — extensión requires migration, nunca hot-fix.

---

## Routing Git para este plan

### M1–M5 (Rust / axon-lang)

Commits en este repo (`axon-lang`), pusheados a ambos remotes:

```bash
git push origin master && git push enterprise master
```

Prefijo: `feat(enterprise): ...` cuando el cambio es enterprise-only; `feat(runtime): ...` cuando aplica al open-source también.

### Fase 10 (Python / axon-enterprise)

Commits en el repo `axon-enterprise` (sibling directory). Subir directo:

```bash
cd ../axon-enterprise
git push origin master
git tag v1.1.0-alpha.X && git push origin v1.1.0-alpha.X    # alpha per sub-fase
git tag v1.1.0 && git push origin v1.1.0                    # GA al terminar 10.m
```

Prefijo: `feat(fase-10.X): ...` donde X es la sub-fase (a, b, c, ...). El tag `v*` dispara el workflow `release.yml` que construye y publica la imagen a ECR (`axon/axon-enterprise:1.1.0`).
