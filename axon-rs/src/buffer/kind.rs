//! [`BufferKind`] — content-kind tags for multimodal buffers.
//!
//! Unlike the closed catalogues in v1.4.0 (trust proofs,
//! backpressure policies), the `BufferKind` registry is **open**.
//! Adopters register domain-specific kinds at startup:
//!
//! ```
//! # use axon::buffer::BufferKind;
//! let my_kind = BufferKind::new("siemens_dicom");
//! assert_eq!(my_kind.slug(), "siemens_dicom");
//! ```
//!
//! The registry seeds with a conservative list of common kinds so
//! flows that just need "pcm16 at 16kHz" don't have to register
//! anything:
//!
//! | Slug       | Typical source                              |
//! |------------|---------------------------------------------|
//! | `raw`      | Untagged bytes (default for new buffers)    |
//! | `pcm16`    | 16-bit signed PCM audio                     |
//! | `mulaw8`   | 8-bit μ-law telephony audio                 |
//! | `wav`      | WAV container (header + PCM)                |
//! | `mp3`      | MPEG-1/2 Audio Layer III                    |
//! | `opus`     | Opus (WebRTC / Discord / Zoom)              |
//! | `jpeg`     | Baseline JPEG image                         |
//! | `png`      | PNG image                                   |
//! | `webp`     | WebP image                                  |
//! | `mp4`      | MPEG-4 container (video)                    |
//! | `webm`     | WebM container                              |
//! | `pdf`      | Portable Document Format                    |
//! | `json`     | UTF-8 encoded JSON                          |
//! | `csv`      | UTF-8 encoded CSV                           |

use std::sync::{Arc, RwLock};

/// Interned content-kind tag. Two kinds are equal when their slug
/// string matches (case-sensitive). Construction via
/// [`BufferKind::new`] is cheap — the registry de-duplicates
/// identical slugs to a single `Arc<str>`.
#[derive(Debug, Clone)]
pub struct BufferKind {
    slug: Arc<str>,
}

impl BufferKind {
    /// Construct (or reuse) a kind from a slug. Registers the kind
    /// in the global [`BufferKindRegistry`] so observability tooling
    /// can enumerate every kind currently in use.
    pub fn new(slug: impl Into<String>) -> Self {
        let slug = slug.into();
        let arc = BufferKindRegistry::global().intern(&slug);
        BufferKind { slug: arc }
    }

    /// Slug lookup — stable, case-sensitive string.
    pub fn slug(&self) -> &str {
        &self.slug
    }

    // ── Seeded kinds ────────────────────────────────────────────

    pub fn raw() -> Self {
        Self::new("raw")
    }
    pub fn pcm16() -> Self {
        Self::new("pcm16")
    }
    pub fn mulaw8() -> Self {
        Self::new("mulaw8")
    }
    pub fn wav() -> Self {
        Self::new("wav")
    }
    pub fn mp3() -> Self {
        Self::new("mp3")
    }
    pub fn opus() -> Self {
        Self::new("opus")
    }
    pub fn jpeg() -> Self {
        Self::new("jpeg")
    }
    pub fn png() -> Self {
        Self::new("png")
    }
    pub fn webp() -> Self {
        Self::new("webp")
    }
    pub fn mp4() -> Self {
        Self::new("mp4")
    }
    pub fn webm() -> Self {
        Self::new("webm")
    }
    pub fn pdf() -> Self {
        Self::new("pdf")
    }
    pub fn json() -> Self {
        Self::new("json")
    }
    pub fn csv() -> Self {
        Self::new("csv")
    }
}

impl PartialEq for BufferKind {
    fn eq(&self, other: &Self) -> bool {
        // Compare Arc identity first (interned kinds match fast),
        // then fall back to string compare when the registry
        // returned a stale reference (very rare).
        Arc::ptr_eq(&self.slug, &other.slug) || self.slug == other.slug
    }
}

impl Eq for BufferKind {}

