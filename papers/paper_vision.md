# Visión Epistémica y Termodinámica de la Información: Hacia una Percepción Activa Pura en axon-lang

**Autor:** Ricardo Velit — Equipo de Arquitectura Cognitiva, axon-lang
**Fecha:** Marzo 2026
**Estado:** Propuesta de Implementación y Sustento Formal
**Versión:** 2.0

---

## Resumen Abstracto

El paradigma dominante en la visión artificial moderna depende intrínsecamente
de modelos conexionistas masivos (CNNs, ViTs) que operan como aproximadores
estadísticos de caja negra. Este documento propone y formaliza un enfoque
radicalmente distinto para `axon-lang`: la **Visión Epistémica**. Sustituyendo
la regresión estadística por la termodinámica de la información, el Análisis
Topológico de Datos (TDA) y la Inferencia Activa, dotamos a _axon_ de una
capacidad de percepción visual matemáticamente verificable, de latencia
predecible y acotable, y libre de dependencias de SDKs externos (OpenAI,
Anthropic). El modelo proyecta la imagen como una Variedad de Riemann,
extrayendo invariantes topológicos mediante homología persistente sobre
complejos cúbicos, y resuelve la atención visual minimizando la Energía Libre
Variacional del sistema cognitivo PEM.

**Palabras clave:** Homología Persistente, Difusión Anisotrópica, Inferencia
Activa, Energía Libre Variacional, Variedades de Riemann, Complejos Cúbicos,
Filtros de Gabor, Teoría de Haces, axon-lang.

---

## 1. Introducción: La Falacia de la Visión Estadística

Las librerías y APIs de visión actuales tratan las imágenes como tensores
espaciales sobre los cuales se aplican convoluciones o mecanismos de
_self-attention_ aprendidos a partir de petabytes de datos. Desde una
perspectiva filosófica y epistémica, estos sistemas no "ven"; simplemente
correlacionan patrones de píxeles con distribuciones latentes preentrenadas.

Para `axon-lang`, cuyo núcleo operativo se basa en la **certidumbre lógica,
contratos formales y modelado epistémico (MEK y PEM)**, depender de cajas negras
introduce una entropía inaceptable. Si un agente axon debe certificar un
contrato de seguridad, la base visual de dicha decisión debe ser deductiva y
geométricamente determinista, no probabilísticamente heurística.

La solución reside en la **Física Computacional y la Topología**: tratar la
imagen no como una tabla de píxeles, sino como un campo físico en el que un
agente interactúa para disipar su incertidumbre (Inferencia Activa).

### 1.1. Contribuciones

Este paper presenta las siguientes contribuciones formales:

1. **Formalización Riemanniana completa** de la imagen como variedad diferenciable
   con tensores métricos explícitos y geodésicas computables.
2. **Pipeline TDA con complejidad acotada** usando complejos cúbicos nativos
   sobre la rejilla de píxeles, evitando la explosión combinatoria de complejos
   simpliciales.
3. **Prueba de estabilidad** de la Firma Topológica bajo perturbaciones acotadas,
   derivada del Teorema de Estabilidad del Cuello de Botella.
4. **Integración formal** con el motor PEM existente de axon-lang, extendiendo
   `CognitiveManifold` y `FreeEnergyMinimizer` al dominio visual.
5. **Resolución del gap semántico** mediante Haces Topológicos (Sheaf Theory)
   que vinculan invariantes topológicos con categorías ontológicas.
6. **Análisis de complejidad algorítmica** riguroso con cotas computacionales
   verificables.

---

## 2. Formalización Matemática y Física

La arquitectura visual de axon se descompone en cinco postulados matemáticos
rigurosos, cada uno construido sobre teoremas establecidos en la literatura.

### 2.1. La Imagen como un Campo Diferenciable (Variedad de Riemann)

**Definición 2.1** *(Campo de Imagen).* Sea $\Omega \subset \mathbb{R}^2$ un
dominio compacto rectangular. Definimos una imagen en escala de grises como
una función suave por partes $I: \Omega \to \mathbb{R}_{\geq 0}$, y una imagen
a color como $\mathbf{I}: \Omega \to \mathbb{R}^3_{\geq 0}$ en el espacio $(R,G,B)$.

**Definición 2.2** *(Variedad de Riemann Inducida).* Dada $I(x,y)$, definimos
la variedad $\mathcal{M}_I = (\Omega, g_I)$ donde el tensor métrico inducido es:

$$g_I(x,y) = \begin{pmatrix} 1 + \left(\frac{\partial I}{\partial x}\right)^2 & \frac{\partial I}{\partial x}\frac{\partial I}{\partial y} \\ \frac{\partial I}{\partial x}\frac{\partial I}{\partial y} & 1 + \left(\frac{\partial I}{\partial y}\right)^2 \end{pmatrix}$$

