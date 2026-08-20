# El Sistema Inmunitario Cognitivo
## Formalización de las Primitivas `immune`, `reflex` y `heal` para Defensa Activa de Aplicaciones en el Lenguaje Cognitivo AXON

> [!ABSTRACT]
> El estado del arte en defensa de aplicaciones impulsadas por IA padece una asimetría profunda: las herramientas existentes protegen el modelo de lenguaje (mediante filtros de prompts, taint analysis, IFC) pero dejan desprotegida la arquitectura anfitriona (red, base de datos, sesiones, código). AXON v1.0.0 introdujo `shield` como gatekeeper epitelial del LLM. Este paper formaliza el siguiente estrato defensivo: **el Sistema Inmunitario Cognitivo**, descompuesto en tres primitivas composables — `immune` (detección por inferencia activa), `reflex` (respuesta determinista O(1)) y `heal` (autosanación cibernética via OTS bajo Linear Logic). La descomposición preserva la composicionalidad del lenguaje, permite adopción gradual, y satisface restricciones regulatorias (GxP, HIPAA, SOC 2) mediante modos operativos auditables. La fundamentación matemática reutiliza el Principio de Energía Libre Variacional ya implementado en `psyche`, y el grounding tipológico se ancla en el lattice epistémico estándar de AXON con envelopes ΛD (Lambda Data, v0.22).

---

## 1. Motivación y Posicionamiento

### 1.1 La Asimetría Defensiva del Estado del Arte

Los pipelines de IA contemporáneos despliegan dos clases de defensa, mutuamente exclusivas en su foco:

1. **Defensa centrada en el modelo**: filtros de prompt injection, jailbreak detectors, redactores de PII, sandboxes de tool use. Esta familia, cuya implementación canónica en AXON es la primitiva `shield`, opera como una barrera epitelial: estática, ex-ante, basada en taint analysis e IFC.

2. **Defensa centrada en la infraestructura**: WAFs, IDS/IPS, EDR, sistemas SIEM. Esta familia, externa al lenguaje cognitivo, opera con heurísticas y firmas, sin acceso al estado epistémico del agente.

La discontinuidad entre ambas clases es crítica. Un atacante que evade el WAF y entrega un payload aparentemente benigno al endpoint de AXON puede aún ser bloqueado por `shield` antes de tocar el LLM. Pero un atacante que opera *después* del LLM — exfiltrando datos por canales laterales, manipulando sesiones autenticadas, o explotando vulnerabilidades de Día Cero en el binario anfitrión — queda fuera del modelo de amenaza de `shield`.

Este paper postula que la defensa cognitiva completa requiere una segunda primitiva, formalmente independiente de `shield`, cuyo foco sea **la salud sistémica de la infraestructura anfitriona**, no la integridad del prompt.

### 1.2 Tesis Central

Postulamos la existencia de un **Sistema Inmunitario Cognitivo** dentro del runtime de AXON, descompuesto en tres primitivas formales:

- **`immune`** — el detector de anomalías por Inferencia Activa
- **`reflex`** — la respuesta motora determinista de latencia sub-milisegundo
- **`heal`** — la autosanación adaptativa mediante síntesis ontológica de parches

Esta descomposición no es estética. Cada primitiva tiene una semántica formal independiente, una superficie de auditoría distinta, y un modo de fallo aislado. La composición de las tres recupera el sistema completo; el uso aislado de cualquiera permite adopción gradual y modos regulados.

---

## 2. Fundamento Biológico: La Teoría de Redes Inmunitarias de Jerne

Propuesta por el Premio Nobel Niels K. Jerne en 1974, la **Teoría de Redes Idiotípicas** (*Idiotypic Network Theory*) reformuló la comprensión de la defensa biológica. Hasta entonces, el paradigma dominante asumía que los anticuerpos permanecían inactivos hasta el ingreso de un patógeno. Jerne postuló que el sistema inmunológico es un **inmenso retículo cognitivo interactivo**: las regiones variables de los anticuerpos (idiotopos) no solo reconocen antígenos invasores, sino que se reconocen y se unen mutuamente (a los paratopos de otros anticuerpos), formando un circuito cerrado de estimulación y supresión que sostiene homeostasis aún en ausencia total de infección.

