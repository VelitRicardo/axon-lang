//! The knowledge base catalogue.
//!
//! All documentation lives as markdown with YAML frontmatter under
//! `src/axon-emcp/knowledge/` (inside the crate so `cargo publish`
//! ships the corpus as part of the binary tarball). This module:
//!
//! 1. Discovers the corpus root (dev mode, installed binary, or
//!    `AXON_EMCP_KNOWLEDGE_DIR` override — in this order).
//! 2. Parses every `primitives/*.md` into a [`Primitive`] entry.
//! 3. Indexes them by name so [`Catalog::primitive(name)`] is O(1).
//!
//! The corpus is **the source of truth**: edits land as markdown
//! diffs, the server reloads them at startup. There is no compiled
//! catalog and no generation step. A primitive added to
//! `src/knowledge/primitives/foo.md` is automatically visible to the
//! MCP layer the next time the server starts.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use gray_matter::engine::YAML;
use gray_matter::Matter;
use include_dir::{include_dir, Dir};
use serde::Deserialize;

/// The knowledge corpus compiled into the binary at build time.
///
/// `cargo install --path src/axon-emcp` ships this baked-in copy so
/// the installed server runs without any filesystem dependency. At
/// runtime [`Catalog::load_default`] still prefers (a) the
/// `AXON_EMCP_KNOWLEDGE_DIR` env override or (b) the in-tree dev path
/// (both let operators hot-edit `.md` files without rebuilding); the
/// embedded copy is the (c) fallback that always works.
static EMBEDDED_KNOWLEDGE: Dir<'_> =
    include_dir!("$CARGO_MANIFEST_DIR/knowledge");

/// One primitive's full documentation, parsed from its markdown source.
///
/// The markdown frontmatter carries the structured fields the MCP layer
/// projects directly (`top_level`, `category`, `grammar`); the body
/// after the frontmatter is the prose reference the agent reads when
/// it asks for the full doc.
/// What the runtime actually does with this primitive, **derived from the
/// compiler's ledger** rather than written down here.
///
/// # Why derived and not a frontmatter field
///
/// The obvious design is a `status:` line in each document's frontmatter. It is
/// also a COPY of `axon-frontend`'s `ADVERTISED` table, and this project has
/// paid repeatedly for exactly that shape: two places holding one fact, the
/// second drifting silently because nothing forces it to move. A corpus doc
/// claiming `attested` over a row the ledger calls `Unwired` would be worse
/// than no status at all — it would launder the debt through the one surface an
/// agent trusts.
///
/// So there is no second copy. `axon-emcp` already depends on `axon-frontend`;
/// it reads `advertised::status_of` at load time, and a reclassification in the
/// compiler reaches the agent on the next build with nothing to update.
///
/// `None` means the name has no ledger row at all — for a primitive that is
/// itself a finding, and `axon.primitive_doc` says so rather than staying quiet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeReality {
    /// A fixture written in AXON is compiled and executed by a gate.
    Attested,
    /// Delivered, with a gap the ledger names.
    Partial,
    /// **The engine exists, is tested, and no production path reaches it.**
    Unwired,
    /// Advertised and not delivered.
    NotImplemented,
    /// Refuses — at compile time or at dispatch — rather than approximating.
    FailsClosed,
    /// Advertised on trust: no gate has verified it.
    Unaudited,
}

impl RuntimeReality {
    fn of(name: &str) -> Option<Self> {
        use axon_frontend::advertised::RuntimeStatus as S;
        Some(match axon_frontend::advertised::status_of(name)? {
            S::Real { .. } | S::Attested { .. } => Self::Attested,
            S::Partial { .. } => Self::Partial,
            S::Unwired { .. } => Self::Unwired,
            S::NotImplemented { .. } => Self::NotImplemented,
            S::FailsClosed { .. } => Self::FailsClosed,
            S::Unaudited => Self::Unaudited,
        })
    }

    /// The machine-readable slug returned to the agent.
    pub fn slug(self) -> &'static str {
        match self {
            Self::Attested => "attested",
            Self::Partial => "partial",
            Self::Unwired => "unwired",
            Self::NotImplemented => "not_implemented",
            Self::FailsClosed => "fails_closed",
            Self::Unaudited => "unaudited",
        }
    }

    /// Does a program that uses this primitive today actually get the behaviour
    /// the document describes?
    ///
    /// `false` for `Unwired` and `NotImplemented` — to an adopter those two are
    /// indistinguishable: the primitive does not do what the summary says, and
    /// nothing tells them. That is the whole reason this field exists.
    pub fn is_deliverable(self) -> bool {
        !matches!(self, Self::Unwired | Self::NotImplemented)
    }

    /// The sentence an agent must read BEFORE the document body, for the states
    /// where writing the code would be a mistake. Empty where there is nothing
    /// to warn about.
    pub fn warning(self) -> &'static str {
        match self {
            Self::Unwired => {
                "⚠️ UNWIRED — the engine exists and is tested, and NO production path \
                 reaches it. A program using this compiles and does not do what the text \
                 below describes. Do not generate code that relies on it; say so if asked."
            }
            Self::NotImplemented => {
                "⚠️ NOT IMPLEMENTED — advertised and not delivered, recorded as debt in the \
                 compiler's own ledger. Do not generate code that relies on it; say so if asked."
            }
            Self::Partial => {
                "⚠️ PARTIAL — the runtime delivers this with a documented gap against what \
                 the reference describes. Check the gap before relying on the edge cases."
            }
            Self::Unaudited => {
                "ℹ️ UNAUDITED — advertised on trust: no gate has verified that the runtime \
                 does what this page says. Treat it as documentation, not as a guarantee."
            }
            Self::FailsClosed | Self::Attested => "",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Primitive {
    /// Canonical name as it appears in source (`persona`, `flow`,
    /// `socket`, `axonendpoint`, …). Used as the dictionary key.
    pub name: String,
    /// One-line summary the agent sees in the listing.
    pub summary: String,
    /// Which family the primitive belongs to. Drives the
    /// `axon.primitives(filter)` category facet.
    pub category: Category,
    /// `true` ⇒ this primitive is a top-level declaration (it stands
    /// alone at the program root). `false` ⇒ it only appears nested
    /// inside another construct (e.g. `step` inside a `flow`).
    pub top_level: bool,
    /// The EBNF fragment for this primitive (extract from the official
    /// grammar). Empty string is allowed for primitives whose grammar
    /// is delegated to a parent construct.
    pub grammar: String,
    /// The cycle that introduced this primitive (e.g. `"v1.1.0"`,
    /// `"v2.3.0"`). Lets the agent answer "since when has this
    /// existed?" honestly.
    pub since: String,
    /// What the runtime actually does with this primitive, read from the
    /// compiler ledger at load time. `None` = no ledger row, which is itself
    /// a finding and is reported as one.
    pub reality: Option<RuntimeReality>,
    /// The prose body — the full markdown that follows the frontmatter.
    /// Returned verbatim by `axon.primitive_doc(name)`.
    pub body: String,
}

/// The primitive families an agent can filter by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// `persona`, `flow`, `reason`, `anchor`, `tool`, `probe`, `weave`,
    /// `validate`, `context`, `memory`, `intent`, `refine`. The
    /// "what an LLM does" layer.
    Cognition,
    /// Cognitive I/O — `resource`, `fabric`, `manifest`, `observe`,
    /// `reconcile`, `lease`, `ensemble`, `topology`, `session`,
    /// `immune`, `reflex`, `heal`, `compliance`, `component`, `view`.
    CognitiveIo,
    /// `axonstore`, `dataspace`, `corpus`, `pix`, the four-pillar
    /// persistence layer.
    DataPlane,
    /// `socket`, `session` choice grammar, the v2.3.0 session-type
    /// algebra surface (post-v2.3.0).
    SessionTypes,
    /// `daemon`, `listen`, `axonendpoint`, `axpoint`, `mcp`, `taint`,
    /// the actor / wire surface.
    Wire,
    /// `shield`, `mandate`, `lambda`, `forge`, `agent`, `ots`, `psyche`,
    /// `compute`, `logic`. Specialised cognitive operators.
    Operators,
}

impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Category::Cognition => "cognition",
            Category::CognitiveIo => "cognitive_io",
            Category::DataPlane => "data_plane",
            Category::SessionTypes => "session_types",
            Category::Wire => "wire",
            Category::Operators => "operators",
        }
    }
}

