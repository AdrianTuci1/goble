use std::collections::HashSet;

use crate::store::Store;
use anyhow::Result;

pub(super) fn list_entities(store: &Store, args: &serde_json::Value) -> Result<String> {
    let entity_type = args["entity_type"].as_str().unwrap_or("agents");
    match entity_type {
        "agents" => {
            let rows = store.list_agents()?;
            Ok(format!(
                "agents: {:?}",
                rows.into_iter()
                    .map(|(id, name, _, _, _)| (id, name))
                    .collect::<Vec<_>>()
            ))
        }
        "workflows" => {
            let rows = store.list_workflows()?;
            Ok(format!(
                "workflows: {:?}",
                rows.into_iter()
                    .map(|(id, name, _, _, _, _, _, _)| (id, name))
                    .collect::<Vec<_>>()
            ))
        }
        "teams" => {
            let rows = store.list_teams()?;
            Ok(format!(
                "teams: {:?}",
                rows.into_iter()
                    .map(|(id, name, _, _)| (id, name))
                    .collect::<Vec<_>>()
            ))
        }
        "workers" => {
            let rows = store.list_workers()?;
            Ok(format!(
                "workers: {:?}",
                rows.into_iter()
                    .map(|(id, name, host, status, _, _, _, _)| (id, name, host, status))
                    .collect::<Vec<_>>()
            ))
        }
        _ => Ok(format!("unknown entity type {entity_type}")),
    }
}

pub(super) fn search_store(store: &Store, args: &serde_json::Value) -> Result<String> {
    let query = args["query"].as_str().unwrap_or("").to_lowercase();
    let types: HashSet<String> = args["entity_types"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_else(|| {
            ["agents", "workflows", "teams"]
                .iter()
                .map(|s| s.to_string())
                .collect()
        });

    let mut results = Vec::new();
    if types.contains("agents") {
        for (id, name, _, _, _) in store.list_agents()? {
            if id.to_lowercase().contains(&query) || name.to_lowercase().contains(&query) {
                results.push(("agent", id, name));
            }
        }
    }
    if types.contains("workflows") {
        for (id, name, _, _, _, _, _, _) in store.list_workflows()? {
            if id.to_lowercase().contains(&query) || name.to_lowercase().contains(&query) {
                results.push(("workflow", id, name));
            }
        }
    }
    if types.contains("teams") {
        for (id, name, _, _) in store.list_teams()? {
            if id.to_lowercase().contains(&query) || name.to_lowercase().contains(&query) {
                results.push(("team", id, name));
            }
        }
    }
    Ok(format!("search results: {:?}", results))
}
