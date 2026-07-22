use goble_core::agent::AgentSpec;
use goble_core::execution::ExecutionTrace;
use goble_core::mcp_registry::McpRegistry;
use goble_core::worker::{WorkerId, WorkerStatus};
use goble_core::worker_pool::{WorkerPool, WorkerPoolStrategy, WorkerSnapshot};

#[test]
fn test_mcp_registry_resolve_adds_mcp_id() {
    let registry = McpRegistry::builtin();
    let server = registry.resolve("filesystem").unwrap();
    assert_eq!(server.id, "mcp-filesystem");
}

#[test]
fn test_worker_pool_lowest_load_selects_best() {
    let mut pool = WorkerPool::new(WorkerPoolStrategy::LowestLoad);
    let workers = vec![
        WorkerSnapshot {
            worker_id: WorkerId::generate(),
            name: "busy".to_string(),
            url: "ws://a".to_string(),
            status: WorkerStatus::Online,
            load: 9,
            tags: vec![],
        },
        WorkerSnapshot {
            worker_id: WorkerId::generate(),
            name: "free".to_string(),
            url: "ws://b".to_string(),
            status: WorkerStatus::Online,
            load: 1,
            tags: vec![],
        },
    ];
    let selected = pool.select(&workers).unwrap();
    assert_eq!(selected.name, "free");
}

#[test]
fn test_execution_trace_sequential_view() {
    let mut trace = ExecutionTrace::new(goble_core::agent::AgentId::generate());
    let root = trace.add_root_step("agent");
    let root_id = root.id.clone();
    trace.add_child_step(&root_id, "child").unwrap();
    let view = trace.sequential_view();
    assert_eq!(view.len(), 2);
    assert_eq!(view[0].0, 0);
    assert_eq!(view[1].0, 1);
}
