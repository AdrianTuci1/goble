use anyhow::Result;

/// Request an interactive remote desktop handoff. The agent calls this when it
/// decides it needs a real GUI; the host that owns the screen registry opens the
/// stream and shows it in a screen pane. This function only validates the target
/// and acknowledges — actually opening the desktop is the host's job.
pub(super) fn open_screen(args: &serde_json::Value) -> Result<String> {
    let host = args["host"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("open_screen requires a `host` argument"))?;
    let port = args["port"].as_u64().unwrap_or(3389);
    Ok(format!("handoff requested to {host}:{port}"))
}
