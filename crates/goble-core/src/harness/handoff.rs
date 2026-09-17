use anyhow::Result;

/// Request an interactive remote desktop handoff. The agent calls this when it
/// decides it needs a real GUI; the host that owns the screen registry opens the
/// stream and shows it in a screen pane. This function only validates the target
/// and acknowledges — actually opening the desktop is the host's job, and so is
/// resolving the credential's value: what is acknowledged and what travels on is
/// the credential *name* the model wrote, never the account behind it.
pub(super) fn open_screen(args: &serde_json::Value) -> Result<String> {
    let host = args["host"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("open_screen requires a `host` argument"))?;
    let credential = credential_name(args)?;
    let port = args["port"].as_u64().unwrap_or(3389);
    Ok(format!(
        "handoff requested to {host}:{port} with stored credential `{credential}`"
    ))
}

/// The `credential` argument is a *reference*: the name of a credential stored
/// on the host, resolved there when the RDP connection is built. The same shape
/// rule the handoff event applies (`RemoteScreenConfig::is_credential_name`) is
/// applied here so an account line or a pasted value is refused with a message
/// for the model instead of being echoed into this tool's result.
fn credential_name(args: &serde_json::Value) -> Result<&str> {
    let credential = args["credential"].as_str().unwrap_or("");
    if credential.is_empty() {
        anyhow::bail!(
            "open_screen requires a `credential` argument naming a stored desktop credential; \
             store the account on the host first (the `credentials` tool lists the names)"
        );
    }
    if credential.contains(|c: char| c.is_whitespace() || c.is_control() || c == ':') {
        anyhow::bail!(
            "open_screen's `credential` is the NAME of a credential stored on the host, not a \
             username, a password or an account line; the host resolves its value when it opens \
             the desktop"
        );
    }
    Ok(credential)
}
