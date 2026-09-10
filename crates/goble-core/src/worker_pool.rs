use crate::worker::{WorkerId, WorkerStatus};

/// Lightweight snapshot of a worker used for scheduling decisions.
#[derive(Debug, Clone)]
pub struct WorkerSnapshot {
    pub worker_id: WorkerId,
    pub name: String,
    pub url: String,
    pub status: WorkerStatus,
    pub load: u8,
    pub tags: Vec<String>,
}

impl WorkerSnapshot {
    pub fn is_available(&self) -> bool {
        matches!(self.status, WorkerStatus::Online | WorkerStatus::Idle)
    }
}

/// Strategy for picking a worker from a pool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerPoolStrategy {
    RoundRobin,
    LowestLoad,
    TaggedFirst { tag: String },
}

/// Holds snapshots of known workers and selects one for a task.
#[derive(Debug, Clone)]
pub struct WorkerPool {
    strategy: WorkerPoolStrategy,
    last_index: usize,
}

impl WorkerPool {
    pub fn new(strategy: WorkerPoolStrategy) -> Self {
        Self {
            strategy,
            last_index: 0,
        }
    }

    pub fn select<'a>(&mut self, workers: &'a [WorkerSnapshot]) -> Option<&'a WorkerSnapshot> {
        let available: Vec<&'a WorkerSnapshot> =
            workers.iter().filter(|w| w.is_available()).collect();
        if available.is_empty() {
            return None;
        }

        match self.strategy {
            WorkerPoolStrategy::RoundRobin => {
                let idx = self.last_index % available.len();
                self.last_index = idx + 1;
                Some(available[idx])
            }
            WorkerPoolStrategy::LowestLoad => available.into_iter().min_by_key(|w| w.load),
            WorkerPoolStrategy::TaggedFirst { ref tag } => {
                let tagged = available.iter().find(|w| w.tags.contains(tag)).copied();
                tagged.or_else(|| available.into_iter().next())
            }
        }
    }

    pub fn set_strategy(&mut self, strategy: WorkerPoolStrategy) {
        self.strategy = strategy;
        self.last_index = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_worker(id: &str, load: u8, status: WorkerStatus, tags: Vec<&str>) -> WorkerSnapshot {
        WorkerSnapshot {
            worker_id: WorkerId(id.to_string()),
            name: id.to_string(),
            url: format!("ws://{}", id),
            status,
            load,
            tags: tags.into_iter().map(|t| t.to_string()).collect(),
        }
    }

    #[test]
    fn test_round_robin() {
        let mut pool = WorkerPool::new(WorkerPoolStrategy::RoundRobin);
        let workers = vec![
            make_worker("a", 0, WorkerStatus::Online, vec![]),
            make_worker("b", 0, WorkerStatus::Online, vec![]),
        ];
        let first = pool.select(&workers).unwrap().worker_id.clone();
        let second = pool.select(&workers).unwrap().worker_id.clone();
        let third = pool.select(&workers).unwrap().worker_id.clone();
        assert_eq!(first, WorkerId("a".to_string()));
        assert_eq!(second, WorkerId("b".to_string()));
        assert_eq!(third, WorkerId("a".to_string()));
    }

    #[test]
    fn test_lowest_load() {
        let mut pool = WorkerPool::new(WorkerPoolStrategy::LowestLoad);
        let workers = vec![
            make_worker("a", 5, WorkerStatus::Online, vec![]),
            make_worker("b", 1, WorkerStatus::Online, vec![]),
            make_worker("c", 3, WorkerStatus::Online, vec![]),
        ];
        let selected = pool.select(&workers).unwrap();
        assert_eq!(selected.worker_id.0, "b");
    }

    #[test]
    fn test_tagged_first() {
        let mut pool = WorkerPool::new(WorkerPoolStrategy::TaggedFirst {
            tag: "gpu".to_string(),
        });
        let workers = vec![
            make_worker("a", 0, WorkerStatus::Online, vec!["cpu"]),
            make_worker("b", 0, WorkerStatus::Online, vec!["gpu"]),
        ];
        let selected = pool.select(&workers).unwrap();
        assert_eq!(selected.worker_id.0, "b");
    }

    #[test]
    fn test_offline_workers_skipped() {
        let mut pool = WorkerPool::new(WorkerPoolStrategy::LowestLoad);
        let workers = vec![
            make_worker("a", 0, WorkerStatus::Offline, vec![]),
            make_worker("b", 2, WorkerStatus::Online, vec![]),
        ];
        let selected = pool.select(&workers).unwrap();
        assert_eq!(selected.worker_id.0, "b");
    }

    #[test]
    fn test_empty_pool() {
        let mut pool = WorkerPool::new(WorkerPoolStrategy::RoundRobin);
        let workers: Vec<WorkerSnapshot> = vec![];
        assert!(pool.select(&workers).is_none());
    }

    #[test]
    fn test_tagged_group_selection() {
        let mut pool = WorkerPool::new(WorkerPoolStrategy::RoundRobin);
        let workers = vec![
            make_worker("cpu-1", 0, WorkerStatus::Online, vec!["cpu"]),
            make_worker("cpu-2", 0, WorkerStatus::Online, vec!["cpu"]),
            make_worker("gpu-1", 0, WorkerStatus::Online, vec!["gpu"]),
            make_worker("gpu-2", 0, WorkerStatus::Online, vec!["gpu"]),
        ];
        let gpu_group: Vec<WorkerSnapshot> = workers
            .into_iter()
            .filter(|w| w.tags.iter().any(|t| t == "gpu"))
            .collect();
        let first = pool.select(&gpu_group).unwrap().worker_id.clone();
        let second = pool.select(&gpu_group).unwrap().worker_id.clone();
        let third = pool.select(&gpu_group).unwrap().worker_id.clone();
        assert_eq!(first, WorkerId("gpu-1".to_string()));
        assert_eq!(second, WorkerId("gpu-2".to_string()));
        assert_eq!(third, WorkerId("gpu-1".to_string()));
    }

    #[test]
    fn test_tagged_first_fallback_to_any_available() {
        let mut pool = WorkerPool::new(WorkerPoolStrategy::TaggedFirst {
            tag: "gpu".to_string(),
        });
        let workers = vec![
            make_worker("cpu-1", 0, WorkerStatus::Online, vec!["cpu"]),
            make_worker("cpu-2", 0, WorkerStatus::Online, vec!["cpu"]),
        ];
        let selected = pool.select(&workers).unwrap();
        assert_eq!(selected.worker_id.0, "cpu-1");
    }
}
