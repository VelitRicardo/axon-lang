//! v2.46.0 — declared cognitive time: the runtime half of `now:`.
//!
//! The cognitive completion of `axon://logic/time_is_an_explicit_input`
//! (v2.27.0): a step (or the program's `context` frame) DECLARES the IANA zone
//! its cognition runs in (`now: "America/Bogota"`, v2.46.0); this module
//! SUPPLIES the instant and RENDERS the deterministic system-prompt line;
//! the envelope RECORDS `(captured_utc, tz, tzdb_version, zones)` so the
//! exact prompt the model saw is reconstructible byte-for-byte.
//!
//! Three laws, mirrored from v2.27.0:
//! - **One instant per run.** The capture happens once (lazily, at the
//!   first `now:`-bearing step) and every subsequent step renders THAT
//!   instant in its declared zone — two steps in one run can never
//! disagree about "now" (plan vivo section 5, the per-run fork).
//! - **The frontend format-checked; the runtime is the authority.** A zone
//!   that passes `axon-T892`'s shape law but is not in the tz database
//!   fails CLOSED here (a loud dispatch error, never a silent omission).
//! - **Replayable.** The rendered line is a pure function of
//!   `(capture, zone, tzdb version)` — [`render_line`] has no clock read.

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

/// The run's single captured instant. `Copy` — cheap to lift out of the
/// shared state without borrowing across a render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemporalCapture {
    pub utc: DateTime<Utc>,
}

impl TemporalCapture {
    /// Capture from the wall clock (production).
    pub fn system() -> Self {
        Self { utc: Utc::now() }
    }

    /// Capture from an explicit instant (tests / replay).
    pub fn at(utc: DateTime<Utc>) -> Self {
        Self { utc }
    }
}

/// Shared per-run temporal state: the lazily-set capture + the zones
/// actually rendered (first-use order, deduplicated). Lives behind
/// `Arc<Mutex<…>>` on the dispatch context so `par` branches share ONE
/// capture and the collector reads the final state after the walk (the
/// v2.21.0 `store_row_counts` discipline — the lock is never held across
/// an `.await`).
#[derive(Debug, Default)]
pub struct TemporalState {
    pub capture: Option<TemporalCapture>,
    pub zones: Vec<String>,
}

/// The envelope/audit record: what instant the run saw, under which tz
/// database, rendered in which declared zones. Elided from the wire when
/// absent (`skip_serializing_if` at the envelope field) — every pre-v2.46.0
/// flow's wire stays byte-identical.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemporalRecord {
    /// RFC 3339 UTC instant of the run's single capture.
    pub captured_utc: String,
    /// IANA tz-database release the render resolved against (v2.27.0).
    pub tzdb_version: String,
    /// Declared zones actually rendered this run, first-use order.
    pub zones: Vec<String>,
}

/// A declared zone that passed the compile-time format law but is not in
/// this build's tz database. Fail-closed: the step errors, loudly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownZone {
    pub zone: String,
}

impl std::fmt::Display for UnknownZone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "declared `now:` zone '{}' is not a known IANA timezone (tzdb {})",
            self.zone,
            crate::window::tz_db_version()
        )
    }
}

/// Render the deterministic system-prompt line for `capture` in `zone`.
/// Pure — no clock read; DST-correct via chrono-tz (the v2.27.0 machinery).
/// The line shape is versioned by convention: changing it is a wire-visible
/// prompt change and must be release-noted.
pub fn render_line(capture: &TemporalCapture, zone: &str) -> Result<String, UnknownZone> {
    let tz = crate::window::parse_tz(zone).ok_or_else(|| UnknownZone {
        zone: zone.to_string(),
    })?;
    let local = capture.utc.with_timezone(&tz);
    Ok(format!(
        "Current datetime: {} ({}; tzdb {}; captured at run start).",
        local.to_rfc3339_opts(SecondsFormat::Secs, false),
        zone,
        crate::window::tz_db_version()
    ))
}

/// The effective zone for a step: its own `now:` overrides the frame's
/// (`context` declaration) — absent both, no temporal injection.
pub fn effective_zone<'a>(
    step_zone: Option<&'a str>,
    frame_zone: Option<&'a str>,
) -> Option<&'a str> {
    step_zone.or(frame_zone)
}

/// Compose the step's effective system prompt: the base prompt plus — when
/// a zone is declared — the rendered temporal line. Captures lazily into
/// `state` (once per run) and records the zone (first-use order). This is
/// the single seam both engines call, so the injected text is identical by
/// construction on the streaming and non-streaming paths.
pub fn compose_effective_system(
    base: &str,
    step_zone: Option<&str>,
    frame_zone: Option<&str>,
    state: &mut TemporalState,
) -> Result<String, UnknownZone> {
    let Some(zone) = effective_zone(step_zone, frame_zone) else {
        return Ok(base.to_string());
    };
    let capture = *state
        .capture
        .get_or_insert_with(TemporalCapture::system);
    let line = render_line(&capture, zone)?;
    if !state.zones.iter().any(|z| z == zone) {
        state.zones.push(zone.to_string());
    }
    Ok(if base.is_empty() {
        line
    } else {
        format!("{base}\n\n{line}")
    })
}

