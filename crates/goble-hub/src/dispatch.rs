//! Target selection for a run.
//!
//! A workflow asks for tags, not a hostname. The hub picks the healthiest
//! worker that satisfies every tag and still has capacity. Selection is
//! deterministic: lowest load ratio first, then worker id, so two schedulers
//! looking at the same health snapshot agree without a lock.

use crate::store::RunState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerHealth {
    pub worker_id: String,
    pub tags: Vec<String>,
    pub healthy: bool,
    pub load: u32,
    pub capacity: u32,
    /// Unix seconds of the worker's last heartbeat.
    pub last_heartbeat: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetPolicy {
    pub tags: Vec<String>,
    pub sandbox: String,
}

/// A worker may be selected if it is healthy, has capacity, heartbeated inside
/// the window, and carries every tag the workflow asked for.
pub fn select_worker<'a>(
    policy: &TargetPolicy,
    workers: &'a [WorkerHealth],
    now: i64,
    heartbeat_window: i64,
) -> Option<&'a WorkerHealth> {
    workers
        .iter()
        .filter(|worker| worker.healthy)
        .filter(|worker| worker.capacity > 0 && worker.load < worker.capacity)
        .filter(|worker| now.saturating_sub(worker.last_heartbeat) <= heartbeat_window)
        .filter(|worker| {
            policy
                .tags
                .iter()
                .all(|tag| worker.tags.iter().any(|candidate| candidate == tag))
        })
        .min_by(|left, right| {
            load_ratio(left)
                .cmp(&load_ratio(right))
                .then_with(|| left.worker_id.cmp(&right.worker_id))
        })
}

/// The reaper's rule: a sandbox never outlives its run's deadline plus a grace
/// period, and a terminal run's sandbox is always collected. A provider that
/// cannot list sandboxes uses the worker's own timer instead.
pub fn should_reap(state: RunState, deadline: Option<i64>, now: i64, grace: i64) -> bool {
    if state.is_terminal() {
        return true;
    }
    deadline
        .map(|deadline| now > deadline.saturating_add(grace))
        .unwrap_or(false)
}

/// Load as a fraction in basis points, so `1/3` and `2/6` compare equally and
/// integer division does not throw away the difference between 1/1000 and
/// 999/1000.
fn load_ratio(worker: &WorkerHealth) -> u64 {
    if worker.capacity == 0 {
        return u64::MAX;
    }
    (worker.load as u64 * 10_000) / worker.capacity as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worker(id: &str, tags: &[&str], load: u32, capacity: u32, heartbeat: i64) -> WorkerHealth {
        WorkerHealth {
            worker_id: id.into(),
            tags: tags.iter().map(|tag| tag.to_string()).collect(),
            healthy: true,
            load,
            capacity,
            last_heartbeat: heartbeat,
        }
    }

    fn policy(tags: &[&str]) -> TargetPolicy {
        TargetPolicy {
            tags: tags.iter().map(|tag| tag.to_string()).collect(),
            sandbox: "none".into(),
        }
    }

    #[test]
    fn the_least_loaded_matching_worker_is_selected() {
        let workers = vec![
            worker("busy", &["builder"], 7, 8, 100),
            worker("idle", &["builder"], 1, 8, 100),
            worker("half", &["builder"], 4, 8, 100),
        ];
        let selected = select_worker(&policy(&["builder"]), &workers, 100, 60).unwrap();
        assert_eq!(selected.worker_id, "idle");
    }

    #[test]
    fn load_ratio_is_used_rather_than_absolute_load() {
        let workers = vec![
            worker("small", &["builder"], 1, 2, 100),
            worker("large", &["builder"], 2, 16, 100),
        ];
        let selected = select_worker(&policy(&["builder"]), &workers, 100, 60).unwrap();
        assert_eq!(selected.worker_id, "large");
    }

    #[test]
    fn every_tag_must_match() {
        let workers = vec![
            worker("linux-only", &["builder", "linux"], 0, 8, 100),
            worker("both", &["builder", "linux", "gpu"], 2, 8, 100),
        ];
        let selected = select_worker(&policy(&["builder", "linux"]), &workers, 100, 60).unwrap();
        assert_eq!(selected.worker_id, "linux-only");
        assert_eq!(
            select_worker(&policy(&["gpu"]), &workers, 100, 60)
                .unwrap()
                .worker_id,
            "both"
        );
        assert!(select_worker(&policy(&["windows"]), &workers, 100, 60).is_none());
    }

    #[test]
    fn an_empty_policy_matches_any_healthy_worker() {
        let workers = vec![worker("any", &[], 0, 8, 100)];
        assert!(select_worker(&policy(&[]), &workers, 100, 60).is_some());
    }

    #[test]
    fn unhealthy_full_and_stale_workers_are_skipped() {
        let mut unhealthy = worker("unhealthy", &["builder"], 0, 8, 100);
        unhealthy.healthy = false;
        let full = worker("full", &["builder"], 8, 8, 100);
        let stale = worker("stale", &["builder"], 0, 8, 0);
        let workers = vec![unhealthy, full, stale];
        assert!(select_worker(&policy(&["builder"]), &workers, 100, 60).is_none());
    }

    #[test]
    fn a_worker_with_zero_capacity_is_never_selected() {
        let workers = vec![worker("zero", &["builder"], 0, 0, 100)];
        assert!(select_worker(&policy(&["builder"]), &workers, 100, 60).is_none());
    }

    #[test]
    fn ties_are_broken_by_worker_id() {
        let workers = vec![
            worker("zeta", &["builder"], 1, 8, 100),
            worker("alpha", &["builder"], 1, 8, 100),
        ];
        let selected = select_worker(&policy(&["builder"]), &workers, 100, 60).unwrap();
        assert_eq!(selected.worker_id, "alpha");
    }

    #[test]
    fn the_heartbeat_window_includes_the_boundary() {
        let workers = vec![worker("edge", &["builder"], 0, 8, 40)];
        assert!(select_worker(&policy(&["builder"]), &workers, 100, 60).is_some());
        assert!(select_worker(&policy(&["builder"]), &workers, 101, 60).is_none());
    }

    #[test]
    fn a_terminal_run_reaps_its_sandbox() {
        assert!(should_reap(RunState::Succeeded, None, 100, 900));
        assert!(should_reap(RunState::Failed, Some(100), 101, 900));
        assert!(should_reap(RunState::Cancelled, Some(100), 101, 900));
    }

    #[test]
    fn a_live_run_keeps_its_sandbox_until_the_grace_period_ends() {
        assert!(!should_reap(RunState::Running, Some(1_000), 1_100, 900));
        assert!(!should_reap(RunState::Queued, None, 10_000, 900));
        assert!(should_reap(RunState::Running, Some(1_000), 1_901, 900));
    }
}