impl PartialOrd for BufferKind {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for BufferKind {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.slug.as_ref().cmp(other.slug.as_ref())
    }
}

impl std::hash::Hash for BufferKind {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.slug.hash(state);
    }
}

impl std::fmt::Display for BufferKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.slug)
    }
}

// ── Global registry ──────────────────────────────────────────────────

/// Interning registry. Adopters rarely use this directly — the
/// [`BufferKind::new`] constructor goes through the global instance.
/// Tests can swap the global via [`BufferKindRegistry::set_global`].
pub struct BufferKindRegistry {
    inner: RwLock<BufferKindRegistryInner>,
}

struct BufferKindRegistryInner {
    slugs: std::collections::HashMap<String, Arc<str>>,
}

impl BufferKindRegistry {
    pub fn new() -> Self {
        let mut inner = BufferKindRegistryInner {
            slugs: std::collections::HashMap::new(),
        };
        for seeded in SEEDED_KINDS {
            let arc: Arc<str> = Arc::from(*seeded);
            inner.slugs.insert((*seeded).to_string(), arc);
        }
        BufferKindRegistry {
            inner: RwLock::new(inner),
        }
    }

    /// Returns the process-wide singleton. Built lazily on first
    /// access with the seeded kinds already registered.
    pub fn global() -> &'static BufferKindRegistry {
        use std::sync::OnceLock;
        static GLOBAL: OnceLock<BufferKindRegistry> = OnceLock::new();
        GLOBAL.get_or_init(BufferKindRegistry::new)
    }

    /// Intern a slug, returning the canonical `Arc<str>`.
    pub fn intern(&self, slug: &str) -> Arc<str> {
        // Fast path — read lock, hit cache.
        {
            let guard = self.inner.read().expect("registry poisoned");
            if let Some(existing) = guard.slugs.get(slug) {
                return Arc::clone(existing);
            }
        }
        // Slow path — promote to write lock.
        let mut guard = self.inner.write().expect("registry poisoned");
        Arc::clone(
            guard
                .slugs
                .entry(slug.to_string())
                .or_insert_with(|| Arc::from(slug)),
        )
    }

    /// Enumerate every currently registered slug (sorted, stable
    /// order for tests / tracing).
    pub fn known_slugs(&self) -> Vec<String> {
        let guard = self.inner.read().expect("registry poisoned");
        let mut v: Vec<String> = guard.slugs.keys().cloned().collect();
        v.sort();
        v
    }
}

impl Default for BufferKindRegistry {
    fn default() -> Self {
        Self::new()
    }
}

const SEEDED_KINDS: &[&str] = &[
    "raw", "pcm16", "mulaw8", "wav", "mp3", "opus", "jpeg", "png", "webp",
    "mp4", "webm", "pdf", "json", "csv",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeded_kinds_are_equal_across_constructors() {
        assert_eq!(BufferKind::raw(), BufferKind::new("raw"));
        assert_eq!(BufferKind::pcm16(), BufferKind::new("pcm16"));
    }

    #[test]
    fn intern_returns_same_arc_for_same_slug() {
        let a = BufferKind::new("custom_kind_a");
        let b = BufferKind::new("custom_kind_a");
        assert_eq!(a, b);
        // Interned → both clones point at the same Arc.
        assert!(Arc::ptr_eq(&a.slug, &b.slug));
    }

    #[test]
    fn different_slugs_are_not_equal() {
        assert_ne!(BufferKind::new("a"), BufferKind::new("b"));
    }

    #[test]
    fn known_slugs_includes_seeded() {
        let slugs = BufferKindRegistry::global().known_slugs();
        for seeded in SEEDED_KINDS {
            assert!(
                slugs.contains(&seeded.to_string()),
                "seeded kind {seeded} missing from registry"
            );
        }
    }

    #[test]
    fn display_format_matches_slug() {
        let k = BufferKind::new("opus");
        assert_eq!(format!("{k}"), "opus");
    }
}
