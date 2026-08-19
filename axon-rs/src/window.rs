//! v2.27.0 — the runtime for the `window` temporal execution guard (v2.27.0).
//!
//! Pure, total, timezone-aware functions over an [`IRWindow`]. The frontend
//! only FORMAT-checks the timezone string (it is zero-dependency); here we do
//! the authoritative IANA resolution + the DST-correct day/hour membership math
//! via `chrono-tz`. Every decision is a pure function of `(now, the window, the
//! tz-database version)` — the v2.27.0 doctrine `axon://logic/time_is_an_explicit_input`.
//!
//! Granularity is the hour (the `window` grammar's `hours: 9..18` are inclusive
//! 0–23 bounds). v2.27.0 adds holiday exclusion: a tick whose LOCAL date is
//! in the window's `exclude` set (ISO `YYYY-MM-DD` literals) is OUTSIDE the
//! window regardless of the hour spans.

use chrono::{DateTime, Datelike, Duration, Timelike, Utc, Weekday};
use chrono_tz::Tz;

use crate::ir_nodes::IRWindow;

/// Resolve an IANA timezone name → [`Tz`]. `None` for an unknown name — this is
/// the AUTHORITATIVE membership check the frontend's format check defers to.
pub fn parse_tz(name: &str) -> Option<Tz> {
    name.trim().parse::<Tz>().ok()
}

/// v2.27.0 — the IANA timezone-database release this build resolves against
/// (e.g. `"2024a"`). A window decision is a pure function of `(now, the window,
/// THIS version)` — recording it alongside a deferred/guarded run makes the
/// decision replayable across a tz-db upgrade (the `time_is_an_explicit_input`
/// doctrine). An upstream extension point: the enterprise defer-ledger audits it
/// rather than re-deriving a tz-db version of its own.
pub fn tz_db_version() -> &'static str {
    chrono_tz::IANA_TZDB_VERSION
}

/// Map a weekday name (`Mon`..`Sun`) to a chrono [`Weekday`].
fn weekday_of(name: &str) -> Option<Weekday> {
    Some(match name {
        "Mon" => Weekday::Mon,
        "Tue" => Weekday::Tue,
        "Wed" => Weekday::Wed,
        "Thu" => Weekday::Thu,
        "Fri" => Weekday::Fri,
        "Sat" => Weekday::Sat,
        "Sun" => Weekday::Sun,
        _ => return None,
    })
}

/// Is `wd` within the inclusive weekday range `[start, end]`? Supports wrap-
/// around: `Fri..Mon` covers Fri, Sat, Sun, Mon.
fn weekday_in_range(wd: Weekday, start: Weekday, end: Weekday) -> bool {
    let (w, s, e) = (
        wd.num_days_from_monday(),
        start.num_days_from_monday(),
        end.num_days_from_monday(),
    );
    if s <= e {
        s <= w && w <= e
    } else {
        w >= s || w <= e
    }
}

/// v2.27.0 — is `now` (UTC) inside ANY allowed span of `window`, evaluated in
/// the window's timezone? `None` when the timezone is not a valid IANA name
/// (the caller fail-closes). A malformed span never matches (the v2.27.0 type
/// checker rejects those at compile time).
pub fn is_in_window(now: DateTime<Utc>, window: &IRWindow) -> Option<bool> {
    let tz = parse_tz(&window.timezone)?;
    let local = now.with_timezone(&tz);
    // v2.27.0 — a holiday (an excluded LOCAL date) is OUTSIDE the window
    // regardless of the hour spans. The dates are ISO `YYYY-MM-DD` literals
    // (compile-validated, `axon-T826`); compare `now`'s local date the same way.
    if !window.exclude.is_empty() {
        let local_date = local.format("%Y-%m-%d").to_string();
        if window.exclude.iter().any(|d| d == &local_date) {
            return Some(false);
        }
    }
    let wd = local.weekday();
    let hour = local.hour() as i64;
    for span in &window.allow {
        let (Some(ds), Some(de)) = (weekday_of(&span.day_start), weekday_of(&span.day_end))
        else {
            continue;
        };
        if weekday_in_range(wd, ds, de) && span.hour_start <= hour && hour <= span.hour_end {
            return Some(true);
        }
    }
    Some(false)
}

