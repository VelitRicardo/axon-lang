# Optimización Matemática de Prompt Engineering

> **Investigación formal sobre los fundamentos matemáticos que permiten tratar la ingeniería
> de prompts como un problema de optimización con garantías teóricas.**

---

## 1. Formalización del Problema

### 1.1 El Espacio de Prompts como Espacio Métrico

Sea $\mathcal{P}$ el espacio de todos los prompts posibles (secuencias de tokens de un
vocabulario $V$). Cada prompt $p \in \mathcal{P}$ es una secuencia finita $p = (t_1, t_2, \dots, t_n)$
donde $t_i \in V$ y $|V| \approx 32\,000 - 128\,000$ según el tokenizador.

Definimos una **función de calidad** que mapea cada prompt a un score de desempeño
contra una tarea objetivo:

$$Q : \mathcal{P} \times \mathcal{T} \to [0, 1]$$

donde $\mathcal{T}$ es el espacio de tareas. El objetivo de la optimización es:

$$p^* = \arg\max_{p \in \mathcal{P}} \; Q(p, \tau) \quad \text{sujeto a} \; |p| \leq L_{\max}$$

Este problema es **NP-hard** en el caso general porque:
- $|\mathcal{P}|$ es exponencialmente grande: $|V|^{L_{\max}}$
- $Q$ es una **función de caja negra** no-diferenciable
- La evaluación de $Q$ requiere una llamada costosa al LLM (latencia + costo monetario)

### 1.2 Taxonomía de Enfoques Matemáticos

| Marco Teórico | Pregunta que responde | Aplicación a Prompts |
|---|---|---|
| **Teoría de la Información** | ¿Cuánta información transmite el prompt al modelo? | Minimizar entropía condicional del output |
| **Optimización Bayesiana** | ¿Cómo buscar eficientemente en un espacio vasto? | Modelos subrogados + funciones de adquisición |
| **Complejidad de Kolmogorov** | ¿Cuál es la descripción más compacta de la tarea? | Principio de mínima descripción para prompts |
| **Retículos (Lattice Theory)** | ¿Cómo ordenar y componer restricciones? | Cálculo de restricciones epistémicas |
| **Teoría de Control** | ¿Cómo ajustar dinámicamente el prompt? | Feedback loops prompt → output → refinamiento |
| **Satisfacción de Restricciones** | ¿Cómo garantizar propiedades del output? | Verificación formal de constraints |

---

## 2. Marco Información-Teórico

### 2.1 Canal de Shannon Prompt → LLM

Un LLM puede modelarse como un **canal de comunicación ruidoso** en el sentido de Shannon.
El prompt $p$ es el mensaje de entrada, y la respuesta $r$ es la salida del canal:

$$I(P; R) = H(R) - H(R|P)$$

donde:
- $H(R)$ — entropía (incertidumbre) de la respuesta sin prompt
- $H(R|P)$ — entropía condicional de la respuesta dado el prompt
- $I(P; R)$ — información mutua: cuánta incertidumbre reduce el prompt

**Principio 1 — Maximización de Información Mutua:**
Un prompt óptimo maximiza $I(P; R)$, es decir, reduce al máximo la incertidumbre de la
respuesta. Un prompt vago tiene alta $H(R|P)$ (muchas respuestas posibles); un prompt
preciso la minimiza.

### 2.2 Entropía como Métrica de Calidad de Prompt

Para un LLM autoregresivo, la distribución predictiva sobre el siguiente token
$t_{i+1}$ dado el contexto previo es:

$$P(t_{i+1} | t_1, \dots, t_i) = \text{softmax}\left(\frac{z_{i+1}}{\tau}\right)$$

La **entropía del token** mide la incertidumbre del modelo:

$$H(t_{i+1}) = -\sum_{v \in V} P(v | t_1, \dots, t_i) \log P(v | t_1, \dots, t_i)$$

**Interpretación práctica:**
- $H \to 0$ → El modelo tiene alta confianza (determinístico)
- $H \to \log|V|$ → Incertidumbre máxima (uniformemente aleatorio)

Un prompt bien diseñado produce **baja entropía en los tokens relevantes** del output
y **alta entropía controlada** donde se busca creatividad (modo `speculate` en AXON).

### 2.3 Divergencia KL como Función de Pérdida

La distancia entre la distribución de respuestas que queremos ($R^*$, la distribución
objetivo) y la que obtenemos ($R_p$, dado el prompt $p$) se mide con la
**divergencia de Kullback-Leibler**:

