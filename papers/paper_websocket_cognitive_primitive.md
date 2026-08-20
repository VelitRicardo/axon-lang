# WebSocket as a Cognitive Primitive: Session-Typed Bidirectional Dialogue in AXON

**AXON Research Paper — Concept Proposal v1.0**
**Subject:** Linear Logic, Session Types, Dialogical Logic / Game Semantics, π-calculus
**Integration:** AXON typed channels (π-mobility), the `socket` primitive, `shield`, `cognitive_states` reconnection, the bidirectional dual of the SSE stream primitive

---

## Abstract

The WebSocket protocol (RFC 6455, 2011) is, by its own design, an *untyped* bidirectional channel: it frames only `text`, `binary`, and `control`, and explicitly delegates all protocol meaning to "the application layer." A decade and a half of industrial practice has therefore typed only **individual messages** (JSON-Schema, Zod, tRPC discriminated unions) and never **the conversation**. The result is a primitive that cannot state, let alone verify, *which utterance may follow which* — and whose reference implementations concede that the interface "does not support backpressure," collapsing under load into unbounded buffering or saturation.

This paper elevates the WebSocket from a byte transport to a **cognitive primitive**: a *session-typed bidirectional dialogue*. We ground it on the Curry–Howard correspondence of Caires & Pfenning — **session types *are* propositions of intuitionistic linear logic** — under which a connection is well-formed iff its two endpoints carry **dual** types, and is then **deadlock-free and protocol-conformant by construction**. We give backpressure an algebraic home as a **credit-refined linear index** (decidable in Presburger arithmetic, after Rast). Philosophically, the connection ceases to be transport and becomes a **dialogue game** (Lorenzen–Lorenz; Abramsky game semantics): two endpoints are Proponent and Opponent, duality *is* their role structure, and a well-typed protocol *is* a strategy. Finally we show that AXON's existing SSE primitive is the **single-polarity fragment** of this construction: WebSocket is the bidirectional *dual completion* of the stream.

---

## 1. The Epistemic Gap of the Untyped Channel

Let a protocol be an alternating exchange of utterances over a connection between two parties $p, q$. RFC 6455 equips the connection with a frame alphabet
$$\Sigma_{6455} = \{\texttt{text},\ \texttt{binary},\ \texttt{control}\}$$
and a transition relation that is *total* on $\Sigma_{6455}^{*}$: every byte sequence is "valid." The protocol's grammar — the language $\mathcal{L} \subseteq \Sigma^{*}$ of *admissible conversations* — is left entirely to the application. Two pathologies follow.

**(i) Semantic insecurity.** The connection cannot reject a well-formed-but-illegal utterance (a `msg` where only `close` was admissible). Per-message validators (Zod, tRPC) check membership $m \in \mathcal{M}$ in a *flat* message set $\mathcal{M}$; they cannot check membership in the *sequential* language $\mathcal{L}$, because $\mathcal{L}$ is a property of the **state of the dialogue**, not of any message. Typing the alphabet is not typing the language.

**(ii) Resource insecurity.** RFC 6455 carries no notion of consumption rate. The canonical reference implementation states plainly that "the WebSocket interface does not support backpressure"; when arrival rate exceeds service rate the endpoint "fills memory or reaches 100% CPU." The protocol has no *credit*.

In Heideggerian terms the untyped socket is *Vorhandenheit* — a present-at-hand object one must inspect, guess about, and validate post hoc. AXON seeks *Zuhandenheit*: a connection whose lawful conduct is **constitutive of its type**, not asserted at runtime.

---

## 2. Philosophy: Meaning as Dialogue

We adopt the **dialogical theory of meaning** (Lorenzen & Lorenz, 1950s): the meaning of a proposition is the **two-player game** in which a *Proponent* ($\mathsf{P}$) asserts and defends it against an *Opponent* ($\mathsf{O}$); the proposition is valid iff $\mathsf{P}$ possesses a **winning strategy**. Game semantics (Blass; Abramsky–Jagadeesan–Malacaria; Hyland–Ong) makes this precise for **linear logic**, exhibiting proofs as strategies and cut as the interaction of two strategies.

