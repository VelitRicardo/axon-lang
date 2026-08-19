//! Consumable posting quotas (paper §2.3, §2.4).
//!
//! Substrate for **axon-W018** (quota pressure) and the §72 linear budget that meters
//! publishing. A platform posting quota is a consumable resource: Instagram allows 100
//! API-published posts per 24-hour moving window per account; TikTok's Direct Post allows ~15
//! per creator per 24 hours. Modeling these as linear budgets means a flow that would exceed the
//! quota is unrepresentable, and one approaching it warns (W018).

use crate::platform::Platform;

/// The scope over which a quota is counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaScope {
    PerAccount,
    PerCreator,
    PerApp,
    PerMember,
}

/// A consumable posting quota: `limit` publishes are permitted per `window_secs`, counted over
/// `scope`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quota {
    pub limit: u32,
    pub window_secs: u64,
    pub scope: QuotaScope,
    pub source: &'static str,
}

/// The publish quota a platform enforces, if any (paper §II). `None` = no documented per-post
/// publish quota of this consumable shape (LinkedIn and Facebook document request-rate limits,
/// which are modeled as request budgets elsewhere, not per-post publish quotas).
pub fn publish_quota(platform: Platform) -> Option<Quota> {
    match platform {
        Platform::Instagram => Some(Quota {
            limit: 100,
            window_secs: 86_400,
            scope: QuotaScope::PerAccount,
            source: "paper_axon_agora.md section 2.3 [IG-CP]",
        }),
        Platform::TikTok => Some(Quota {
            limit: 15,
            window_secs: 86_400,
            scope: QuotaScope::PerCreator,
            source: "paper_axon_agora.md section 2.4 [TT-CSG]",
        }),
        Platform::LinkedIn | Platform::FacebookPages => None,
    }
}

/// The fraction of a quota at which axon-W018 (quota pressure) warns — 0.85 by
/// founder-ratified D116.11.
pub const PRESSURE_THRESHOLD: f64 = 0.85;

/// Whether `used` publishes against `quota` have crossed the pressure threshold (axon-W018).
pub fn quota_pressure(used: u32, quota: &Quota) -> bool {
    f64::from(used) >= f64::from(quota.limit) * PRESSURE_THRESHOLD
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instagram_quota_is_100_per_account_per_day() {
        let q = publish_quota(Platform::Instagram).unwrap();
        assert_eq!(q.limit, 100);
        assert_eq!(q.window_secs, 86_400);
        assert_eq!(q.scope, QuotaScope::PerAccount);
    }

    #[test]
    fn tiktok_quota_is_15_per_creator_per_day() {
        let q = publish_quota(Platform::TikTok).unwrap();
        assert_eq!(q.limit, 15);
        assert_eq!(q.scope, QuotaScope::PerCreator);
    }

    #[test]
    fn platforms_without_a_publish_quota_return_none() {
        assert!(publish_quota(Platform::LinkedIn).is_none());
        assert!(publish_quota(Platform::FacebookPages).is_none());
    }

    #[test]
    fn pressure_triggers_at_eighty_five_percent() {
        let q = publish_quota(Platform::Instagram).unwrap();
        assert!(!quota_pressure(84, &q));
        assert!(quota_pressure(85, &q));
        assert!(quota_pressure(100, &q));
    }
}