Aplicado a los **Sistemas Inmunitarios Artificiales** (*Artificial Immune Systems*, AIS), este modelo da pie a una arquitectura defensiva que internaliza el "Fenotipo Digital" de la aplicación anfitriona. La primitiva `immune` no busca firmas de malware conocido; aprende y mapea la **homeostasis normal** del ecosistema AXON: volúmenes de peticiones a base de datos, vectores de invocación de herramientas, densidad semántica del flujo de la interfaz, latencias características. Cuando este equilibrio interconectado es perturbado por un ataque polimórfico de Día Cero — para el cual no existe firma previa — la red inmunitaria detecta la disrupción de su homeostasis y activa una respuesta.

Esta fundamentación biológica justifica la decisión arquitectónica clave: `immune` no compite con WAFs basados en firmas; opera en una capa ortogonal de detección semántica.

---

## 3. Fundamento Matemático: Inferencia Activa y Energía Libre Variacional

Para cuantificar la pérdida de homeostasis, `immune` recurre a la **Inferencia Activa** fundamentada en el **Principio de Energía Libre** (*Free Energy Principle*, FEP) de Karl Friston — la misma arquitectura topodinámica que sustenta el motor de perfilado psicológico `psyche` en AXON v0.18.

### 3.1 Formulación Variacional

El Principio de Energía Libre dicta que cualquier sistema autopoético que persista en el tiempo debe minimizar la "sorpresa" estadística impuesta por su entorno. Esta sorpresa es un límite superior tratable matemáticamente, denominado **Energía Libre Variacional** $F$:

$$F = \mathbb{E}_q[\ln q(s) - \ln p(o,s)]$$

Donde:
- $o$ representa las **observaciones** del entorno (tráfico HTTP entrante, latencia de base de datos, patrones de autenticación, secuencias de invocación de tools)
- $s$ representa los **estados latentes ocultos** que causan esas observaciones (operación normal vs asalto de exfiltración, sesión legítima vs sesión secuestrada)
- $q(s)$ es la distribución variacional aprendida por `AppVigilante` durante el período de baseline
- $p(o,s)$ es la distribución generativa conjunta del modelo

### 3.2 Divergencia KL como Métrica Operacional

En la implementación, `immune` integra un componente de fondo —`AppVigilante`— que computa continuamente la **divergencia de Kullback-Leibler** entre la distribución de actividad orgánica aprendida y el comportamiento entrante en tiempo real:

$$D_{KL}(q_{baseline} \parallel p_{observed}) = \sum_i q_i \ln \frac{q_i}{p_i}$$

Cuando $D_{KL}$ excede el umbral configurado por `sensitivity`, el sistema entra en estado de **Vigilancia de Estado Traumático**, determinando matemáticamente que la infraestructura está bajo asedio anómalo. La gradación de la respuesta no es binaria sino topológica: la magnitud de $F$ mapea directamente al nivel epistémico del `HealthReport` (ver §6).

### 3.3 Reutilización de Infraestructura `psyche`

Crítico para la viabilidad de implementación: el motor FEP de AXON ya existe. La primitiva `psyche` (v0.18) implementa el cálculo variacional para metacognición y self-awareness scoring. La primitiva `immune` reutiliza el mismo solver, parametrizado con observables de infraestructura en lugar de observables cognitivos. **No se requiere nueva matemática; se requiere nueva configuración del solver existente.**

---

## 4. Arquitectura: Las Tres Primitivas Composables

### 4.1 `immune` — El Detector de Anomalías

**Responsabilidad única:** monitorear continuamente las observables declaradas, computar la divergencia KL contra la baseline aprendida, y emitir un `HealthReport` tipado epistémicamente.

```axon
immune AppVigilante {
  watch:        [network_traffic, sql_queries, auth_logs, tool_invocations]
  sensitivity:  0.9
  baseline:     learned_homeostasis
  training:     7d                                    // período obligatorio de aprendizaje
  scope:        tenant                                // blast radius explícito
  epistemic:    {
    output_type:   HealthReport
    certainty:     believe                            // c ∈ [0.85, 0.99]
    tau:           300s                               // half-life de la observación
    decay:         exponential
  }
  output:       HealthReport
}
```

