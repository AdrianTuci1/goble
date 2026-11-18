//! Remote harness runtimes: agent CLIs that run **inside** a provider sandbox.
//!
//! [`super::catalog`] lists the CLIs a user may already have on their machine
//! and Goble can open in a terminal pane. A remote harness is a different
//! shape: the harness is installed in the sandbox image, and the worker that
//! runs in that sandbox drives it for a turn and streams its events back over
//! the worker protocol. This module is the one place that names such a
//! runtime, its image and the environment the image and the provisioner agree
//! on.
//!
//! The first runtime is [`DSH`], the DeepSeek Harness headless profile. It is
//! installed from npm in the `goble-worker-dsh` image (see
//! `deploy/worker/Dockerfile`, `RUNTIME=dsh`), which runs the Goble worker as
//! usual and selects the DSH run path with `GOBLE_HARNESS=dsh`. The harness
//! itself runs as `dsh --profile headless --patch <harness.yml> --json
//! --permission-mode danger-full-access --approval-policy never -`, reads the
//! task on stdin and writes newline-delimited JSON events. Goble never drives
//! an interactive terminal for it: the sandbox is the boundary, exactly as in
//! the dsh-remote reference.
//!
//! Credentials are deliberately *not* part of the image. The image carries the
//! harness and its composition file; the DeepSeek account credentials are
//! copied into the sandbox at run time (sandbox env or the worker's secret
//! vault), written to `$DSH_HOME/.credentials.yaml`, and deleted again after
//! the run so they cannot land in a filesystem snapshot.

use std::collections::BTreeMap;

/// Environment variable that selects the harness a worker drives. The DSH
/// image sets it to `dsh`; a plain worker image leaves it unset and keeps its
/// native runtime.
pub const HARNESS_ENV: &str = "GOBLE_HARNESS";

/// The value of [`HARNESS_ENV`] that selects the remote DSH runtime.
pub const DSH: &str = "dsh";

/// The harness conversation a sandbox was created to resume.
///
/// The provisioner writes the binding's native session id here when it creates
/// (or restores) a sandbox for a project that already has a conversation; the
/// worker's DSH runner passes it as `--session-id` for a thread that has no
/// newer session of its own. The same variable name is written by
/// `goble-desktop-service`'s provisioner (`crates/goble-desktop-service/src/state/remote.rs`);
/// it lives here too so a runtime (and its docs) can name it without depending
/// on the desktop crate.
pub const NATIVE_SESSION_ENV: &str = "GOBLE_NATIVE_SESSION_ID";

/// The registry reference of the image that carries the DSH runtime.
pub const DSH_IMAGE: &str = "ghcr.io/adriantuci1/goble-worker-dsh";

/// One remote harness runtime an image can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteHarnessSpec {
    /// The catalog id, and the value [`HARNESS_ENV`] takes to select it.
    pub id: &'static str,
    /// The name a client shows for it.
    pub name: &'static str,
    /// The command the worker executes inside the sandbox.
    pub command: &'static str,
    /// The registry image that carries it (a tag is appended by the caller).
    pub image: &'static str,
    /// The composition the image installs, passed as `--patch`.
    pub harness_path: &'static str,
    /// Where the harness keeps its home (credentials, JSONL sessions).
    pub home: &'static str,
    /// The model the harness starts on when a client names none.
    pub default_model: &'static str,
    /// The environment variable that overrides [`Self::default_model`].
    pub model_env: &'static str,
    /// A one-line, honest description of what authenticating it needs.
    pub credentials: &'static str,
}

impl RemoteHarnessSpec {
    /// The registry reference for a tag (e.g. `latest`, `0.2.2-rc.3`).
    pub fn image_for_tag(&self, tag: &str) -> String {
        let tag = tag.trim();
        if tag.is_empty() {
            self.image.to_string()
        } else {
            format!("{}:{tag}", self.image)
        }
    }

    /// The environment that selects this runtime and carries `credentials_yaml`
    /// into the sandbox.
    ///
    /// An empty `credentials_yaml` adds nothing: the image is still usable, the
    /// run fails with the harness's own "not signed in" message, and the
    /// worker says which secret is missing instead of pretending the run
    /// started. The value is intended for the sandbox *environment* (never a
    /// file in the image); the worker writes it to the credentials file for the
    /// duration of a run and deletes it afterwards.
    pub fn sandbox_env(&self, credentials_yaml: Option<&str>) -> BTreeMap<String, String> {
        let mut env = BTreeMap::from([(HARNESS_ENV.to_string(), self.id.to_string())]);
        if let Some(credentials) = credentials_yaml
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            env.insert("DSH_CREDENTIALS_YAML".to_string(), credentials.to_string());
        }
        env
    }

    /// The sandbox environment a provisioner should pass for a chosen model.
    pub fn sandbox_env_with_model(
        &self,
        credentials_yaml: Option<&str>,
        model: Option<&str>,
    ) -> BTreeMap<String, String> {
        let mut env = self.sandbox_env(credentials_yaml);
        if let Some(model) = model.map(str::trim).filter(|value| !value.is_empty()) {
            env.insert(self.model_env.to_string(), model.to_string());
        }
        env
    }
}

