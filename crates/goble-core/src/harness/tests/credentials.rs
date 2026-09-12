use super::*;
use crate::harness::Harness;

use crate::harness::credentials::{expand_credential_refs, list_credentials, list_principals};

#[test]
fn test_credential_expansion_and_listing() {
    let store = Store::open_in_memory().unwrap();
    assert_eq!(list_credentials(&store).unwrap(), "no credentials stored");

    store.set_credential("github_token", "ghs_secret").unwrap();
    assert_eq!(
        list_credentials(&store).unwrap(),
        "stored credentials: github_token"
    );

    // Placeholder is substituted server-side; the value only flows into the
    // process argv, never into a returned tool result or the transcript.
    let cmd = r#"curl -H "Authorization: Bearer {{credential:github_token}}" url"#;
    assert_eq!(
        expand_credential_refs(&store, cmd).unwrap(),
        r#"curl -H "Authorization: Bearer ghs_secret" url"#
    );
    assert_eq!(
        expand_credential_refs(
            &store,
            "echo {{credential:github_token}} {{credential:github_token}}"
        )
        .unwrap(),
        "echo ghs_secret ghs_secret"
    );
    // A command with no credential reference passes through untouched.
    assert_eq!(
        expand_credential_refs(&store, "git status").unwrap(),
        "git status"
    );
    // An unknown credential is an error, never a silently-expanded secret.
    assert!(expand_credential_refs(&store, "{{credential:nope}}").is_err());

    // The `credentials` tool is advertised to the model and only lists names.
    let harness = Harness::new(store);
    assert!(harness.list_tools().iter().any(|t| t.name == "credentials"));
}

#[test]
fn test_principals_lists_grants() {
    let store = Store::open_in_memory().unwrap();
    store
        .insert_principal("p1", "user", "Ada", "2024-01-01T00:00:00Z")
        .unwrap();
    assert!(list_principals(&store).unwrap().contains("no grants"));

    store.grant_access("p1", "run", "workspace").unwrap();
    let listing = list_principals(&store).unwrap();
    assert!(listing.contains("principal p1 (user, Ada)"));
    assert!(listing.contains("run:workspace"));

    let harness = Harness::new(store);
    assert!(harness.list_tools().iter().any(|t| t.name == "principals"));
}