`immune` no toma acción. No bloquea. No parchea. Es un sensor formal cuyo único producto es un `HealthReport` que otras primitivas pueden consumir. Esta pureza permite que `immune` opere en cualquier modo (incluido `audit_only`) sin riesgo de efecto colateral.

### 4.2 `reflex` — La Respuesta Motora Determinista

**Responsabilidad única:** ejecutar acciones de mitigación con latencia O(1), sin invocar al LLM, sin pasar por el Sistema 2 cognitivo.

`reflex` está arquitectónicamente alineado con el **Fast-Path Enclave** de AXON, el mismo subsistema que ejecuta `compute` y `logic` (v0.22) — cero tokens, latencia de nanosegundos, ejecución determinista probada formalmente.

```axon
reflex TerminateConnection {
  trigger:      health_report.certainty == doubt      // epistemic gate
  target:       tcp_socket(request.ip)
  action:       drop
  effect:       capability_revoke
  audit:        signed_trace("connection_terminated")
  sla:          < 1ms                                 // contrato operacional
}

reflex AlertSOC {
  trigger:      health_report.kl_divergence > 0.95
  target:       siem_pipeline
  action:       emit
  payload:      {
    severity:   critical
    incident:   $health_report.anomaly_signature
    evidence:   $health_report.observation_window
  }
  audit:        signed_trace("soc_alert")
}
```

**Contrato de invariantes de `reflex`:**

- No invoca LLM
- No realiza I/O bloqueante de larga duración
- Toda activación produce una entrada de traza firmada con HMAC
- Idempotencia: invocaciones múltiples con el mismo `trigger` no duplican efecto
- Determinismo: dado el mismo `health_report`, produce el mismo efecto

### 4.3 `heal` — La Autosanación bajo Linear Logic

**Responsabilidad única:** sintetizar parches efímeros mediante OTS, aplicarlos bajo semántica de recurso lineal, y colapsarlos tras ventana de revisión humana o expiración.

Esta es la primitiva más radical y la más restrictiva. Por defecto, se despliega en `human_in_loop`. Solo en entornos no regulados (e.g., investigación) se permite `adversarial`.

```axon
heal ZeroDayPatcher {
  source:        AppVigilante
  trigger:       health_report.classification == zero_day
  
  pipeline: {
    biopsy:      probe(payload, depth: structural)    // primitivo probe existente
    synthesize:  ots {                                 // primitivo ots existente
      curry_howard: true
      proof_carrying: true
      target_morphism: vulnerability_closure
    }
    verify:      shield                                // shield re-valida el parche
    apply:       ephemeral
  }
  
  lifetime:      linear                               // Linear Logic: one-shot
  scope:         tenant
  mode:          human_in_loop                        // audit_only | human_in_loop | adversarial
  
  review: {
    sla:         24h
    reviewer:    role:security_architect
    on_timeout:  rollback
    on_reject:   rollback + axonstore.persist(engram)
    on_approve:  promote_to_permanent_via shield
  }
  
  collapse: {
    ontological: true
    persist_engram_to: axonstore("immune_memory")
    audit:       signed_trace("heal_collapsed")
  }
}
```

---

## 5. Tipología Epistémica del `HealthReport`

Toda primitiva en AXON participa del **lattice epistémico estándar** (`know`, `believe`, `speculate`, `doubt`). Definir el tipo del output de `immune` es requisito para integración con el resto del lenguaje.

### 5.1 Estructura del `HealthReport`

`HealthReport` es un envelope ΛD (Lambda Data, v0.22) con la siguiente forma:

$$\psi_{HR} = \langle T, V, E \rangle$$

Donde:
- $T$ = tipo estructural: `HealthReport`
- $V$ = vector de valores: `{kl_divergence, anomaly_signature, observation_window, classification}`
- $E$ = envelope epistémico: $\langle c, \tau, \rho, \delta \rangle$

### 5.2 Mapeo de Energía Libre a Nivel Epistémico

