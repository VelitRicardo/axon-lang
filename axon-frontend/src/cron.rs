//! v2.4.0 — POSIX 5-field cron schedule parser + validator.
//!
//! The static half of the `daemon` scheduling surface. A `listen` whose channel
//! is `"cron:<expr>"` is a **time-based** trigger: a `TimerSource` (v2.4.0) fires
//! the handler on the cron cadence. This module parses + validates the 5-field
//! expression `minute hour day-of-month month day-of-week` (with `*`, ranges
//! `a-b`, lists `a,b`, and steps `*/n` / `a-b/n`) and EXPANDS each field to its
//! sorted set of matching values — the same representation the v2.4.0 scheduler
//! uses to compute the next fire time (membership test per field). Validation
//! and scheduling share one source of truth, so a schedule that type-checks is
//! exactly a schedule the runtime can fire.
//!
//! Day-of-week accepts both `0` and `7` for Sunday (POSIX); `7` is normalised to
//! `0`. Field order is the standard 5-field cron (no seconds field, no
//! `@`-nicknames — deferred; named in the v2.4.0 plan).

use std::collections::BTreeSet;
use std::fmt;

/// A parsed, validated cron schedule: each field expanded to its sorted set of
/// matching values. Cheap to test membership against a wall-clock field (v2.4.0).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronSchedule {
    /// Matching minutes (0–59).
    pub minute: Vec<u32>,
    /// Matching hours (0–23).
    pub hour: Vec<u32>,
    /// Matching days of month (1–31).
    pub day_of_month: Vec<u32>,
    /// Matching months (1–12).
    pub month: Vec<u32>,
    /// Matching days of week (0–6, Sunday = 0; `7` is normalised to `0`).
    pub day_of_week: Vec<u32>,
}

/// Why a cron expression failed validation. Carries enough to render a precise
/// `axon-E0789` diagnostic (which field, what value, why).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CronError {
    /// A 5-field expression was required; `found` fields were present.
    WrongFieldCount { found: usize },
    /// One field did not parse / was out of range.
    BadField {
        /// 0-based field index.
        index: usize,
        /// Human field name (`"minute"`, `"day-of-week"`, …).
        name: &'static str,
        /// The offending field spelling.
        value: String,
        /// What's wrong.
        reason: String,
    },
}

impl fmt::Display for CronError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CronError::WrongFieldCount { found } => write!(
                f,
                "a cron expression needs exactly 5 fields \
                 (minute hour day-of-month month day-of-week), found {found}"
            ),
            CronError::BadField { name, value, reason, .. } => {
                write!(f, "cron {name} field '{value}': {reason}")
            }
        }
    }
}

/// `(name, lo, hi)` per field. Day-of-week's upper bound is 7 (POSIX Sunday);
/// 7 is folded to 0 after expansion.
const FIELD_BOUNDS: [(&str, u32, u32); 5] = [
    ("minute", 0, 59),
    ("hour", 0, 23),
    ("day-of-month", 1, 31),
    ("month", 1, 12),
    ("day-of-week", 0, 7),
];

impl CronSchedule {
    /// Parse + validate a 5-field cron expression. `Ok` ⇒ the v2.4.0 scheduler
    /// can fire it; `Err` ⇒ a precise `CronError`.
    pub fn parse(expr: &str) -> Result<CronSchedule, CronError> {
        let fields: Vec<&str> = expr.split_whitespace().collect();
        if fields.len() != 5 {
            return Err(CronError::WrongFieldCount { found: fields.len() });
        }
        let mut sets: Vec<Vec<u32>> = Vec::with_capacity(5);
        for (i, (name, lo, hi)) in FIELD_BOUNDS.iter().enumerate() {
            sets.push(parse_field(fields[i], i, name, *lo, *hi)?);
        }
        // Day-of-week: fold 7 → 0 (both mean Sunday).
        let mut dow: BTreeSet<u32> = sets[4].iter().map(|&v| if v == 7 { 0 } else { v }).collect();
        let _ = &mut dow;
        Ok(CronSchedule {
            minute: sets[0].clone(),
            hour: sets[1].clone(),
            day_of_month: sets[2].clone(),
            month: sets[3].clone(),
            day_of_week: dow.into_iter().collect(),
        })
    }
}

fn bad(index: usize, name: &'static str, value: &str, reason: impl Into<String>) -> CronError {
    CronError::BadField {
        index,
        name,
        value: value.to_string(),
        reason: reason.into(),
    }
}

