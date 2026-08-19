//! The platforms and governed operations `axon-agora` exposes.

/// A social platform. Owned-only posture: every connector acts on an asset the tenant
/// owns — a Page, a professional account, an organization — never an arbitrary member account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Platform {
    LinkedIn,
    FacebookPages,
    Instagram,
    TikTok,
}

impl Platform {
    /// Every platform, for exhaustive iteration (matrix tests, surface generation).
    pub const ALL: [Platform; 4] = [
        Platform::LinkedIn,
        Platform::FacebookPages,
        Platform::Instagram,
        Platform::TikTok,
    ];

    /// The module-namespace segment: `import agora.<as_str>.{ … }`.
    pub fn as_str(self) -> &'static str {
        match self {
            Platform::LinkedIn => "linkedin",
            Platform::FacebookPages => "facebook",
            Platform::Instagram => "instagram",
            Platform::TikTok => "tiktok",
        }
    }

    /// The `tool.provider` catalog value that routes to this platform's connector
    /// (`agora_<segment>`). The frontend's `VALID_TOOL_PROVIDERS` and the runtime dispatch arm
    /// both derive from this — one naming authority, no drift.
    pub fn provider(self) -> &'static str {
        match self {
            Platform::LinkedIn => "agora_linkedin",
            Platform::FacebookPages => "agora_facebook",
            Platform::Instagram => "agora_instagram",
            Platform::TikTok => "agora_tiktok",
        }
    }

    /// Resolve a `tool.provider` catalog value back to its platform.
    pub fn from_provider(provider: &str) -> Option<Platform> {
        Platform::ALL.into_iter().find(|p| p.provider() == provider)
    }
}

/// A governed connector operation — the verb a cognitive agent invokes. Each maps to an official
/// API call gated by an OAuth scope ([`crate::scope`]), possibly governed by a multi-step
/// protocol ([`crate::protocol`]), possibly refused by platform posture ([`crate::posture`]),
/// and possibly metered by a consumable quota ([`crate::quota`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Operation {
    ReadComments,
    ReadReactions,
    ReadMetrics,
    Reply,
    Moderate,
    Publish,
    Edit,
    Delete,
}

impl Operation {
    /// Every operation, for exhaustive iteration.
    pub const ALL: [Operation; 8] = [
        Operation::ReadComments,
        Operation::ReadReactions,
        Operation::ReadMetrics,
        Operation::Reply,
        Operation::Moderate,
        Operation::Publish,
        Operation::Edit,
        Operation::Delete,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Operation::ReadComments => "read_comments",
            Operation::ReadReactions => "read_reactions",
            Operation::ReadMetrics => "read_metrics",
            Operation::Reply => "reply",
            Operation::Moderate => "moderate",
            Operation::Publish => "publish",
            Operation::Edit => "edit",
            Operation::Delete => "delete",
        }
    }

    /// Resolve the wire/`runtime:` form back to the operation. The `tool` surface names its
    /// operation in the `runtime:` field (the same field `http` tools use for the URL slug);
    /// the dispatch arm parses it with this.
    pub fn parse(s: &str) -> Option<Operation> {
        Operation::ALL.into_iter().find(|op| op.as_str() == s)
    }

    /// Whether this operation writes to the platform. Writes are governed egress (v2.60.0/v2.69.0) and
    /// carry provenance; reads return data born `Untrusted` (v2.52.0/T908).
    pub fn is_write(self) -> bool {
        matches!(
            self,
            Operation::Reply
                | Operation::Moderate
                | Operation::Publish
                | Operation::Edit
                | Operation::Delete
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_arrays_are_complete_and_distinct() {
        assert_eq!(Platform::ALL.len(), 4);
        assert_eq!(Operation::ALL.len(), 8);
        for (i, a) in Operation::ALL.iter().enumerate() {
            for b in &Operation::ALL[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn reads_and_writes_partition_the_operations() {
        let writes = Operation::ALL.iter().filter(|o| o.is_write()).count();
        let reads = Operation::ALL.iter().filter(|o| !o.is_write()).count();
        assert_eq!(writes, 5); // reply, moderate, publish, edit, delete
        assert_eq!(reads, 3); // read_comments, read_reactions, read_metrics
    }

    #[test]
    fn provider_mapping_is_a_bijection() {
        for p in Platform::ALL {
            assert!(p.provider().starts_with("agora_"));
            assert_eq!(Platform::from_provider(p.provider()), Some(p));
        }
        assert_eq!(Platform::from_provider("http"), None);
        assert_eq!(Platform::from_provider("agora_myspace"), None);
    }

    #[test]
    fn operation_parse_roundtrips_and_rejects_unknowns() {
        for op in Operation::ALL {
            assert_eq!(Operation::parse(op.as_str()), Some(op));
        }
        assert_eq!(Operation::parse("post"), None);
        assert_eq!(Operation::parse(""), None);
    }
}
