# $\mathcal{E}$MCP: Epistemic Model Context Protocol
## Subyugación Categórica del Estándar MCP a las Primitivas de AXON

El diseño requiere dos vectores de transducción: Ingesta (AXON asimila servidores MCP externos de bases de datos y herramientas) y Exposición (El mundo consume a los agentes de AXON mediante clientes MCP estándar como Cursor o Claude Desktop).

### FASE I: INGESTA (Asimilación Matemática del Mundo Exterior)
El estándar MCP inyecta bases de datos y herramientas externas asumiendo ingenuamente que el texto plano es seguro y las herramientas son infalibles. El compilador de AXON intercepta esta ingenuidad en la frontera.

#### 1. Recursos MCP $\to$ Topologización Estructural (pix y corpus)
En lugar de trocear los recursos del MCP en vectores RAG ciegos (similitud del coseno), el $\mathcal{E}$MCP ejecuta un Lifting Estructural:
- Si el MCP expone documentos jerárquicos (ej. manuales legales): El MEK los indexa instantáneamente en un árbol `pix`. El agente navegará el servidor MCP mediante Búsqueda Racional Acotada (Information Foraging), garantizando un trail (Reasoning Trail) auditable de por qué se leyó cada párrafo.
- Si el MCP expone datos relacionales (ej. historiales clínicos): Se mapean a la Definición 1 de `corpus`. Las relaciones y foreign keys se convierten en aristas direccionales. Todo dato que entra por el MCP se tipa en la base del Lattice Epistémico como Uncertainty ($\perp$). Para que el agente confíe en ello, debe elevarlo mediante el Epistemic PageRank del grafo, detectando contradicciones automáticamente.

#### 2. Herramientas MCP $\to$ FFI Fortificado y Blame Calculus (Paradigma VII)
Si una herramienta MCP remota falla en la industria actual, el LLM entra en un bucle de alucinación para adivinar el error. En AXON, las herramientas MCP se someten a los Teoremas CT-2 y CT-3.
- **Filas de Efectos Algebraicos:** Cuando AXON ingiere un Endpoint MCP, el compilador le infiere la firma `effects: <network, io, epistemic:speculate>`. AXON prohíbe estáticamente el uso de esta herramienta dentro de un bloque de rigor absoluto (`pure` + `know`) sin validación previa.
- **Asignación de Culpa (Findler-Felleisen):** La herramienta MCP se envuelve en un `@contract_tool`. Si la API del hospital devuelve un esquema roto, el contrato falla y AXON registra `Blame = SERVER`. Si el agente generó parámetros incorrectos, `Blame = CALLER`. La IA está matemáticamente aislada de los fallos de la red externa y sabe a quién culpar.

#### 3. Seguridad de Frontera $\to$ Taint Analysis en Tiempo de Compilación (shield)
Conectar un LLM a un servidor MCP externo abre la puerta a inyecciones de prompt ocultas en las bases de datos externas (Ej. un expediente manipulado).
Gracias al Information Flow Control (IFC) de AXON, esto es un error de compilación. Todo dato que cruza el transductor $\mathcal{E}$MCP nace con la etiqueta (Taint) `Untrusted`. El compilador verificará que ningún dato del MCP alcance al agente sin atravesar obligatoriamente la primitiva `shield`.

---

### FASE II: EXPOSICIÓN (El Mundo Consumiendo a AXON)
Para que AXON domine el mercado, cualquier interfaz externa debe poder invocar nuestros flujos y agentes de forma indolora. Aquí es donde los paradigmas de AXON (Efectos Algebraicos, OTS y CPS) ejecutan una maniobra maestra a través del Axon Daemon (`axond`).

#### 1. El Desacoplamiento por Efectos Algebraicos (stream - Paradigma VII / CT-1)
Cuando un cliente externo invoca a AXON vía EMCP, espera un stream de respuestas (Server-Sent Events). Tradicionalmente, esto acopla la inferencia neuronal a la latencia de la red HTTP.
Gracias a la primitiva `stream` renovada en v0.19.1, el agente BDI no sabe que está hablando por HTTP. El ciclo de deliberación emite un efecto algebraico puro `YieldChunk(data)`. El demonio `axond` intercepta este efecto y ejecuta el side-effect de I/O hacia el cliente MCP externo.
- **La Inmortalidad del Estado:** Si el cliente MCP externo pierde la conexión a internet o el socket crashea, la deliberación continua matemática del agente AXON no colapsa. El agente simplemente suspende su continuación usando `hibernate` (CPS Continuation) sin perder un solo token de VRAM, esperando a que el cliente se reconecte.