| Rango de $D_{KL}$ | Nivel Epistémico | Certeza $c$ | Acción Recomendada |
|---|---|---|---|
| $[0, 0.3)$ | `know` | $1.0$ | Operación normal, sin acción |
| $[0.3, 0.6)$ | `believe` | $[0.85, 0.99]$ | Logging incremental, sin reflex |
| $[0.6, 0.9)$ | `speculate` | $[0.50, 0.85)$ | Reflex de observación pasiva |
| $[0.9, 1.0]$ | `doubt` | $(0, 0.50)$ | Reflex de mitigación + heal eventual |

### 5.3 Decay Temporal Obligatorio

La memoria inmunológica biológica decae. La memoria inmunológica de AXON también. Cada `HealthReport` lleva un `tau` (half-life) configurable. Tras `tau`, la certeza se divide a la mitad; tras `5×tau`, el reporte se purga del estado activo.

Esto previene **lock-in defensivo** donde una anomalía transitoria mantiene a `immune` en estado de alerta permanente, degradando la operación legítima.

---

## 6. Linear Logic para Parches Efímeros

### 6.1 El Problema de la Aplicación Múltiple

Un parche sintetizado por OTS modifica el comportamiento del runtime. Aplicarlo dos veces puede:
- Corromper estado (e.g., doble-validación de un mismo input)
- Crear inconsistencia (e.g., dos validators concurrentes con políticas divergentes)
- Generar overhead computacional sin beneficio

La solución formal: tratar cada parche como un **recurso lineal** en el sentido de Girard's Linear Logic — exactamente la misma semántica que `axonstore` usa para tokens transaccionales (v0.30).

### 6.2 Tipo Lineal del Parche

Sea $P$ un parche sintetizado por OTS. Definimos su tipo:

$$P : !Synthesized \multimap Applied \multimap Collapsed$$

Donde $\multimap$ es la implicación lineal: cada estado consume su predecesor. La cadena **Synthesized → Applied → Collapsed** es estrictamente unidireccional y cada transición consume el token anterior.

### 6.3 Garantías Operacionales

| Propiedad | Garantía Linear Logic |
|---|---|
| Aplicación única | Token `Synthesized` consumido al entrar en `Applied` |
| Colapso garantizado | Token `Applied` debe transicionar a `Collapsed` |
| No revival post-colapso | Token `Collapsed` no produce sucesores |
| Auditoría completa | Cada transición emite signed_trace |

Esto convierte la frase poética "Colapso Ontológico" del diseño original en una **propiedad demostrable del sistema de tipos**.

---

## 7. Modos de Operación y Compliance Regulatorio

Para que AXON Enterprise sea adoptable en LegalTech, Pharma, Medicina y Fintech, las primitivas defensivas autónomas deben respetar restricciones regulatorias estrictas (GxP, HIPAA, SOC 2 Type II, GDPR Art. 22).

### 7.1 Modo `audit_only`

Configuración por defecto en industrias reguladas.

- `immune` opera y emite `HealthReport`
- `reflex` se ejecuta solo para emisión de alertas (no para mitigación)
- `heal` queda completamente deshabilitado
- Toda actividad se persiste en `axonstore("immune_audit")` para revisión forense

Cumple: **GxP, HIPAA, GDPR Art. 22 (no decisión automatizada)**.

### 7.2 Modo `human_in_loop`

Configuración por defecto en Enterprise general.

- `immune` opera plenamente
- `reflex` se ejecuta para mitigaciones reversibles (terminate connection, rate limit)
- `heal` sintetiza parches pero requiere **aprobación humana explícita** dentro del SLA
- Si no hay aprobación → rollback automático y persistencia del engram

Cumple: **SOC 2, ISO 27001, mandatos internos de change management**.

### 7.3 Modo `adversarial`

Configuración para entornos no regulados, hostiles, de alta velocidad (CTF, honeypots, sandboxes de investigación).

- `immune` con `sensitivity` máxima
- `reflex` ejecuta toda mitigación disponible
- `heal` aplica parches sintetizados de forma autónoma con ventana de revisión post-hoc

Solo permitido cuando la organización ha firmado un **Risk Acceptance Statement** explícito.

### 7.4 Matriz de Modos por Industria