/// The frontmatter shape we expect on every `primitives/*.md`. A file
/// missing a required field is a hard error (the loader refuses to
/// start the server) — the agent should never see partial entries.
#[derive(Debug, Deserialize)]
struct Frontmatter {
    name: String,
    summary: String,
    category: Category,
    top_level: bool,
    #[serde(default)]
    grammar: String,
    #[serde(default)]
    since: String,
}

/// One reference document — the citation-ready prose the agent reads
/// when it wants to quote a fact from the AXON manual. `kind` controls
/// where the document lives on disk and which axon:// URI exposes it
/// (see [`ReferenceKind`]).
///
/// Phase 3 ships three kinds: **grammar** (top-level vs. nested
/// polarity, composition rules, EBNF), **logic** (when-to-use-what
/// reasoning), and **compliance** (per-framework annotation maps).
#[derive(Debug, Clone)]
pub struct Reference {
    /// Which family this document belongs to. Drives both the
    /// on-disk directory and the served `axon://<kind>/<slug>` URI.
    pub kind: ReferenceKind,
    /// The URL slug — the file's base name without `.md`. Matches the
    /// frontmatter `name:` field; the loader rejects mismatches.
    pub slug: String,
    /// One-line summary. Surfaces in the `resources/list` MCP payload
    /// so the agent can pick which document to read without fetching
    /// the body first.
    pub summary: String,
    /// Human-readable title rendered above the markdown body when the
    /// agent reads the resource. Free-form (no enum constraint).
    pub title: String,
    /// The prose body — the markdown that follows the frontmatter,
    /// returned verbatim by `resources/read`.
    pub body: String,
}

/// The reference-document family. Each kind maps to a directory under
/// `src/knowledge/` and to an `axon://<kind>/<slug>` URI namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceKind {
    /// `axon://grammar/<slug>` — language-level rules: top-level vs.
    /// nested polarity, composition + nesting rules, EBNF.
    Grammar,
    /// `axon://logic/<slug>` — when-to-use-what reasoning, idioms,
    /// session-duality algebra, composition heuristics.
    Logic,
    /// `axon://compliance/<framework>` — per-framework annotation
    /// maps (HIPAA, GDPR, PCI_DSS, SOC2, SOX, GxP, FISMA, NIST_800_53).
    Compliance,
}

impl ReferenceKind {
    /// The URI segment + directory name (matches the `serde` rename).
    pub fn as_str(self) -> &'static str {
        match self {
            ReferenceKind::Grammar => "grammar",
            ReferenceKind::Logic => "logic",
            ReferenceKind::Compliance => "compliance",
        }
    }
    /// Every kind in stable order — used by `resources/list` to walk
    /// the catalog deterministically.
    pub const fn all() -> &'static [ReferenceKind] {
        &[
            ReferenceKind::Grammar,
            ReferenceKind::Logic,
            ReferenceKind::Compliance,
        ]
    }
}

/// Frontmatter shape for reference documents. Smaller than the
/// primitive frontmatter: no category, no top-level polarity, no
/// grammar fragment — those concepts only apply to primitives.
#[derive(Debug, Deserialize)]
struct ReferenceFrontmatter {
    name: String,
    summary: String,
    title: String,
}

/// One domain scaffold template — the raw `.axon` source the
/// `axon.compose` tool returns to an agent that asked for a typed
/// starter program. Each template is hand-authored and proven to
/// compile end-to-end through the same `axon-frontend` pipeline
/// `axon.check` uses (see `tests/phase4_templates_compile.rs`).
///
/// Phase 4 ships 8 templates: `generic`, `healthcare`, `banking`,
/// `government`, `legal`, `chat`, `retrieval`, `multi_agent`.
#[derive(Debug, Clone)]
pub struct Template {
    /// File stem — `generic`, `healthcare`, …. Doubles as the URI
    /// slug and the `axon.compose` lookup key.
    pub slug: String,
    /// The raw `.axon` source returned to the caller verbatim.
    pub source: String,
}

/// One focused, idiomatic example program — the corpus surface
/// `axon.examples` returns. Where [`Template`] answers *"give me a
/// starting point for a healthcare service"*, an [`Example`] answers
/// *"show me how to use `weave` correctly"* or *"what does an
/// idempotent endpoint look like"*. Each example is a minimal complete
/// program (~20–60 LOC) demonstrating **one** idea, and every example
/// is drift-gated through `axon-frontend` (see
/// `tests/phase9_examples_compile.rs`).
///
/// On disk: `src/axon-emcp/knowledge/examples/<slug>.md` — YAML
/// frontmatter (metadata) followed by pure AXON source (the body).
/// We use `.md` (not `.axon`) so the frontmatter doesn't have to be
/// embedded in comments; the loader splits the two cleanly.
#[derive(Debug, Clone)]
pub struct Example {
    /// Slug — matches the file stem AND the frontmatter `name:`.
    /// Doubles as the `axon.examples` lookup key.
    pub name: String,
    /// Human-readable title (one phrase, no trailing period).
    pub title: String,
    /// One-line summary — what idea this example demonstrates.
    pub summary: String,
    /// Closed topic taxonomy — `axon.examples(topic: ...)` filters by
    /// this field. The closed catalog lives in [`ExampleTopic`].
    pub topic: ExampleTopic,
    /// The primitive names this example exercises idiomatically. Used
    /// by `axon.examples(primitive: "weave")` to find every example
    /// that demonstrates a given primitive. Strings (not the typed
    /// [`Primitive`] reference) so the field is independent of corpus
    /// load ordering — the runtime resolves the link if needed.
    pub primitives: Vec<String>,
    /// The raw `.axon` source — the body after the frontmatter,
    /// returned verbatim to the caller. Drift-gated through
    /// `axon-frontend` so the agent never receives a broken example.
    pub source: String,
}

/// The closed taxonomy `axon.examples(topic: ...)` exposes. The 10
/// entries cover the conceptual axes an agent reasons along when
/// asking *"show me how to do X"*. New topics MUST land here + in
/// [`ExampleTopic::all`] + in the `axon.examples` tool input-schema
/// enum — the three sides of the drift gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ExampleTopic {
    /// Composition idioms — how primitives plug into one another
    /// (`flow` + `step` + `tool`, `weave`, multi-step chaining).
    Composition,
    /// Session-types showcase — `session` duality, `socket` binding,
    /// `select`/`branch`/`loop`/`end` algebra.
    SessionTypes,
    /// Defensive composition — `shield`, `mandate`, breach
    /// policies, sanitisation patterns.
    Shields,
    /// Algebraic effects — `lambda`, `apply`, `effects: <...>`
    /// declarations, higher-order flow composition.
    Effects,
    /// `Stream<T>` end-to-end — flows that emit streams, SSE
    /// transport, per-step backpressure policies.
    Streaming,
    /// Persistence — `axonstore`, `dataspace`, `pix`, `corpus`,
    /// `transact` writes, four-pillar data plane.
    Data,
    /// Multi-agent + lifecycle — `ensemble`, `forge`, `reflex`,
    /// `heal`, `agent` cooperation patterns.
    Agents,
    /// HTTP REST + actor wire — `axonendpoint`, `mcp`, `listen`,
    /// `daemon`, `axpoint`.
    Endpoints,
    /// Memory primitives — scope catalog (`ephemeral`, `session`,
    /// `persistent`, `none`), lifecycle constraints.
    Memory,
    /// Type-driven safety — `anchor` invariants, `psyche` constraints,
    /// `validate` predicates, refinement types in practice.
    Validation,
}

impl ExampleTopic {
    /// The URI / JSON-Schema slug — matches the `serde` rename.
    pub fn as_str(self) -> &'static str {
        match self {
            ExampleTopic::Composition => "composition",
            ExampleTopic::SessionTypes => "session_types",
            ExampleTopic::Shields => "shields",
            ExampleTopic::Effects => "effects",
            ExampleTopic::Streaming => "streaming",
            ExampleTopic::Data => "data",
            ExampleTopic::Agents => "agents",
            ExampleTopic::Endpoints => "endpoints",
            ExampleTopic::Memory => "memory",
            ExampleTopic::Validation => "validation",
        }
    }
    /// Every topic in stable order — used by `axon.examples` to
    /// iterate deterministically and by the input-schema enum to
    /// stay in sync with the closed catalog.
    pub const fn all() -> &'static [ExampleTopic] {
        &[
            ExampleTopic::Composition,
            ExampleTopic::SessionTypes,
            ExampleTopic::Shields,
            ExampleTopic::Effects,
            ExampleTopic::Streaming,
            ExampleTopic::Data,
            ExampleTopic::Agents,
            ExampleTopic::Endpoints,
            ExampleTopic::Memory,
            ExampleTopic::Validation,
        ]
    }
    /// Parse a topic slug — case-insensitive, hyphen/underscore
    /// tolerant. `None` if no match.
    pub fn parse(s: &str) -> Option<Self> {
        let norm = s.trim().to_ascii_lowercase().replace('-', "_");
        ExampleTopic::all()
            .iter()
            .copied()
            .find(|t| t.as_str() == norm)
    }
}