/// v2.27.0 — the next instant ≥ `now` that is inside the window (the input to
/// the v2.27.0 defer ledger). Steps hour-by-hour, bounded to one week + a margin
/// (a non-empty window opens within 7 days). `None` if the timezone is invalid
/// or — defensively — no span opens within the bound. Hour-granular: the
/// returned instant is the top of the opening UTC hour.
pub fn next_window_open(now: DateTime<Utc>, window: &IRWindow) -> Option<DateTime<Utc>> {
    parse_tz(&window.timezone)?; // authoritative tz validation, once
    let mut probe = now
        .with_minute(0)
        .and_then(|t| t.with_second(0))
        .and_then(|t| t.with_nanosecond(0))?;
    for _ in 0..(8 * 24) {
        if is_in_window(probe, window) == Some(true) {
            return Some(probe);
        }
        probe += Duration::hours(1);
    }
    None
}

/// v2.27.0 — the supervisor's decision for a single scheduled tick under a
/// bound `window`. Computed by [`decide`]; honored by the OSS single-process
/// daemon driver and (for `Defer`) the v2.27.0 enterprise defer-ledger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowAction {
    /// Inside the window — fire normally.
    Fire,
    /// Outside + `on_outside: skip` — drop the tick (fire-forward, like an
    /// unguarded daemon that simply had nothing to do this minute). Also the
    /// fail-closed action when the timezone cannot be resolved.
    Skip,
    /// Outside + `on_outside: warn` — fire anyway, but the caller audits a
    /// `window:outside` warning so the breach is observable.
    Warn,
    /// Outside + `on_outside: defer` — the tick should run at the next window
    /// opening. `open_at` is that instant (the v2.27.0 defer-ledger input), or
    /// `None` if no opening was found within the bound. The OSS single-process
    /// supervisor cannot persist a ledger, so it degrades this to a logged skip;
    /// the enterprise supervisor records a coalesced fire-once row.
    Defer { open_at: Option<DateTime<Utc>> },
}

