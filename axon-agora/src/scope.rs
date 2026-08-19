//! Capability×scope matrix (paper II). The OAuth scope each operation requires, per platform.
//!
//! This is the substrate for **axon-T956** (scope coverage): the frontend's authorization-
//! coverage law (v2.44.0/v2.45.0) refuses a connector op whose required scope the importing program has
//! not been granted. The matrix is exhaustive over `Platform × Operation` — adding an operation
//! forces every platform to declare its scope (the compiler enforces coverage, no wildcard).

use crate::platform::{Operation, Platform};

/// The OAuth scope(s) required to perform `op` on `platform`, per each platform's official API
/// documentation (paper II, source-confirmed 2026-07). All scopes in the returned slice are
/// required together (conjunction).
pub fn required_scopes(platform: Platform, op: Operation) -> &'static [&'static str] {
    use Operation::*;
    use Platform::*;
    match (platform, op) {
        // ── LinkedIn — organization pages via Community Management (paper section 2.1). ──
        // Member-level social reading (`r_member_social`) is CLOSED; org-level scopes only.
        (LinkedIn, Publish) | (LinkedIn, Edit) | (LinkedIn, Delete) | (LinkedIn, Reply)
        | (LinkedIn, Moderate) => &["w_organization_social"],
        (LinkedIn, ReadComments) | (LinkedIn, ReadReactions) | (LinkedIn, ReadMetrics) => {
            &["r_organization_social"]
        }

        // ── Facebook Pages — permissions map 1:1 onto operations (paper section 2.2). ──
        (FacebookPages, Publish) | (FacebookPages, Edit) => &["pages_manage_posts"],
        (FacebookPages, Reply) | (FacebookPages, Moderate) | (FacebookPages, Delete) => {
            &["pages_manage_engagement"]
        }
        (FacebookPages, ReadComments) | (FacebookPages, ReadReactions)
        | (FacebookPages, ReadMetrics) => &["pages_read_engagement"],

        // ── Instagram — professional accounts, Instagram Login path (paper section 2.3). ──
        (Instagram, Publish) | (Instagram, Edit) => {
            &["instagram_business_basic", "instagram_business_content_publish"]
        }
        (Instagram, Reply) | (Instagram, Moderate) | (Instagram, Delete) => {
            &["instagram_business_basic", "instagram_business_manage_comments"]
        }
        (Instagram, ReadComments) | (Instagram, ReadReactions) | (Instagram, ReadMetrics) => {
            &["instagram_business_basic"]
        }

        // ── TikTok — Content Posting API + Display/Research read (paper section 2.4). ──
        (TikTok, Publish) | (TikTok, Edit) | (TikTok, Delete) => &["video.publish"],
        (TikTok, ReadMetrics) => &["video.list"],
        (TikTok, ReadComments) | (TikTok, ReadReactions) | (TikTok, Reply)
        | (TikTok, Moderate) => &["comment.list"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::{Operation, Platform};

    #[test]
    fn every_platform_operation_pair_has_a_scope() {
        // Exhaustiveness at runtime too: no pair returns an empty scope set.
        for p in Platform::ALL {
            for op in Operation::ALL {
                assert!(
                    !required_scopes(p, op).is_empty(),
                    "{:?}/{:?} has no required scope",
                    p,
                    op
                );
            }
        }
    }

    #[test]
    fn facebook_scopes_map_one_to_one() {
        assert_eq!(required_scopes(Platform::FacebookPages, Operation::Publish), &["pages_manage_posts"]);
        assert_eq!(required_scopes(Platform::FacebookPages, Operation::Moderate), &["pages_manage_engagement"]);
        assert_eq!(required_scopes(Platform::FacebookPages, Operation::ReadComments), &["pages_read_engagement"]);
    }

    #[test]
    fn instagram_publish_requires_content_publish_scope() {
        let scopes = required_scopes(Platform::Instagram, Operation::Publish);
        assert!(scopes.contains(&"instagram_business_content_publish"));
    }

    #[test]
    fn tiktok_publish_requires_video_publish() {
        assert_eq!(required_scopes(Platform::TikTok, Operation::Publish), &["video.publish"]);
    }

    #[test]
    fn linkedin_never_requires_a_member_social_scope() {
        // `r_member_social` is a CLOSED permission (paper section 2.1) — the matrix must never demand it.
        for op in Operation::ALL {
            for scope in required_scopes(Platform::LinkedIn, op) {
                assert_ne!(*scope, "r_member_social");
                assert_ne!(*scope, "w_member_social");
            }
        }
    }
}