/// Frontmatter shape for an [`Example`]. The body following the
/// frontmatter block is the raw AXON source (no further parsing — the
/// drift gate compiles it through `axon-frontend`).
#[derive(Debug, Deserialize)]
struct ExampleFrontmatter {
    name: String,
    title: String,
    summary: String,
    topic: ExampleTopic,
    #[serde(default)]
    primitives: Vec<String>,
}

/// One MCP **prompt** — a parameterized prompt template the host
/// (Claude Code / Cursor / Continue / …) surfaces to the human user
/// as a named recipe. When the user picks it, the host calls
/// `prompts/get` with the user's argument values; we render the body
/// with `{{arg}}` substitution and return the resulting message.
///
/// Phase 5 ships three: `flow_design`, `shield_design`,
/// `session_design`.
#[derive(Debug, Clone)]
pub struct Prompt {
    /// Slug — matches the file stem AND the frontmatter `name:`.
    /// Doubles as the `prompts/get` lookup key.
    pub name: String,
    /// Human-readable title surfaced in `prompts/list`.
    pub title: String,
    /// One-line summary for `prompts/list` discovery.
    pub summary: String,
    /// Declared arguments in declaration order. Each `{{name}}` in
    /// the body is substituted from this list at render time.
    pub arguments: Vec<PromptArgument>,
    /// Markdown body with `{{name}}` placeholders. Returned to the
    /// host (typically rendered into a single user-role message).
    pub body: String,
}

/// One declared argument of a [`Prompt`]. The MCP spec requires every
/// prompt advertise its argument schema so the host can surface a
/// form-style picker; this struct mirrors that schema (the
/// `Serialize` impl emits exactly the wire shape `prompts/list`
/// expects).
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct PromptArgument {
    /// Argument identifier — also the placeholder name in the body.
    pub name: String,
    /// Human-readable description shown to the user when the host
    /// renders the picker.
    pub description: String,
    /// `true` ⇒ the host must collect a value before calling
    /// `prompts/get`. `false` ⇒ optional; the renderer substitutes
    /// the literal "(unspecified)" when omitted.
    #[serde(default)]
    pub required: bool,
}

/// Frontmatter shape on `prompts/*.md`. The body markdown follows
/// (verbatim, with `{{arg}}` placeholders to render later).
#[derive(Debug, Deserialize)]
struct PromptFrontmatter {
    name: String,
    title: String,
    summary: String,
    #[serde(default)]
    arguments: Vec<PromptArgument>,
}

/// The in-process knowledge catalogue. Built once at startup and held
/// behind an `Arc` for cheap clone across the async dispatcher.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    primitives: BTreeMap<String, Primitive>,
    /// Reference docs keyed by `(kind, slug)`. The `BTreeMap` gives us
    /// deterministic iteration so `resources/list` ordering does not
    /// drift across runs.
    references: BTreeMap<(ReferenceKind, String), Reference>,
    /// `axon.compose` scaffold templates keyed by slug. The body is
    /// the raw `.axon` source — `axon.compose` returns it after
    /// confirming it still parses through the live `axon-frontend`
    /// pipeline (the verification round-trips through
    /// `compiler_pipeline::run`).
    templates: BTreeMap<String, Template>,
    /// Phase 5 — MCP prompts keyed by `name`. Surfaced via
    /// `prompts/list` + `prompts/get` so the host can offer them
    /// as slash-commands or chat-menu entries.
    prompts: BTreeMap<String, Prompt>,
    /// Phase 9 — `axon.examples` corpus keyed by slug. The body of
    /// every entry is pure AXON source that compiles end-to-end
    /// through `axon-frontend` (see `tests/phase9_examples_compile.rs`
    /// — the drift gate that runs on every `cargo test`).
    examples: BTreeMap<String, Example>,
}

impl Catalog {
    /// Discover + load the corpus. Resolution order (first hit wins):
    ///
    /// 1. **`AXON_EMCP_KNOWLEDGE_DIR`** env var — operator override
    ///    (lets ops pin the corpus to an explicit path, e.g. for a
    ///    custom on-prem distribution).
    /// 2. **In-tree dev path** (`<crate>/../knowledge`) — present when
    ///    running `cargo run` from inside the repo; lets contributors
    ///    hot-edit `.md` files without rebuilding the binary.
    /// 3. **Embedded corpus** ([`EMBEDDED_KNOWLEDGE`]) — baked into the
    ///    binary at compile time by `include_dir!`. Always present.
    ///    This is what a `cargo install`-ed user gets.
    ///
    /// The function only fails if a higher-priority location was
    /// requested explicitly (via the env var) and turned out to be
    /// invalid — in that case we surface the problem rather than
    /// silently falling through to the embedded copy. An unset env
    /// var + missing dev path just transparently uses the embedded
    /// copy.
    pub fn load_default() -> Result<Self, LoadError> {
        // (1) explicit env override — always wins, and if it points
        // somewhere that does not parse, that's a hard error: the
        // operator asked us to use that corpus.
        if let Ok(env) = std::env::var("AXON_EMCP_KNOWLEDGE_DIR") {
            let path = PathBuf::from(env);
            if !path.is_dir() {
                return Err(LoadError::MissingDir(path));
            }
            return Self::load_from(&path);
        }
        // (2) in-tree dev path — `<crate>/knowledge`. Present when
        // the binary runs from the source tree (corpus lives inside
        // the crate so it ships with `cargo publish`). After
        // `cargo install` the embedded copy below is the canonical
        // source; the in-tree path lets contributors hot-edit `.md`
        // files without rebuilding.
        let dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("knowledge");
        if dev_path.is_dir() {
            return Self::load_from(&dev_path);
        }
        // (3) embedded fallback — the corpus baked into the binary by
        // `include_dir!` at compile time. This is what users of a
        // `cargo install`-ed binary will hit by default.
        Self::load_embedded()
    }

