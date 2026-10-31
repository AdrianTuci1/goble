//! The desktop's cron loop for workflow triggers.
//!
//! A workflow's `trigger` column is the schedule of record, so a schedule is
//! just an enabled workflow whose trigger is `Trigger::Cron`. Every tick the
//! loop recomputes the next occurrence from the expression and fires a run when
//! the clock crosses it — once per occurrence, because the next occurrence is
//! advanced before the run starts. A freshly seen schedule starts at its next
//! occurrence and is never fired immediately.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use goble_core::agent::Trigger;

use super::DesktopState;

/// How often the desktop checks its cron workflows.
pub const DEFAULT_TICK: Duration = Duration::from_secs(30);

/// Start the loop. Call once, after the event bus is installed, from a state
/// that lives in a tokio runtime. The loop stops when the state is dropped.
pub fn start_workflow_scheduler(state: &Arc<DesktopState>) {
    let weak = Arc::downgrade(state);
    tokio::spawn(async move {
        let mut next: HashMap<String, DateTime<Utc>> = HashMap::new();
        let mut warned: HashSet<String> = HashSet::new();
        loop {
            tokio::time::sleep(DEFAULT_TICK).await;
            let Some(state) = weak.upgrade() else {
                return;
            };
            tick_once(&state, &mut next, &mut warned, Utc::now());
        }
    });
}

/// One scheduler pass. Kept separate from the loop so a test can drive it with
/// a fixed clock.
pub(super) fn tick_once(
    state: &Arc<DesktopState>,
    next: &mut HashMap<String, DateTime<Utc>>,
    warned: &mut HashSet<String>,
    now: DateTime<Utc>,
) {
    let mut live = HashSet::new();
    for info in state.list_workflows() {
        let Trigger::Cron { expression } = &info.trigger else {
            continue;
        };
        live.insert(info.id.clone());
        if !info.enabled {
            next.remove(&info.id);
            continue;
        }
        match plan(expression, now, next.get(&info.id).copied()) {
            Plan::Wait => {}
            Plan::Seed(at) => {
                next.insert(info.id.clone(), at);
            }
            Plan::Fire(at) => {
                next.insert(info.id.clone(), at);
                warned.remove(&info.id);
                if let Err(error) = state.run_workflow(&info.id) {
                    log::error!(
                        "workflow {} did not start on its schedule: {error:#}",
                        info.id
                    );
                }
            }
            Plan::Invalid(message) => {
                next.remove(&info.id);
                if warned.insert(info.id.clone()) {
                    log::warn!(
                        "workflow {} has an invalid cron expression: {message}",
                        info.id
                    );
                }
            }
        }
    }
    next.retain(|id, _| live.contains(id));
    warned.retain(|id| live.contains(id));
}

/// What to do about one cached schedule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Plan {
    /// Not due yet.
    Wait,
    /// First sight: adopt the next occurrence without firing.
    Seed(DateTime<Utc>),
    /// Due now: fire, and move to the occurrence after `now`.
    Fire(DateTime<Utc>),
    /// The expression cannot be scheduled.
    Invalid(String),
}

/// The decision for a schedule cached at `cached`.
pub(super) fn plan(expression: &str, now: DateTime<Utc>, cached: Option<DateTime<Utc>>) -> Plan {
    let advance = || match goble_workflow::schedule::next_after(expression, now) {
        Ok(Some(at)) => Ok(at),
        Ok(None) => Err(String::new()),
        Err(error) => Err(error.to_string()),
    };
    match cached {
        Some(at) if now >= at => match advance() {
            Ok(next) => Plan::Fire(next),
            Err(message) if message.is_empty() => Plan::Wait,
            Err(message) => Plan::Invalid(message),
        },
        Some(_) => Plan::Wait,
        None => match advance() {
            Ok(next) => Plan::Seed(next),
            Err(message) if message.is_empty() => Plan::Wait,
            Err(message) => Plan::Invalid(message),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(hour: u32, minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 3, 1, hour, minute, 0).unwrap()
    }

    #[test]
    fn a_fresh_schedule_is_seeded_and_does_not_fire_immediately() {
        assert_eq!(plan("0 9 * * *", at(8, 0), None), Plan::Seed(at(9, 0)));
    }

    #[test]
    fn a_due_schedule_fires_and_advances_to_the_next_occurrence() {
        assert_eq!(
            plan("0 9 * * *", at(9, 0), Some(at(9, 0))),
            Plan::Fire(at(9, 0) + chrono::Duration::days(1))
        );
    }

    #[test]
    fn a_schedule_waits_until_its_time() {
        assert_eq!(plan("0 9 * * *", at(8, 59), Some(at(9, 0))), Plan::Wait);
    }

    #[test]
    fn two_ticks_inside_one_occurrence_fire_once() {
        // Cross the occurrence, then plan again with the advanced cache.
        let second = Plan::Fire(at(9, 0) + chrono::Duration::days(1));
        let Plan::Fire(next) = second else {
            unreachable!()
        };
        assert_eq!(plan("0 9 * * *", at(9, 0), Some(next)), Plan::Wait);
    }

    #[test]
    fn a_bad_expression_is_reported_once_with_its_reason() {
        match plan("every day", at(9, 0), None) {
            Plan::Invalid(message) => assert!(message.contains("fields"), "{message}"),
            other => panic!("expected invalid, got {other:?}"),
        }
    }
}
