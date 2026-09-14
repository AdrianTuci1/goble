use crate::harness::*;

use crate::harness::guide::user_guide;

#[test]
fn test_user_guide_lists_topics() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path().to_path_buf();
    std::fs::write(dir.join("06-remote-access.md"), "# Remote Access\nBody\n").unwrap();
    std::fs::write(dir.join("07-mobile-access.md"), "# Mobile Access\nBody\n").unwrap();

    let out = user_guide(&serde_json::json!({}), &dir).unwrap();
    assert!(out.contains("remote-access"));
    assert!(out.contains("Mobile Access"));
}

#[test]
fn test_user_guide_reads_topic_by_bare_name() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path().to_path_buf();
    std::fs::write(
        dir.join("06-remote-access.md"),
        "# Remote Access\n\nYou can expose this machine via Tailscale.\n",
    )
    .unwrap();

    let out = user_guide(&serde_json::json!({"topic": "remote-access"}), &dir).unwrap();
    assert!(out.contains("Tailscale"));
}

#[test]
fn test_user_guide_reads_topic_by_filename() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path().to_path_buf();
    std::fs::write(
        dir.join("04-credentials.md"),
        "# Credentials\n\nSecrets stay hidden.\n",
    )
    .unwrap();

    let out = user_guide(&serde_json::json!({"topic": "04-credentials.md"}), &dir).unwrap();
    assert!(out.contains("Secrets stay hidden"));
}

#[test]
fn test_user_guide_unknown_topic_lists_available() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path().to_path_buf();
    std::fs::write(dir.join("09-tools.md"), "# Tools\n").unwrap();

    let out = user_guide(&serde_json::json!({"topic": "nope"}), &dir).unwrap();
    assert!(out.contains("No user guide topic"));
    assert!(out.contains("tools"));
}

#[test]
fn test_user_guide_missing_dir_is_helpful() {
    let out = user_guide(&serde_json::json!({}), std::path::Path::new("/no/such/dir")).unwrap();
    assert!(out.contains("not available yet"));
}

#[test]
fn test_system_prompt_mentions_user_guide() {
    assert!(HARNESS_SYSTEM_PROMPT.contains("user_guide"));
    assert!(harness_tool_definitions()
        .iter()
        .any(|t| t.name == "user_guide"));
}