    /// Load the catalog from the compile-time-embedded corpus. Used by
    /// `load_default` as the final fallback; also exposed directly so
    /// tests can exercise the embedded path without the env-var dance.
    pub fn load_embedded() -> Result<Self, LoadError> {
        let prims_dir = EMBEDDED_KNOWLEDGE.get_dir("primitives").ok_or_else(|| {
            LoadError::MissingDir(PathBuf::from("(embedded)/primitives"))
        })?;
        let mut primitives = BTreeMap::new();
        for file in prims_dir.files() {
            if !is_markdown(file.path()) {
                continue;
            }
            let raw = file.contents_utf8().ok_or_else(|| {
                LoadError::BadFrontmatter(
                    PathBuf::from(file.path()),
                    "embedded file is not valid UTF-8".into(),
                )
            })?;
            let prim = parse_primitive(raw, file.path())?;
            if primitives.insert(prim.name.clone(), prim.clone()).is_some() {
                return Err(LoadError::DuplicateName(
                    prim.name,
                    PathBuf::from(file.path()),
                ));
            }
        }
        // Phase 3 — load reference docs (grammar/, logic/, compliance/).
        // Each kind is OPTIONAL at the corpus root: a stripped-down
        // distribution that ships only `primitives/` still loads. The
        // server logs a debug-level note when a kind is absent so
        // operators can tell the loader did not silently swallow files.
        let mut references = BTreeMap::new();
        for kind in ReferenceKind::all() {
            let Some(dir) = EMBEDDED_KNOWLEDGE.get_dir(kind.as_str()) else {
                continue;
            };
            for file in dir.files() {
                if !is_markdown(file.path()) {
                    continue;
                }
                let raw = file.contents_utf8().ok_or_else(|| {
                    LoadError::BadFrontmatter(
                        PathBuf::from(file.path()),
                        "embedded file is not valid UTF-8".into(),
                    )
                })?;
                let refr = parse_reference(raw, file.path(), *kind)?;
                let key = (refr.kind, refr.slug.clone());
                if references.insert(key, refr.clone()).is_some() {
                    return Err(LoadError::DuplicateName(
                        format!("{}/{}", refr.kind.as_str(), refr.slug),
                        PathBuf::from(file.path()),
                    ));
                }
            }
        }
        // Phase 4 — load scaffold templates from `templates/*.axon`.
        // Templates carry no frontmatter; the file stem IS the slug
        // and the entire file is the AXON source. The directory is
        // optional (catalogs without templates still load).
        let mut templates = BTreeMap::new();
        if let Some(dir) = EMBEDDED_KNOWLEDGE.get_dir("templates") {
            for file in dir.files() {
                if !is_axon(file.path()) {
                    continue;
                }
                let slug = match file.path().file_stem().and_then(|s| s.to_str()) {
                    Some(s) => s.to_string(),
                    None => continue,
                };
                let source = file.contents_utf8().ok_or_else(|| {
                    LoadError::BadFrontmatter(
                        PathBuf::from(file.path()),
                        "embedded template is not valid UTF-8".into(),
                    )
                })?;
                let tpl = Template { slug: slug.clone(), source: source.to_string() };
                if templates.insert(slug.clone(), tpl).is_some() {
                    return Err(LoadError::DuplicateName(
                        slug,
                        PathBuf::from(file.path()),
                    ));
                }
            }
        }
        // Phase 5 — load MCP prompts from `prompts/*.md`.
        let mut prompts = BTreeMap::new();
        if let Some(dir) = EMBEDDED_KNOWLEDGE.get_dir("prompts") {
            for file in dir.files() {
                if !is_markdown(file.path()) {
                    continue;
                }
                let raw = file.contents_utf8().ok_or_else(|| {
                    LoadError::BadFrontmatter(
                        PathBuf::from(file.path()),
                        "embedded prompt is not valid UTF-8".into(),
                    )
                })?;
                let p = parse_prompt(raw, file.path())?;
                if prompts.insert(p.name.clone(), p.clone()).is_some() {
                    return Err(LoadError::DuplicateName(
                        p.name,
                        PathBuf::from(file.path()),
                    ));
                }
            }
        }
        // Phase 9 — load examples from `examples/*.md`. Each file
        // carries YAML frontmatter (metadata) followed by raw AXON
        // source (the body). The directory is optional — catalogs
        // without examples still load.
        let mut examples = BTreeMap::new();
        if let Some(dir) = EMBEDDED_KNOWLEDGE.get_dir("examples") {
            for file in dir.files() {
                if !is_markdown(file.path()) {
                    continue;
                }
                let raw = file.contents_utf8().ok_or_else(|| {
                    LoadError::BadFrontmatter(
                        PathBuf::from(file.path()),
                        "embedded example is not valid UTF-8".into(),
                    )
                })?;
                let ex = parse_example(raw, file.path())?;
                if examples.insert(ex.name.clone(), ex.clone()).is_some() {
                    return Err(LoadError::DuplicateName(
                        ex.name,
                        PathBuf::from(file.path()),
                    ));
                }
            }
        }
        Ok(Catalog { primitives, references, templates, prompts, examples })
    }

    /// Load from an explicit path. Public so tests + tools can drive
    /// the loader without touching the filesystem env.
    pub fn load_from(root: &Path) -> Result<Self, LoadError> {
        let primitives_dir = root.join("primitives");
        if !primitives_dir.is_dir() {
            return Err(LoadError::MissingDir(primitives_dir));
        }
        let mut primitives = BTreeMap::new();
        for entry in std::fs::read_dir(&primitives_dir)
            .map_err(|e| LoadError::Io(primitives_dir.clone(), e))?
        {
            let entry = entry.map_err(|e| LoadError::Io(primitives_dir.clone(), e))?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            let raw = std::fs::read_to_string(&path)
                .map_err(|e| LoadError::Io(path.clone(), e))?;
            let prim = parse_primitive(&raw, &path)?;
            if primitives.insert(prim.name.clone(), prim.clone()).is_some() {
                return Err(LoadError::DuplicateName(prim.name, path));
            }
        }
        // Phase 3 — reference docs live in sibling directories.
        // Each is optional; if `grammar/` does not exist the catalog
        // simply has no grammar references (a minimal corpus with
        // only `primitives/` still loads).
        let mut references = BTreeMap::new();
        for kind in ReferenceKind::all() {
            let dir = root.join(kind.as_str());
            if !dir.is_dir() {
                continue;
            }
            for entry in std::fs::read_dir(&dir).map_err(|e| LoadError::Io(dir.clone(), e))? {
                let entry = entry.map_err(|e| LoadError::Io(dir.clone(), e))?;
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) != Some("md") {
                    continue;
                }
                let raw = std::fs::read_to_string(&path)
                    .map_err(|e| LoadError::Io(path.clone(), e))?;
                let refr = parse_reference(&raw, &path, *kind)?;
                let key = (refr.kind, refr.slug.clone());
                if references.insert(key, refr.clone()).is_some() {
                    return Err(LoadError::DuplicateName(
                        format!("{}/{}", refr.kind.as_str(), refr.slug),
                        path,
                    ));
                }
            }
        }
        // Phase 4 — templates directory (optional).
        let mut templates = BTreeMap::new();
        let tpl_dir = root.join("templates");
        if tpl_dir.is_dir() {
            for entry in std::fs::read_dir(&tpl_dir).map_err(|e| LoadError::Io(tpl_dir.clone(), e))? {
                let entry = entry.map_err(|e| LoadError::Io(tpl_dir.clone(), e))?;
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) != Some("axon") {
                    continue;
                }
                let slug = match path.file_stem().and_then(|s| s.to_str()) {
                    Some(s) => s.to_string(),
                    None => continue,
                };
                let source = std::fs::read_to_string(&path)
                    .map_err(|e| LoadError::Io(path.clone(), e))?;
                let tpl = Template { slug: slug.clone(), source };
                if templates.insert(slug.clone(), tpl).is_some() {
                    return Err(LoadError::DuplicateName(slug, path));
                }
            }
        }
        // Phase 5 — prompts directory (optional).
        let mut prompts = BTreeMap::new();
        let pr_dir = root.join("prompts");
        if pr_dir.is_dir() {
            for entry in std::fs::read_dir(&pr_dir).map_err(|e| LoadError::Io(pr_dir.clone(), e))? {
                let entry = entry.map_err(|e| LoadError::Io(pr_dir.clone(), e))?;
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) != Some("md") {
                    continue;
                }
                let raw = std::fs::read_to_string(&path)
                    .map_err(|e| LoadError::Io(path.clone(), e))?;
                let p = parse_prompt(&raw, &path)?;
                if prompts.insert(p.name.clone(), p.clone()).is_some() {
                    return Err(LoadError::DuplicateName(p.name, path));
                }
            }
        }
        // Phase 9 — examples directory (optional). Same shape as
        // the embedded-path loader above: frontmatter + raw AXON body.
        let mut examples = BTreeMap::new();
        let ex_dir = root.join("examples");
        if ex_dir.is_dir() {
            for entry in std::fs::read_dir(&ex_dir).map_err(|e| LoadError::Io(ex_dir.clone(), e))? {
                let entry = entry.map_err(|e| LoadError::Io(ex_dir.clone(), e))?;
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) != Some("md") {
                    continue;
                }
                let raw = std::fs::read_to_string(&path)
                    .map_err(|e| LoadError::Io(path.clone(), e))?;
                let ex = parse_example(&raw, &path)?;
                if examples.insert(ex.name.clone(), ex.clone()).is_some() {
                    return Err(LoadError::DuplicateName(ex.name, path));
                }
            }
        }
        Ok(Catalog { primitives, references, templates, prompts, examples })
    }

    /// Empty catalog — used only by unit tests that exercise the
    /// server's wire shape without needing the corpus.
    pub fn empty_for_tests() -> Self {
        Catalog::default()
    }

    pub fn primitive_count(&self) -> usize {
        self.primitives.len()
    }

    /// Lookup one primitive by canonical name. `None` if absent.
    pub fn primitive(&self, name: &str) -> Option<&Primitive> {
        self.primitives.get(name)
    }

    /// Iterate every primitive (in BTreeMap order — alphabetical, stable).
    pub fn primitives(&self) -> impl Iterator<Item = &Primitive> {
        self.primitives.values()
    }

    /// Total reference-doc count across every [`ReferenceKind`].
    pub fn reference_count(&self) -> usize {
        self.references.len()
    }

    /// Reference-doc count for one specific kind. O(n) over the
    /// references map, but n is small (tens, not thousands).
    pub fn reference_count_of(&self, kind: ReferenceKind) -> usize {
        self.references.keys().filter(|(k, _)| *k == kind).count()
    }

    /// Lookup one reference doc by `(kind, slug)`. `None` if absent.
    pub fn reference(&self, kind: ReferenceKind, slug: &str) -> Option<&Reference> {
        self.references.get(&(kind, slug.to_string()))
    }

    /// Iterate every reference doc (in BTreeMap order — `(kind, slug)`
    /// ascending). Stable across runs so `resources/list` is too.
    pub fn references(&self) -> impl Iterator<Item = &Reference> {
        self.references.values()
    }

    /// Iterate every reference doc of one kind (in slug order).
    pub fn references_of(&self, kind: ReferenceKind) -> impl Iterator<Item = &Reference> {
        self.references
            .iter()
            .filter(move |((k, _), _)| *k == kind)
            .map(|(_, v)| v)
    }

    /// Total template count across every domain slug.
    pub fn template_count(&self) -> usize {
        self.templates.len()
    }

    /// Lookup one template by slug. `None` if absent.
    pub fn template(&self, slug: &str) -> Option<&Template> {
        self.templates.get(slug)
    }

    /// Iterate every template (in BTreeMap order — alphabetical, stable).
    pub fn templates(&self) -> impl Iterator<Item = &Template> {
        self.templates.values()
    }

    /// Phase 5 — total prompt count.
    pub fn prompt_count(&self) -> usize {
        self.prompts.len()
    }

    /// Phase 5 — lookup one prompt by name. `None` if absent.
    pub fn prompt(&self, name: &str) -> Option<&Prompt> {
        self.prompts.get(name)
    }

    /// Phase 5 — iterate every prompt (in BTreeMap order — alphabetical,
    /// stable). Used by `prompts/list` so the host's render of the
    /// available recipes does not jitter across runs.
    pub fn prompts(&self) -> impl Iterator<Item = &Prompt> {
        self.prompts.values()
    }

    /// Phase 9 — total example count across every topic.
    pub fn example_count(&self) -> usize {
        self.examples.len()
    }

    /// Phase 9 — lookup one example by slug. `None` if absent.
    pub fn example(&self, name: &str) -> Option<&Example> {
        self.examples.get(name)
    }

    /// Phase 9 — iterate every example (in BTreeMap order — alphabetical,
    /// stable). `axon.examples` walks this for the unfiltered listing.
    pub fn examples(&self) -> impl Iterator<Item = &Example> {
        self.examples.values()
    }

    /// Phase 9 — iterate every example of one topic (in slug order).
    /// Used by `axon.examples(topic: ...)` filtering.
    pub fn examples_of(&self, topic: ExampleTopic) -> impl Iterator<Item = &Example> {
        self.examples.values().filter(move |e| e.topic == topic)
    }

    /// Phase 9 — iterate every example that exercises a given
    /// primitive. Case-sensitive (primitive names are canonical
    /// lowercase). Used by `axon.examples(primitive: "weave")`.
    pub fn examples_using<'a>(&'a self, primitive: &'a str) -> impl Iterator<Item = &'a Example> + 'a {
        self.examples
            .values()
            .filter(move |e| e.primitives.iter().any(|p| p == primitive))
    }
}

