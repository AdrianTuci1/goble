//! Cron parsing and next-occurrence math for workflow triggers.
//!
//! People write five-field cron (`0 9 * * *`), while the `cron` crate wants a
//! seconds field. [`normalize_cron`] bridges the two: a five-field expression
//! gains a leading `0`, a six/seven-field expression is used as written. The
//! stored expression is never rewritten, so what the user typed is what the UI
//! shows.

use chrono::{DateTime, Utc};
use cron::Schedule;

use crate::WorkflowError;

/// The expression the `cron` crate understands, derived from `expr`.
///
/// Accepts five fields (minute hour day-of-month month day-of-week), six
/// (seconds first) or seven (with a year).
pub fn normalize_cron(expr: &str) -> Result<String, WorkflowError> {
    let trimmed = expr.trim();
    let fields: Vec<&str> = trimmed.split_whitespace().collect();
    match fields.len() {
        5 => Ok(format!("0 {}", fields.join(" "))),
        6 | 7 => Ok(fields.join(" ")),
        n => Err(WorkflowError::Cron(format!(
            "{trimmed:?} has {n} fields; expected 5 (minute hour day month weekday), 6 (with seconds) or 7 (with a year)"
        ))),
    }
}

/// Parse `expr` into a [`Schedule`].
pub fn parse_cron(expr: &str) -> Result<Schedule, WorkflowError> {
    normalize_cron(expr)?
        .parse()
        .map_err(|e| WorkflowError::Cron(format!("{expr:?}: {e}")))
}

/// Whether `expr` is a cron expression this build can schedule.
pub fn validate_cron(expr: &str) -> Result<(), WorkflowError> {
    parse_cron(expr).map(|_| ())
}

/// The first occurrence strictly after `after`, or `None` when the schedule has
/// no further occurrence (for example a one-shot year in the past).
pub fn next_after(
    expr: &str,
    after: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, WorkflowError> {
    Ok(parse_cron(expr)?.after(&after).next())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn five_field_expressions_gain_a_seconds_field() {
        assert_eq!(normalize_cron("0 9 * * *").unwrap(), "0 0 9 * * *");
        assert_eq!(normalize_cron("*/5 * * * *").unwrap(), "0 */5 * * * *");
    }

    #[test]
    fn six_field_expressions_are_used_as_written() {
        assert_eq!(normalize_cron("30 0 9 * * *").unwrap(), "30 0 9 * * *");
    }

    #[test]
    fn a_bad_field_count_names_the_expected_shape() {
        let err = normalize_cron("every day").unwrap_err();
        assert!(err.to_string().contains("2 fields"), "{err}");
        assert!(err.to_string().contains("expected 5"), "{err}");
    }

    #[test]
    fn next_after_is_strictly_later_and_advances_by_the_step() {
        let nine = Utc.with_ymd_and_hms(2026, 3, 1, 9, 0, 0).unwrap();
        // `after` is exclusive, so a tick exactly on the occurrence moves on.
        let next = next_after("0 9 * * *", nine).unwrap().unwrap();
        assert_eq!(next, Utc.with_ymd_and_hms(2026, 3, 2, 9, 0, 0).unwrap());

        let before = Utc.with_ymd_and_hms(2026, 3, 1, 8, 59, 0).unwrap();
        let next = next_after("0 9 * * *", before).unwrap().unwrap();
        assert_eq!(next, nine);
    }

    #[test]
    fn five_minute_schedules_advance_by_five_minutes() {
        let start = Utc.with_ymd_and_hms(2026, 3, 1, 9, 2, 0).unwrap();
        let first = next_after("*/5 * * * *", start).unwrap().unwrap();
        assert_eq!(first, Utc.with_ymd_and_hms(2026, 3, 1, 9, 5, 0).unwrap());
        let second = next_after("*/5 * * * *", first).unwrap().unwrap();
        assert_eq!(second, Utc.with_ymd_and_hms(2026, 3, 1, 9, 10, 0).unwrap());
    }

    #[test]
    fn validation_rejects_nonsense() {
        assert!(validate_cron("not a cron").is_err());
        assert!(validate_cron("0 9 * * *").is_ok());
    }
}
