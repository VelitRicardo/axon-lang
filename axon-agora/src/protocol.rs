//! Session-typed publishing protocols (paper section 2.3, section 2.4).
//!
//! Substrate for **axon-T957** (protocol typestate): the multi-step publish flows must be driven
//! in the platform's mandated order, so calling a step out of order — e.g. Instagram
//! `media_publish` before the container reaches `FINISHED` — is a violation the type system can
//! reject rather than a runtime 400. After Hu & Yoshida, "Hybrid Session Verification through
//! Endpoint API Generation" (FASE 2016): each protocol state permits only its valid next step.

use crate::platform::Platform;

/// The ordered steps of a platform's publish protocol. A single-element protocol is atomic; a
/// multi-element protocol (Instagram, TikTok) must be driven in order.
pub fn publish_protocol(platform: Platform) -> &'static [&'static str] {
    match platform {
        // Instagram: create container → poll status (until FINISHED) → publish (paper section 2.3).
        Platform::Instagram => &["create_container", "poll_status", "publish"],
        // TikTok Direct Post: query creator info → init → upload → poll status (paper section 2.4).
        Platform::TikTok => &["query_creator_info", "init", "upload", "poll_status"],
        // LinkedIn / Facebook Pages: single-call publish.
        Platform::LinkedIn | Platform::FacebookPages => &["publish"],
    }
}

/// Whether a platform's publish protocol is multi-step (requires typestate discipline).
pub fn is_multi_step(platform: Platform) -> bool {
    publish_protocol(platform).len() > 1
}

/// A protocol-order violation (axon-T957 substrate): a step was called out of the mandated
/// order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolViolation {
    pub platform: Platform,
    /// The 0-based position in the call sequence where the violation occurred.
    pub position: usize,
    /// The step the protocol required at this position.
    pub expected: &'static str,
    /// The step that was actually attempted.
    pub got: String,
}

/// Validate that `called` drives the platform's publish protocol in order — a full run, or a
/// valid in-order prefix. Returns the first out-of-order (or past-the-end) step as a violation.
pub fn validate_sequence(platform: Platform, called: &[&str]) -> Result<(), ProtocolViolation> {
    let steps = publish_protocol(platform);
    for (i, &step) in called.iter().enumerate() {
        match steps.get(i) {
            Some(&expected) if expected == step => continue,
            Some(&expected) => {
                return Err(ProtocolViolation {
                    platform,
                    position: i,
                    expected,
                    got: step.to_string(),
                })
            }
            None => {
                return Err(ProtocolViolation {
                    platform,
                    position: i,
                    expected: "<end of protocol>",
                    got: step.to_string(),
                })
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_step_platforms_are_atomic() {
        assert!(!is_multi_step(Platform::LinkedIn));
        assert!(!is_multi_step(Platform::FacebookPages));
        assert!(is_multi_step(Platform::Instagram));
        assert!(is_multi_step(Platform::TikTok));
    }

    #[test]
    fn instagram_in_order_run_is_valid() {
        assert!(validate_sequence(
            Platform::Instagram,
            &["create_container", "poll_status", "publish"]
        )
        .is_ok());
    }

    #[test]
    fn instagram_publish_before_finished_is_a_violation() {
        // The classic typestate error: publish before polling the container to FINISHED.
        let err = validate_sequence(Platform::Instagram, &["create_container", "publish"]).unwrap_err();
        assert_eq!(err.position, 1);
        assert_eq!(err.expected, "poll_status");
        assert_eq!(err.got, "publish");
    }

    #[test]
    fn tiktok_must_query_creator_first() {
        let err = validate_sequence(Platform::TikTok, &["init"]).unwrap_err();
        assert_eq!(err.position, 0);
        assert_eq!(err.expected, "query_creator_info");
    }

    #[test]
    fn a_valid_prefix_is_accepted() {
        assert!(validate_sequence(Platform::TikTok, &["query_creator_info", "init"]).is_ok());
    }

    #[test]
    fn steps_past_the_end_are_rejected() {
        let err = validate_sequence(Platform::FacebookPages, &["publish", "publish"]).unwrap_err();
        assert_eq!(err.position, 1);
        assert_eq!(err.expected, "<end of protocol>");
    }
}
