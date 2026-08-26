//! The Goble user guide, embedded into the binary.
//!
//! `app_home::GobleHome::ensure_base()` seeds `~/.goble/docs/user-guide/` from this
//! list on first launch. Each entry is `(file name, file contents)`; the contents
//! are glued in at compile time so the native app does not depend on a source tree
//! being installed at runtime.

/// The user-guide topics, in display order. Names follow the `.grok` convention
/// (`NN-topic.md`) so the guide reads as one ordered document.
pub const USER_GUIDE: &[(&str, &str)] = &[
    (
        "01-getting-started.md",
        include_str!("../assets/user-guide/01-getting-started.md"),
    ),
    (
        "02-workspaces.md",
        include_str!("../assets/user-guide/02-workspaces.md"),
    ),
    (
        "03-authentication.md",
        include_str!("../assets/user-guide/03-authentication.md"),
    ),
    (
        "04-credentials.md",
        include_str!("../assets/user-guide/04-credentials.md"),
    ),
    (
        "05-principals-and-access.md",
        include_str!("../assets/user-guide/05-principals-and-access.md"),
    ),
    (
        "06-remote-access.md",
        include_str!("../assets/user-guide/06-remote-access.md"),
    ),
    (
        "07-mobile-access.md",
        include_str!("../assets/user-guide/07-mobile-access.md"),
    ),
    (
        "08-agents.md",
        include_str!("../assets/user-guide/08-agents.md"),
    ),
    (
        "09-tools.md",
        include_str!("../assets/user-guide/09-tools.md"),
    ),
    (
        "10-skills.md",
        include_str!("../assets/user-guide/10-skills.md"),
    ),
    (
        "11-mcp-servers.md",
        include_str!("../assets/user-guide/11-mcp-servers.md"),
    ),
    (
        "12-memory.md",
        include_str!("../assets/user-guide/12-memory.md"),
    ),
    (
        "13-sandbox.md",
        include_str!("../assets/user-guide/13-sandbox.md"),
    ),
    (
        "14-executions-and-trace.md",
        include_str!("../assets/user-guide/14-executions-and-trace.md"),
    ),
    (
        "15-threads.md",
        include_str!("../assets/user-guide/15-threads.md"),
    ),
    (
        "16-configuration.md",
        include_str!("../assets/user-guide/16-configuration.md"),
    ),
    (
        "17-plugins-and-workflows.md",
        include_str!("../assets/user-guide/17-plugins-and-workflows.md"),
    ),
    (
        "18-monitoring-usage.md",
        include_str!("../assets/user-guide/18-monitoring-usage.md"),
    ),
];