/// The DeepSeek Harness headless runtime. Its composition is adapted from
/// dsh-remote's `gateway/harness/cordis.patch.yml`; the image installs a pinned
/// `@deepseek-ai/dsh` and the worker's DSH runner executes it.
pub const DSH_SPEC: RemoteHarnessSpec = RemoteHarnessSpec {
    id: DSH,
    name: "DeepSeek Harness",
    command: "dsh",
    image: DSH_IMAGE,
    harness_path: "/opt/dsh/harness.yml",
    home: "/home/goble/.dsh",
    default_model: "deepseek-flash",
    model_env: "GOBLE_DSH_MODEL",
    credentials:
        "a DeepSeek account sign-in (dsh login) stored as the worker secret `dsh_credentials`, or DSH_CREDENTIALS_YAML in the sandbox environment",
};

/// Every remote harness runtime Goble knows.
pub const REMOTE_HARNESSES: &[RemoteHarnessSpec] = &[DSH_SPEC];

/// The runtime with `id`, if Goble knows one.
pub fn remote_harness_by_id(id: &str) -> Option<&'static RemoteHarnessSpec> {
    let id = id.trim().to_ascii_lowercase();
    REMOTE_HARNESSES.iter().find(|harness| harness.id == id)
}

/// Whether `id` names a remote harness runtime (as opposed to a local CLI or
/// the native harness). A blank value is the native harness.
pub fn is_remote_harness(id: &str) -> bool {
    remote_harness_by_id(id).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dsh_is_the_registered_remote_runtime() {
        let spec = remote_harness_by_id("dsh").expect("dsh is a remote harness");
        assert_eq!(spec.command, "dsh");
        assert_eq!(
            remote_harness_by_id("DSH ").map(|s| s.id),
            Some("dsh"),
            "the id is matched case-insensitively and trimmed"
        );
        assert!(is_remote_harness("dsh"));
        assert!(!is_remote_harness("claude"), "claude runs locally");
        assert!(!is_remote_harness(""), "blank selects the native harness");
        assert_eq!(
            remote_harness_by_id("nope").map(|s| s.id),
            None,
            "unknown runtimes are not guessed at"
        );
    }

    #[test]
    fn the_image_reference_carries_its_tag() {
        let spec = remote_harness_by_id("dsh").unwrap();
        assert_eq!(
            spec.image_for_tag("latest"),
            "ghcr.io/adriantuci1/goble-worker-dsh:latest"
        );
        assert_eq!(
            spec.image_for_tag(" 0.2.2-rc.3 "),
            "ghcr.io/adriantuci1/goble-worker-dsh:0.2.2-rc.3"
        );
        assert_eq!(spec.image_for_tag(""), DSH_IMAGE);
    }

    #[test]
    fn sandbox_env_selects_the_runtime_and_carries_credentials_only_when_set() {
        let spec = remote_harness_by_id("dsh").unwrap();
        let env = spec.sandbox_env(None);
        assert_eq!(env.get(HARNESS_ENV).map(String::as_str), Some("dsh"));
        assert!(
            !env.contains_key("DSH_CREDENTIALS_YAML"),
            "no credential variable is invented"
        );

        let env = spec.sandbox_env(Some("  token: abc\n"));
        assert_eq!(
            env.get("DSH_CREDENTIALS_YAML").map(String::as_str),
            Some("token: abc")
        );

        let env = spec.sandbox_env(Some("   "));
        assert!(
            !env.contains_key("DSH_CREDENTIALS_YAML"),
            "a blank credential adds no variable"
        );
    }

    #[test]
    fn a_model_override_lands_under_the_runtime_model_variable() {
        let spec = remote_harness_by_id("dsh").unwrap();
        let env = spec.sandbox_env_with_model(Some("token: a"), Some("deepseek-pro"));
        assert_eq!(
            env.get(spec.model_env).map(String::as_str),
            Some("deepseek-pro")
        );
        let env = spec.sandbox_env_with_model(None, Some("   "));
        assert!(!env.contains_key(spec.model_env));
    }

    #[test]
    fn the_native_session_variable_matches_the_provisioner() {
        // The desktop-service provisioner owns the same literal; a worker that
        // renamed its side would silently stop resuming binding sessions.
        assert_eq!(NATIVE_SESSION_ENV, "GOBLE_NATIVE_SESSION_ID");
    }

    #[test]
    fn the_honest_defaults_name_the_model_and_credential_source() {
        let spec = remote_harness_by_id("dsh").unwrap();
        assert_eq!(spec.default_model, "deepseek-flash");
        assert_eq!(spec.harness_path, "/opt/dsh/harness.yml");
        assert!(
            spec.credentials.contains("dsh_credentials"),
            "the credentials line names the worker secret: {}",
            spec.credentials
        );
        assert!(
            !spec.credentials.to_ascii_lowercase().contains("password"),
            "no password login is offered for a runtime that signs in by account token"
        );
    }
}