| Industria | Modo Recomendado | Justificación |
|---|---|---|
| Pharma / Clinical Trials | `audit_only` | GxP exige cambios documentados con CAPA |
| Hospital / Telemedicina | `audit_only` | HIPAA + decisión clínica humana |
| Banca / Fintech | `human_in_loop` | SOC 2 + mandatos de change management |
| Legaltech | `human_in_loop` | Cadena de custodia documental |
| Gobierno / Defensa | `human_in_loop` o `adversarial` | Depende del clearance del entorno |
| SaaS B2B no regulado | `human_in_loop` | Default seguro |
| Investigación / CTF | `adversarial` | Velocidad sobre control |

---

## 8. Multi-Tenancy y Blast Radius

### 8.1 El Riesgo del Parche Cross-Tenant

En arquitecturas multi-tenant con RLS en PostgreSQL y AWS Secrets Manager particionado, una mitigación o parche aplicado al runtime sin scope explícito puede:
- Afectar tenants no comprometidos (false positive con impacto cross-tenant)
- Violar contratos de aislamiento (SLA por tenant)
- Crear vectores de ataque side-channel (un tenant adversario provoca anomalías para parchar el código de otro)

### 8.2 Declaración Obligatoria de `scope`

Toda primitiva del Sistema Inmunitario requiere un campo `scope` declarado:

```axon
scope: tenant      // afecta solo al tenant actual (default Enterprise)
scope: flow        // afecta solo al flow específico
scope: global      // afecta toda la instalación (requiere flag explícito)
scope: cohort:X    // afecta a un conjunto declarado de tenants
```

El compilador rechaza declaraciones de `immune`, `reflex` o `heal` sin `scope` explícito. No existe default implícito para `global`: debe ser una decisión consciente del arquitecto, registrada en CI.

### 8.3 Aislamiento por Tenant en `axonstore`

Los engramas inmunológicos persistidos por `heal.collapse` se almacenan en namespaces tenant-aislados:

```
axonstore://immune_memory/{tenant_id}/engrams/{incident_id}
```

Garantizado por las políticas RLS estándar de AXON Enterprise.

---

## 9. Observabilidad y Auditoría

### 9.1 Trazas Firmadas como Requisito No Negociable

Toda activación de `reflex` y toda transición de estado de `heal` produce una entrada en `/v1/traces` con:

```json
{
  "trace_id": "uuid-v4",
  "primitive": "reflex|heal|immune",
  "instance_name": "TerminateConnection",
  "tenant": "tenant_id",
  "timestamp_ns": 1234567890123456789,
  "trigger": {
    "kl_divergence": 0.96,
    "epistemic_level": "doubt",
    "observation_window": "[t-300s, t]"
  },
  "action": {
    "type": "drop_connection",
    "target": "192.0.2.1:443",
    "linear_token": "syn-uuid → app-uuid"
  },
  "hmac_sha256": "abc...",
  "signed_by": "axon-rs:immune-subsystem:v1.0.0"
}
```

### 9.2 Endpoint de Auditoría

Exposición vía API estándar de AXON:

```
GET  /v1/immune/incidents              → lista incidents activos
GET  /v1/immune/incidents/{id}         → detalle del incident
GET  /v1/immune/engrams                → memoria inmunológica persistida
POST /v1/heal/{id}/approve             → aprobación humana de patch (require role)
POST /v1/heal/{id}/reject              → rechazo + rollback
GET  /v1/immune/baseline               → distribución q aprendida
PUT  /v1/immune/baseline/retrain       → re-entrenamiento manual (audit-required)
```

### 9.3 Mapeo a Frameworks de Compliance

| Framework | Requisito | Implementación AXON |
|---|---|---|
| SOC 2 Type II — CC7.2 | Logging de eventos de seguridad | signed_trace por reflex |
| HIPAA — §164.312(b) | Audit controls | trazas firmadas + RLS engrams |
| GDPR Art. 22 | No decisión automatizada | mode: audit_only obligatorio |
| GxP / 21 CFR Part 11 | Electronic records integrity | HMAC + WORM en axonstore |
| ISO 27001 — A.12.4 | Logging y monitoreo | endpoint /v1/immune/incidents |
| PCI DSS Req. 10 | Track all access | trace por session terminate |