/// v2.27.0 — decide what to do with a tick firing at `now` under `window`.
/// Pure + total. An unresolvable timezone fails CLOSED to [`WindowAction::Skip`]
/// (never fire under a guard we cannot evaluate). `on_outside` has already been
/// catalog-checked at compile time (`axon-T824`); any non-`warn`/`defer` value
/// (i.e. `skip`) and defensively any unknown maps to `Skip`.
pub fn decide(now: DateTime<Utc>, window: &IRWindow) -> WindowAction {
    match is_in_window(now, window) {
        Some(true) => WindowAction::Fire,
        Some(false) => match window.on_outside.as_str() {
            "warn" => WindowAction::Warn,
            "defer" => WindowAction::Defer {
                open_at: next_window_open(now, window),
            },
            _ => WindowAction::Skip,
        },
        None => WindowAction::Skip,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir_nodes::{IRWindow, IRWindowSpan};

    fn span(d0: &str, d1: &str, h0: i64, h1: i64) -> IRWindowSpan {
        IRWindowSpan {
            day_start: d0.into(),
            day_end: d1.into(),
            hour_start: h0,
            hour_end: h1,
        }
    }
    fn window(tz: &str, allow: Vec<IRWindowSpan>) -> IRWindow {
        window_on(tz, allow, "defer")
    }
    fn window_on(tz: &str, allow: Vec<IRWindowSpan>, on_outside: &str) -> IRWindow {
        IRWindow {
            node_type: "window",
            source_line: 0,
            source_column: 0,
            name: "W".into(),
            timezone: tz.into(),
            allow,
            exclude: Vec::new(),
            on_outside: on_outside.into(),
        }
    }
    /// A UTC instant from an ISO-ish string `YYYY-MM-DDТHH:MM:SSZ`.
    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    // ── tz resolution (the frontend↔runtime parity boundary) ─────────────

    #[test]
    fn parse_tz_accepts_real_iana_names_rejects_others() {
        for ok in ["America/Bogota", "UTC", "Europe/Madrid", "Asia/Kolkata", "America/New_York"] {
            assert!(parse_tz(ok).is_some(), "{ok} should resolve");
        }
        for bad in ["Bogota", "Mars/Olympus", "", "PST8PDT_typo"] {
            assert!(parse_tz(bad).is_none(), "{bad} should NOT resolve");
        }
    }

    #[test]
    fn tz_db_version_is_a_nonempty_iana_release() {
        // e.g. "2024a" — a 4-digit year + a lowercase release letter.
        let v = tz_db_version();
        assert!(!v.is_empty());
        assert!(v.chars().next().unwrap().is_ascii_digit(), "starts with the year: {v}");
    }

    #[test]
    fn invalid_tz_is_none_fail_closed() {
        let w = window("Bogota", vec![span("Mon", "Fri", 9, 18)]);
        assert_eq!(is_in_window(utc("2026-06-29T14:00:00Z"), &w), None);
        assert_eq!(next_window_open(utc("2026-06-29T14:00:00Z"), &w), None);
    }

    // ── is_in_window — the parity corpus ─────────────────────────────────

    #[test]
    fn window_parity_corpus() {
        // BusinessHours: America/Bogota (UTC-5, no DST), Mon..Fri 9..18.
        let bh = window("America/Bogota", vec![span("Mon", "Fri", 9, 18)]);
        // 2026-06-29 is a MONDAY. 14:00 UTC = 09:00 Bogota → in window.
        assert_eq!(is_in_window(utc("2026-06-29T14:00:00Z"), &bh), Some(true));
        // 13:00 UTC = 08:00 Bogota → before 9 → outside.
        assert_eq!(is_in_window(utc("2026-06-29T13:00:00Z"), &bh), Some(false));
        // 00:00 UTC Mon = 19:00 Sun Bogota → Sunday + late → outside.
        assert_eq!(is_in_window(utc("2026-06-29T00:00:00Z"), &bh), Some(false));
        // Saturday (2026-07-04) 17:00 UTC = 12:00 Bogota → weekend → outside.
        assert_eq!(is_in_window(utc("2026-07-04T17:00:00Z"), &bh), Some(false));

        // UTC window, all week, 0..23 → always in.
        let always = window("UTC", vec![span("Mon", "Sun", 0, 23)]);
        assert_eq!(is_in_window(utc("2026-01-01T03:00:00Z"), &always), Some(true));

        // Weekday wrap-around: Fri..Mon covers Saturday.
        let weekend = window("UTC", vec![span("Fri", "Mon", 0, 23)]);
        assert_eq!(is_in_window(utc("2026-07-04T12:00:00Z"), &weekend), Some(true)); // Sat
        assert_eq!(is_in_window(utc("2026-07-01T12:00:00Z"), &weekend), Some(false)); // Wed
    }

    #[test]
    fn excluded_holiday_is_outside_regardless_of_hours() {
        // BusinessHours Bogota Mon..Fri 9..18, with 2026-06-29 (a Monday) excluded.
        let mut bh = window("America/Bogota", vec![span("Mon", "Fri", 9, 18)]);
        bh.exclude = vec!["2026-06-29".to_string()];
        // 14:00 UTC = 09:00 Bogota Monday — inside the hour span, but the date is
        // a holiday → OUTSIDE.
        assert_eq!(is_in_window(utc("2026-06-29T14:00:00Z"), &bh), Some(false));
        // The NEXT day (2026-06-30 Tuesday) at 09:00 Bogota is back inside.
        assert_eq!(is_in_window(utc("2026-06-30T14:00:00Z"), &bh), Some(true));
        // The exclusion is evaluated in the window's tz: 2026-06-30 03:00 UTC is
        // still 2026-06-29 22:00 Bogota — the holiday — so OUTSIDE (also after 18).
        assert_eq!(is_in_window(utc("2026-06-30T03:00:00Z"), &bh), Some(false));
    }

    #[test]
    fn defer_skips_a_holiday_to_the_next_open_day() {
        // With Monday excluded, the next opening from Monday 08:00 Bogota is
        // TUESDAY 09:00 Bogota (= 2026-06-30 14:00 UTC), not Monday 09:00.
        let mut bh = window("America/Bogota", vec![span("Mon", "Fri", 9, 18)]);
        bh.exclude = vec!["2026-06-29".to_string()];
        let open = next_window_open(utc("2026-06-29T13:00:00Z"), &bh).unwrap();
        assert_eq!(open, utc("2026-06-30T14:00:00Z"));
    }

    #[test]
    fn dst_shifts_the_local_hour() {
        // New York 9..10, Mon..Sun. 13:00 UTC is 09:00 EDT (summer, UTC-4) →
        // in window; in winter (UTC-5) the same 13:00 UTC is 08:00 EST → out.
        let w = window("America/New_York", vec![span("Mon", "Sun", 9, 10)]);
        assert_eq!(is_in_window(utc("2026-07-06T13:00:00Z"), &w), Some(true)); // EDT
        assert_eq!(is_in_window(utc("2026-01-05T13:00:00Z"), &w), Some(false)); // EST
    }

    // ── next_window_open ─────────────────────────────────────────────────

    #[test]
    fn next_window_open_finds_the_next_opening() {
        let bh = window("America/Bogota", vec![span("Mon", "Fri", 9, 18)]);
        // 2026-06-29 08:00 Bogota = 13:00 UTC (Monday, before open). Next open
        // is 09:00 Bogota = 14:00 UTC the same day.
        let open = next_window_open(utc("2026-06-29T13:00:00Z"), &bh).unwrap();
        assert_eq!(open, utc("2026-06-29T14:00:00Z"));
        // If already inside, returns the current hour.
        let inside = next_window_open(utc("2026-06-29T15:30:00Z"), &bh).unwrap();
        assert_eq!(inside, utc("2026-06-29T15:00:00Z"));
    }

    // ── decide — the supervisor's per-tick action ────────────────────────

    #[test]
    fn decide_inside_fires_regardless_of_on_outside() {
        // Monday 14:00 UTC = 09:00 Bogota → inside. on_outside is irrelevant.
        for oo in ["skip", "warn", "defer"] {
            let w = window_on("America/Bogota", vec![span("Mon", "Fri", 9, 18)], oo);
            assert_eq!(decide(utc("2026-06-29T14:00:00Z"), &w), WindowAction::Fire, "{oo}");
        }
    }

    #[test]
    fn decide_outside_honors_on_outside() {
        let spans = || vec![span("Mon", "Fri", 9, 18)];
        // 13:00 UTC = 08:00 Bogota Monday → outside (before 9).
        let t = utc("2026-06-29T13:00:00Z");

        let skip = window_on("America/Bogota", spans(), "skip");
        assert_eq!(decide(t, &skip), WindowAction::Skip);

        let warn = window_on("America/Bogota", spans(), "warn");
        assert_eq!(decide(t, &warn), WindowAction::Warn);

        let defer = window_on("America/Bogota", spans(), "defer");
        // Defers to the next opening — 09:00 Bogota = 14:00 UTC the same day.
        assert_eq!(
            decide(t, &defer),
            WindowAction::Defer { open_at: Some(utc("2026-06-29T14:00:00Z")) }
        );
    }

    #[test]
    fn decide_invalid_tz_fails_closed_to_skip() {
        let w = window_on("Bogota", vec![span("Mon", "Fri", 9, 18)], "warn");
        // Even with on_outside: warn, an unresolvable tz never fires.
        assert_eq!(decide(utc("2026-06-29T14:00:00Z"), &w), WindowAction::Skip);
    }
}