A WebSocket connection is *literally* such a game. The two endpoints are the two players. The legal moves at each point are the dialogue's rules. We therefore posit the **Dialogical Identity**:
$$
\boxed{\ \text{connection} \;\equiv\; \text{dialogue game} \qquad \text{endpoint duality} \;\equiv\; (\mathsf{P}, \mathsf{O})\ \text{role split} \qquad \text{conformant run} \;\equiv\; \text{play of a strategy}\ }
$$
The socket is not a pipe carrying data; it is a *dialogue whose admissible plays are its type*. This is the philosophical content that the four-frame alphabet of RFC 6455 cannot express.

---

## 3. The Logical Core: Propositions as Sessions

Caires & Pfenning (CONCUR'10; MSCS'16) establish a Curry–Howard isomorphism between **dual intuitionistic linear logic** and a session-typed π-calculus: linear propositions are session types, sequent proofs are processes, and **cut elimination is communication**. We adopt the session reading directly.

### 3.1 Session types

$$
S,T \;::=\; \mathbf{end} \;\mid\; {!}\,A.\,S \;\mid\; {?}\,A.\,S \;\mid\; \oplus\{\ell_i : S_i\}_{i\in I} \;\mid\; \&\{\ell_i : S_i\}_{i\in I} \;\mid\; \mu X.\,S \;\mid\; X
$$
read as: terminate; **send** a value of type $A$ then behave as $S$; **receive** $A$ then $S$; **select** a label $\ell_i$ (internal choice); **offer** a branch (external choice); recursive and variable. A value type $A$ may itself be a session (channel mobility — the π-calculus, AXON Fase 13).

### 3.2 Duality — the well-formedness of a connection

Duality $(\cdot)^{\perp}$ is the involution that swaps the two sides of every exchange:
$$
\mathbf{end}^{\perp}=\mathbf{end},\quad ({!}A.S)^{\perp}={?}A.\,S^{\perp},\quad ({?}A.S)^{\perp}={!}A.\,S^{\perp},
$$
$$
\big(\oplus\{\ell_i:S_i\}\big)^{\perp}=\&\{\ell_i:S_i^{\perp}\},\qquad \big(\&\{\ell_i:S_i\}\big)^{\perp}=\oplus\{\ell_i:S_i^{\perp}\},\qquad (\mu X.S)^{\perp}=\mu X.\,S^{\perp}.
$$
**Connection law.** A connection with endpoints $c, \bar c$ typed $S, \bar S$ is well-formed iff
$$
\boxed{\ \bar S = S^{\perp}\ }.
$$
This single algebraic equation is what RFC 6455 lacks: it is the static guarantee that "what one end sends, the other expects." Involutivity $\;(S^{\perp})^{\perp}=S\;$ makes the relation symmetric (neither end is privileged) — formally the $(\mathsf P,\mathsf O)$ swap of §2.

### 3.3 Typing judgment and progress

Processes are typed $\;\Gamma ; \Delta \vdash P :: c{:}S\;$ ($\Delta$ linear channels, $\Gamma$ shared). The linear discipline forces each channel to be used **exactly along its protocol**; the logical reading gives, for the cut (the composition of two dual processes on a channel), the operational rule
$$
\dfrac{\;\Gamma;\Delta_1 \vdash P :: c{:}S \qquad \Gamma;\Delta_2, c{:}S^{\perp} \vdash Q :: d{:}T\;}{\;\Gamma;\Delta_1,\Delta_2 \vdash (\nu c)(P \mid Q) :: d{:}T\;}\ \textsc{(cut)}
$$
**Theorem 1 (Conformance + Deadlock-freedom).** *If $\;\cdot;\cdot \vdash (\nu c)(P\mid Q) :: \mathbf{end}\;$ with $P,Q$ on dual endpoints, then every reduction sequence either terminates at $\mathbf{end}$ or steps; no reachable state is stuck on a pending complementary action.* (Cut elimination of intuitionistic linear logic = global progress of the session; Caires–Pfenning. For $n>2$ parties, the same guarantee follows from well-formed **multiparty** global types via endpoint projection — Honda–Yoshida–Carbone.)