---

## 10. Integración con Primitivos Existentes

La fortaleza arquitectónica del diseño descompuesto es que **ninguna de las tres primitivas reinventa funcionalidad existente**. Cada una se ancla en infraestructura ya probada (1,466 tests pasando en v1.0.0):

| Necesidad | Primitivo Reutilizado | Versión |
|---|---|---|
| Monitoreo continuo en background | `daemon` + `listen` (π-Calculus, OTP) | v0.27.5 |
| Memoria inmunológica de largo plazo | `axonstore` con esquema HoTT | v0.30.6 |
| Cálculo de Energía Libre Variacional | Solver FEP de `psyche` | v0.18 |
| Fast-path determinista para `reflex` | `compute` / `logic` | v0.22 |
| Síntesis de herramientas tipadas | `ots` con Curry-Howard | v0.18 |
| Validación del parche post-síntesis | `shield` con taint analysis | v0.13 |
| Convergencia controlada de loops de healing | `mandate` con Lyapunov | v0.24.1 |
| Trazabilidad firmada de eventos | `/v1/traces` + middleware HMAC | v1.0.0 |
| Análisis de payload sospechoso | `probe` | v0.7 |
| Capability enforcement del parche aplicado | `shield.allow_tools` | v0.13 |

**Consecuencia operacional:** la implementación neta de las tres primitivas se reduce a un orquestador delgado que compone primitivos existentes. La superficie de código nuevo es del orden de 2,000–3,000 líneas en `axon-rs`, no de un módulo de seguridad completo.

---

## 11. Sintaxis Formal y Ejemplo Integrado

### 11.1 Gramática EBNF

```ebnf
ImmuneDecl  ::= "immune" Identifier "{" ImmuneBody "}"
ImmuneBody  ::= WatchField SensitivityField BaselineField
                ScopeField EpistemicField OutputField

ReflexDecl  ::= "reflex" Identifier "{" ReflexBody "}"
ReflexBody  ::= TriggerField TargetField ActionField
                EffectField AuditField SlaField

HealDecl    ::= "heal" Identifier "{" HealBody "}"
HealBody    ::= SourceField TriggerField PipelineBlock
                LifetimeField ScopeField ModeField
                ReviewBlock CollapseBlock

PipelineBlock ::= "pipeline" ":" "{" 
                  "biopsy" ":" ProbeExpr
                  "synthesize" ":" OtsExpr
                  "verify" ":" ShieldRef
                  "apply" ":" ApplyMode
                  "}"

ReviewBlock ::= "review" ":" "{" SlaField ReviewerField
                "on_timeout" ":" Action
                "on_reject" ":" Action
                "on_approve" ":" Action "}"

CollapseBlock ::= "collapse" ":" "{" 
                  "ontological" ":" Bool
                  "persist_engram_to" ":" AxonstoreRef
                  "audit" ":" SignedTraceCall "}"
```

### 11.2 Despliegue Completo de un Sistema Inmunitario