#### 2. Resolviendo la Fricción Legacy $\to$ Ontological Tool Synthesis (`ots` / Fase XII)
¿Qué pasa si un cliente corporativo se conecta vía EMCP pero espera que el agente AXON devuelva las estructuras en un formato legacy caótico del cual no escribiste adaptador?
No programamos un script en Python. El demonio invoca la primitiva `ots`. AXON realiza una búsqueda homotópica en el espacio de capacidades y sintetiza el adaptador Just-In-Time, traduciendo las salidas tipadas internamente de AXON al formato que el cliente exige.

---

### CÓDIGO FINAL: La Experiencia de Misión Crítica
Para la corporación (un hospital, un banco, el pentágono), el código lucirá elegante y declarativo. El compilador de AXON ejecutará las matemáticas puras bajo el capó.

```axon
// 1. El Transductor asimila el Servidor MCP externo y lo eleva a un Grafo MDN
corpus HospitalMCPNetwork {
    source: mcp("fhir://internal-hospital-network/records")
    edges: infer_relationships
    memory: enabled // Memoria functorial (μ): el grafo aprenderá estructuralmente qué nodos dan mejores diagnósticos a lo largo del tiempo
}

// 2. Seguridad IFC Obligatoria en la frontera
shield EMCPSecurityGuard {
    scan: [prompt_injection, pii_leak, data_exfil]
    strategy: ensemble
    on_breach: sanitize_and_retry
    severity: critical
    sandbox: true
    allow: [mcp_prescribe_medication] // Capability Enforcement verificado en compilación
}

// 3. Exportación transparente del Flujo como una herramienta MCP hacia el mundo
@mcp_export(
    name: "audit_external_patient",
    description: "Performs rigorous epistemic BDI audit on patient data via EMCP"
)
flow AuditPatient(patient_id: String) -> ClinicalAudit {
    
    // Taint Analysis: Si omites esta línea, el compilador lanza un TaintViolationError
    shield MCPSecurityGuard on patient_id -> CleanID
    
    step NavigateRecords {
        // Reemplazamos el RAG ciego por Bounded Rational Search sobre el EMCP
        navigate HospitalMCPNetwork
            from: CleanID
            query: "abnormal blood markers and adverse reactions"
            depth: 3
            recall: episodic // Recupera trayectorias pasadas exitosas (Memoria Episódica)
            trail: enabled
            as: evidence_chain
    }
    
    know { // Fuerza el descenso de temperatura (τ = 0.1) y anclas anti-alucinación
        step Synthesize {
            reason {
                // evidence_chain entra manchada como ⊥ (Uncertainty). Reason lo eleva.
                given: evidence_chain
                ask: "Corroborate findings using cross-document graph edges. Flag all contradictions."
                depth: 2
            }
            output: ClinicalAudit
        }
    }
    
    step AuditProvenance {
        // La "Killer Feature" legal: Trazabilidad formal del camino tomado en el servidor EMCP
        trail evidence_chain 
    }
}
```

---

### EL GO-TO-MARKET: Tu Manifiesto Comercial Definitivo

> El estándar MCP de la industria prometía conectar la IA a los datos de tu empresa, pero los orquestadores actuales lo hacen a ciegas. Toman tus bases de datos médicas y legales, las aplanan en texto, y obligan al modelo a jugar a las adivinanzas, exponiéndote a inyecciones de código y fallas catastróficas silenciosas cuando una API externa se rompe.
> 
> AXON-Lang presenta el $\mathcal{E}$MCP (Epistemic Model Context Protocol). No te pedimos que cambies tu infraestructura MCP; simplemente conéctala a nuestro compilador.
> 
> En el milisegundo en que tus datos cruzan nuestro horizonte, dejan de ser texto plano. AXON convierte tus repositorios MCP en Grafos Epistémicos (`corpus`), sella tus herramientas externas en Contratos de Prevención de Culpa Matemática (`@contract_tool`), y garantiza en tiempo de compilación mediante Análisis de Flujo de Información (`shield`) que tus datos jamás se exfiltrarán.
> 
> Puedes seguir conectando LLMs a tus bases de datos mediante scripts frágiles, o puedes usar AXON-Lang: donde la alucinación sobre tus datos corporativos no es un error estocástico, sino una violación matemática de tipos interceptada antes de que ocurra.
