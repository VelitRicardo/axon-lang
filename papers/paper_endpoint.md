# El Sistema Nervioso Periférico Computacional
## Formalización de Primitivas Nativas de E/S Continua y Red Autónoma en el Lenguaje Cognitivo AXON

> [!ABSTRACT]
> El estado del arte en la orquestación de Inteligencia Artificial padece de un profundo "desajuste de impedancia semántica": separa la capa de transporte de datos (servidores web imperativos) del núcleo de inferencia cognitiva (librerías estocásticas). AXON-LANG evoluciona de un paradigma de "oráculo estático" a un **Sistema Nervioso Periférico (SNP)** mediante el primitivo `axonendpoint`. Al integrar la interfaz HTTP directamente en el AST y la IR, AXON consolida un **Manto de Markov** (*Markov Blanket*) computacional que dota a los agentes de percepción continua, contrapresión cognitiva (*backpressure*) y validación semántica nativa, transformándolo en el primer **Sistema Operativo Cognitivo Autonómico**.

---

### 1. Fundamentación Filosófica y Epistemológica: El Enactivismo y la Manta de Markov

En la ciencia cognitiva contemporánea, el paradigma computacionalista clásico ha sido superado por el **Enactivismo** y la **Cognición Corporeizada** (*Embodied Cognition*). Una inteligencia genuina no razona en el vacío; la cognición emerge del acoplamiento estructural continuo entre el agente y su entorno.

Actualmente, los módulos de AXON (motor **PEM** y memoria **MDN**) actúan como un Sistema Nervioso Central (SNC) aislado: despiertan y mueren. Al introducir `axonendpoint`, otorgamos a AXON una superficie sensorial asíncrona y permanente.

Bajo el **Principio de Energía Libre** de Karl Friston, un sistema adaptativo se define por una **Manta de Markov** (*Markov Blanket*). Este límite estadístico aísla los estados epistémicos internos de las fluctuaciones ambientales. El `axonendpoint` es el **Estado Sensorial** nativo de esta Manta: transduce el caos del ciberespacio en **Tipos Epistémicos** estructurados, cerrando el ciclo percepción-acción para mantener la autopoiesis del agente.

---

### 2. Formalidad Lógica y Matemática: Tipos Dependientes y Cálculo $\pi$

Formalizamos el enrutamiento mediante **Coálgebras** para flujos continuos y **$\pi$-cálculo** para concurrencia. Sea $\mathcal{H}$ la categoría de eventos de red (payloads HTTP) y $\mathcal{E}$ la categoría de Tipos Epistémicos de AXON.

Un flujo cognitivo estático es un morfismo $\Phi : A \to B$. Expandimos el modelo a una semántica reactiva definiendo el endpoint $E$ en una ruta $r$:

$$ E(r) \triangleq request(x \in \mathcal{H}) . \left( F_{in}(x) \xrightarrow{\tau} Flow\langle x_{t}, c \rangle \ \Big| \ c(y_{t}) . F_{out}(y_{t}) \xrightarrow{\text{HTTP 200}} E(r) \right) $$

Donde $F_{in}$ y $F_{out}$ son funtores de transducción en la frontera epistémica. Bajo la correspondencia de **Curry-Howard**, la sintaxis propuesta actúa como una prueba lógica constructiva:

```axon
body_type: Document
execute: AnalyzeDocument(body_type)
output: StructuredReport
```

El compilador exige la demostración formal **AOT** (*Ahead of Time*):

$$ \frac{\Gamma \vdash \text{body\_type} : \text{Document} \quad \Gamma \vdash \text{AnalyzeDocument} : \text{Document} \to \text{StructuredReport}}{\Gamma \vdash \text{AnalyzeDocument}(\text{body\_type}) : \text{StructuredReport}} $$

Si $F_{in}$ falla semánticamente, la Manta de Markov rechaza el estímulo en la capa de red (**HTTP 406**), sin consumir tokens del LLM ni despertar los motores de inferencia.

---

### 3. Arquitectura Computacional: Mutación del Pipeline de AXON

La implementación requiere modificaciones quirúrgicas en cinco capas:

#### A. Lexer y Parser (`axon/compiler/`)
Expansión de la gramática **EBNF** para reconocer la infraestructura como cognición (**CaC**):

```ebnf
EndpointDecl ::= "axonendpoint" HttpMethod StringLiteral "{" EndpointBody "}"
EndpointBody ::= TriggerDef AuthDef BodyDef ExecuteDef OutputDef
```

#### B. Árbol de Sintaxis Abstracta (AST)
Se introduce `AxonEndpointNode` como un nodo raíz paralelo a `FlowNode`. Almacena la topología de red y referencias cruzadas al flujo cognitivo.

#### C. Type Checker
Evaluación Estática Adelantada. Si el desarrollador promete un `StructuredReport` pero el flujo retorna un `RawString`, la compilación se aborta antes de abrir el puerto TCP, garantizando la ausencia de errores 500.

#### D. Generador de IR
Abatimiento (*lowering*) del AST a la instrucción reactiva `IR_ServerEndpoint`, indicando al motor instanciar un bucle de eventos asíncrono permanente.

#### E. El "Sensory Runtime"
Se habilita el subcomando `axon serve <file.axon>`. Bajo el capó (`axon/runtime/routers/`), se levanta un servidor **ASGI** embebido de ultra-alto rendimiento:
1. **Interceptación**: `data_dispatcher.py` recibe el socket TCP.
2. **Validación**: El payload pasa por `semantic_validator.py`.
3. **Ejecución**: El executor instancia el flujo lógico.
4. **Respuesta**: El Tipo Epistémico se serializa a JSON de vuelta al cliente.

---

### 4. Innovación Radical: La Destrucción del Desajuste de Impedancia

Al elevar `axonendpoint` a primitivo nativo, AXON adquiere tres fosos defensivos (*Moats*) tecnológicos:

#### I. Backpressure Cognitivo (*Cognitive Backpressure*)
Los servidores HTTP tradicionales encolan peticiones según CPU/RAM. En AXON, el endpoint está acoplado al `psyche_engine.py`. Si el agente detecta alta entropía o agotamiento de tokens, aplica contrapresión semántica, ralentizando la ingesta en el nivel TCP o devolviendo respuestas de degradación elegante.

#### II. Escudo Anti-Inyección en Capa de Transporte (*Semantic Edge Shielding*)
La directiva `auth` y `body_type` invoca el módulo `primitiva_shield_axon` en el límite exacto de la red. Si un payload contiene anomalías o ataques de inyección, es rechazado en microsegundos sin que el LLM sea invocado.

#### III. Auto-Curación de Red Oculta (*Transport-Layer Self-Healing*)
Si el LLM alucina y rompe la estructura del JSON prometido, AXON no devuelve un HTTP 500. El endpoint está soldado al `retry_engine.py`, que detecta la violación del contrato e itera silenciosamente para corregir el razonamiento internamente antes de emitir un **HTTP 200**.

---

### 5. Telemetría Holográfica End-to-End (`axon/runtime/tracer.py`)

La traza generada (`sample.trace.json`) captura la cadena de causalidad absoluta: desde la cabecera HTTP original, el casting del tipo semántico, el razonamiento interno y las tools invocadas, hasta la respuesta final. Representa la observabilidad algorítmica perfecta del ciclo **Estímulo $\to$ Respuesta**.