```axon
// ── 1. SENSOR: detección continua via daemon ───────────────────────
daemon AppImmuneDaemon {
  strategy:    reactive
  supervisor:  one_for_one
  listen:      [http_inbound, sql_layer, auth_events, tool_dispatch]
  
  on_tick: {
    immune AppVigilante {
      watch:        [network_traffic, sql_queries, auth_logs]
      sensitivity:  0.9
      baseline:     learned_homeostasis
      training:     7d
      scope:        tenant
      epistemic: {
        certainty:  believe
        tau:        300s
        decay:      exponential
      }
      output:       HealthReport
    }
  }
}

// ── 2. RESPUESTA O(1): reflex deterministas ────────────────────────
reflex TerminateConnection {
  trigger:  health_report.certainty == doubt
  target:   tcp_socket(request.ip)
  action:   drop
  effect:   capability_revoke
  audit:    signed_trace("connection_terminated")
  sla:      < 1ms
}

reflex RateLimit {
  trigger:  health_report.certainty == speculate
  target:   tenant_quota(request.tenant)
  action:   throttle(rate: 10/s)
  audit:    signed_trace("rate_limited")
  sla:      < 1ms
}

reflex AlertSOC {
  trigger:  health_report.kl_divergence > 0.95
  target:   siem_pipeline
  action:   emit
  payload: {
    severity:  critical
    incident:  $health_report.anomaly_signature
    evidence:  $health_report.observation_window
  }
  audit:    signed_trace("soc_alert")
}

// ── 3. AUTOSANACIÓN: heal bajo Linear Logic ───────────────────────
heal ZeroDayPatcher {
  source:    AppVigilante
  trigger:   health_report.classification == zero_day
  
  pipeline: {
    biopsy:      probe(request.payload, depth: structural)
    synthesize:  ots {
      curry_howard:    true
      proof_carrying:  true
      target_morphism: vulnerability_closure
    }
    verify:      shield(strict: true)
    apply:       ephemeral
  }
  
  lifetime:  linear
  scope:     tenant
  mode:      human_in_loop
  
  review: {
    sla:         24h
    reviewer:    role:security_architect
    on_timeout:  rollback
    on_reject:   rollback + axonstore.persist(engram)
    on_approve:  promote_to_permanent_via shield
  }
  
  collapse: {
    ontological:        true
    persist_engram_to:  axonstore("immune_memory")
    audit:              signed_trace("heal_collapsed")
  }
}
```

### 11.3 Ejemplo Mínimo (Modo Audit-Only, Pharma)

Para una organización farmacéutica que solo puede operar en `audit_only`:

```axon
immune ClinicalTrialVigilante {
  watch:        [database_writes, audit_log_modifications]
  sensitivity:  0.95
  baseline:     learned_homeostasis
  scope:        tenant
  output:       HealthReport
}

reflex AuditAlert {
  trigger:  health_report.certainty in [doubt, speculate]
  target:   compliance_team_pager
  action:   notify
  audit:    signed_trace("compliance_anomaly")
}

// Sin heal — modo audit_only, decisión 100% humana
```

---

## 12. Diferencias Formales con `shield`

| Dimensión | `shield` (Gatekeeper Estático) | Sistema Inmunitario (`immune` + `reflex` + `heal`) |
|---|---|---|
| **Foco de defensa** | Integridad del LLM frente a inyecciones, jailbreaks, exfiltración de PII | Integridad de la aplicación anfitriona, red, sesiones, código |
| **Modelo de amenaza** | OWASP LLM Top 10 (LLM01-LLM10) | OWASP Web/API Top 10 + amenazas de Día Cero |
| **Mecánica matemática** | Análisis de Taint, IFC, Retículo de Denning | Inferencia Activa (FEP), divergencia KL, redes idiotípicas |
| **Capacidad de intervención** | Restrictiva, ex-ante, epistémica: redactar, sanitizar, abortar prompt | Cinética, reactiva, autosanadora: bloquear, rate-limit, parchear |
| **Latencia** | Tiempo de compilación + inspección de prompt | Continuo + reflex O(1) sub-milisegundo |
| **Estado** | Stateless por request | Stateful: baseline aprendida + memoria inmunológica |
| **Output epistémico** | Veredict booleano + tainted regions | `HealthReport` con envelope ΛD completo |
| **Modos regulatorios** | Único (siempre activo) | Tres modos (audit_only, human_in_loop, adversarial) |
| **Analogía biológica** | Piel, mucosa, sistema respiratorio (filtro epitelial) | Sistema inmune adaptativo + arco reflejo neuromotor |
| **Composición** | Bloque ortogonal único | Tres primitivas composables independientemente |

**Insight clave:** `shield` y el Sistema Inmunitario no se solapan — operan en planos defensivos ortogonales. Una arquitectura AXON Enterprise madura despliega ambos simultáneamente: `shield` como filtro epitelial del LLM, y `immune`/`reflex`/`heal` como inmunidad adaptativa de la infraestructura.

---

## 13. Modelo de Amenaza Cubierto