/// Match the FS loader's filter — only `.md` files count. Pulled out
/// so the embedded path and the FS path stay in lockstep.
fn is_markdown(p: &Path) -> bool {
    p.extension()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("md"))
        .unwrap_or(false)
}

/// Same shape for `.axon` files — used by the template loader so the
/// embedded path and the FS path treat extensions identically.
fn is_axon(p: &Path) -> bool {
    p.extension()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("axon"))
        .unwrap_or(false)
}

/// What can go wrong loading the corpus. Surfaced as a fatal startup
/// error — the server refuses to run on a malformed knowledge base.
#[derive(Debug)]
pub enum LoadError {
    /// The corpus root exists but is missing the expected subdir
    /// (`primitives/`), or the operator-supplied
    /// `AXON_EMCP_KNOWLEDGE_DIR` does not name an existing directory.
    MissingDir(PathBuf),
    /// A filesystem error happened while reading a path.
    Io(PathBuf, std::io::Error),
    /// A markdown file failed frontmatter parsing.
    BadFrontmatter(PathBuf, String),
    /// A file is missing the `---\n…\n---` frontmatter block entirely.
    NoFrontmatter(PathBuf),
    /// Two primitives declare the same `name` field — ambiguous.
    DuplicateName(String, PathBuf),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::MissingDir(p) => write!(f, "expected directory not found: {}", p.display()),
            LoadError::Io(p, e) => write!(f, "I/O error reading {}: {}", p.display(), e),
            LoadError::BadFrontmatter(p, msg) => {
                write!(f, "frontmatter error in {}: {}", p.display(), msg)
            }
            LoadError::NoFrontmatter(p) => {
                write!(f, "missing YAML frontmatter block in {}", p.display())
            }
            LoadError::DuplicateName(n, p) => {
                write!(f, "primitive `{n}` already exists (second definition at {})", p.display())
            }
        }
    }
}

impl std::error::Error for LoadError {}

fn parse_primitive(raw: &str, path: &Path) -> Result<Primitive, LoadError> {
    let matter = Matter::<YAML>::new();
    let parsed = matter.parse(raw);
    let fm: Frontmatter = parsed
        .data
        .ok_or_else(|| LoadError::NoFrontmatter(path.to_path_buf()))?
        .deserialize()
        .map_err(|e| LoadError::BadFrontmatter(path.to_path_buf(), e.to_string()))?;
    // Read the ledger BEFORE `fm.name` moves into the struct.
    let name_for_ledger = fm.name.clone();
    Ok(Primitive {
        name: fm.name,
        summary: fm.summary,
        category: fm.category,
        top_level: fm.top_level,
        grammar: fm.grammar,
        since: fm.since,
        reality: RuntimeReality::of(&name_for_ledger),
        body: parsed.content,
    })
}

/// Phase 5 — parse one MCP prompt document. Frontmatter shape:
/// `name`, `title`, `summary`, optional `arguments: [{ name,
/// description, required }, ...]`. The body is the rendered prompt
/// template with `{{name}}` placeholders (substitution happens at
/// `prompts/get` time, not at load time).
///
/// The frontmatter `name:` MUST match the file stem so the URL slug
/// (the `prompts/get` lookup key) cannot drift from the on-disk
/// layout — same invariant as the reference loader.
fn parse_prompt(raw: &str, path: &Path) -> Result<Prompt, LoadError> {
    let matter = Matter::<YAML>::new();
    let parsed = matter.parse(raw);
    let fm: PromptFrontmatter = parsed
        .data
        .ok_or_else(|| LoadError::NoFrontmatter(path.to_path_buf()))?
        .deserialize()
        .map_err(|e| LoadError::BadFrontmatter(path.to_path_buf(), e.to_string()))?;
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    if !stem.is_empty() && fm.name != stem {
        return Err(LoadError::BadFrontmatter(
            path.to_path_buf(),
            format!(
                "frontmatter name `{}` does not match file stem `{}` \
                 — the prompt slug must equal the filename",
                fm.name, stem
            ),
        ));
    }
    Ok(Prompt {
        name: fm.name,
        title: fm.title,
        summary: fm.summary,
        arguments: fm.arguments,
        body: parsed.content,
    })
}

/// Phase 9 — parse one example document. Frontmatter shape: `name`,
/// `title`, `summary`, `topic` (closed enum), optional `primitives`
/// (list of primitive name strings). The body after the frontmatter
/// is the raw AXON source returned verbatim — the drift gate
/// `tests/phase9_examples_compile.rs` runs it through `axon-frontend`
/// so a corpus regression is impossible to land.
///
/// The frontmatter `name:` MUST match the file stem so the URL slug
/// (the `axon.examples` lookup key) cannot drift from the on-disk
/// layout — same invariant as the reference + prompt loaders.
fn parse_example(raw: &str, path: &Path) -> Result<Example, LoadError> {
    let matter = Matter::<YAML>::new();
    let parsed = matter.parse(raw);
    let fm: ExampleFrontmatter = parsed
        .data
        .ok_or_else(|| LoadError::NoFrontmatter(path.to_path_buf()))?
        .deserialize()
        .map_err(|e| LoadError::BadFrontmatter(path.to_path_buf(), e.to_string()))?;
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    if !stem.is_empty() && fm.name != stem {
        return Err(LoadError::BadFrontmatter(
            path.to_path_buf(),
            format!(
                "frontmatter name `{}` does not match file stem `{}` \
                 — the example slug must equal the filename",
                fm.name, stem
            ),
        ));
    }
    // Trim leading whitespace + a possible single blank line between
    // the frontmatter boundary and the first AXON declaration. We do
    // not trim the trailing whitespace — preserving a final newline
    // keeps `axon check` diagnostics line-accurate when the agent
    // pastes the source into a follow-up `axon.check` call.
    let source = parsed.content.trim_start().to_string();
    Ok(Example {
        name: fm.name,
        title: fm.title,
        summary: fm.summary,
        topic: fm.topic,
        primitives: fm.primitives,
        source,
    })
}