$$D_{KL}(R^* \| R_p) = \sum_{r} R^*(r) \log \frac{R^*(r)}{R_p(r)}$$

La optimización de prompts busca minimizar esta divergencia:

$$p^* = \arg\min_{p \in \mathcal{P}} \; D_{KL}(R^* \| R_p)$$

Esto conecta directamente con la **temperatura** ($\tau$) del LLM:
- $\tau \to 0$: Concentra la distribución → baja divergencia para tareas factuales
- $\tau \to \infty$: Aplana la distribución → explorativa, alta diversidad

### 2.4 Principio de Mínima Entropía Cruzada

La **entropía cruzada** entre la distribución objetivo y la del modelo:

$$H(R^*, R_p) = -\sum_{r} R^*(r) \log R_p(r) = H(R^*) + D_{KL}(R^* \| R_p)$$

Como $H(R^*)$ es constante, minimizar la entropía cruzada equivale a minimizar la
divergencia KL. El prompt óptimo es aquel que alinea la distribución condicional del LLM
lo más posible con la distribución deseada de respuestas.

---

## 3. Optimización Bayesiana de Prompts

### 3.1 Framework Formal

La Optimización Bayesiana (BO) trata la función de calidad $Q(p)$ como un proceso
estocástico y construye un **modelo subrogado** que aproxima $Q$ a partir de evaluaciones
previas:

$$Q(p) \sim \mathcal{GP}(m(p), k(p, p'))$$

donde:
- $\mathcal{GP}$ es un Proceso Gaussiano
- $m(p)$ es la función media (prior sobre la calidad)
- $k(p, p')$ es la función kernel (covarianza entre prompts)

### 3.2 Función de Adquisición

En cada iteración $t$, la BO selecciona el siguiente prompt a evaluar maximizando una
**función de adquisición** $\alpha(p)$:

$$p_{t+1} = \arg\max_{p \in \mathcal{P}} \; \alpha(p | \mathcal{D}_t)$$

donde $\mathcal{D}_t = \{(p_i, Q(p_i))\}_{i=1}^t$ es el historial de evaluaciones.

**Upper Confidence Bound (UCB):**
$$\alpha_{UCB}(p) = \mu(p) + \kappa \cdot \sigma(p)$$

- $\mu(p)$ — predicción media del GP (explotación)
- $\sigma(p)$ — desviación estándar del GP (exploración)
- $\kappa > 0$ — balance exploración-explotación

**Expected Improvement (EI):**
$$\alpha_{EI}(p) = \mathbb{E}\left[\max(Q(p) - Q^+, 0)\right]$$

donde $Q^+ = \max_{p_i \in \mathcal{D}_t} Q(p_i)$ es la mejor calidad observada.

### 3.3 Kernel Semántico para Prompts

El desafío particular de aplicar BO a prompts es definir $k(p, p')$ sobre secuencias
de texto. Propuesta de kernel semántico:

$$k_{semantic}(p, p') = \exp\left(-\frac{\|e(p) - e(p')\|^2}{2\ell^2}\right)$$

donde $e(p) \in \mathbb{R}^d$ es el embedding del prompt (obtenido de un encoder como
Sentence-BERT) y $\ell$ es la longitud característica del kernel.

Este kernel captura la **similitud semántica**: prompts con significado parecido tendrán
valores de calidad correlacionados, permitiendo al GP interpolar eficientemente.

### 3.4 Algoritmo OPRO (Optimization by PROmpting)

OPRO (Yang et al., Google DeepMind) usa el LLM como optimizador meta-nivel:

```
Entrada: tarea τ, métrica Q, presupuesto T
Inicializar: pool P₀ = {p₁, ..., pₖ} (prompts semilla)

Para t = 1, ..., T:
  1. Evaluar Q(pᵢ) ∀ pᵢ ∈ Pₜ₋₁
  2. Ordenar por Q descendente → ranking
  3. Construir meta-prompt:
     "Estos prompts fueron probados [lista con scores].
      Genera un prompt nuevo que supere al mejor."
  4. LLM genera candidato p_new
  5. Pₜ = Pₜ₋₁ ∪ {p_new}

Retornar: argmax Q(pᵢ)
```

**Convergencia:** OPRO converge empíricamente en $O(\sqrt{T})$ iteraciones si la
función de calidad es Lipschitz-continua en el espacio semántico de embeddings.

---

## 4. Complejidad de Kolmogorov y Mínima Descripción

### 4.1 Principio MDL Aplicado a Prompts

La **Complejidad de Kolmogorov** $K(x)$ de una cadena $x$ es la longitud del programa
más corto que produce $x$. Aplicada a prompt engineering:

$$K(\tau | p) = \text{longitud mínima de } p \text{ que especifica la tarea } \tau$$

**Principio de Mínima Descripción para Prompts (MDL-P):**

$$p^* = \arg\min_{p : Q(p,\tau) \geq \theta} \; |p|$$

Es decir: entre todos los prompts que alcanzan calidad $\geq \theta$, elegir el más corto.

**Justificación teórica:** Un prompt más corto con high quality implica que el modelo
ya tiene un prior fuerte sobre la tarea — el prompt actúa como un "puntero" a un
programa que el LLM ya contiene en sus pesos.

### 4.2 Operador de Novedad K(x|K)

Extendiendo Kolmogorov, definimos la **novedad condicional** de un output $x$ dado el
conocimiento previo $K$ del modelo:

$$\text{Novelty}(x | K) = K(x) - K(x | K)$$

- Si $\text{Novelty} \to 0$ → el output es predecible, redundante
- Si $\text{Novelty} \to K(x)$ → genuinamente nuevo, no derivable del training data

Este operador es central en la primitiva `forge` de AXON, donde controla el tradeoff
entre utilidad (bajo novelty → conservador) y sorpresa (alto novelty → creativo).

### 4.3 Compresión como Métrica de Eficiencia

Definimos la **eficiencia de un prompt** como:

$$\eta(p) = \frac{Q(p, \tau)}{|p|} = \frac{\text{calidad del output}}{\text{tokens del prompt}}$$

Un prompt eficiente maximiza calidad por token. Esto se conecta directamente con
**costos de API** ($\text{costo} \propto |p|$) y con la **ventana de contexto** finita.

La **ratio de compresión semántica** del prompt es:

$$\rho(p) = \frac{I(P; R)}{|p|} \quad (\text{bits de información por token})$$

Un prompt con alto $\rho$ transmite máxima información por token — es la versión
información-teórica de "cada palabra cuenta".

---

## 5. Retículos y Cálculo de Restricciones

### 5.1 Lattice Epistémico de AXON

AXON ya implementa un retículo parcialmente ordenado $(T, \leq)$ para tipos epistémicos:

```
⊤ (Any)
├── FactualClaim
│   └── CitedFact
│       └── HighConfidenceFact
├── Opinion
├── Uncertainty  ← propaga hacia arriba (taint)
└── Speculation
⊥ (Never)
```

**Formalización como retículo:**
- $(T, \leq)$ es un retículo con supremo ($\lor$) e ínfimo ($\land$)
- La operación de join: $T_1 \lor T_2$ = tipo más general que subsume ambos
- La operación de meet: $T_1 \land T_2$ = tipo más específico que ambos satisfacen
- **Taint propagation:** si $T_i = \text{Uncertainty}$, entonces $T_i \lor T_j = \text{Uncertainty}$

### 5.2 Constraint Function como Homomorfismo de Retículos

La función de restricción epistémica de AXON:

$$C : \text{Mode} \to (\tau, p, A)$$

es un **homomorfismo de retículos** que preserva el orden:

$$m_1 \leq m_2 \implies C(m_1).\tau \leq C(m_2).\tau$$

Concretamente:
- `know` $\leq$ `believe` $\leq$ `speculate` (orden por libertad)
- $C(\text{know}).\tau = 0.1 \leq C(\text{believe}).\tau = 0.3 \leq C(\text{speculate}).\tau = 0.9$
- Anchors inyectados: $C(\text{know}).A \supseteq C(\text{believe}).A \supseteq C(\text{speculate}).A$

### 5.3 Satisfacción de Restricciones como CSP

Un programa AXON con $n$ anchors define un **Problema de Satisfacción de Restricciones**:

$$\text{CSP} = (X, D, C)$$

donde:
- $X = \{x_1, \dots, x_m\}$ son las variables del output (claims, scores, entities)
- $D = \{D_1, \dots, D_m\}$ son los dominios (tipos semánticos)
- $C = \{c_1, \dots, c_n\}$ son los constraints de los anchors

**Ejemplo:** Anchor `NoHallucination` genera:
$$c_{NH}: \forall x_i \in X_{\text{claims}} : \text{has\_citation}(x_i) = \text{true}$$

El **self-healing** del RetryEngine es un **solver CSP con backtracking adaptativo**:
1. Generar output candidato $r$
2. Verificar $r \models C$ (¿satisface todas las restricciones?)
3. Si no: identificar $c_j$ violado → inyectar $c_j$ como feedback → regenerar

---

## 6. Teoría de Control para Prompt Dinámico

### 6.1 Prompt como Señal de Control

Modelamos el sistema LLM como un **sistema dinámico discreto**:

$$s_{t+1} = f(s_t, u_t, w_t)$$

donde:
- $s_t$ — estado del contexto (historial de conversación)
- $u_t$ — señal de control (prompt/instrucción en turno $t$)
- $w_t$ — ruido (estocasticidad del LLM)
- $f$ — dinámica del sistema (el LLM + runtime)

### 6.2 Controlador PID para Refinamiento

Un controlador **PID** (Proporcional-Integral-Derivativo) adaptativo:

$$u_t = K_p \cdot e_t + K_i \cdot \sum_{j=0}^{t} e_j + K_d \cdot (e_t - e_{t-1})$$

donde $e_t = Q^* - Q(r_t)$ es el error entre la calidad deseada y la obtenida.

- **Proporcional ($K_p$):** Corrige proporcionalmente al error actual
  → "Tu respuesta no incluyó citas; incluye citas."
- **Integral ($K_i$):** Corrige errores acumulados persistentes
  → "Has fallado 3 veces en incluir citas; esto es crítico."
- **Derivativo ($K_d$):** Responde a la tasa de cambio del error
  → "Estás mejorando pero aún falta; un ajuste fino más."

El `RetryEngine` de AXON ya implementa un controlador similar con `pass_failure_context`.

### 6.3 Estabilidad y Convergencia

**Teorema (convergencia informal):** Si la función de calidad $Q$ es
$L$-Lipschitz-continua en el espacio de prompts y el controlador tiene ganancia total
$|K_p + K_i + K_d| < 1/L$, entonces el sistema de refinamiento converge a un
punto fijo $r^*$ con $Q(r^*) \geq Q^* - \varepsilon$ en $O(\log(1/\varepsilon))$
iteraciones.

Esto fundamenta matemáticamente el `refine(max_attempts: N)` de AXON: con anchors bien
definidos (alta pendiente de la señal de error), el sistema converge rápidamente.

---

## 7. Optimización Multi-Objetivo

### 7.1 Frente de Pareto para Prompts

En la práctica, optimizar prompts involucra múltiples objetivos simultáneos:

$$\text{Maximizar: } \mathbf{Q}(p) = \begin{pmatrix} Q_{\text{precision}}(p) \\ Q_{\text{recall}}(p) \\ Q_{\text{creativity}}(p) \\ -\text{cost}(p) \\ -\text{latency}(p) \end{pmatrix}$$

Un prompt $p_1$ **domina** a $p_2$ ($p_1 \succ p_2$) si es al menos igual de bueno en
todos los objetivos y estrictamente mejor en al menos uno.

El **frente de Pareto** $\mathcal{F}^*$ es el conjunto de prompts no-dominados:

$$\mathcal{F}^* = \{p \in \mathcal{P} : \nexists p' \in \mathcal{P} \; \text{tal que} \; p' \succ p\}$$

### 7.2 Scalarización para Decisión

Para seleccionar un punto del frente de Pareto, usamos scalarización con pesos:

$$Q_{\text{scalar}}(p) = \sum_{i} w_i \cdot Q_i(p) \quad \text{con } \sum w_i = 1$$

Los pesos codifican la preferencia del usuario/tarea:
- Tarea factual (modo `know`): $w_{\text{precision}} = 0.5, w_{\text{recall}} = 0.3, w_{\text{creativity}} = 0.0$
- Tarea creativa (modo `speculate`): $w_{\text{creativity}} = 0.5, w_{\text{precision}} = 0.1$

---

## 8. Formalización del Pipeline Best-of-N (Consensus)

### 8.1 Modelo Estadístico

Dado un prompt $p$ ejecutado $N$ veces con temperatura $\tau > 0$, obtenemos $N$
respuestas independientes $(r_1, \dots, r_N) \sim R_p$. La primitiva `consensus` de AXON
selecciona:

$$r^* = \arg\max_{r_i} \; S(r_i, \mathcal{A})$$

donde $S$ es la función de score del reward anchor $\mathcal{A}$.

### 8.2 Bound de Calidad

**Teorema (Best-of-N Improvement):** Si $Q(r_i) \sim F$ (distribución de calidad), entonces:

$$\mathbb{E}[\max(Q(r_1), \dots, Q(r_N))] = \int_0^1 [1 - (1-q)^N] \, dF(q)$$

Para distribución uniforme $F = U[0,1]$:

$$\mathbb{E}[\max] = \frac{N}{N+1}$$

Es decir, con $N = 7$ ramas (como en `forge` de AXON), $\mathbb{E}[\text{best}] = 0.875$.
La mejora marginal decrece: pasar de $N=3$ a $N=7$ gana +12.5%, pero de $N=7$ a $N=15$
solo gana +6.25%. Esto justifica el rango práctico $N \in [3, 10]$.

### 8.3 Temperatura Efectiva del Forge

El `forge` de AXON usa la fórmula:

$$\tau_{\text{eff}} = \tau_{\text{base}} \times (0.5 + 0.5 \times \text{novelty})$$

Donde `novelty` $\in [0, 1]$ controla el tradeoff utilidad-sorpresa:

| Novelty | $\tau_{\text{eff}} / \tau_{\text{base}}$ | Comportamiento |
|---|---|---|
| 0.0 | 0.50 | Conservador, alta utilidad |
| 0.5 | 0.75 | Balanceado |
| 1.0 | 1.00 | Máxima divergencia, alta sorpresa |

---

## 9. Métricas Formales de Evaluación

### 9.1 Cuadro de Métricas Matematizadas

| Métrica | Fórmula | Qué mide |
|---|---|---|
| **Information Density** | $\rho(p) = I(P;R) / \|p\|$ | Información por token del prompt |
| **Entropy Reduction** | $\Delta H = H(R) - H(R\|P)$ | Cuánta incertidumbre elimina el prompt |
| **Constraint Satisfaction Rate** | $\text{CSR} = \|\{c \in C : r \models c\}\| / \|C\|$ | Fracción de anchors satisfechos |
| **Pareto Efficiency** | $\eta_P = 1 / \|\{p' : p' \succ p\}\|$ | Proximidad al frente de Pareto |
| **Compression Ratio** | $\text{CR} = \|p\| / \|p_{\text{naive}}\|$ | Eficiencia vs prompt bruto |
| **Convergence Rate** | $\gamma = Q(r_t) / Q(r_{t-1})$ | Velocidad de mejora por iteración |
| **Novelty Score** | $\nu = K(x) - K(x\|K)$ | Genuina novedad del output |

---

## 10. Síntesis: Principios Matemáticos Clave

> **10 Principios para la Optimización Matemática de Prompts:**

1. **Maximizar información mutua** $I(P; R)$: Cada token del prompt debe reducir incertidumbre
2. **Minimizar complejidad de Kolmogorov** $K(\tau|p)$: El prompt más corto que logra el objetivo es el mejor
3. **Usar modelos subrogados** (GP/BO): No evaluar ciegamente; predecir y priorizar
4. **Tratar constraints como CSP**: Los anchors son restricciones formales verificables
5. **Control por feedback**: El refinamiento es un sistema de control en lazo cerrado
6. **Best-of-N tiene retornos decrecientes**: $N \in [3, 10]$ es el rango óptimo práctico
7. **La temperatura es un operador sobre distribuciones**: No es un "slider de creatividad"
8. **Homorfismo de restricciones**: Los modos epistémicos son funciones monótonas sobre el retículo
9. **Optimización multi-objetivo**: Los prompts viven en un frente de Pareto, no en un ranking linear
10. **Convergencia requiere Lipschitz-continuidad**: Con anchors bien definidos, el self-healing converge $O(\log(1/\varepsilon))$

---

## Referencias Académicas

- Shannon, C.E. (1948). *A Mathematical Theory of Communication*
- Kolmogorov, A.N. (1963). *On Tables of Random Numbers*
- Yang et al. (2023). *Large Language Models as Optimizers*. Google DeepMind
- Khattab et al. (2024). *DSPy: Compiling Declarative Language Model Calls*. Stanford NLP
- Boden, M.A. (2004). *The Creative Mind: Myths and Mechanisms*
- Snoek et al. (2012). *Practical Bayesian Optimization of Machine Learning Hyperparameters*
- Poincaré, H. (1908). *Science et Méthode* (sobre el proceso creativo)
- Rissanen, J. (1978). *Modeling by Shortest Data Description* (MDL)
- Li & Vitányi (2008). *An Introduction to Kolmogorov Complexity and Its Applications*
- Mockus, J. (1989). *Bayesian Approach to Global Optimization*