Este tensor $g_I$ induce una geometría intrínseca sobre la imagen donde las
regiones de alto gradiente (bordes) tienen curvatura elevada y las regiones
suaves son localmente planas. La longitud geodésica entre dos puntos
$p, q \in \Omega$ se define:

$$d_g(p,q) = \inf_{\gamma} \int_0^1 \sqrt{\dot{\gamma}(t)^T \cdot g_I(\gamma(t)) \cdot \dot{\gamma}(t)} \, dt$$

donde $\gamma: [0,1] \to \Omega$ son curvas que conectan $p$ y $q$.

**Proposición 2.1.** *La curvatura escalar de Gauss de $\mathcal{M}_I$ en un punto $(x,y)$ es:*

$$K(x,y) = \frac{\det(\text{Hess}(I))}{\left(1 + \|\nabla I\|^2\right)^2}$$

*donde $\text{Hess}(I)$ es la matriz Hessiana de $I$. Los puntos con $K > 0$
corresponden a extremos locales (picos/valles de intensidad), los puntos con
$K < 0$ a puntos de silla (cruces de bordes), y los puntos con $K = 0$ a
regiones localmente planas.*

**Justificación formal.** Esta formulación se basa en el Teorema de Gauss-Bonnet,
que conecta la curvatura intrínseca con la topología global:

$$\int_{\mathcal{M}_I} K \, dA + \int_{\partial \mathcal{M}_I} k_g \, ds = 2\pi \chi(\mathcal{M}_I)$$

donde $\chi$ es la característica de Euler. Esto permite a axon verificar la
consistencia topológica global de la imagen mediante una integral sobre
cantidades locales, proporcionando un **certificado geométrico** verificable.

### 2.2. Preprocesamiento: Difusión Anisotrópica Regularizada

Para reducir el ruido sin destruir las fronteras estructurales de los objetos,
aplicamos la **Ecuación de Difusión Anisotrópica (Perona-Malik)** con
regularización de Catté.

**Definición 2.3** *(Difusión PM Regularizada).* Dado el campo de imagen
$I: \Omega \times [0, T] \to \mathbb{R}$, definimos la evolución temporal:

$$\frac{\partial I}{\partial t} = \nabla \cdot \left( c\left(\|\nabla G_\sigma * I\|\right) \nabla I \right)$$

donde $G_\sigma$ es un kernel Gaussiano de escala $\sigma > 0$ que garantiza
bienestar (*well-posedness*) del problema, y el coeficiente de conductividad es:

$$c(s) = \frac{1}{1 + \left(\frac{s}{\lambda}\right)^2}, \quad \lambda > 0$$

**Teorema 2.1** *(Bienestar de la PM Regularizada, Catté et al. 1992).* *Para
$\sigma > 0$ fijo y datos iniciales $I_0 \in L^\infty(\Omega)$, la EDP
regularizada admite una única solución débil $I \in L^2(0,T; H^1(\Omega)) \cap
C([0,T]; L^2(\Omega))$.*

**Observación 2.1** *(Carácter forward-backward).* La ecuación PM original
($\sigma = 0$) es **matemáticamente mal planteada** (ill-posed) debido a la
difusión retrógrada en zonas de gradiente alto. La regularización $G_\sigma *$
resuelve esta patología al suavizar el gradiente antes de evaluar la
conductividad, preservando las propiedades deseables de preservación de bordes.

**Discretización numérica.** Para la implementación en axon, empleamos el
esquema explícito de Euler con paso temporal restringido por la condición CFL:

$$I^{n+1}_{i,j} = I^n_{i,j} + \Delta t \sum_{p \in \mathcal{N}(i,j)} c\left(\|\nabla_{i,j \to p} G_\sigma * I^n\|\right) \cdot \left(I^n_p - I^n_{i,j}\right)$$

con la restricción de estabilidad $\Delta t \leq \frac{1}{4 \max c}$ para
la rejilla 2D con 4-vecindario. La complejidad por iteración es
$\mathcal{O}(W \times H)$ donde $W, H$ son las dimensiones de la imagen.

### 2.3. Codificación de Fase: Banco de Filtros de Gabor

**Definición 2.4** *(Filtro de Gabor 2D).* Un filtro de Gabor complejo se
define como el producto de un envelope Gaussiano y una señal sinusoidal:

$$\psi_{\lambda,\theta,\varphi,\sigma,\gamma}(x,y) = \exp\left(-\frac{x'^2 + \gamma^2 y'^2}{2\sigma^2}\right) \cdot \exp\left(i\left(\frac{2\pi x'}{\lambda} + \varphi\right)\right)$$

donde las coordenadas rotadas son:

$$x' = x\cos\theta + y\sin\theta, \quad y' = -x\sin\theta + y\cos\theta$$

y los parámetros son:
- $\lambda$: longitud de onda de la sinusoide (frecuencia espacial $f = 1/\lambda$)
- $\theta$: orientación del filtro ($\theta \in \{0, \pi/N_\theta, \ldots, (N_\theta-1)\pi/N_\theta\}$)
- $\varphi$: fase de la sinusoide (0 para par, $\pi/2$ para impar)
- $\sigma$: desviación estándar del envelope Gaussiano
- $\gamma$: razón de aspecto espacial de la elipsoide

**Fundamentación neurocientífica.** Los filtros de Gabor modelan los campos
receptivos de las células simples en la corteza visual primaria V1
(Hubel & Wiesel, 1962; Daugman, 1985). La función de Gabor satisface el
**límite de incertidumbre de Heisenberg conjunto**:

$$\Delta x \cdot \Delta \xi \geq \frac{1}{4\pi}$$

minimizando el producto de resolución espacio-frecuencia, lo que la convierte
en el filtro óptimo para localización simultánea espacial y frecuencial.

**Definición 2.5** *(Tensor de Fase).* Para una imagen $I$ procesada por un
banco de $N_\lambda \times N_\theta$ filtros de Gabor, definimos el Tensor de
Fase:

$$\Phi_{I}(\lambda_k, \theta_l)(x,y) = |I * \psi_{\lambda_k, \theta_l}|(x, y) + i \cdot \arg(I * \psi_{\lambda_k, \theta_l})(x,y)$$

Este tensor $\Phi_I \in \mathbb{C}^{W \times H \times N_\lambda \times N_\theta}$
codifica la respuesta multiescala y multiorientación. Su módulo captura la
energía local y su fase la posición precisa de las estructuras.

### 2.4. Indexación Estructural vía Homología Persistente (TDA)

#### 2.4.1. Complejos Cúbicos sobre Rejillas de Píxeles

**Definición 2.6** *(Complejo Cúbico de Imagen).* Para una imagen discreta
$I: \{0,...,W-1\} \times \{0,...,H-1\} \to \mathbb{R}$, definimos el complejo
cúbico $\mathcal{K}_I$ mediante:

- **0-cubos** (vértices): cada píxel $(i,j)$
- **1-cubos** (aristas): cada par de píxeles adyacentes $\{(i,j), (i\pm1,j)\}$ o $\{(i,j), (i,j\pm1)\}$
- **2-cubos** (caras): cada cuadrado $\{(i,j), (i+1,j), (i,j+1), (i+1,j+1)\}$

La **filtración de sub-nivel** se construye ordenando los cubos por el valor
máximo de intensidad de sus vértices:

$$\mathcal{K}^{(\epsilon)}_I = \{c \in \mathcal{K}_I : \max_{v \in c} I(v) \leq \epsilon\}$$

para $\epsilon \in [\min I, \max I]$, produciendo una secuencia anidada:

$$\emptyset = \mathcal{K}^{(\epsilon_0)} \subseteq \mathcal{K}^{(\epsilon_1)} \subseteq \cdots \subseteq \mathcal{K}^{(\epsilon_m)} = \mathcal{K}_I$$

**Ventaja computacional sobre complejos simpliciales.** Para una imagen de
$N = W \times H$ píxeles, el complejo cúbico tiene exactamente:
- $N$ vértices (0-cubos)
- $\approx 2N$ aristas (1-cubos)
- $\approx N$ caras (2-cubos)

Total: $|\mathcal{K}_I| \approx 4N = \mathcal{O}(N)$, frente al $\mathcal{O}(N^3)$
de un complejo de Vietoris-Rips. **Esta reducción es crítica para la
viabilidad computacional.**

#### 2.4.2. Cómputo de Homología Persistente

**Definición 2.7** *(Números de Betti Persistentes).* Para la filtración
$\{\mathcal{K}^{(\epsilon)}\}_\epsilon$, los números de Betti persistentes se
definen:

$$\beta_n^{a,b} = \text{rank}\left(H_n(\mathcal{K}^{(a)}) \to H_n(\mathcal{K}^{(b)})\right), \quad a \leq b$$

que codifican:
- $\beta_0^{a,b}$: componentes conexas que nacen antes de $a$ y persisten hasta $b$
- $\beta_1^{a,b}$: ciclos unidimensionales (agujeros) persistentes
- $\beta_2^{a,b}$: cavidades (en extensiones 3D o hiperespectrales)

**Diagrama de Persistencia.** El diagrama $\text{Dgm}_n(I) \subset \{(b,d) \in \mathbb{R}^2 : b < d\}$
registra los pares (nacimiento, muerte) de cada clase homológica en dimensión $n$.

**Complejidad algorítmica.** El cómputo de homología persistente sobre
complejos cúbicos utiliza la reducción matricial de Smith sobre
$\mathbb{Z}/2\mathbb{Z}$:

| Operación | Complejidad | Referencia |
|-----------|-------------|------------|
| Construcción del complejo cúbico | $\mathcal{O}(N)$ | Trivial (rejilla) |
| Reducción matricial (worst-case) | $\mathcal{O}(N^\omega)$ donde $\omega \leq 2.373$ | Milosavljević et al. (2011) |
| $H_0$ vía Union-Find | $\mathcal{O}(N \cdot \alpha(N))$ | Tarjan (1975) |
| CubicalRipser (práctica, $H_0 + H_1$) | $\mathcal{O}(N \log N)$ empírico | Kaji et al. (2020) |

**Para una imagen de 1024×1024 ($N \approx 10^6$):** el cómputo práctico
con CubicalRipser toma $\mathcal{O}(N \log N) \approx 2 \times 10^7$ operaciones,
ejecutable en $<100$ ms en hardware moderno. Comparado con la inferencia de
una CNN ($\approx 10^9$ FLOPs para ResNet-50), el TDA es **50× más eficiente
computacionalmente** para extraer estructura.

#### 2.4.3. Firma Topológica y Teorema de Estabilidad

**Definición 2.8** *(Firma Topológica).* Un objeto $\mathcal{O}$ detectado
en la imagen $I$ se define formalmente por su Firma Topológica:

$$\text{Firma}(\mathcal{O}) = \{ (b_i, d_i) \in \text{Dgm}(I_{\mathcal{O}}) \mid d_i - b_i > \tau \}$$

donde $\tau > 0$ es un umbral de persistencia que filtra el ruido topológico
(features de vida corta).

**Teorema 2.2** *(Estabilidad del Cuello de Botella, Cohen-Steiner, Edelsbrunner & Harer, 2007).*
*Sean $f, g: X \to \mathbb{R}$ funciones continuas tame sobre un espacio
topológico triangulable $X$. Entonces:*

$$d_B\left(\text{Dgm}(f), \text{Dgm}(g)\right) \leq \|f - g\|_\infty$$

*donde $d_B$ es la distancia del cuello de botella (bottleneck) entre diagramas
de persistencia.*

**Corolario 2.1** *(Robustez al ruido).* *Sea $I$ una imagen y $\tilde{I} = I + \eta$
una perturbación con $\|\eta\|_\infty \leq \delta$. Entonces:*

$$d_B\left(\text{Firma}(I), \text{Firma}(\tilde{I})\right) \leq \delta$$

*Esto significa que la Firma Topológica es Lipschitz-estable con constante 1.
**Una perturbación de a lo sumo $\delta$ en la intensidad de los píxeles
desplaza cada punto del diagrama de persistencia en a lo sumo $\delta$.***

**Extensión: Distancia de Wasserstein $p$.** Para comparaciones más finas
entre firmas topológicas, definimos:

$$W_p(\text{Dgm}(f), \text{Dgm}(g)) = \left(\inf_\gamma \sum_{x \in \text{Dgm}(f)} \|x - \gamma(x)\|_\infty^p\right)^{1/p}$$

donde $\gamma$ recorre todas las biyecciones parciales entre los diagramas
(incluyendo la diagonal). La estabilidad en Wasserstein-$p$ fue demostrada
por Skraba & Turner (2020) bajo condiciones más fuertes de regularidad.

**Propiedad 2.1** *(Invarianza topológica).* *La Firma Topológica es invariante
bajo isomorfismos topológicos del espacio subyacente. En particular, es
invariante a:*
- *Traslación: $I(x + a, y + b) \mapsto \text{Firma}$ idéntica*
- *Rotación (con interpolación exacta)*
- *Deformaciones continuas (homeomorfismos)*
- *Cambios monótonos de intensidad (reparametrizaciones de la filtración)*

### 2.5. Percepción como Inferencia Activa (Principio de Energía Libre)

Integrado en el módulo PEM (Psychological Epistemic Modeling) existente de
axon-lang, la percepción no es pasiva. El agente axon posee un estado cognitivo
sobre una variedad Riemanniana $\mathcal{M}_{PEM}$ con dinámica SDE
(documentada en `cognitive_state.py`):

$$d\psi_t = -\nabla U(\psi_t, I_t)\,dt + \sigma \cdot dW_t$$

#### 2.5.1. Modelo Generativo Visual

**Definición 2.9** *(Modelo Generativo).* El agente axon mantiene un modelo
generativo conjunto $P(\mathbf{o}, \mathbf{s})$ donde:

- $\mathbf{o} = (\Phi_I, \text{Dgm}(I))$: observaciones sensoriales (tensor de fase + diagrama de persistencia)
- $\mathbf{s} = (s^{\text{topo}}, s^{\text{geom}}, s^{\text{sem}})$: estados latentes estructurales

El modelo se factoriza:

$$P(\mathbf{o}, \mathbf{s}) = P(\mathbf{o} | \mathbf{s}) \cdot P(\mathbf{s})$$

donde $P(\mathbf{o} | \mathbf{s})$ es el modelo de emisión (cómo las estructuras
generan observaciones) y $P(\mathbf{s})$ es el prior epistémico del agente
(creencias sobre qué estructuras son probables).

#### 2.5.2. Energía Libre Variacional Visual

**Definición 2.10** *(Energía Libre Variacional Visual).* La percepción visual
ocurre minimizando:

$$\mathcal{F}_{V} = \underbrace{D_{KL}\left[Q(\mathbf{s}) \| P(\mathbf{s})\right]}_{\text{Complejidad}} - \underbrace{\mathbb{E}_{Q(\mathbf{s})}\left[\log P(\mathbf{o} | \mathbf{s})\right]}_{\text{Precisión}}$$

donde $Q(\mathbf{s})$ es la distribución variacional aproximada (las creencias
del agente sobre los estados del mundo).

**Proposición 2.2.** *$\mathcal{F}_V$ es una cota superior de la sorpresa
negativa (negative model evidence):*

$$\mathcal{F}_V \geq -\log P(\mathbf{o})$$

*La igualdad se alcanza si y solo si $Q(\mathbf{s}) = P(\mathbf{s} | \mathbf{o})$
(inferencia exacta).*

#### 2.5.3. Atención Visual como Foveación Epistémica

**Definición 2.11** *(Ganancia de Información Esperada).* El agente dirige su
"mirada" computacional a la región $\omega \subset \Omega$ que maximiza la
Ganancia de Información Esperada:

$$\mathcal{G}(\omega) = H\left[Q(\mathbf{s})\right] - \mathbb{E}_{P(\mathbf{o}_\omega | \omega)}\left[H\left[Q(\mathbf{s} | \mathbf{o}_\omega)\right]\right]$$

donde $H[\cdot]$ es la entropía de Shannon y $\mathbf{o}_\omega$ son las
observaciones locales (tensor de fase + topología local) en la región $\omega$.

**Algoritmo 1** *(Foveación Epistémica Activa).*
```
Entrada: Imagen I, presupuesto computacional B
Salida: Modelo estructural completo Q*(s)

1. Inicializar Q(s) ← prior epistémico P(s)
2. Particionar Ω en regiones candidatas {ω_1, ..., ω_K}
3. MIENTRAS F_V > ε Y presupuesto B no agotado:
   a. Para cada ω_k no visitada:
      - Computar G(ω_k) usando Q(s) actual
   b. Seleccionar ω* = argmax_k G(ω_k)
   c. Computar observaciones locales:
      - Φ_I(ω*) ← Banco de Gabor sobre I|_{ω*}
      - Dgm(I|_{ω*}) ← TDA local sobre I|_{ω*}
   d. Actualizar Q(s) mediante inferencia variacional
   e. Actualizar F_V
4. RETORNAR Q*(s) y Firma(I) completa
```

**Conexión con el motor PEM existente.** Este algoritmo se integra directamente
con `FreeEnergyMinimizer` (documentado en `active_inference.py`), donde:

- La **Ganancia de Información Esperada** $\mathcal{G}$ corresponde al
  componente `EpistemicValue.compute()`
- El **valor pragmático** (mantener al agente en la zona alostática) corresponde
  a `PragmaticValue.compute()`
- La **selección de región óptima** implementa
  `FreeEnergyMinimizer.select_optimal()`

La puntuación compuesta de cada trayectoria visual es:

$$\text{Score}(\pi) = -G(\pi) = \alpha \cdot E_\pi + (1 - \alpha) \cdot P_\pi$$

donde $\alpha \in [0,1]$ es el peso epistémico (configurado en `PsycheProfile`,
default $\alpha = 0.6$), $E_\pi$ es el valor epistémico (ganancia de
información) y $P_\pi$ es el valor pragmático (seguridad cognitiva).

---

## 3. Resolución del Gap Semántico: Haces Topológicos

### 3.1. El Problema

Los números de Betti y los diagramas de persistencia capturan la **forma** de
un objeto pero no su **significado**. Una taza y una rosquilla son
topológicamente equivalentes ($\beta_1 = 1$), pero semánticamente distintas.
Para resolver esto sin recurrir a modelos estadísticos, introducimos **Haces
Topológicos** (Sheaves).

### 3.2. Formalización

**Definición 3.1** *(Haz Semántico sobre Firma Topológica).* Definimos un haz
$\mathcal{S}$ sobre el espacio de firmas topológicas $\mathcal{T}$ como un
funtor:

$$\mathcal{S}: \text{Open}(\mathcal{T})^{op} \to \textbf{Set}$$

que asigna a cada abierto $U \subseteq \mathcal{T}$ (una región del espacio
de firmas) un conjunto $\mathcal{S}(U)$ de **etiquetas semánticas compatibles**,
y para cada inclusión $V \hookrightarrow U$ una restricción
$\text{res}_{U,V}: \mathcal{S}(U) \to \mathcal{S}(V)$.

**Definición 3.2** *(Secciones Globales como Clasificación).* Una sección
global $s \in \Gamma(\mathcal{T}, \mathcal{S})$ es una asignación coherente
de semántica a cada firma topológica. La clasificación de un objeto
$\mathcal{O}$ es:

$$\text{Clase}(\mathcal{O}) = \text{res}_{\mathcal{T}, U_{\mathcal{O}}}\left(\Gamma(\mathcal{T}, \mathcal{S})\right)$$

donde $U_{\mathcal{O}} \subset \mathcal{T}$ es el vecindario abierto de
$\text{Firma}(\mathcal{O})$ en el espacio topológico de firmas.

### 3.3. Construcción Práctica del Haz

En la implementación de axon, el haz se instancia mediante una **base de
conocimiento topológica** $(KB)$:

$$KB = \{(P_k, C_k, \mathcal{R}_k) : k = 1, \ldots, M\}$$

donde:
- $P_k = \{(b_i, d_i)\}$ es un prototipo de firma topológica
- $C_k$ es la categoría semántica (codificada en el ontology MDN)
- $\mathcal{R}_k$ son restricciones geométricas contextuales (razón de aspecto,
  curvatura promedio, relaciones espaciales)

La clasificación se resuelve por cercanía en el espacio de Wasserstein:

$$\text{Clase}(\mathcal{O}) = \arg\min_k W_2\left(\text{Firma}(\mathcal{O}), P_k\right) + \lambda \cdot d_{\text{geom}}(\mathcal{O}, \mathcal{R}_k)$$

**Proposición 3.1** *(No regresión a estadística).* Este esquema NO es un
clasificador estadístico porque:
1. Los prototipos $P_k$ se definen axiomáticamente, no se aprenden de datos
2. La distancia $W_2$ opera sobre invariantes topológicos deterministas
3. Las restricciones geométricas $\mathcal{R}_k$ son contratos formales verificables
4. La base de conocimiento es completamente auditable e interpretable

---

## 4. Integración en la Arquitectura axon-lang

### 4.1. Tabla de Mapeo Módulo → Componente Visual

| Motor axon | Módulo existente | Extensión Visual | Función |
|------------|-----------------|------------------|---------|
| **MEK** | `holographic_codec.py` | `gabor_phase_codec.py` [NEW] | Codificación de fase Gabor (V1 biomimético) |
| **PIX** | `indexer.py` | `topological_indexer.py` [NEW] | Indexación por Firma Topológica |
| **PIX** | `navigator.py` | `visual_navigator.py` [NEW] | Navegación foveal por regiones |
| **PEM** | `cognitive_state.py` | Extensión (nuevas dimensiones) | Dimensión `visual_certainty` |
| **PEM** | `active_inference.py` | Extensión (epistemic visual) | `VisualEpistemicValue` |
| **PEM** | `density_matrix.py` | Sin cambios | Proyectores para evidencia visual |
| **PEM** | `safety_types.py` | `visual_safety.py` [NEW] | Contratos de seguridad visual |

### 4.2. Extensión del Estado Cognitivo

Se añade una nueva dimensión cognitiva al espacio $\mathcal{M}_{PEM}$:

```python
CognitiveDimension(
    name='visual_certainty',
    lower=0.0, upper=1.0,
    default=0.0,
    curvature=1.5   # Alta resistencia: la certeza visual es costosa
)
```

El tensor métrico del manifold cognitivo se extiende:

$$g_{ij}^{PEM+V} = \text{diag}(\kappa_{\text{affect}}, \kappa_{\text{load}}, \kappa_{\text{certainty}}, \kappa_{\text{openness}}, \kappa_{\text{trust}}, \kappa_{\text{visual\_certainty}})$$

manteniendo la estructura diagonal documentada en `CognitiveManifold`.

### 4.3. Contratos Formales de Seguridad Visual

```axon
shield VisualSafety {
    // Contrato de consistencia topológica
    check topological_consistency {
        let dgm = perceive(image);
        assert dgm.stability_bound < epsilon
            : "Perception exceeds noise tolerance";
    }

    // Contrato de verificación geométrica
    check geometric_verification {
        let sig_A = topological_signature(img_A);
        let sig_B = topological_signature(img_B);
        assert wasserstein_distance(sig_A, sig_B) < delta
            : "Structural divergence exceeds safety contract";
    }

    // Contrato de atención completa
    check attention_coverage {
        let coverage = foveal_coverage(image, budget);
        assert coverage.free_energy < threshold
            : "Insufficient visual evidence for decision";
    }
}
```

---

## 5. Pipeline Computacional Completo

### 5.1. Diagrama de Flujo

```
Imagen I(x,y)
    ├──[1]──→ Difusión Anisotrópica Regularizada (§2.2)
    │         └── I_filtered ← PM(I, σ, λ, T)
    │              Complejidad: O(W×H×iter)
    │
    ├──[2]──→ Codificación de Fase Gabor (§2.3)
    │         └── Φ_I ← GaborBank(I_filtered, λ_k, θ_l)
    │              Complejidad: O(W×H×N_λ×N_θ) = O(N×F)
    │
    ├──[3]──→ TDA sobre Complejo Cúbico (§2.4)
    │         └── Dgm(I) ← CubicalPH(I_filtered)
    │              Complejidad: O(N log N) práctico
    │
    ├──[4]──→ Foveación Epistémica Activa (§2.5)
    │         ├── Iterar: seleccionar ωₖ, computar G(ωₖ)
    │         ├── Actualizar Q(s) mediante PEM.process_signal()
    │         └── Minimizar F_V hasta convergencia
    │              Complejidad: O(K × N_local × log N_local)
    │
    └──[5]──→ Clasificación por Haz Semántico (§3)
              └── Clase(O) ← argmin W_2(Firma(O), P_k)
                   Complejidad: O(|Dgm|² × M)
```

### 5.2. Análisis de Complejidad Total

| Etapa | Complejidad | Para 1024×1024 | Para 4K (3840×2160) |
|-------|-------------|----------------|---------------------|
| PM Difusión (10 iter) | $\mathcal{O}(10 \cdot N)$ | ~10M ops | ~83M ops |
| Gabor (8λ × 8θ = 64) | $\mathcal{O}(64 \cdot N)$ | ~67M ops | ~530M ops |
| TDA ($H_0 + H_1$) | $\mathcal{O}(N \log N)$ | ~20M ops | ~190M ops |
| Foveación (K=16 regiones) | $\mathcal{O}(16 \cdot N/16 \cdot \log(N/16))$ | ~16M ops | ~140M ops |
| Clasificación Sheaf (M=100) | $\mathcal{O}(|Dgm|^2 \cdot 100)$ | ~1M ops | ~4M ops |
| **TOTAL** | | **~114M ops** | **~947M ops** |
| ResNet-50 (referencia) | | **~4,089M FLOPs** | **~4,089M FLOPs** |

**Conclusión:** El pipeline epistémico es **35× más eficiente** que una CNN
estándar para imágenes de 1024×1024 y **4× más eficiente** para 4K, con la
propiedad adicional de ser **determinista, verificable y auditable**.

> **Nota importante:** "Latencia cero" fue una afirmación del paper v1.0 que
> corregimos. La latencia correcta es **predecible y acotable**: dado el
> presupuesto computacional $B$, la latencia máxima es $T_{max} = \mathcal{O}(B \cdot N)$.
> Esto es cualitativamente superior a la latencia CNN que depende de la
> complejidad del modelo, no de los datos.

---

## 6. Limitaciones y Trabajo Futuro

### 6.1. Limitaciones Conocidas

1. **Gap semántico residual.** Los Haces Topológicos requieren una base de
   conocimiento axiomática $(KB)$ que debe ser construida manualmente o
   importada de ontologías existentes. Esto es un costo de ingeniería no
   trivial, aunque elimina la dependencia de datos de entrenamiento.

2. **Escalabilidad del $H_2$ y dimensiones superiores.** La homología
   persistente en dimensión $\geq 2$ sobre complejos cúbicos puede ser
   computacionalmente intensiva. Para aplicaciones 3D/volumétricas, se requiere
   investigación en algoritmos especializados (e.g., distributed PH).

3. **Texturas complejas.** La topología captura forma, no textura. Objetos
   con formas idénticas pero texturas distintas (e.g., dos esferas de
   materiales diferentes) requieren los Filtros de Gabor (§2.3) como
   complemento al TDA.

4. **Oclusión y escenas complejas.** La propuesta actual asume objetos
   segmentables. Escenas densas con oclusión parcial requieren extensiones
   del modelo de foveación para manejar hipótesis competitivas en el espacio
   de estados $\mathbf{s}$.

5. **Well-posedness de PM con $\sigma \to 0$.** La regularización de Catté
   añade un hiperparámetro $\sigma$ cuya selección óptima depende de la escala
   del ruido en la imagen. Un esquema adaptativo basado en la estimación de
   ruido local se plantea como trabajo futuro.

### 6.2. Trabajo Futuro

1. **Benchmark experimental.** Implementar el pipeline completo y medir:
   - Tiempo de ejecución vs. CNNs/ViTs en CIFAR-10, ImageNet-1K
   - Precisión de clasificación usando KB topológica
   - Robustez al ruido adversarial comparado con modelos estadísticos

2. **Integración con Lambda Data ($\Lambda D$).** Exportar Firmas Topológicas
   como vectores epistémicos cross-módulo mediante el sistema EMS de axon-lang.

3. **Visual POMDP.** Formalizar la foveación como un Proceso de Decisión de
   Markov Parcialmente Observable, integrando con el `FreeEnergyMinimizer`
   para planificación no miope de secuencias de atención.

4. **Haces de Gabor.** Extender la Teoría de Haces del §3 para operar
   directamente sobre el Tensor de Fase $\Phi_I$, permitiendo clasificación
   joint topológica-frecuencial.

5. **Contratos formales verificados.** Conectar los shields del §4.3 con el
   TypeChecker de axon para hacer las aserciones de seguridad visual
   **compilables** en tiempo de compilación, no solo verificables en runtime.

---

## 7. Comparación con el Estado del Arte

| Criterio | CNNs/ViTs | axon Visión Epistémica |
|----------|-----------|----------------------|
| **Determinismo** | No (pesos estocásticos) | Sí (álgebra determinista) |
| **Verificabilidad** | Caja negra | Contratos formales |
| **Robustez al ruido** | Frágil (adversarial attacks) | Lipschitz-1 (Teorema 2.2) |
| **Invarianza** | Aprendida (data-dependent) | Axiomática (topológica) |
| **Eficiencia** | $\mathcal{O}(10^9)$ FLOPs | $\mathcal{O}(10^7-10^8)$ ops |
| **Dependencias externas** | PyTorch + CUDA + Modelos | Pure Python / C++ mínimo |
| **Latencia** | Variable (carga GPU) | Predecible y acotable |
| **Interpretabilidad** | Grad-CAM (post-hoc) | Firma topológica (intrínseca) |
| **Semántica** | Aprendida end-to-end | Axiomática + Haces |
| **Requiere entrenamiento** | Sí (petabytes) | No (solo KB ontológica) |

---

## 8. Conclusiones

Este paper formaliza la Visión Epistémica como un paradigma de percepción
visual radicalmente distinto al enfoque conexionista dominante. La propuesta
se construye sobre cinco pilares matemáticos validados:

1. **Geometría diferencial** (variedades Riemannianas, tensores métricos)
2. **Física computacional** (difusión anisotrópica, ecuaciones PDE)
3. **Topología algebraica** (homología persistente, complejos cúbicos)
4. **Teoría de la información** (energía libre variacional, inferencia activa)
5. **Álgebra categorial** (haces topológicos, funtores)

Cada componente se integra orgánicamente con la arquitectura existente de
axon-lang: los filtros de Gabor extienden el codec holográfico MEK, la
homología persistente extiende el indexador PIX, y la inferencia activa ya
está implementada en el motor PEM. La resolución del gap semántico mediante
Haces Topológicos cierra el ciclo sin recurrir a regresión estadística.

El resultado es un sistema de percepción visual que es **determinista**,
**verificable**, **eficiente** y **epistemológicamente fundamentado** — las
cuatro propiedades que el paradigma conexionista no puede garantizar
simultáneamente y que axon-lang requiere para sus contratos formales.

---

## Referencias

1. Perona, P. & Malik, J. (1990). "Scale-space and edge detection using anisotropic diffusion." *IEEE TPAMI*, 12(7), 629-639.
2. Catté, F., Lions, P.L., Morel, J.M. & Coll, T. (1992). "Image selective smoothing and edge detection by nonlinear diffusion." *SIAM J. Numer. Anal.*, 29(1), 182-193.
3. Cohen-Steiner, D., Edelsbrunner, H. & Harer, J. (2007). "Stability of Persistence Diagrams." *Discrete Comput. Geom.*, 37(1), 103-120.
4. Edelsbrunner, H. & Harer, J. (2010). *Computational Topology: An Introduction.* AMS.
5. Friston, K. (2010). "The free-energy principle: A unified brain theory?" *Nat. Rev. Neurosci.*, 11(2), 127-138.
6. Busemeyer, J.R. & Bruza, P.D. (2012). *Quantum Models of Cognition and Decision.* Cambridge University Press.
7. Daugman, J.G. (1985). "Uncertainty relation for resolution in space, spatial frequency, and orientation." *J. Opt. Soc. Am. A*, 2(7), 1160-1169.
8. Hubel, D.H. & Wiesel, T.N. (1962). "Receptive fields, binocular interaction and functional architecture in the cat's visual cortex." *J. Physiol.*, 160(1), 106-154.
9. Kaji, S., Sudo, T. & Ahara, K. (2020). "Cubical Ripser: Software for computing persistent homology of image and volume data." *arXiv:2005.12692*.
10. Milosavljević, N., Morozov, D. & Skraba, P. (2011). "Zigzag persistent homology in matrix multiplication time." *SoCG*, 216-225.
11. Skraba, P. & Turner, K. (2020). "Wasserstein stability for persistence diagrams." *arXiv:2006.16824*.
12. Pirolli, P. & Card, S. (1999). "Information Foraging." *Psychological Review*, 106(4), 643-675.
13. Curry, J. (2014). "Sheaves, Cosheaves and Applications." *arXiv:1303.3255v2*.
14. do Carmo, M.P. (1992). *Riemannian Geometry.* Birkhäuser.
15. Tarjan, R.E. (1975). "Efficiency of a Good But Not Linear Set Union Algorithm." *JACM*, 22(2), 215-225.
16. Gauss, C.F. (1827). *Disquisitiones generales circa superficies curvas.* Commentationes Societatis Regiae Scientiarum Gottingensis Recentiores.

---

*© 2026 Equipo de Arquitectura Cognitiva, axon-lang. Todos los derechos reservados.*
