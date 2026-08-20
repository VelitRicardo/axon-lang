# axon-agora — Research Paper

> **The first official library of axon-lang** — Governed Autonomous Social Action as a Typed, Proof-Carrying Act
> July 2026 · Design & research foundation (§Fase 116) · Native connectors for LinkedIn, Meta (Facebook Pages), Instagram, and TikTok

---

## Provenance note (read this first)

This document is a **research foundation and design paper**, written *before* implementation,
per the founder's instruction that every Axon capability be founded on real, juried research.
It is **not** an implementation report. As of this writing (§Fase 116 opened, 2026-07-18) the
`axon-agora` library **does not yet exist** — no connector, no module, no test. The §111
anti-drift doctrine is in force: this paper claims nothing about a shipping artifact. Every
statement in **§II (Platform Ground Truth)** is a fact about the *external* world — the
platforms' own 2026 developer documentation, terms of use, and case law — each carrying a
citation to a primary source that was fetched and adversarially verified (three independent
skeptic votes per claim; `3-0` = unanimously confirmed). Every statement in **§V (Architecture)**
and beyond is **proposed design**, written in the conditional, and grounded in Axon primitives
that *do* already ship (each cross-referenced to its landing Fase).

The research was conducted with a fan-out/verify harness over 24 primary and secondary
sources yielding 111 extracted claims; 25 load-bearing claims were put to adversarial
verification, 13 confirmed unanimously against primary platform documentation. The remaining
load-bearing platform-policy claims — those whose panel run was truncated by a usage limit —
were then **re-fetched and read directly at their primary sources on 2026-07-18** and are quoted
verbatim (§II tags them [verified]). Where a claim was refuted or remains secondary, this paper
says so in place (§II.2 note, §II.5 note).

**The name is ratified** (D116.1, founder 2026-07-18, amended to the final name same day):
**`axon-agora`**, module namespace `agora` — the Greek *agora*, the public square where a citizen
*speaks in public*. The name encodes this paper's philosophical pillar (§4.3): publishing is a
delegated **performative speech act**, and the agent speaks in the agora on its principal's
behalf. The alternatives — `axon-social` (descriptive but flat) and `axon-social-connet` (a typo
for *connect*, and redundant) — were set aside. The founder also ratified D116.3 (owned-only
posture) and D116.5 (OSS/ENT split) the same day; the build order lives in the living plan
(`axon-enterprise/docs/fase/fase_116_axon_agora.md`).

---

## Abstract

`axon-agora` is proposed as **axon-lang's first official library**: native, governed
connectors that let a developer's cognitive agent act *directly* inside LinkedIn, Facebook
Pages, Instagram, and TikTok — reading comments, reactions, and metrics; moderating and
replying; editing and deleting; and publishing — as one step inside a larger multi-tool task,
with **zero human input at execution time**.

The central finding of the research is that **the zero-input problem is not a technical
problem — it is a governance problem, and no existing ecosystem solves it at the language
level.** Every operation `axon-agora` targets already exists in the four platforms' *official*
APIs (§II). What the platforms condition is not capability but **autonomy**: LinkedIn's API
Terms prohibit using the APIs "to automate posting" [L-TOS]; TikTok requires "express" per-post
user consent before content is even transmitted [TT-CSG]; Instagram meters publishing as a
hard consumable quota of 100 posts / 24h [IG-CP]. These are not obstacles to route around — an
entire failure literature (§III) documents what happens when they are: banned accounts
(instagrapi [IG-UNOF]), a $500,000 breach-of-contract judgment (hiQ v. LinkedIn [HIQ]), a rogue
agent that sent 500+ unsolicited messages (§III.4 [OSO]), and a 700-organization OAuth-token
breach (§III.3 [NANGO]). The defining survey finding: catalogued defenses against rogue agents
are *exclusively* infrastructure- and process-level (scoped tokens, least privilege,
out-of-band confirmation, peer review) — **none operate at the language or type-system level**
[OSO].

`axon-agora` fills exactly that gap. It does not invent governance; it **inherits** it. Axon
already treats egress as a first-class governed act (`deliver` §105, `notify` §110, the
governed channel §114), already custodies secrets so their value never enters cognition (§94),
already mints scoped ephemeral credentials whose grants are a subset of the minter's (§92),
already meters linear budgets as consumable resources (§72), already stamps acquired data
`Untrusted` at birth (§98/T908), and — as of §115 — already has a real module system for
which `axon-agora` would be **the first official consumer**. The library's contribution is to
express each platform's *published protocol* as a **session-typed** connector (§IV.1, after Hu
& Yoshida [HY16]) whose posting quotas are **linear resources** (§IV.1, after Girard [GIR] and
the graded-resource semantics of [GRADE]), whose scopes are **static deontic permissions**
(§IV.2, after [DEON]), whose act of publishing is a **delegated performative speech act**
(§IV.3, after Austin/Searle and the FIPA-ACL negative result [PITT]), and whose authority is an
**object capability** attenuated from a broader OAuth grant (§IV.4, after Miller [MIL]).

The result, if built to the design in §V, is the first system in which "an agent posts to
LinkedIn" is a statement a *compiler* can refuse.

---

## I. Problem Statement: The Zero-Input Governance Gap

### 1.1 What the developer wants

A developer building an agent in Axon wants to write a flow like:

> *"Read the last 50 comments on our latest LinkedIn post, summarize sentiment, draft three
> replies to the most negative in our brand voice, check the campaign metrics from the ads
> tool, and if engagement dropped more than 20% week-over-week, publish a follow-up post — all
> without me in the loop."*

Every verb in that sentence — *read comments, summarize, draft, reply, check metrics, publish*
— is an action **inside** a social network, interleaved with a non-social tool (the ads
metrics). This is the multi-tool, zero-input agent task. The question this paper answers is:
**what does it actually take to make that flow legal, safe, and typed?**

### 1.2 The four concrete problems

