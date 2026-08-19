//! Owned-only posture refusals (D116.3; paper §2.1, §2.3, §2.4).
//!
//! Substrate for **axon-T958**: an operation the platform forbids in the owned-only posture is
//! refused loudly, with the platform's own rule and the fix in the message (the §111 posture —
//! refuse with the remedy, never silently). `axon-agora` connects to assets the tenant OWNS;
//! member-level automation, consumer-account publishing, and unattended/unaudited public posting
//! where a platform forbids them are compile-time refusals, not runtime surprises.

use crate::platform::{Operation, Platform};

/// The kind of target an operation acts on. Owned kinds are permitted; member/consumer kinds sit
/// outside the owned-only posture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    OwnedPage,
    OwnedProfessionalAccount,
    OwnedOrganization,
    MemberAccount,
    ConsumerAccount,
}

/// Whether the API client has passed the platform's app audit (TikTok Content Posting API).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppAudit {
    Audited,
    Unaudited,
}

/// Whether the act runs fully unattended or with per-act human confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attendance {
    Unattended,
    HumanConfirmed,
}

/// A loud refusal carrying the platform rule and the remediation (axon-T958).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostureRefusal {
    pub code: &'static str,
    pub reason: &'static str,
    pub fix: &'static str,
    pub source: &'static str,
}

/// Check the owned-only posture. `Ok(())` means the operation is permitted for this target,
/// audit status, and attendance; `Err` is a loud refusal naming the platform rule and the fix.
pub fn posture_check(
    platform: Platform,
    op: Operation,
    target: TargetKind,
    audit: AppAudit,
    attendance: Attendance,
) -> Result<(), PostureRefusal> {
    // LinkedIn: the API Terms prohibit automating posting; member-level automation is forbidden.
    // Only organization pages, under an approved Community Management use case, may be automated.
    if platform == Platform::LinkedIn && op.is_write() && target == TargetKind::MemberAccount {
        return Err(PostureRefusal {
            code: "axon-T958",
            reason: "LinkedIn's API Terms of Use section 3.1(26) prohibit using the APIs to automate posting on LinkedIn; member-level automation is forbidden.",
            fix: "Target an organization Page (OwnedOrganization) under an approved Community Management use case.",
            source: "paper_axon_agora.md section 2.1 [L-TOS]",
        });
    }

    // Instagram: content publishing via the official API is available only to professional
    // (business/creator) accounts, never consumer accounts.
    if platform == Platform::Instagram
        && op == Operation::Publish
        && target == TargetKind::ConsumerAccount
    {
        return Err(PostureRefusal {
            code: "axon-T958",
            reason: "Instagram content publishing via the official API is available only to professional (business/creator) accounts.",
            fix: "Convert the target to an Instagram professional account (OwnedProfessionalAccount).",
            source: "paper_axon_agora.md section 2.3 [IG-CP]",
        });
    }

    // TikTok: unaudited clients are restricted to SELF_ONLY, and public posting requires express
    // per-post user consent — so fully unattended public posting is forbidden either way.
    if platform == Platform::TikTok && op == Operation::Publish {
        if audit == AppAudit::Unaudited {
            return Err(PostureRefusal {
                code: "axon-T958",
                reason: "TikTok restricts unaudited API clients to SELF_ONLY (private) posting; public posting requires passing TikTok's audit.",
                fix: "Complete TikTok's Content Posting API audit before publishing publicly.",
                source: "paper_axon_agora.md section 2.4 [TT-CSG]",
            });
        }
        if attendance == Attendance::Unattended {
            return Err(PostureRefusal {
                code: "axon-T958",
                reason: "TikTok requires express, per-post user consent before content is transmitted; fully unattended public posting is not permitted.",
                fix: "Route TikTok publishing through a per-post human-confirmation step (Attendance::HumanConfirmed).",
                source: "paper_axon_agora.md section 2.4 [TT-CSG]",
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linkedin_member_automation_is_refused() {
        let r = posture_check(
            Platform::LinkedIn,
            Operation::Publish,
            TargetKind::MemberAccount,
            AppAudit::Audited,
            Attendance::HumanConfirmed,
        )
        .unwrap_err();
        assert_eq!(r.code, "axon-T958");
        assert!(r.reason.contains("section 3.1(26)"));
        assert!(!r.fix.is_empty());
    }

    #[test]
    fn linkedin_org_publishing_is_permitted() {
        assert!(posture_check(
            Platform::LinkedIn,
            Operation::Publish,
            TargetKind::OwnedOrganization,
            AppAudit::Audited,
            Attendance::Unattended,
        )
        .is_ok());
    }

    #[test]
    fn instagram_consumer_publish_is_refused_but_professional_is_ok() {
        assert!(posture_check(
            Platform::Instagram,
            Operation::Publish,
            TargetKind::ConsumerAccount,
            AppAudit::Audited,
            Attendance::Unattended,
        )
        .is_err());
        assert!(posture_check(
            Platform::Instagram,
            Operation::Publish,
            TargetKind::OwnedProfessionalAccount,
            AppAudit::Audited,
            Attendance::Unattended,
        )
        .is_ok());
    }

    #[test]
    fn tiktok_unaudited_public_post_is_refused() {
        let r = posture_check(
            Platform::TikTok,
            Operation::Publish,
            TargetKind::OwnedProfessionalAccount,
            AppAudit::Unaudited,
            Attendance::HumanConfirmed,
        )
        .unwrap_err();
        assert!(r.reason.contains("SELF_ONLY"));
    }

    #[test]
    fn tiktok_unattended_public_post_is_refused_even_when_audited() {
        let r = posture_check(
            Platform::TikTok,
            Operation::Publish,
            TargetKind::OwnedProfessionalAccount,
            AppAudit::Audited,
            Attendance::Unattended,
        )
        .unwrap_err();
        assert!(r.reason.contains("consent"));
    }

    #[test]
    fn tiktok_audited_human_confirmed_post_is_permitted() {
        assert!(posture_check(
            Platform::TikTok,
            Operation::Publish,
            TargetKind::OwnedProfessionalAccount,
            AppAudit::Audited,
            Attendance::HumanConfirmed,
        )
        .is_ok());
    }

    #[test]
    fn reads_are_never_posture_refused() {
        for p in Platform::ALL {
            for op in Operation::ALL.iter().filter(|o| !o.is_write()) {
                assert!(posture_check(
                    p,
                    *op,
                    TargetKind::MemberAccount,
                    AppAudit::Unaudited,
                    Attendance::Unattended,
                )
                .is_ok());
            }
        }
    }
}
