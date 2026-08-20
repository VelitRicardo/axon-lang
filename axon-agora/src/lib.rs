//! # axon-agora
//!
//! The first official library of axon-lang: governed native connectors for **LinkedIn**,
//! **Facebook Pages**, **Instagram**, and **TikTok**, so a cognitive agent can act directly
//! inside those networks — reading comments and metrics, moderating and replying, editing and
//! publishing — with zero human input at execution time, as one step inside a larger multi-tool
//! task.
//!
//! This crate is the **OSS protocol layer** (v2.77.0, decision the design decision): the single source of
//! truth that both the `agora.*` module surface (`.axon` files) and the axon-frontend governance
//! laws (`axon-T956`/`T957`/`T958`/`W018`) consume. It contains no network I/O and no
//! credentials — per-tenant token custody, the refresh daemon, webhook ingress, and audit sinks
//! live in the enterprise layer (v2.77.0+). What it does contain:
//!
//! - [`scope`] — the capability×scope matrix (which OAuth scope each operation requires).
//! - [`protocol`] — the session-typed publishing protocols (Instagram's `create → poll →
//!   publish`, TikTok's `query → init → upload → poll`).
//! - [`posture`] — the owned-only posture refusals (what each platform forbids, with the rule
//!   and the fix).
//! - [`quota`] — the consumable posting quotas (Instagram 100/24h, TikTok ~15/creator/24h).
//! - [`connector`] — the uniform [`SocialConnector`] seam every platform's native core
//!   implements, reached in production as a tool provider.
//!
//! Every platform fact encoded here is sourced to `papers/paper_axon_agora.md` II, itself
//! confirmed against primary platform documentation (2026-07).

pub mod connector;
pub mod facebook;
mod graph;
pub mod instagram;
pub mod linkedin;
pub mod oauth;
pub mod platform;
pub mod tiktok;
pub mod posture;
pub mod protocol;
pub mod quota;
pub mod scope;

pub use connector::{
    CallContext, Comment, ConnectorError, Metrics, ModerationAction, PublishReceipt,
    PublishRequest, Reaction, SocialConnector,
};
pub use facebook::{FacebookPagesConfig, FacebookPagesConnector};
pub use instagram::{InstagramConfig, InstagramConnector};
pub use linkedin::{LinkedInConfig, LinkedInConnector};
pub use tiktok::{TikTokConfig, TikTokConnector};
pub use oauth::{
    needs_refresh, refresh_grant, refresh_long_lived, refresh_mechanism, OAuthError,
    RefreshGrantConfig, RefreshMechanism, RefreshedTokens,
};
pub use platform::{Operation, Platform};
pub use posture::{posture_check, AppAudit, Attendance, PostureRefusal, TargetKind};
pub use protocol::{is_multi_step, publish_protocol, validate_sequence, ProtocolViolation};
pub use quota::{publish_quota, quota_pressure, Quota, QuotaScope};
pub use scope::required_scopes;