1. **Capability is real but conditioned.** All four platforms *can* do every operation above
   through official APIs — but each wraps autonomy in review gates, consent requirements, and
   automation prohibitions that differ per platform and per operation (§II). A naive connector
   that ignores these ships the developer straight into a ToS violation.

2. **The protocols are multi-step and stateful.** Instagram and TikTok publishing are not one
   call. Instagram is `create-container → poll-status → publish` with a five-state machine and
   a 24-hour container expiry [IG-CP]; TikTok is `query-creator-info → init → upload → poll`
   [TT-DP]. Calling these out of order is a runtime error at best and a malformed public post
   at worst. This is a *typestate* problem (§IV.1).

3. **The quotas are consumable.** Instagram: 100 posts / 24h [IG-CP]. TikTok Direct Post:
   ~15 posts / creator / 24h [TT-CSG]. These are not rate limits to retry past — the User
   Agreements make circumventing them a *contractual* violation [L-UA]. This is a *linear
   resource* problem (§IV.1).

4. **The credentials are long-lived and dangerous.** A never-expiring Facebook Page token [FB-TOK]
   or a 365-day TikTok refresh token [TT-OAUTH] is precisely the credential a fully unattended
   agent needs — and precisely the credential whose theft exposed 700+ organizations [NANGO].
   Custody of these cannot live in agent-visible code. This is a *secret-custody* and
   *object-capability* problem (§IV.4, §V.3).

### 1.3 Why not just call the APIs / use an SDK / scrape?

- **Scraping is settled law and it lost.** hiQ v. LinkedIn ended in December 2022 with hiQ
  permanently enjoined from scraping, ordered to delete all scraped data and derived code, and
  paying $500,000 — losing on **breach of contract**, independent of the CFAA question [HIQ].
  The lesson the research makes unambiguous: even where scraping public data is not a *computer-
  fraud* crime, it remains an enforceable *contract* breach. `axon-agora` is official-API-only,
  by construction (D116.4).