Deadlock-freedom is thus not tested; it is **the cut-elimination theorem**, transported.

---

## 4. The Mathematical Algorithm: Projection, Credit-Refinement, and the SSE Fragment

The logical core gives *correctness*; the following gives the **algorithm** — the decidable machinery that the AXON compiler runs, and the part that carries the algorithmic weight.

### 4.1 Multiparty projection (the $n$-agent case — Kivi's skills/tools)

A multi-participant dialogue is given as a **global type** $G$ over roles $\mathcal{R}=\{r_1,\dots,r_n\}$. The compiler computes, for each role $r$, the **local** session type by the projection operator $G\!\restriction\! r$:
$$
(r_1 \!\to\! r_2 : \langle A\rangle.\,G)\!\restriction\! r \;=\;
\begin{cases}
{!}A.\,(G\!\restriction\! r) & r = r_1\\[2pt]
{?}A.\,(G\!\restriction\! r) & r = r_2\\[2pt]
G\!\restriction\! r & r \notin\{r_1,r_2\}\ \text{(merge)}
\end{cases}
$$
**Theorem 2 (Safe realizability).** *If $G$ is well-formed and every $G\!\restriction\! r$ is defined, then the network $\{\,r : G\!\restriction\! r\,\}_{r\in\mathcal R}$ enjoys communication safety, progress, and session fidelity.* The endpoints AXON spawns for a many-skilled agent are exactly the projections of one declared global dialogue.

### 4.2 Backpressure as a typed resource (closing RFC 6455's footgun)

We refine §3.1 with a **credit index** $n\in\mathbb{N}$ (after Rast / arithmetic-refined session types). The flow-controlled send becomes
$$
{!}^{\,n}A.\,S \quad\text{(``send $A$, consuming one of $n$ credits'')},\qquad
\textsc{Send}:\ \dfrac{\;n>0\;}{\;{!}^{\,n}A.\,S \;\longrightarrow\; {!}^{\,n-1}A.\,S\;}
$$
and the dual receiver **grants** credit, $\;{?}^{\,n}A.S\;$, replenishing the window. The crucial consequence: a send with $n=0$ has **no typing rule** — it is a *type error*, not a runtime memory blow-up. The side conditions are linear-arithmetic constraints over $\mathbb{N}$; their satisfiability is **decidable (Presburger)**, so the check is a terminating algorithm:
$$
\textsf{TypeOK}(S) \;\equiv\; \bigwedge (\text{credit constraints of } S)\ \ \text{is valid in } \langle\mathbb{N},0,{+},{<}\rangle.
$$
Backpressure stops being an operational accident and becomes a **theorem about the type**. This is the algorithmic heart: *the WebSocket's one structural defect, made a decidable type-level invariant.*

### 4.3 The decidable conformance algorithm

Given a declared dialogue $G$ and endpoint implementations:
$$
\textsc{Check}(G) = \mathbf{1.}\ \text{project } \{G\!\restriction\! r\}\ \ \mathbf{2.}\ \text{verify pairwise duality } \bar S \stackrel{?}{=} S^{\perp}\ \ \mathbf{3.}\ \text{discharge credit constraints in Presburger}\ \ \mathbf{4.}\ \text{type each endpoint process against } G\!\restriction\! r.
$$
Steps 1–2 are linear in $|G|$ (regular-coinductive equality for $\mu$-types); step 3 is decidable; step 4 is linear-time syntax-directed. The whole pipeline is **decidable and terminating** — admissible for a compiler pass.

### 4.4 SSE is the single-polarity fragment