/// Project the run's final temporal state into the envelope record.
/// `None` when no `now:`-bearing step ever rendered (zero wire drift).
pub fn record_of(state: &TemporalState) -> Option<TemporalRecord> {
    let capture = state.capture?;
    if state.zones.is_empty() {
        return None;
    }
    Some(TemporalRecord {
        captured_utc: capture.utc.to_rfc3339_opts(SecondsFormat::Secs, true),
        tzdb_version: crate::window::tz_db_version().to_string(),
        zones: state.zones.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn fixed() -> TemporalCapture {
        // 2026-07-07 19:33:05 UTC — 14:33:05 in Bogotá (UTC-5, no DST).
        TemporalCapture::at(Utc.with_ymd_and_hms(2026, 7, 7, 19, 33, 5).unwrap())
    }

    #[test]
    fn render_is_deterministic_and_zone_correct() {
        let cap = fixed();
        let l1 = render_line(&cap, "America/Bogota").unwrap();
        let l2 = render_line(&cap, "America/Bogota").unwrap();
        assert_eq!(l1, l2, "pure function of (capture, zone, tzdb)");
        assert!(
            l1.contains("2026-07-07T14:33:05-05:00"),
            "Bogotá renders UTC-5: {l1}"
        );
        assert!(l1.contains("(America/Bogota; tzdb "));
        assert!(l1.contains("captured at run start"));
    }

    #[test]
    fn render_utc() {
        let l = render_line(&fixed(), "UTC").unwrap();
        assert!(l.contains("2026-07-07T19:33:05+00:00"), "{l}");
    }

    #[test]
    fn render_is_dst_correct() {
        // 2026-01-07 19:33 UTC — New York is EST (UTC-5) in January…
        let winter = TemporalCapture::at(Utc.with_ymd_and_hms(2026, 1, 7, 19, 33, 5).unwrap());
        let l = render_line(&winter, "America/New_York").unwrap();
        assert!(l.contains("14:33:05-05:00"), "EST: {l}");
        // …and EDT (UTC-4) in July.
        let l = render_line(&fixed(), "America/New_York").unwrap();
        assert!(l.contains("15:33:05-04:00"), "EDT: {l}");
    }

    #[test]
    fn unknown_zone_fails_closed() {
        // Passes the frontend shape law (contains '/', no edge slashes) but
        // is NOT in the tz database — the runtime is the authority.
        let err = render_line(&fixed(), "Fake/Zone").unwrap_err();
        assert_eq!(err.zone, "Fake/Zone");
        assert!(err.to_string().contains("not a known IANA timezone"));
    }

    #[test]
    fn step_zone_overrides_frame_zone() {
        assert_eq!(effective_zone(Some("UTC"), Some("America/Bogota")), Some("UTC"));
        assert_eq!(effective_zone(None, Some("America/Bogota")), Some("America/Bogota"));
        assert_eq!(effective_zone(None, None), None);
    }

    #[test]
    fn compose_injects_once_per_zone_and_shares_one_capture() {
        let mut state = TemporalState {
            capture: Some(fixed()),
            zones: Vec::new(),
        };
        let s1 = compose_effective_system("BASE", Some("UTC"), None, &mut state).unwrap();
        assert!(s1.starts_with("BASE\n\n"), "{s1}");
        assert!(s1.contains("Current datetime:"));
        let s2 =
            compose_effective_system("BASE", None, Some("America/Bogota"), &mut state).unwrap();
        assert!(s2.contains("14:33:05-05:00"), "same capture, Bogotá zone: {s2}");
        // Zones recorded first-use order, deduplicated.
        let _ = compose_effective_system("BASE", Some("UTC"), None, &mut state).unwrap();
        assert_eq!(state.zones, vec!["UTC".to_string(), "America/Bogota".to_string()]);
    }

    #[test]
    fn compose_without_zone_is_identity_and_records_nothing() {
        let mut state = TemporalState::default();
        let s = compose_effective_system("BASE", None, None, &mut state).unwrap();
        assert_eq!(s, "BASE");
        assert!(state.capture.is_none(), "no capture without a declared zone");
        assert!(record_of(&state).is_none());
    }

    #[test]
    fn record_projects_capture_and_zones() {
        let mut state = TemporalState {
            capture: Some(fixed()),
            zones: Vec::new(),
        };
        let _ = compose_effective_system("", Some("America/Bogota"), None, &mut state).unwrap();
        let rec = record_of(&state).expect("record");
        assert_eq!(rec.captured_utc, "2026-07-07T19:33:05Z");
        assert_eq!(rec.zones, vec!["America/Bogota".to_string()]);
        assert_eq!(rec.tzdb_version, crate::window::tz_db_version());
        // Wire shape: serializes with exactly these three keys.
        let json = serde_json::to_string(&rec).unwrap();
        assert!(json.contains("\"captured_utc\""));
        assert!(json.contains("\"tzdb_version\""));
        assert!(json.contains("\"zones\""));
    }
}
