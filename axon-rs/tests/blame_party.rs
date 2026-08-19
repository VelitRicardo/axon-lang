//! v2.83.0 — **the blame RESPONSIBILITY axis, and the third catalog that
//! did not get created.**
//!
//! `paper_agent.md` (Eje 2) specifies a trimodal Findler-Felleisen blame
//! calculus — `Blame ∈ {Orchestrator, SubAgent, Environment}` — as the axis of
//! **who** is answerable, distinct from `BlameKind`'s **what** degraded.
//!
//! section 0.2 framed this as "implement the paper's axis alongside `BlameKind`". Asked
//! the section 0.4 question first — *what already reads this?* — and the axis turned out
//! to ALREADY EXIST: [`axon::emcp::Blame`] `{None, Caller, Server, Network}`,
//! whose own doc cites *"the contract-based blame calculus from ℰMCP spec
//! (CT-2/CT-3)"*. The same calculus, specified twice, in two vocabularies.
//!
//! Adding the paper's names as a new enum would have made a THIRD catalog of one
//! concept inside one runtime — the enum-shaped version of
//! [[feedback_free_string_field_breeds_fake_catalog]], which this project has now
//! seen three times. So `BlameParty` uses the vocabulary that is already on a
//! wire an adopter reads (a failed ℰMCP call surfaces as `"… [blame=server]"`),
//! and `emcp::Blame` projects onto it.
//!
//! **The orthogonality is load-bearing and measurable.** If every `BlameKind`
//! determined a `BlameParty`, `party` would not be a second axis — it would be a
//! function of the first, and storing it would be storing a derivation. Two of
//! the five kinds genuinely do not determine it, which is why those two take the
//! party as a PARAMETER and default to `None`. section 3 pins that.

use axon::emcp::Blame;
use axon::wire_envelope::{BlameContext, BlameKind, BlameParty};
use axon::wire_envelope_producers::{
    blame_for_anchor_breach, blame_for_backend_soft_fail, blame_for_shield_rejection,
    blame_for_store_breach, blame_for_type_mismatch,
};

// ── section 1 — ONE axis: `emcp::Blame` projects onto `BlameParty` ──────────────────

/// The bridge that prevents the third catalog. Every ℰMCP blame value has a
/// place on the shared axis.
#[test]
fn s1_emcp_blame_projects_onto_the_shared_axis() {
    assert_eq!(Blame::Caller.party(), Some(BlameParty::Caller));
    assert_eq!(Blame::Server.party(), Some(BlameParty::Server));
    // The paper's environmental blame is wider than the network — timeouts, FFI
    // breaks, memory corruption. ℰMCP raises only the network case.
    assert_eq!(Blame::Network.party(), Some(BlameParty::Environment));
}

/// `Blame::None` means *the call succeeded*; `party: None` means *no party could
/// be determined*. Different facts, and they must not collapse into each other.
#[test]
fn s1b_emcp_none_is_absence_of_failure_not_absence_of_attribution() {
    assert_eq!(Blame::None.party(), None);
}

/// ℰMCP's own wire spelling is UNTOUCHED. Renaming it to match a paper would
/// break a live contract to buy nothing.
#[test]
fn s1c_the_emcp_wire_spelling_is_unchanged() {
    assert_eq!(Blame::None.as_str(), "none");
    assert_eq!(Blame::Caller.as_str(), "caller");
    assert_eq!(Blame::Server.as_str(), "server");
    assert_eq!(Blame::Network.as_str(), "network");
}

// ── section 2 — the paper's assignments, where it makes them BY NAME ────────────────

/// `anchor_breach` → negative blame. The paper names this case outright: *"la
/// activación de un fallo por anchor_breach … cuando el sub-agente intenta
/// modificar recursos fuera de su ámbito de contención concedido"*.
#[test]
fn s2_anchor_breach_is_the_papers_negative_blame() {
    let b = blame_for_anchor_breach("Triage", "budget_ok", "warn", 0.71);
    assert_eq!(b.kind, BlameKind::AnchorBreach);
    assert_eq!(b.party, Some(BlameParty::Server));
}