/// Parse one reference document. The `expected_kind` is the directory
/// the file was found in (`grammar/`, `logic/`, `compliance/`); the
/// loader uses it as the document's `kind` so a misplaced file is
/// impossible — the on-disk layout drives the URI namespace.
fn parse_reference(
    raw: &str,
    path: &Path,
    expected_kind: ReferenceKind,
) -> Result<Reference, LoadError> {
    let matter = Matter::<YAML>::new();
    let parsed = matter.parse(raw);
    let fm: ReferenceFrontmatter = parsed
        .data
        .ok_or_else(|| LoadError::NoFrontmatter(path.to_path_buf()))?
        .deserialize()
        .map_err(|e| LoadError::BadFrontmatter(path.to_path_buf(), e.to_string()))?;
    // Phase 3 invariant — the frontmatter `name:` MUST match the
    // file's base name (stem). Without this guard, a document
    // declaring `name: hipaa` but stored as `pci_dss.md` would shadow
    // the *real* pci_dss entry on next load. We catch this early so
    // the agent never resolves an `axon://compliance/pci_dss` to
    // a HIPAA body.
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if !stem.is_empty() && fm.name != stem {
        return Err(LoadError::BadFrontmatter(
            path.to_path_buf(),
            format!(
                "frontmatter name `{}` does not match file stem `{}` \
                 — the URL slug must equal the filename",
                fm.name, stem
            ),
        ));
    }
    Ok(Reference {
        kind: expected_kind,
        slug: fm.name,
        summary: fm.summary,
        title: fm.title,
        body: parsed.content,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn write_primitive(dir: &Path, name: &str, content: &str) {
        let path = dir.join(format!("{name}.md"));
        let mut f = fs::File::create(path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }

    #[test]
    fn loader_parses_a_well_formed_primitive() {
        let tmp = tempdir();
        let prim_dir = tmp.join("primitives");
        fs::create_dir_all(&prim_dir).unwrap();
        write_primitive(
            &prim_dir,
            "socket",
            r#"---
name: socket
summary: typed WebSocket transport (v2.3.0)
category: session_types
top_level: true
grammar: |
  socket Name { protocol: SessionRef, ... }
since: v2.3.0
---

# `socket`

Body prose.
"#,
        );
        let cat = Catalog::load_from(&tmp).unwrap();
        assert_eq!(cat.primitive_count(), 1);
        let p = cat.primitive("socket").unwrap();
        assert_eq!(p.summary, "typed WebSocket transport (v2.3.0)");
        assert!(p.top_level);
        assert_eq!(p.category, Category::SessionTypes);
        assert!(p.body.contains("Body prose"));
    }

    #[test]
    fn loader_rejects_missing_frontmatter() {
        let tmp = tempdir();
        let prim_dir = tmp.join("primitives");
        fs::create_dir_all(&prim_dir).unwrap();
        write_primitive(&prim_dir, "bad", "# no frontmatter\n");
        let err = Catalog::load_from(&tmp).expect_err("must reject");
        assert!(matches!(err, LoadError::NoFrontmatter(_)));
    }

    #[test]
    fn loader_rejects_duplicate_names() {
        let tmp = tempdir();
        let prim_dir = tmp.join("primitives");
        fs::create_dir_all(&prim_dir).unwrap();
        let body = |n| {
            format!(
                "---\nname: {n}\nsummary: x\ncategory: cognition\ntop_level: true\n---\n"
            )
        };
        write_primitive(&prim_dir, "a", &body("dup"));
        write_primitive(&prim_dir, "b", &body("dup"));
        let err = Catalog::load_from(&tmp).expect_err("dup name must fail");
        assert!(matches!(err, LoadError::DuplicateName(name, _) if name == "dup"));
    }

    #[test]
    fn embedded_corpus_loads_and_contains_at_least_the_socket_primitive() {
        // Phase 1 — the `include_dir!` macro bakes `src/knowledge/`
        // into the binary at compile time. `load_embedded` must
        // round-trip every entry through the same frontmatter parser
        // the FS loader uses, so the wire shape an MCP client sees
        // does not depend on whether the corpus was disk-served or
        // baked-in.
        let cat = Catalog::load_embedded().expect("embedded corpus must load");
        assert!(
            cat.primitive_count() >= 1,
            "embedded corpus is empty — include_dir! resolved no files"
        );
        let socket = cat
            .primitive("socket")
            .expect("Phase 0 ships the socket primitive — must be embedded");
        assert!(socket.top_level);
        assert_eq!(socket.category, Category::SessionTypes);
        // The embedded body must include the canonical opening — the
        // markdown was authored as `# \`socket\`` followed by the
        // primitive's introductory paragraph.
        assert!(socket.body.contains("socket"));
        assert!(socket.body.contains("WebSocket"));
    }

    /// v1.2.0 — **the coverage gate**.
    ///
    /// The closed contract between `axon-frontend::PRIMITIVE_REGISTRY`
    /// and `src/knowledge/primitives/*.md`:
    ///
    /// - Every registry entry with `doc_status: Documented` MUST have
    ///   a corresponding `.md` in the embedded corpus.
    /// - Every `.md` in the embedded corpus MUST have a registry
    ///   entry with `doc_status: Documented`.
    ///
    /// `Pending` entries are visible in the registry (the catalogue
    /// is honestly complete) but the gate does NOT require their
    /// docs — that is the v1.2.0–d roadmap. When a primitive's
    /// `.md` lands, its registry entry flips Pending → Documented;
    /// the gate then enforces the new pairing.
    ///
    /// A failure here is structured: it names every missing doc + every
    /// orphan doc, so the contributor knows exactly which side of the
    /// pair to fix. This is the "formula" v1.2.0 promised — the
    /// corpus cannot drift from the language by omission or by
    /// accidental orphan.
    #[test]
    fn registry_and_corpus_coverage_is_closed_on_documented_entries() {
        use axon_frontend::{DocStatus, PRIMITIVE_REGISTRY};
        use std::collections::HashSet;

        let cat = Catalog::load_embedded().expect("embedded corpus must load");

        // Forward direction: every Documented entry has a .md.
        let documented: Vec<&'static str> = PRIMITIVE_REGISTRY
            .iter()
            .filter(|i| i.doc_status == DocStatus::Documented)
            .map(|i| i.name)
            .collect();
        let missing_docs: Vec<&'static str> = documented
            .iter()
            .copied()
            .filter(|name| cat.primitive(name).is_none())
            .collect();

        // Reverse direction: every .md has a Documented registry entry.
        let documented_set: HashSet<&'static str> = documented.iter().copied().collect();
        let orphan_docs: Vec<String> = cat
            .primitives()
            .filter(|p| !documented_set.contains(p.name.as_str()))
            .map(|p| p.name.clone())
            .collect();

        // The closed set must be balanced on both sides.
        if !missing_docs.is_empty() || !orphan_docs.is_empty() {
            let summary = axon_frontend::coverage_summary();
            panic!(
                "v1.2.0 coverage gate failed:\n\
                 \n\
                 missing docs (in registry as Documented, no .md):\n  \
                   {missing_docs:?}\n\
                 orphan docs (.md but not in registry as Documented):\n  \
                   {orphan_docs:?}\n\
                 \n\
                 catalog summary: {} total, {} documented, {} pending.\n\
                 \n\
                 Fix:\n  \
                   - For each missing doc, run: \
                     `axon-emcp scaffold-primitive <name>` + fill in the body.\n  \
                   - For each orphan doc, either add the entry to \
                     axon_frontend::PRIMITIVE_REGISTRY as DocStatus::Documented, \
                     or delete the .md if the primitive was removed.",
                summary.total, summary.documented, summary.pending,
            );
        }

        // Cross-validate the closed Category catalogue too — every
        // registry entry's category string must deserialise into the
        // local `Category` enum, otherwise the loader would silently
        // skip it. This catches drift between axon-frontend (which
        // uses strings) and axon-emcp (which uses the typed enum).
        for info in PRIMITIVE_REGISTRY {
            let parsed: Result<Category, _> = serde_json::from_value(
                serde_json::Value::String(info.category.to_string()),
            );
            assert!(
                parsed.is_ok(),
                "primitive `{}` registry category `{}` does not deserialise \
                 into axon-emcp's Category enum — closed-catalog drift",
                info.name, info.category
            );
        }
    }

    /// v1.2.0 — informational coverage statistics. Logged as a
    /// stable observation so future telemetry (v1.3.0) can pin the
    /// regression surface here. Not strict: only asserts the counts
    /// agree internally.
    #[test]
    fn registry_coverage_summary_is_internally_consistent() {
        let s = axon_frontend::coverage_summary();
        assert_eq!(s.total, s.documented + s.pending);
        // v1.2.0 closes the coverage cycle: 45 total (47 - taint -
        // logic, both lex-only with no parser production), 45
        // Documented, 0 Pending. **100% coverage achieved**. Any
        // future drop is a regression the gate catches.
        // v2.12.0: 45 → 46 with `ledger` (audit chain) split from `pix`.
        // v2.19.0: 46 → 48 with `observable` + `quant`.
        // v2.27.0: 48 → 49 with `window` (the temporal execution guard).
        // v2.26.0: 49 → 50 with `json` (the open semi-structured value type).
        // v2.34.0: 50 → 54 with the π-calc channel quartet
        // (`channel` / `emit` / `publish` / `discover`, Kivi brief #51 B.2).
        // v2.37.0: 54 → 55 with `upstream` (the outbound vendor connection).
        // v2.37.0: 55 → 56 with `voice` (the inspectable voice-agent sugar).
        // v2.38.0: 56 → 57 with `cors` (the named browser-origin policy).
        // v2.40.0: 57 → 58 with `cache` (the result-memoization policy).
        // v2.42.0: 58 → 60 with `savant` + `synth`.
        // v2.43.0: 60 → 62 with `warden` + `scope`.
        // v2.46.0: 62 → 64 with `credential` + `mint`.
        // v2.48.0: 64 → 65 with `rotate` (mediated secret renewal).
        // v2.53.0: 65 → 66 with `document` (native OOXML synthesis). v2.54.0 adds
        // NO primitive — read/edit are tool providers, so the count
        // holds at 66; this reconciles the gate to the registry.
        // v2.60.0: 66 → 67 with `deliver` (Governed CRM Delivery, egress-dual).
        // v2.67.0: 67 → 66 — `transact` RETRACTED (axon-T938). It never opened
        // a transaction: the runtime set an unread marker string, the block's
        // body was never lowered into the IR, and nothing was rolled back. A
        // retracted primitive leaves the closed set and its corpus doc is
        // deleted, so the coverage gate stays in lockstep on both sides.
        // v2.69.0: 66 → 91 — the CENSUS BACKFILL. Twenty-five constructs
        // with live parser productions (sharpest: `budget`, tracked in NO
        // table while its dead runtime survived every green build) entered
        // the registry as `Pending`; the v2.69.0 docs-tier then landed all 25
        // corpus docs (verified against the runtime audit — several honestly
        // document a refusal or a KNOWN_DEBT gap), restoring 100% coverage
        // at the new, honest denominator of 91.
        // v2.87.0: 91 -> 94 with the algebraic-effect trio `effect` / `perform`
        // / `handle`. Recorded here on 2026-08-20, when the crate's frontend pin
        // finally moved from 1.59.0 to 3.1.0 — the entries had been in the
        // registry for two majors, and this crate could not see them. That is
        // what a stale pin costs: the coverage gate was green the whole time,
        // measuring a registry nobody was shipping.
        // v4.4.0: 94 -> 99 — the four session verbs (`send`, `receive`,
        // `select`, `branch`) and the regulatory badge (`compliance`). All
        // five were Attested in the ledger and advertised in the README with
        // NO registry row, so `axon.primitives` never listed them: five
        // constructs the compiler delivered and an agent had no way to find.
        // v4.5.0: 99 -> 103 — typed declassification. `identifier` (what a
        // type IS), `declassify` (the verb that retires a regulatory class),
        // `attest` (the signature over what no compiler decided) and `census`
        // (the world-fact a three-digit postal code leans on). Without these
        // rows an agent asked to write a HIPAA de-identification finds the
        // badge that says data is regulated and nothing that says how it
        // stops being.
        assert_eq!(s.total, 103);
    }

    /// Phase 5 — every MCP prompt shipped under
    /// `src/knowledge/prompts/` must be embedded with a non-empty
    /// title, summary, body, and at least one declared argument.
    /// Hosts (Claude Code, Cursor, Continue) gate their slash-command
    /// surface on this catalogue; a missing or malformed entry would
    /// vanish from the menu silently.
    #[test]
    fn embedded_corpus_contains_every_phase_5_prompt() {
        let cat = Catalog::load_embedded().expect("embedded corpus must load");
        let expected = ["flow_design", "shield_design", "session_design"];
        for name in expected {
            let p = cat
                .prompt(name)
                .unwrap_or_else(|| panic!("prompt `{name}` must be embedded"));
            assert_eq!(p.name, name, "{name}: slug drift");
            assert!(!p.title.is_empty(), "{name}: title empty");
            assert!(!p.summary.is_empty(), "{name}: summary empty");
            assert!(!p.body.is_empty(), "{name}: body empty");
            assert!(
                !p.arguments.is_empty(),
                "{name}: at least one argument must be declared so the host can render a picker"
            );
            // Every declared argument carries a name + description.
            for arg in &p.arguments {
                assert!(!arg.name.is_empty(), "{name}: empty argument name");
                assert!(!arg.description.is_empty(), "{name}: empty argument description for `{}`", arg.name);
            }
        }
        assert_eq!(
            cat.prompt_count(),
            expected.len(),
            "prompt count drift — add the new prompt to the expected list"
        );
    }

    /// Phase 4 — every scaffold template shipped under
    /// `src/knowledge/templates/` must be embedded into the binary
    /// (so `cargo install --path src/axon-emcp` keeps `axon.compose`
    /// fully self-contained), with the file stem as the slug.
    #[test]
    fn embedded_corpus_contains_every_phase_4_template() {
        let cat = Catalog::load_embedded().expect("embedded corpus must load");
        let expected = [
            // v1.1.0 baseline (8):
            "generic", "healthcare", "banking", "government",
            "legal", "chat", "retrieval", "multi_agent",
            // v1.2.0 — vertical extensions (4):
            "legaltech", "fintech", "pharmatech", "medic_research",
            // v1.2.0 — agent patterns (8):
            "chat_research", "chat_tools", "chat_skills", "whatsapp",
            "voice", "dev", "sales_consultive", "sales_widget",
            // v1.2.0 — application patterns (13):
            "workflow_automation", "business_intelligence",
            "corporate_integration", "self_learning",
            "document_analysis", "ticket_triage",
            "content_moderation", "knowledge_extraction",
            "compliance_monitoring", "recruitment",
            "education", "financial_advisor", "data_pipeline",
            // v2.37.0 — the blessed voice-preset template (the whole
            // program is one `voice` declaration).
            "voice_preset",
        ];
        for slug in expected {
            let t = cat
                .template(slug)
                .unwrap_or_else(|| panic!("template `{slug}` must be embedded"));
            assert_eq!(t.slug, slug);
            assert!(
                !t.source.is_empty(),
                "template `{slug}`: empty source — compose would return nothing"
            );
            // Every template carries at least one `flow` declaration — the
            // canonical agent-facing surface — EXCEPT `voice_preset`
            // (v2.37.0), whose whole program is one `voice`
            // declaration that desugars to a flow-shaped session; `axon
            // desugar` is how an adopter inspects the expansion.
            assert!(
                t.source.contains("flow ") || t.source.contains("voice "),
                "template `{slug}`: missing `flow` declaration"
            );
        }
        assert_eq!(
            cat.template_count(),
            expected.len(),
            "template count drift — add the new template to the expected list"
        );
    }

    /// Phase 3 — every reference doc shipped under
    /// `src/knowledge/{grammar,logic,compliance}/` must be embedded
    /// and round-trip through the same frontmatter parser the FS
    /// loader uses. The `(kind, slug)` table is the canonical list
    /// of what `resources/list` surfaces from the embedded corpus.
    #[test]
    fn embedded_corpus_contains_every_phase_3_reference_doc() {
        let cat = Catalog::load_embedded().expect("embedded corpus must load");

        // (kind, slug) tuples for every Phase 3 doc we ship.
        let expected: &[(ReferenceKind, &str)] = &[
            (ReferenceKind::Grammar, "top_level"),
            (ReferenceKind::Grammar, "composition"),
            (ReferenceKind::Grammar, "ebnf"),
            (ReferenceKind::Logic, "flow_composition"),
            (ReferenceKind::Logic, "session_duality"),
            // v2.9.0 — the use/apply law (dispatch vs cognitive delegation).
            (ReferenceKind::Logic, "dispatch_vs_cognition"),
            // v2.15.0 — effects execute structurally, independent of the
            // output mode (`navigate` is not LLM-conditioned).
            (ReferenceKind::Logic, "effect_execution_is_mode_invariant"),
            // v2.23.0 — the Advantage Witness transversal law.
            (ReferenceKind::Logic, "no_unwitnessed_advantage"),
            // v2.26.0 — total/pure expression law.
            (ReferenceKind::Logic, "total_expressions"),
            // v2.27.0 — time is an explicit, recorded input.
            (ReferenceKind::Logic, "time_is_an_explicit_input"),
            // v2.28.0 — an effect under a budget is a linear resource.
            (ReferenceKind::Logic, "effects_are_linear"),
            // v2.26.0 — open data is navigated totally; a shape is a lens.
            (ReferenceKind::Logic, "open_data_is_total"),
            // v2.31.0 — a channel's declared delivery is enforced or flagged.
            (ReferenceKind::Logic, "delivery_is_a_kept_promise"),
            // v2.44.0 — every dispatching boundary is covered or explicitly public.
            (ReferenceKind::Logic, "every_boundary_is_guarded"),
            // v2.45.0 — every declared guard is satisfiable (the completeness dual).
            (ReferenceKind::Logic, "every_requirement_is_grantable"),
            // v2.46.0 — delegation is attenuation: grants ⊆ minter, TTL-bounded, depth-1.
            (ReferenceKind::Logic, "authority_only_attenuates"),
            // v2.47.0 — every granted authority projects (the dead-authority
            // closure: the converse quantifier of every_requirement_is_grantable).
            (ReferenceKind::Logic, "every_granted_authority_projects"),
            // v2.48.0 — rotation without revelation: custodied secrets have
            // lifecycles, not readers (the inbound dual of v2.46.0).
            (ReferenceKind::Logic, "rotation_without_revelation"),
            // v2.49.0 — selection without revelation: a flow chooses which
            // borrowed authority to spend (parametric injection) without
            // reading it or leaving the tool's class (the v2.48.0 extension).
            (ReferenceKind::Logic, "selection_without_revelation"),
            // v2.51.0 — connections release across cognition: a pooled DB
            // connection is never held idle across an LLM step (pooler-aware
            // pinning; the resource-lifetime dual of dispatch_vs_cognition).
            (ReferenceKind::Logic, "connections_release_across_cognition"),
            // v2.60.0 — delivery is assertion egress: a value written into a
            // system of record carries its epistemic origin, or the author vouched
            // (the egress-dual of v2.52.0 acquisition + the v2.53.0 document T916 barrier).
            (ReferenceKind::Logic, "delivery_is_assertion_egress"),
            // v2.62.0 — a safe method is PROVEN safe: RFC 10008's normative
            // "QUERY MUST be safe and idempotent" made a compile-time proof
            // (axon-T927) instead of a convention nobody enforces.
            (ReferenceKind::Logic, "a_safe_method_is_proven_safe"),
            // v2.63.0 — analysis is algebra, not conversation: the
            // deterministic data plane (dataspace/ingest/focus/aggregate/
            // associate/explore made REAL; a narrated number is a
            // fabricated one).
            (ReferenceKind::Logic, "analysis_is_algebra_not_conversation"),
            // v2.65.0 — the proof-carrying derivative: differentiation is
            // a compile-time derivation over the closed Expr, re-proven at
            // deploy, never a tape or a finite difference.
            (ReferenceKind::Logic, "a_derivative_is_derived_not_sampled"),
            // v2.66.0 — attention is a governed resource: the third
            // egress dual (notify spends human attention; evidence labels,
            // recipient custody, mandatory windows).
            (ReferenceKind::Logic, "attention_is_a_governed_resource"),
            (ReferenceKind::Compliance, "hipaa"),
            (ReferenceKind::Compliance, "gdpr"),
            (ReferenceKind::Compliance, "pci_dss"),
            (ReferenceKind::Compliance, "sox"),
            (ReferenceKind::Compliance, "soc2"),
            (ReferenceKind::Compliance, "fedramp"),
            (ReferenceKind::Compliance, "gxp"),
            (ReferenceKind::Compliance, "fisma"),
            (ReferenceKind::Compliance, "nist_800_53"),
        ];

        for (kind, slug) in expected {
            let r = cat
                .reference(*kind, slug)
                .unwrap_or_else(|| panic!("{}/{slug} must be embedded", kind.as_str()));
            assert_eq!(r.kind, *kind, "{}/{slug}: kind drift", kind.as_str());
            assert_eq!(&r.slug, slug, "{}/{slug}: slug drift", kind.as_str());
            assert!(
                !r.title.is_empty(),
                "{}/{slug}: title empty — resources/list label would be unhelpful",
                kind.as_str()
            );
            assert!(
                !r.summary.is_empty(),
                "{}/{slug}: summary empty — discovery would have no description",
                kind.as_str()
            );
            assert!(
                !r.body.is_empty(),
                "{}/{slug}: body empty — resources/read would return nothing",
                kind.as_str()
            );
        }

        // Cross-check the family counts so we notice if a new doc
        // lands without being added to the expected list above.
        assert_eq!(
            cat.reference_count_of(ReferenceKind::Grammar),
            3,
            "grammar family count drift"
        );
        assert_eq!(
            cat.reference_count_of(ReferenceKind::Logic),
            // v2.48.0: 14 → 15 with `rotation_without_revelation`.
            // v2.49.0: 15 → 16 with `selection_without_revelation`.
            // v2.51.0: 16 → 17 with `connections_release_across_cognition`.
            // v2.60.0: 17 → 18 with `delivery_is_assertion_egress`.
            // v2.62.0: 18 → 19 with `a_safe_method_is_proven_safe` (RFC 10008 QUERY).
            // v2.63.0: 19 → 20 with `analysis_is_algebra_not_conversation`.
            // v2.65.0: 20 → 21 with `a_derivative_is_derived_not_sampled`.
            // v2.66.0: 21 → 22 with `attention_is_a_governed_resource`.
            22,
            "logic family count drift"
        );
        assert_eq!(
            cat.reference_count_of(ReferenceKind::Compliance),
            9,
            "compliance family count drift"
        );
    }

    /// Phase 2 — the 6 core cognitive primitives an agent touches
    /// before anything else (`persona`, `flow`, `step`, `anchor`,
    /// `tool`, `reason`). Verifies each one is embedded, well-typed
    /// against the `Category` discriminator, and carries the
    /// `top_level` polarity we promise in the catalogue. A regression
    /// here is a regression in the agent's onboarding surface — the
    /// first primitive lookups would 404.
    #[test]
    fn embedded_corpus_contains_the_six_core_cognitive_primitives() {
        let cat = Catalog::load_embedded().expect("embedded corpus must load");

        // (name, expected category, expected top-level polarity)
        let expected: &[(&str, Category, bool)] = &[
            ("persona", Category::Cognition, true),
            ("flow", Category::Cognition, true),
            ("step", Category::Cognition, false),
            ("anchor", Category::Cognition, true),
            ("tool", Category::Cognition, true),
            ("reason", Category::Cognition, false),
        ];

        for (name, category, top_level) in expected {
            let p = cat
                .primitive(name)
                .unwrap_or_else(|| panic!("`{name}` must be embedded after Phase 2"));
            assert_eq!(
                p.category, *category,
                "{name}: category drift (catalogue says {:?}, expected {category:?})",
                p.category
            );
            assert_eq!(
                p.top_level, *top_level,
                "{name}: top_level drift (catalogue says {}, expected {top_level})",
                p.top_level
            );
            assert!(
                !p.summary.is_empty(),
                "{name}: summary is empty — agent listings would be unhelpful"
            );
            assert!(
                !p.body.is_empty(),
                "{name}: body is empty — `axon.primitive_doc({name})` would return nothing"
            );
            // Every Phase 2 doc opens with a backtick-wrapped name header
            // (`# \`persona\``, `# \`flow\``, …) — anchors the "what is
            // this?" answer at the top of the agent's prose response.
            let opener = format!("# `{name}`");
            assert!(
                p.body.contains(&opener),
                "{name}: body must open with `{opener}` — missing canonical header"
            );
        }
    }

    /// A throwaway temp dir, no `tempfile` dep — keeps the dependency
    /// surface minimal. We use process-id + a monotonic counter so
    /// concurrent test runs don't collide.
    fn tempdir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "axon-emcp-test-{}-{n}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }
}