/// Parse one comma-separated field into its expanded, sorted, deduped values.
fn parse_field(
    spec: &str,
    index: usize,
    name: &'static str,
    lo: u32,
    hi: u32,
) -> Result<Vec<u32>, CronError> {
    if spec.is_empty() {
        return Err(bad(index, name, spec, "empty field"));
    }
    let mut values: BTreeSet<u32> = BTreeSet::new();
    for item in spec.split(',') {
        // Optional `/step` suffix.
        let (range_part, step) = match item.split_once('/') {
            Some((r, s)) => {
                let step = s
                    .parse::<u32>()
                    .map_err(|_| bad(index, name, spec, format!("invalid step '{s}'")))?;
                if step == 0 {
                    return Err(bad(index, name, spec, "step cannot be 0"));
                }
                (r, step)
            }
            None => (item, 1u32),
        };
        // The base range: `*`, `a-b`, or a single `a`.
        let (start, end) = if range_part == "*" {
            (lo, hi)
        } else if let Some((a, b)) = range_part.split_once('-') {
            let a = a
                .parse::<u32>()
                .map_err(|_| bad(index, name, spec, format!("invalid range start '{a}'")))?;
            let b = b
                .parse::<u32>()
                .map_err(|_| bad(index, name, spec, format!("invalid range end '{b}'")))?;
            (a, b)
        } else {
            let v = range_part
                .parse::<u32>()
                .map_err(|_| bad(index, name, spec, format!("invalid value '{range_part}'")))?;
            (v, v)
        };
        if start > end {
            return Err(bad(
                index,
                name,
                spec,
                format!("range start {start} is greater than end {end}"),
            ));
        }
        if start < lo || end > hi {
            return Err(bad(
                index,
                name,
                spec,
                format!("value out of range (allowed {lo}–{hi})"),
            ));
        }
        let mut v = start;
        while v <= end {
            values.insert(v);
            v += step;
        }
    }
    Ok(values.into_iter().collect())
}

/// If `channel` is a cron channel (`"cron:<expr>"`), return the trimmed `<expr>`;
/// otherwise `None`. The single recogniser shared by the type-checker (v2.4.0
/// validation) and the runtime (v2.4.0 scheduling) so the prefix is defined once.
pub fn cron_expr(channel: &str) -> Option<&str> {
    channel.strip_prefix("cron:").map(str::trim)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_five_minutes_expands() {
        let s = CronSchedule::parse("*/5 * * * *").expect("valid");
        assert_eq!(s.minute, vec![0, 5, 10, 15, 20, 25, 30, 35, 40, 45, 50, 55]);
        assert_eq!(s.hour.len(), 24);
        assert_eq!(s.day_of_week, (0..=6).collect::<Vec<_>>());
    }

    #[test]
    fn ranges_lists_and_steps() {
        let s = CronSchedule::parse("0 9-17 * * 1-5").expect("valid weekday business hours");
        assert_eq!(s.minute, vec![0]);
        assert_eq!(s.hour, (9..=17).collect::<Vec<_>>());
        assert_eq!(s.day_of_week, vec![1, 2, 3, 4, 5]);

        let s2 = CronSchedule::parse("0,30 0-6/2 1 * *").expect("valid");
        assert_eq!(s2.minute, vec![0, 30]);
        assert_eq!(s2.hour, vec![0, 2, 4, 6]);
        assert_eq!(s2.day_of_month, vec![1]);
    }

    #[test]
    fn sunday_seven_folds_to_zero() {
        let s = CronSchedule::parse("0 0 * * 7").expect("valid");
        assert_eq!(s.day_of_week, vec![0], "7 is normalised to 0 (Sunday)");
    }

    #[test]
    fn wrong_field_count_rejected() {
        assert_eq!(
            CronSchedule::parse("*/5 * * *"),
            Err(CronError::WrongFieldCount { found: 4 })
        );
        assert!(matches!(
            CronSchedule::parse("* * * * * *"),
            Err(CronError::WrongFieldCount { found: 6 })
        ));
    }

    #[test]
    fn out_of_range_and_malformed_rejected() {
        // minute 60 out of range (0–59).
        assert!(matches!(
            CronSchedule::parse("60 * * * *"),
            Err(CronError::BadField { name: "minute", .. })
        ));
        // month 13 out of range.
        assert!(matches!(
            CronSchedule::parse("0 0 1 13 *"),
            Err(CronError::BadField { name: "month", .. })
        ));
        // step 0 is invalid.
        assert!(matches!(
            CronSchedule::parse("*/0 * * * *"),
            Err(CronError::BadField { .. })
        ));
        // non-numeric.
        assert!(matches!(
            CronSchedule::parse("abc * * * *"),
            Err(CronError::BadField { .. })
        ));
        // inverted range.
        assert!(matches!(
            CronSchedule::parse("30-10 * * * *"),
            Err(CronError::BadField { .. })
        ));
    }

    #[test]
    fn cron_expr_recogniser() {
        assert_eq!(cron_expr("cron:*/5 * * * *"), Some("*/5 * * * *"));
        assert_eq!(cron_expr("cron: 0 0 * * * "), Some("0 0 * * *"));
        assert_eq!(cron_expr("user_events"), None);
        assert_eq!(cron_expr("ticks"), None);
    }
}