/// *"el retorno de tipos no conformes"* — negative blame, named.
#[test]
fn s2b_type_mismatch_is_the_papers_negative_blame() {
    let b = blame_for_type_mismatch("patient.dob", "Date", "String");
    assert_eq!(b.party, Some(BlameParty::Server));
}

/// Not named by the paper, but it follows from the definition: the shield IS the
/// contract guard, and the content it flagged came from the invoked party.
#[test]
fn s2c_shield_rejection_follows_from_the_definition() {
    let b = blame_for_shield_rejection("Hipaa", "Triage", "ssn");
    assert_eq!(b.party, Some(BlameParty::Server));
}

// ── section 3 — the axes are ORTHOGONAL, and here is the evidence ───────────────────

/// A broken mutation chain has two possible authors — a malformed write
/// (`Server`) or storage corruption (`Environment`) — and the KIND cannot tell
/// them apart. The call site can, so it passes the party.
#[test]
fn s3_store_breach_does_not_determine_a_party() {
    let unknown = blame_for_store_breach("transactions", "seg_42", None);
    assert_eq!(
        unknown.party, None,
        "when the runtime did not establish responsibility, the honest value is \
         absent — attributing it to the wrong component is worse than \
         attributing it to nobody"
    );

    let known = blame_for_store_breach("transactions", "seg_42", Some(BlameParty::Environment));
    assert_eq!(
        known.party,
        Some(BlameParty::Environment),
        "and the call site, which knows more than the kind, can supply it"
    );
}

/// Same kind, two parties: a truncated completion is the backend violating a
/// postcondition; a soft rate-limit is infrastructure.
#[test]
fn s3b_one_kind_can_carry_either_party() {
    let postcondition =
        blame_for_backend_soft_fail("anthropic", "truncated_response", Some(BlameParty::Server));
    let infrastructure =
        blame_for_backend_soft_fail("anthropic", "soft_rate_limit", Some(BlameParty::Environment));

    assert_eq!(postcondition.kind, infrastructure.kind);
    assert_ne!(
        postcondition.party, infrastructure.party,
        "ONE kind, TWO parties — this is the proof that `party` is a second axis \
         and not a function of the first. If it were derivable from `kind`, \
         storing it would be storing a derivation."
    );
}

// ── section 4 — v2.0.0's D11 wire contract is UNTOUCHED ───────────────────────────────

/// An envelope with no party must serialise exactly as it did before v2.83.0,
/// so no existing consumer sees a new key and no artifact's shape moves.
#[test]
fn s4_an_absent_party_is_elided_from_the_wire() {
    let b = BlameContext {
        kind: BlameKind::StoreBreach,
        party: None,
        location: "store:ledger".to_string(),
        message: "m".to_string(),
        d_letter: None,
    };
    let j = serde_json::to_value(&b).expect("to_value");
    assert!(
        j.get("party").is_none(),
        "the key must be ABSENT, not null — v2.0.0's D11 wire stays byte-identical \
         for every consumer that predates this axis: {j}"
    );
    assert_eq!(j.get("kind").and_then(|v| v.as_str()), Some("store_breach"));
}

/// And when present it is a stable snake_case slug, like every other closed
/// catalog on this wire.
#[test]
fn s4b_a_present_party_serialises_as_a_stable_slug() {
    for (party, slug) in [
        (BlameParty::Caller, "caller"),
        (BlameParty::Server, "server"),
        (BlameParty::Environment, "environment"),
    ] {
        let b = BlameContext {
            kind: BlameKind::AnchorBreach,
            party: Some(party),
            location: String::new(),
            message: String::new(),
            d_letter: None,
        };
        let j = serde_json::to_value(&b).expect("to_value");
        assert_eq!(
            j.get("party").and_then(|v| v.as_str()),
            Some(slug),
            "party slug must be wire-stable"
        );
    }
}

/// A pre-v2.83.0 envelope — no `party` key at all — must still deserialise.
#[test]
fn s4c_a_legacy_envelope_without_party_still_deserialises() {
    let legacy = serde_json::json!({
        "kind": "anchor_breach",
        "location": "step:Triage",
        "message": "breached",
        "d_letter": null
    });
    let b: BlameContext = serde_json::from_value(legacy).expect("legacy envelope must deserialise");
    assert_eq!(b.kind, BlameKind::AnchorBreach);
    assert_eq!(b.party, None);
}