- **Unofficial clients have no safe operating mode.** The instagrapi community's own guidance is
  that even with session reuse and long inter-action delays, accounts "remain exposed to
  Instagram's aggressive detection and can still be banned"; the fallback is to "give that
  account a rest for the day" [IG-UNOF]. Fully unattended operation is acknowledged *by its own
  users* to be infeasible. (The very discussion thread documenting this was deleted — the
  repository has Discussions disabled and the URL 404s as of 2026-07-18 — a small monument to
  the impermanence of the unofficial ecosystem's knowledge base.)

- **SDKs and aggregators solve integration, not governance.** They are surveyed in §III. The
  short version: they hand the developer a working call and *none* of them makes the platform's
  autonomy conditions legible to a type system.

---

## II. Platform Ground Truth (2026)

Every row below is quoted from the platform's own current documentation, terms, or a court
record. Confidence tags: **[3-0]** = adversarially verified by a three-vote skeptic panel,
unanimous; **[verified]** = confirmed by **direct primary-source fetch (2026-07-18)**, quoted
verbatim (the panel run on this batch was truncated by a usage limit, so each was re-fetched and
read at the source rather than paraphrased — see the note at §II.5).

### 2.1 LinkedIn — Community Management / Marketing Developer Platform

| Operation | Official API? | Gate / Scope | Confidence |
|---|---|---|---|
| Create / update / delete **organization** posts | ✅ Posts API | Community Management, vetted; `w_organization_social` family | [3-0] |
| Manage comments & reactions on org pages | ✅ | Community Management | [3-0] |
| Read analytics (follower / page / share / video) | ✅ | Community Management | [3-0] |
| Posts / comments / reactions **on behalf of a member profile** | ✅ "Profile Management" use case | Approved partners only | [3-0] |
| Read comments / reactions (organic + sponsored) | ✅ **Social Metadata API** | *replaces* legacy `socialActions`, reaction types beyond likes | [3-0] |
| Read member-level social data | ❌ **`r_member_social` is CLOSED** | "We're not accepting access requests at this time" | [3-0] |
| Fully automated posting (general API terms) | ⚠️ **Prohibited** | API ToS **§3.1(26)**: "Use the Content or the APIs to automate posting on the LinkedIn Services" | [verified] |

**Access is human-gated.** The Community Management API is a "Vetted Product" with a limited
**Development Tier** and a **Standard Tier**; upgrading to Standard requires "a screencast video
demonstrating each use case" — and the review script itself asks the applicant to "demonstrate
an application user approving access … via the complete OAuth flow" [LI-CM, LI-REV]. **The
initial authorization grant is expected to be an interactive human step**; only the *subsequent*
API calls run unattended under the granted token.

**Enforcement & lifecycle facts a governed connector must model:**
- LinkedIn "retains the right to monitor your application and suspend or discontinue access …
  even if your application was previously approved" [LI-CM]. Tokens and access are revocable
  platform-side.
- Stored Member Profile Data "may only [be refreshed] … when the Member is actually using your
  Application and not on an automated schedule" (API ToS **§4.3**) [L-TOS] — this *forbids* an
  unattended background refresh daemon for **member** data (contrast Pages, §2.3).
- On member-token expiry, "you must obtain that Member's consent again" (API ToS **§5.2**)
  [L-TOS] — expiry forces a human-in-the-loop re-authorization, not silent perpetual refresh.
- Marketing APIs follow a versioned-release model with active sunsets (e.g., "Marketing Version
  202507 … has been sunset") [LI-CM] — the connector must track a rolling deprecation cadence.
- The User Agreement (§8.2) independently prohibits bots/automation for member-level "create,
  comment, like, share, or re-share" actions, scraping, and access via non-authorized
  interfaces [L-UA]. This is what makes the unofficial `linkedin-api` package categorically
  ToS-violating.

### 2.2 Meta — Facebook Pages API

| Operation | Official API? | Gate / Scope | Confidence |
|---|---|---|---|
| Publish / schedule posts on an **owned Page** | ✅ | `pages_manage_posts` — "Publish and schedule content" | [3-0] / [verified] |
| Moderate comments, delete posts | ✅ | `pages_manage_engagement` — "Moderate comments, delete posts" | [verified] |
| Read Page content | ✅ | `pages_read_engagement` — "Read content posted to the Page" | [verified] |
| Read Page insights / metrics | ✅ | Page analytics endpoints | [verified] |
| Real-time event ingestion (new comments, likes, messages) | ✅ **Webhooks** | official push channel | [verified] |

The permissions map one-to-one onto agent capabilities [FB-PG] — exactly the raw material for a
per-operation capability×scope matrix (§V.2). "To interact with the Pages API, a Page access
token is required," and "if your app needs extended permissions (most Page management features),
a Facebook App Review is required" [FB-PG]: unattended publishing on an owned Page is *permitted*
but *review-gated*. "Webhooks provide real-time updates for changes or events on the Page, such
as new comments, likes, or messages" [FB-PG] — the official event channel (§V.5).

> **Note on a refuted claim.** The harness *refuted* (0-3) the absolute statement "every Pages
> API call requires a Page access token (not merely a user token)." The accurate, primary-
> sourced statement — confirmed by direct fetch — is narrower: "to interact with the Pages API,
> a Page access token is required" [FB-PG] for Page management operations; the refuted framing
> was the over-general "every call / never a user token." The load-bearing custody fact survives
> intact (§2.5).

### 2.3 Instagram — Graph API (professional accounts)

| Operation | Official API? | Gate / Scope | Confidence |
|---|---|---|---|
| Publish (image / video / carousel) | ✅ two-step container | see protocol below | [3-0] |
| Check remaining publish quota | ✅ `GET /{ig}/content_publishing_limit` | — | [3-0] |
| Read comments / metrics | ✅ | `instagram_business_basic` | [primary] |
| Publish on a **consumer** account | ❌ | professional (business/creator) accounts only | [3-0] |

**Publishing is a mandatory typestate protocol** [IG-CP]:
```
POST /{ig}/media           → creation_id            (create container)
GET  /{container}?fields=status_code                (poll: IN_PROGRESS → FINISHED | ERROR | EXPIRED)
POST /{ig}/media_publish   creation_id={container}  (publish; only legal from FINISHED)
```
Meta recommends polling "once per minute, for no more than 5 minutes"; **unpublished containers
expire after 24 hours** [IG-CP]. There is *no single-call publish*.

**Quota is a hard consumable resource:** "Instagram accounts are limited to 100 API-published
posts within a 24-hour moving period" (carousels count as one), and the remaining budget is
programmatically queryable [IG-CP] — a linear resource the runtime can meter *before* it spends
(§IV.1).

**Scopes differ by login path** [IG-CP]:
- *Instagram Login:* `instagram_business_basic` + `instagram_business_content_publish`
- *Facebook Login:* `instagram_basic` + `instagram_content_publish` + `pages_read_engagement`
  (+ `ads_management`/`ads_read` if the user holds the Page role via Business Manager)

### 2.4 TikTok — Content Posting API

| Operation | Official API? | Gate / Scope | Confidence |
|---|---|---|---|
| Publish to a creator account (audited app) | ✅ Direct Post | `video.publish`, app-approved + **user-authorized** | [verified] |
| Publish from an **unaudited** app | ⚠️ **SELF_ONLY** (private), ≤5 users / 24h | "All content posted by unaudited clients … restricted to private viewing mode"; requires **audit** to go public | [3-0] / [verified] |
| Publish without per-post consent | ❌ **Prohibited** | "must only start sending content materials … after the user has expressly consent to the upload" | [3-0] |
| Auto-fill privacy / interaction settings | ❌ **Prohibited** | "manually select the privacy status from a dropdown … no default value"; interaction settings "none should be checked by default" | [3-0] |
| Read comments / metrics | ✅ Display API / Research API | separate scopes | [primary] |

**The audit gate is absolute for public posting:** "Unaudited API Clients can only post contents
in SELF_ONLY viewership" and "can allow up to 5 users to post in a 24 hour window" [TT-CSG].
Public autonomous posting is *impossible* until the app passes TikTok's audit.

**Direct Post is a stateful protocol** [TT-DP]:
```
Query Creator Info                                  (mandatory first step)
POST /v2/post/publish/video/init/  (or /content/init/ for photos)
PUT  {upload_url}   (video bytes, for FILE_UPLOAD source)
POST … poll /v2/post/publish/status/fetch/  with publish_id
```

**Even audited, quota is consumable:** "typically around 15 posts per day/creator account" via
Direct Post [TT-CSG].

**The consent requirement is the hardest zero-input constraint of all four platforms.** TikTok
mandates "express" per-post user consent *before content is transmitted*, and per-post manual
(non-defaulted) selection of privacy and interaction settings [TT-CSG]. Read as written, this
*forbids fully unattended public posting to a creator's account* even for an audited app. This
single fact shapes D116.3 (§IX): `axon-agora`'s TikTok connector is **read/analytics-first**,
with publishing surfaced only in the postures the audit and consent regime actually permit.

### 2.5 Token lifetimes & custody (the zero-input substrate)

| Platform | Access token | Refresh mechanics | Unattended-friendly? |
|---|---|---|---|
| **Facebook Page** | **No expiration** (long-lived Page token) | obtained via `GET /{app-scoped-user-id}/accounts` with a long-lived User token from a Page-role holder | ✅ **most durable** [FB-TOK] |
| Facebook User | ~60 days (long-lived) | exchange short→long needs **app secret, server-side only**; an **expired** token *cannot* be exchanged | ⚠️ daemon must refresh *before* expiry [FB-TOK] |
| **TikTok** | 24h (`86400s`) | `refresh_token` 365 days; **rotates on use** — must persist the newly-returned token atomically | ⚠️ daily refresh; rotation is a custody trap [TT-OAUTH] |
| **LinkedIn (member)** | per grant | **re-consent required** on expiry; no automated member-data refresh | ❌ human re-auth [L-TOS] |

TikTok exposes an explicit **revocation endpoint** (`POST /v2/oauth/revoke/`) and returns
granted scopes as a comma-separated list [TT-OAUTH] — a governed runtime gets a programmatic
kill-switch and a scope-audit surface for free. This maps directly onto Axon's epoch-kill and
grant-subset machinery (§92).

**Custody rule the research dictates:** the Facebook **app secret** must "never [be] client-
side" [FB-TOK]; the TikTok **client_secret** is required server-side for every token operation
[TT-OAUTH]. Neither may ever enter agent-visible code — which is exactly the §94 invariant
(*secret value never enters cognition*), not a new requirement.

> **Verification-coverage note.** The three-vote adversarial panel confirmed 13 of the platform
> claims unanimously (tagged [3-0]). A second batch — the TikTok publishing/consent protocol and
> Direct-Post flow, the LinkedIn ToS-automation prohibition (§3.1(26)) and member-data/re-consent
> clauses (§4.3, §5.2), and the Facebook permission-map and App-Review gate — had its panel run
> truncated by a usage limit, so each was **re-fetched and read directly at its primary source on
> 2026-07-18** and is quoted verbatim (tagged [verified]). The only remaining [primary]-tagged
> rows are TikTok read/analytics via the Display/Research APIs, which are secondary to this
> paper's publishing-and-governance thesis. The platform ground truth is now source-confirmed
> end to end.

---

## III. Research Foundation: Comparative Ecosystems and Their Failure Modes

How does everyone *else* let code talk to these networks, and where does it break?

### 3.1 Official SDKs and language-integrated precedents

The `facebook-business-sdk`, LinkedIn's official clients, and TikTok's SDKs are thin wrappers
over the same REST surfaces mapped in §II. They solve *marshalling*; they encode none of the
platform's autonomy conditions. The most language-*integrated* precedents are instructive:

- **Wolfram Language `ServiceConnect`** and **Ballerina**'s network-typed connectors bring
  external services into the language as first-class objects — the closest philosophical
  cousins to what `axon-agora` proposes — but neither carries a *governance* type: a Ballerina
  client can post to a network with no compiler-visible notion of consent, quota, or scope.
- **Salesforce Apex `ConnectApi`** governs *social* actions inside Salesforce's own walled
  platform, proving the pattern is commercially real — but it governs Salesforce, not the open
  social web.

### 3.2 Aggregator APIs — the closest existing analogue

**Ayrshare** is a hosted aggregator exposing "publishing, comment management, and analytics" over
13 networks including all four targets, with webhooks, priced at "$149–$599+/month" [BUF/AYR].
**Mixpost** is its open-source, self-hosted counterpart (11 networks, all four targets) — the
self-custody end of the spectrum [POSTIZ]. These are the *market-validated* shape of a social
connector layer. Their limitation is precisely the gap: they are **proprietary hosted APIs or
apps**, not **language-level constructs**. The developer still writes ungoverned glue around
them, and the platform's conditions remain invisible to the developer's own type system.

### 3.3 Agent-tool ecosystems and credential sprawl

LangChain tools, MCP servers, **Composio** (~1,000 toolkits, but *shared* credentials with no
per-user isolation) and **Arcade** (per-user auth) represent the agent-native frontier
[V12/NANGO]. The research surfaces the systemic risk directly: "APIs differ in which agent
identity models they support (bot/service identity, per-user OAuth, shared org-level, workspace-
scoped), so a multi-platform connector cannot assume one uniform machine-to-machine auth model"
[NANGO] — and centralized token custody has catastrophic blast radius: "In August 2025, stolen
OAuth tokens from an integration breach exposed customer environments across 700+ organizations"
[NANGO]. Credential sprawl is not a hygiene problem; it is the dominant failure mode.

### 3.4 Ungoverned autonomous action — the incident literature

- **instagrapi** (unofficial IG private API): no safe operating mode; bans persist through every
  recommended mitigation [IG-UNOF] (§I.3).
- **The rogue agent:** "In February 2026, an agent built on OpenClaw went rogue and sent more
  than 500 unsolicited [messages] to its user, the user's spouse, and random contacts" — a
  concrete failure of unattended outbound communication "with no rate or scope governance" [OSO].
- **The survey's verdict:** the catalogued defenses against rogue agents are "exclusively
  infrastructure- and process-level controls (scoped tokens, least privilege, out-of-band
  confirmation, peer review)" — and, in the researcher's words, **"none operate at the language
  or type-system level"** [OSO]. This is the empirical statement of the gap `axon-agora` exists
  to fill.

### 3.5 The comparison, in one table

| System | Multi-network | Official-API-only | Consent/quota in the type system | Credential value out of agent reach | Language-level |
|---|---|---|---|---|---|
| Official SDKs | per-SDK | ✅ | ✗ | ✗ | ✗ |
| Ayrshare / Buffer (hosted) | ✅ | ✅ | ✗ | hosted (3rd-party custody) | ✗ |
| Mixpost (self-hosted OSS) | ✅ | ✅ | ✗ | self-custody | ✗ |
| Composio / Arcade / MCP | ✅ | mixed | ✗ | mixed | ✗ |
| instagrapi / linkedin-api | per-lib | ❌ (ToS-violating) | ✗ | ✗ | ✗ |
| Wolfram / Ballerina / Apex | per-service | ✅ | ✗ | ✗ | **partial** |
| **axon-agora (proposed)** | ✅ (4 to start) | ✅ **by construction** | ✅ **session-typed + linear** | ✅ **§94 custody** | ✅ **native module** |

---

## IV. Academic Foundations — Mapped to the Four Pillars

Axon's charter requires every capability to rest on all four pillars: Mathematical, Logical,
Philosophical, Computational. Each maps to a load-bearing design decision, not decoration.

### 4.1 Mathematical — Session types and linear logic

**The problem:** the platform protocols are multi-step and their quotas are consumable.

**The theory:** Hu & Yoshida's *Hybrid Session Verification through Endpoint API Generation*
(FASE 2016) "generates protocol-specific endpoint APIs from multiparty session types," reifying
"each state [of the endpoint FSM] as a distinct channel type … that permits only the exact I/O
operations in that state" [HY16]. Static type checking then verifies the *ordering* of a
multi-step protocol, "supplemented by very light run-time checks … that enforce a linear usage
discipline," and "the resulting hybrid verification guarantees the absence of protocol violation
errors during the execution of the session" [HY16]. Critically, they implemented it "for Java as
an extension to Scribble" and validated it on *real wire protocols — HTTP and SMTP* — so this is
not a toy result; session-typed protocol APIs ship for mainstream ecosystems.

**The map:** Instagram's `create-container → poll-status → publish` and TikTok's
`query → init → upload → poll` are *exactly* the finite-state protocols this technique was built
for. `axon-agora` compiles each platform's published protocol into a session-typed connector so
that **calling `media_publish` before the container reaches `FINISHED` is a type error, not a
runtime 400.** The hybrid static/runtime split is the key practical lesson: Axon does **not** need
a bespoke verified type theory at every layer — a generated, session-typed connector API plus a
small linearity check at the seam suffices [HY16].

**Quotas as linear resources:** Girard's linear logic treats "logical statements [as] resources
which cannot be duplicated or discarded" [GIR]. The modern graded/coeffect realization gives a
"resource-aware operational semantics … stuck if resources are exhausted," where the type system
statically prevents reaching the stuck state, parametric over "an arbitrary grade algebra"
modeling heterogeneous usage policies [GRADE]. Instagram's 100/24h and TikTok's ~15/creator/24h
become *linear budgets* — and Axon already ships linear budgets (§72). The connector's `publish`
consumes from a quota budget the compiler tracks; **spending past the quota is unrepresentable**,
not merely caught.

### 4.2 Logical — Deontic logic and Hohfeldian delegated power

**The problem:** OAuth scopes are permissions; delegated account access is a conferred power;
and an agent can *technically* violate a norm it is *forbidden* to violate.

**The theory:** Deontic logic is "the formal tool for reasoning about normative multiagent
systems … in which agents can decide whether to follow the explicitly represented norms" [DEON]
— the exact setting of an agent that *could* post against ToS. It distinguishes **static/strong
permission** ("nothing is permitted that does not explicitly occur in the norms") from liberal
negative permission — and **static permission is precisely OAuth-scope semantics**: nothing is
permitted unless a scope explicitly grants it [DEON]. The survey names a limitation that is
itself a design directive: standard deontic logic "does not explicitly represent the norms of
the system, only the obligations and permissions … detached from" them [DEON] — so a governance
system for agents must **reify norms as first-class objects**, which a type system does. And
**contrary-to-duty** reasoning — "obligations … in force only in case of norm violations"
[DEON] — is the formal shape of *breach handling*: what an agent owes *after* an out-of-policy
post exists. Axon already has this seam: `on_breach` and the `BreachSink` (§114.w).

**Delegated authority:** the "declarative power" line — "the capacity of the power-holder of
creating normative positions, involving other agents, simply by 'proclaiming' such positions"
[HOH] — is the Hohfeldian-power analogue of an account owner delegating scoped posting authority
to an agent, and it was shown expressive enough to "represent the contract-net protocol" [HOH],
a real computational delegation mechanism. `axon-agora`'s credentials are *conferred normative
positions*: an agent holds only the powers the owner proclaimed, no more — which is §92's
"grants ⊆ minter" law stated in deontic terms.

### 4.3 Philosophical — Publishing as a delegated performative speech act

**The problem:** what *is* it, precisely, for an agent to publish on a principal's behalf?

**The theory:** to publish is not to describe — it is to *do*. Austin's performatives and
Searle's speech acts frame publication as an **illocutionary act** that changes the social world
by being uttered. When an agent posts *for* a principal, it is a **delegated speaker**. The
multi-agent-systems field already institutionalized this: the FIPA ACL (1997–99), "the first
major standardized agent communication language, is … grounded in speech act theory, with its
syntax defined as performatives" [PITT] — precedent for standardizing autonomous agent "speech"
decades before MCP or LangChain. But FIPA also produced a **foundational negative result** that
directly shapes Axon's design: defining a performative's meaning "in terms of the agent's mental
state is unsuitable as a normative standard" [PITT]. The lesson: **govern agent speech through
observable, protocol- and type-level mechanisms — not through the agent's intentions.** This is
the philosophical charter for `axon-agora` being a *type system* over social action rather than
a prompt-level guardrail. Attention economics (Simon: "a wealth of information creates a poverty
of attention") supplies the ethical weight — an autonomous public speaker *spends the attention
of real audiences* — which Axon already models as a spendable resource in the `notify` attention
ledger (§110).

### 4.4 Computational — Object capabilities and information-flow provenance

**The problem:** an agent must hold *only* the slice of authority it needs, and data read from
a network must not silently become trusted.

**The theory:** Miller's *Robust Composition* founds the object-capability model: "restrict all
inter-object causality to messages sent on references," making "the reference graph … the access
graph" — what code can do is bounded by which references it holds, "enforced at the language
level rather than by external policy" [MIL]. Decisively, Miller distinguishes **permission from
authority** and shows authority can be "attenuated by interposing unprivileged access
abstractions that forward only some messages" [MIL] — the exact precedent for deriving a
**reply-only** or **read-only** social capability from a broader OAuth grant. And he argues
access control and concurrency control "are the same problem — enabling intended causality while
preventing destructive interference" [MIL] — which says permission scopes *and* consumable
quotas belong in **one** governance framework, not two. Axon's warden (§88), authorization
coverage (§89), and capability grantability (§90) are that framework already.

**Provenance:** comments, DMs, and captions read from a network are attacker-controlled text.
Axon already stamps acquired data `Untrusted` at birth (§98/T908) and `Inferred` for derived
facts (§104). A read comment enters cognition *born Untrusted* — so a prompt-injection payload
in a LinkedIn comment cannot launder itself into a trusted instruction that makes the agent post
something. This is information-flow control (JIF/LIO lineage) that the substrate enforces without
`axon-agora` adding anything.

### 4.5 The four-pillar synthesis

| Pillar | Theory | Platform fact it governs | Axon primitive it reuses |
|---|---|---|---|
| **Mathematical** | Session types [HY16]; linear/graded resources [GIR, GRADE] | IG/TikTok multi-step protocols; 100/24h & 15/creator quotas | linear budgets §72; typed flows |
| **Logical** | Deontic static permission & contrary-to-duty [DEON]; declarative power [HOH] | OAuth scopes; delegated account access; breach handling | grants⊆minter §92; `on_breach` §114.w |
| **Philosophical** | Speech-act performatives; FIPA negative result [PITT]; attention economics | publishing as a delegated public act | governed egress §105/§110/§114; attention ledger §110 |
| **Computational** | Object capabilities; permission≠authority [MIL]; IFC | scope attenuation; born-Untrusted reads | warden §88; authz §89; capability §90; `Untrusted` §98 |

---

## V. Architecture (Proposed)

> Everything in §V is **design**, in the conditional. It is grounded in shipping Axon primitives
> (cross-referenced) but no line of it is built.

### 5.1 axon-agora is the first official EMS module

§115 made the Epistemic Module System real: `.axi` interface files, content-addressed caching,
epistemic-compatibility checking, and a faithful linker. `axon-agora` would be **the first
official library distributed as EMS modules** — the EMS's first real consumer beyond the paper's
own two-file example. A developer would write:

```axon
import agora.linkedin.{ read_comments, reply, publish_post, page_metrics }
import agora.instagram.{ publish_media, publishing_budget }
```

and the connector's public surface arrives as a `CognitiveInterface` (§115.b) whose epistemic
floor the ECC (§115.c) checks against the importing program. The connectors' *bodies* — the
session-typed protocol state machines — stay hidden behind the `.axi`, linked faithfully at
build (§115.e). The naming namespace is `agora`; the crate is `axon-agora`.

### 5.2 Shape of a connector: module + native core

The research settles the build question (D116.2): a connector cannot be *pure* Axon source,
because the protocols require real HTTP, TLS, OAuth token exchange, and byte uploads. The design
follows the established **`socket` pattern** — an Axon-language shell over a native (Rust) engine:

- **Axon-source layer (the `.axi` surface):** the session-typed operations (`read_comments`,
  `reply`, `publish_media`, `page_metrics`), their scope requirements as static permissions,
  and their quota costs as linear budget draws. This is what the developer imports and what the
  compiler reasons about.
- **Native Rust core:** the HTTP/OAuth/upload engine, per platform, behind the seam. It performs
  the actual `create-container → poll → publish` dance and the token refresh. It is where the
  §94 secret-custody boundary lives — the token value is held here and *never crosses into
  cognition*.

Each operation is one row of a generated **capability×scope matrix** (from §II's tables): the
connector cannot expose `publish_post` without the program having declared it holds
`w_organization_social`; it cannot expose Instagram `publish_media` without
`instagram_business_content_publish`. This is §89/§90 (authorization coverage + capability
grantability) applied to social scopes.

### 5.3 Credential custody and the token-refresh daemon

Tokens are §94 secrets: the app secret / client_secret and the long-lived access/refresh tokens
live in the native core's custody store, their *values* never entering an Axon expression. The
per-platform refresh mechanics (§2.5) are served by a **§52 daemon**:
- Facebook: chase the never-expiring Page token via the documented `accounts` exchange; refresh
  the ~60-day User token *before* expiry (an expired token cannot be exchanged [FB-TOK]).
- TikTok: daily access-token refresh; **atomically persist the rotated refresh token** [TT-OAUTH]
  (the rotation trap is a real data-loss bug if the write is not atomic — the §115.f atomic-write
  discipline applies).
- LinkedIn member data: **no** unattended refresh — the daemon must *not* refresh member data on
  a schedule [L-TOS]; it surfaces a re-consent requirement instead.

### 5.4 Reads are born Untrusted; writes are governed egress

- **Reads** (`read_comments`, `page_metrics`, mentions) return data born `Untrusted` (§98/T908).
  A comment cannot become a trusted instruction (§4.4).
- **Writes** (`publish_post`, `reply`, `delete`) are governed egress — the §105 `deliver` / §114
  governed-channel lineage. Provenance travels with the act or the act is refused; the published
  content, the target, the scope used, and the consuming budget are all part of the egress
  record. On a platform breach (a post rejected, a scope revoked mid-flow), `on_breach` (§114.w)
  routes to the `BreachSink` — the contrary-to-duty seam (§4.2).

### 5.5 Webhooks as durable event ingestion

Facebook Pages (and the other platforms' event channels) push real-time events [FB-PG]. These
map onto §74 durable event delivery (`emit` → `listen`, at-least-once) and idempotent
webhook-driven ingestion: a new-comment webhook becomes an `emit` the agent's flow can `listen`
for, so the agent reacts to engagement rather than polling — event-driven, not busy-wait.

### 5.6 The multi-tool flow, typed end to end

Returning to §1.1's flow: `read_comments` (born Untrusted) → summarize (cognition) → draft
replies (cognition) → the ads-metrics tool (an ordinary Axon `tool`, §54) → conditional
`publish_post` (governed egress, quota-budgeted, scope-checked). The whole thing is one Axon
flow. The social steps are not special-cased glue; they are typed operations in the same
substrate as every other step — which is the entire point.

---

## VI. Theoretical Guarantees (Design Targets)

If built to §V, `axon-agora` would target these properties — each a *design obligation*, to be
discharged by tests before any such claim ships (§111 doctrine):

1. **Protocol soundness.** A publish call illegal in the current protocol state (e.g.,
   `media_publish` before `FINISHED`) is a **compile-time** type error, by session-typed
   connector generation [HY16]. *(Target; see §VIII.)*
2. **Quota safety.** A flow that would exceed a platform's consumable quota is unrepresentable,
   by linear budgeting [GIR, GRADE] over §72. *(Target.)*
3. **Scope completeness.** No operation is callable without the program statically holding its
   required scope — §89 authorization-coverage extended to social scopes.
4. **Custody.** No token or app-secret *value* ever enters an Axon expression — §94 invariant,
   inherited.
5. **Provenance.** Every read is born `Untrusted`; every write carries provenance or is refused —
   §98 / §105/§114, inherited.
6. **ToS-faithfulness.** The connector exposes *only* operations the platform's official API and
   terms permit in a given posture (e.g., no public TikTok publish from an unaudited app; no
   automated LinkedIn member-data refresh). The capability surface **is** the ToS, encoded.

Guarantees 4–5 are *inherited* from shipping primitives; 1–3 and 6 are the *new* work.

---

## VII. Comparison with Existing Systems

| Property | SDKs | Aggregators | Agent-tool platforms | Unofficial libs | **axon-agora** |
|---|---|---|---|---|---|
| Official-API-only | ✅ | ✅ | mixed | ❌ | **✅ by construction** |
| Protocol order enforced by types | ✗ | ✗ | ✗ | ✗ | **✅ session-typed [HY16]** |
| Quota as consumable type | ✗ | ✗ | ✗ | ✗ | **✅ linear budget §72** |
| Scope as static permission | ✗ | ✗ | partial | ✗ | **✅ §89/§90** |
| Token value out of agent reach | ✗ | hosted | mixed | ✗ | **✅ §94** |
| Reads born Untrusted (anti-injection) | ✗ | ✗ | ✗ | ✗ | **✅ §98/T908** |
| Governed egress w/ provenance | ✗ | ✗ | ✗ | ✗ | **✅ §105/§114** |
| Breach/contrary-to-duty seam | ✗ | ✗ | ✗ | ✗ | **✅ §114.w** |
| Ships as a language module | ✗ | ✗ | ✗ | ✗ | **✅ EMS §115** |

The novel contribution is the **last two columns of every failure story in §III turned into a
type**: the rogue agent had "no rate or scope governance" [OSO] — `axon-agora` makes rate a
linear budget and scope a static permission; the 700-org breach was centralized token custody
[NANGO] — `axon-agora` holds token *values* outside cognition (§94); instagrapi had "no safe
operating mode" [IG-UNOF] — `axon-agora` is official-API-only by construction. No prior system
governs autonomous social action at the language/type-system level; the survey confirms the
category is empty [OSO].

---

## VIII. The Zero-Input Question, Answered

**Can an agent publish and interact on these networks fully unattended?** The honest, per-
platform answer the research yields:

| Platform | Unattended reads/metrics | Unattended publish to **owned** account/page | Blocker |
|---|---|---|---|
| **Facebook Pages** | ✅ | ✅ (review-gated once) | App Review; never-expiring Page token makes steady-state truly unattended [FB-TOK] |
| **Instagram (pro)** | ✅ | ✅ within 100/24h | container protocol + quota; professional account only [IG-CP] |
| **LinkedIn (org page)** | ✅ | ✅ under approved Community Mgmt use case | vetting + screencast; versioned sunsets; **member** automation forbidden [LI-CM, L-TOS] |
| **LinkedIn (member)** | ⚠️ limited | ❌ (general terms forbid automated posting) | API ToS §3.1 [L-TOS]; `r_member_social` closed |
| **TikTok** | ✅ | ⚠️ **not fully** — per-post consent + audit for public | consent "before transmitting"; SELF_ONLY unaudited [TT-CSG] |

**The synthesis:** fully unattended *steady-state* operation is achievable for **owned business
assets** (Facebook Pages, Instagram professional accounts, LinkedIn organization pages under an
approved partner use case) — after a **one-time human authorization** (the OAuth grant, which
LinkedIn's own review script assumes is interactive [LI-REV]). It is **forbidden** for member-
level LinkedIn automation and **constrained** for TikTok public posting. This is *not* a
limitation `axon-agora` should hide — it is the exact boundary the type system should make
legible. The design posture (D116.3): **`axon-agora` is a connector suite for accounts the
tenant OWNS**, publishing where the platform permits unattended publishing and refusing —
loudly, with the reason in the message (§111 posture) — where it does not.

What **no** existing ecosystem provides, and what `axon-agora` would be first to: **the
platform's autonomy conditions, encoded in the type system, so the compiler enforces them
before the agent ever runs** [OSO].

---

## IX. Decisions (D116.x) — Ratified & Open

| # | Decision | Status |
|---|---|---|
| **D116.1** | **Name = `axon-agora`** (namespace `agora`, the public square — the speech-act pillar in the name); `axon-social` and `axon-social-connet` set aside. | ✅ **RATIFIED 2026-07-18** |
| **D116.2** | **Build shape.** Module + native Rust core (the `socket` pattern) — pure-source is impossible (HTTP/OAuth/uploads). | Ratified by research |
| **D116.3** | **Posture.** Connectors for accounts the tenant **OWNS**; publish where unattended publishing is permitted; refuse loudly where it is not (LinkedIn member automation, unaudited TikTok public posts). | ✅ **RATIFIED 2026-07-18** |
| **D116.4** | **Official-API-only, by construction.** No scraping, no unofficial clients — hiQ [HIQ] and instagrapi [IG-UNOF] settle it. | Ratified by research |
| **D116.5** | **OSS/ENT split.** The connector *protocol* layer (session types, scopes, quotas, native cores) is OSS; per-tenant token vaults, refresh daemon, webhook ingress, audit sinks, and OPA connector-policy lean ENT (§94/§92/§106/§114/§82 enterprise seams). | ✅ **RATIFIED 2026-07-18** |
| **D116.6** | **Four to start** (LinkedIn, Facebook Pages, Instagram, TikTok); the connector interface is uniform so YouTube/X/Threads/Bluesky are additive later. | Ratified |
| **D116.7** | **First platforms' order.** Facebook Pages + Instagram first (most unattended-friendly: never-expiring token, clean quota), LinkedIn second (vetting lead time), TikTok last (audit + consent regime is the hardest). | Ratified |

---

## References

**Platform primary sources (fetched & quoted 2026-07):**
- **[LI-CM]** LinkedIn / Microsoft Learn. "Community Management — Overview" (li-lms-2026-06). https://learn.microsoft.com/en-us/linkedin/marketing/community-management/community-management-overview
- **[LI-REV]** LinkedIn / Microsoft Learn. "Community Management — App Review" (li-lms-2025-10). https://learn.microsoft.com/en-us/linkedin/marketing/community-management-app-review
- **[L-TOS]** LinkedIn. "API Terms of Use." https://www.linkedin.com/legal/l/api-terms-of-use
- **[L-UA]** LinkedIn. "Prohibited Software and Extensions" / User Agreement §8. https://www.linkedin.com/help/linkedin/answer/a1341387
- **[FB-PG]** Meta for Developers. "Pages API." https://developers.facebook.com/docs/pages-api/
- **[FB-TOK]** Meta for Developers. "Generate Long-Lived User and Page Access Tokens." https://developers.facebook.com/docs/facebook-login/guides/access-tokens/get-long-lived/
- **[IG-CP]** Meta for Developers. "Instagram Platform — Content Publishing." https://developers.facebook.com/docs/instagram-platform/content-publishing/
- **[TT-CSG]** TikTok for Developers. "Content Sharing Guidelines." https://developers.tiktok.com/doc/content-sharing-guidelines
- **[TT-DP]** TikTok for Developers. "Content Posting API — Get Started." https://developers.tiktok.com/doc/content-posting-api-get-started
- **[TT-OAUTH]** TikTok for Developers. "Manage User Access Tokens." https://developers.tiktok.com/doc/oauth-user-access-token-management

**Legal / case law:**
- **[HIQ]** ZwillGen. "hiQ v. LinkedIn Wrapped Up: Web Scraping Lessons Learned" (2022). https://www.zwillgen.com/alternative-data/hiq-v-linkedin-wrapped-up-web-scraping-lessons-learned/ · Proskauer New Media & Technology Law (Nov 2022). https://newmedialaw.proskauer.com/2022/11/11/

**Ecosystem / failure modes:**
- **[BUF/AYR]** Buffer. "Best Social Media APIs." https://buffer.com/resources/best-social-media-apis/
- **[POSTIZ]** Postiz. "Ayrshare vs Mixpost." https://postiz.com/compare/ayrshare/mixpost
- **[IG-UNOF]** subzeroid/instagrapi Discussion #2224 (deleted; recovered from cache, verified 404 on 2026-07-18). https://github.com/subzeroid/instagrapi
- **[V12]** V12 Labs. "Composio vs. Arcade for AI Agent Tool Authentication" (2026). https://www.v12labs.io/blog/2026-06-16-ai-agent-tool-authentication-composio-arcade
- **[NANGO]** Nango. "A Guide to Secure AI Agent API Authentication." https://nango.dev/blog/guide-to-secure-ai-agent-api-authentication/
- **[OSO]** Oso. "AI Agents Gone Rogue." https://www.osohq.com/developers/ai-agents-gone-rogue

**Academic foundations:**
- **[HY16]** Hu, R., Yoshida, N. (2016). "Hybrid Session Verification through Endpoint API Generation." *FASE 2016*, LNCS 9633. https://doi.org/10.1007/978-3-662-49665-7_24
- **[GIR]** Girard, J.-Y. (1987). "Linear Logic." *Theoretical Computer Science*, 50(1), 1–101.
- **[GRADE]** Graded/coeffect resource-aware type systems (2025). "A resource-aware operational semantics parametric over a grade algebra." arXiv:2507.13792. https://arxiv.org/pdf/2507.13792
- **[DEON]** Gabbay, Horty, Parent, van der Meyden, van der Torre (eds.). *Handbook of Deontic Logic and Normative Systems* (HNMAS chapter). https://icr.uni.lu/leonvandertorre/papers/HNMAS17.pdf
- **[HOH]** Gelati, J., Governatori, G., Rotolo, A., Sartor, G. (2004). "Declarative Power, Representation, and Mandate." *Artificial Intelligence and Law*. https://doi.org/10.1007/s10506-004-1922-2 (Hohfeld, W. N. (1913). "Fundamental Legal Conceptions." *Yale Law Journal*.)
- **[PITT]** Pitt, J., Mamdani, A. (1999). "Some Remarks on the Semantics of FIPA's Agent Communication Language." *Autonomous Agents and Multi-Agent Systems*. https://doi.org/10.1023/A:1010016503852
- **[MIL]** Miller, M. S. (2006). "Robust Composition: Towards a Unified Approach to Access Control and Concurrency Control." PhD thesis, Johns Hopkins University.
- Austin, J. L. (1962). *How to Do Things with Words.* · Searle, J. R. (1969). *Speech Acts.* · Simon, H. A. (1971). "Designing Organizations for an Information-Rich World."

---

*§Fase 116 · research foundation complete · all doctrine forks ratified (D116.1/D116.3/D116.5, 2026-07-18) · living plan at `axon-enterprise/docs/fase/fase_116_axon_agora.md` · implementation begun at §116.a.*