AXON's stream primitive (Fase 33, *SSE as a cognitive primitive*) is **server-emits, client-receives**: the client never sends in the data phase. Its protocol is exactly
$$
S_{\mathrm{SSE}} \;=\; \mu X.\ \&\{\ \texttt{data}: {?}A.\,X,\ \ \texttt{done}: \mathbf{end}\ \}
$$
— a session type with **one polarity** (only $?$ on the consumer; only $\oplus/{!}$ on the producer). The WebSocket session generalizes it by admitting $!$ **and** $?$ at the same channel:
$$
\boxed{\ S_{\mathrm{SSE}} \;=\; \Pi_{\downarrow}\big(S_{\mathrm{WS}}\big)\ }\qquad\text{(SSE = the downstream projection of the bidirectional dialogue).}
$$
Hence WebSocket is not a *different* primitive bolted on; it is the **dual completion** of the one AXON already has. SSE typed half the dialogue (the server's monologue); the session-typed WebSocket types the **whole conversation**.

---

## 5. The AXON `socket` Primitive Specification

The dialogue is surfaced as a `socket` block whose `protocol` *is* a session type; the compiler runs §4's `Check`, the runtime realizes each endpoint as a π-typed channel, and `shield` mediates capability/PII on every utterance.

```axon
socket ChatDialogue<Utterance, Token> {
    // The conversation as a session type (client endpoint's view).
    protocol: rec X. select {
        ask:    send Utterance -> branch {           // client speaks
                    token:  recv Token -> X,          // server streams tokens
                    done:   end,
                },
        cancel: end,
    }
    backpressure: credit(64)        // §4.2 typed flow-control window
    duality:      checked           // §3.2 connection law: peer must be protocol^⊥
    reconnect:    cognitive_state   // resume mid-dialogue (Fase 40.t sealed snapshot)
    legal_basis:  legitimate_interest   // shield/audit on every move (enterprise)
}
```

Upon interpretation AXON: (1) projects + dual-checks + discharges credits (`Check`); (2) emits the endpoint as a typed channel over a Rust `tokio` WebSocket; (3) on disconnect, seals the session continuation as a `cognitive_state` and admits a typed `reconnect`; (4) anchors each utterance in the per-tenant audit hash chain. A protocol violation is a **compile-time** type error; a credit exhaustion is a **typed** stall, never a memory blow-up.

---

## 6. Realization and Honest Scope

**What is composition of established results:** the type theory (Caires–Pfenning, Honda–Yoshida–Carbone), the Rust session-type lineage (Ferrite's judgmental embedding; Rumpsteak's deadlock-free async MPST; Rusty Variation), and AXON's own π-typed channels (Fase 13) + sealed reconnection (Fase 40.t) + SSE stream (Fase 33).

**What is the contribution:** their *fusion into a single language primitive* that (a) types the **conversation**, not the message; (b) makes **backpressure a decidable type-level invariant**, closing RFC 6455's named defect; (c) inherits **multi-tenant RLS isolation + audit-chain provenance** by construction; and (d) is presented as the **bidirectional dual** of the SSE primitive — one coherent stream/dialogue theory rather than two ad-hoc transports. To our knowledge no deployed WebSocket stack types the protocol-as-session with credit-refined backpressure; the field types messages.

**Falsifiable claims** (the engineering must honor the math): every accepted `socket` is duality-checked; no well-typed endpoint can send without credit; a disconnected dialogue resumes only at a continuation its session type admits; conformance checking terminates.

---

## 7. Conclusion

For fifteen years the WebSocket has been an *untyped dialogue* — an alphabet of three frames with the conversation left unsaid, and a documented inability to push back under load. AXON does not patch it; it **re-founds** it. Under the lens of linear logic a connection becomes a proposition, its two ends become dual proofs, its lawful runs become the cut-elimination of those proofs, and its flow control becomes a decidable arithmetic invariant. Under the lens of dialogical logic it becomes what it always secretly was — a *game of meaning between two players*. The bidirectional dialogue is the dual completion of the SSE monologue: with it, AXON's real-time surface is, end to end, a **typed conversation** — the cognitive primitive a demanding production agent actually requires.
