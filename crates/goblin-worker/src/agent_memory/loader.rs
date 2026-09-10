use anyhow::Result;
use goble_core::agent::AgentId;
use goble_core::agent_memory::AgentMemory;
use goble_core::store::Store;

/// Load the canonical memory for an agent, seeding it from the spec prompt on
/// first run so the brief always starts from the agent's identity.
pub fn load_or_create(store: &Store, agent_id: &AgentId, spec_prompt: &str) -> Result<AgentMemory> {
    if let Some(memory) = store.get_agent_memory(&agent_id.0)? {
        return Ok(memory);
    }
    let memory = AgentMemory::new(agent_id.0.clone(), spec_prompt);
    store.put_agent_memory(&memory)?;
    Ok(memory)
}

/// Persist the current memory snapshot for an agent.
pub fn persist(store: &Store, memory: &AgentMemory) -> Result<()> {
    store.put_agent_memory(memory)?;
    Ok(())
}