| Amenaza | Cubierta por | Mecanismo |
|---|---|---|
| SQL Injection (post-LLM, en tool dispatch) | `immune` + `reflex RateLimit` | KL divergence en `sql_queries` |
| Session hijacking | `immune` + `reflex TerminateConnection` | Anomaly en `auth_logs` |
| Tool abuse (invocación masiva) | `immune` + `reflex` | Anomaly en `tool_dispatch` |
| Exfiltración de datos por canal lateral | `immune` + `reflex AlertSOC` | KL divergence en `network_traffic` |
| Vulnerabilidad de Día Cero en binario | `heal ZeroDayPatcher` | OTS + verify(shield) + linear apply |
| Ataque de cadena de suministro en MCP | `immune` (mode adversarial) | Detección de behavioral drift |
| Cross-tenant attack | `scope: tenant` + RLS | Aislamiento declarativo |
| Insider threat | `immune` + `reflex AlertSOC` | Anomaly en `database_writes` |
| Modelo comprometido (LLM con backdoor) | **Fuera de alcance** — requiere `shield` | — |
| Prompt injection | **Fuera de alcance** — requiere `shield` | — |

---

## 14. Roadmap de Implementación

### Phase L.1 — `immune` Core (8 semanas)

- Lexer/Parser para `ImmuneDecl`
- AST node + IR generation
- Type checker integration con epistemic lattice
- Daemon hook para `on_tick`
- Solver FEP parametrizado (reuso de `psyche`)
- Endpoint `POST /v1/immune/baseline/train`
- 200+ tests

### Phase L.2 — `reflex` Fast-Path (4 semanas)

- Lexer/Parser para `ReflexDecl`
- Integration con compute/logic fast-path enclave
- Signed trace middleware
- Idempotency tokens
- 100+ tests
- Benchmark: latencia p99 < 1ms verificada

### Phase L.3 — `heal` con Linear Logic (10 semanas)

- Lexer/Parser para `HealDecl`
- Integration con `ots` + `probe` + `shield`
- Linear token state machine
- Review workflow + role-based approval
- Engram persistence schema en `axonstore`
- Endpoint `POST /v1/heal/{id}/approve`
- 250+ tests
- 3 modos (audit_only, human_in_loop, adversarial) con kill-switch en config

### Phase L.4 — Compliance Hardening (6 semanas)

- Mapeo formal a SOC 2, HIPAA, GxP
- Documentación de Risk Acceptance Statement para modo `adversarial`
- Penetration testing por red team independiente
- Audit log immutability (WORM en axonstore)
- Certificación externa (Compliance Pack auditado por terceros)

**Total estimado:** 28 semanas para v1.1.0 con Sistema Inmunitario completo.

---

## 15. Conclusiones

El Sistema Inmunitario Cognitivo de AXON, descompuesto en `immune`, `reflex` y `heal`, satisface tres requisitos simultáneos que ninguna herramienta de ciberseguridad existente cubre:

1. **Detección sin firmas** mediante Inferencia Activa, capaz de identificar Día Cero por divergencia de homeostasis
2. **Respuesta de latencia muscular** (sub-milisegundo) sin invocación de LLM
3. **Autosanación con garantías formales** mediante OTS + Linear Logic + verificación por `shield`

La descomposición en tres primitivas composables —en lugar de un monolito `immune`— preserva la composicionalidad de AXON, permite adopción gradual, y satisface restricciones regulatorias estrictas mediante modos operativos auditables.

La fundamentación matemática reutiliza infraestructura existente (`psyche` para FEP, `axonstore` para Linear Logic, `ots` para síntesis, `shield` para verificación, `daemon` para monitoreo continuo, `compute`/`logic` para fast-path). La superficie de implementación neta es delgada; el valor cognitivo es transformador.

Con esta extensión, AXON consolida su posición no solo como el primer lenguaje de programación cognitiva tipado, sino como el primer **lenguaje cognitivo con sistema inmunitario formal** — una propiedad sin precedentes en el estado del arte de la ingeniería de software defensivo.

---

> **Phase L Target:** AXON v1.1.0 — Sistema Inmunitario Cognitivo
> **Status:** Concept Proposal v3.0 — Ready for Architectural Review
> **Authored by:** AXON Language Team
> **Predecessors:** v1.0 (Concept Draft), v2.0 (Mathematical Foundations)
