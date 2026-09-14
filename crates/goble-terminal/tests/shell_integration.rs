//! The bash and zsh integration scripts must speak the hook channel that
//! `hooks.rs` decodes, byte for byte. This runs each script in its own shell,
//! asks it for one of every hook, and decodes exactly what the shell wrote —
//! so a field renamed on either side fails here, not in a live pane.

use std::path::{Path, PathBuf};
use std::process::Command;

use goble_terminal::{HookEvent, HookTap};

/// The envelope prefix (`ESC P $ d`) the scripts must write.
const ENVELOPE_PREFIX: &[u8] = b"\x1bP$d";

/// Ask the shell for the handshake plus one of every hook variant. `$'a\tb'`
/// exercises the JSON escaping for a control byte.
const EMIT_EVERY_HOOK: &str = r#". "$GOBLE_INTEGRATION"; __goble_emit_preexec 'echo "hi"'; __goble_emit_command_finished 7; __goble_emit_input_buffer $'a\tb' 3; __goble_emit_clear; __goble_emit_precmd"#;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .map(|meta| meta.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn find_shell(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

fn decode(bytes: &[u8]) -> Vec<HookEvent> {
    let mut tap = HookTap::new();
    let mut passthrough = Vec::new();
    let mut hooks = tap.tap(bytes, &mut passthrough);
    hooks.extend(tap.flush(&mut passthrough));
    assert!(
        passthrough.is_empty(),
        "the scripts emit only hook envelopes, but this reached the parser: {:?}",
        String::from_utf8_lossy(&passthrough)
    );
    assert_eq!(tap.dropped(), 0, "every emitted hook must decode");
    hooks
}

fn integration_round_trips(shell_name: &str) {
    let shell = find_shell(shell_name)
        .unwrap_or_else(|| panic!("{shell_name} is required to check its integration script"));
    let script = manifest_dir()
        .join("assets/shell")
        .join(format!("{shell_name}.sh"));

    let output = Command::new(&shell)
        .env("GOBLE_SKIP_USER_RC", "1")
        .env("GOBLE_INTEGRATION", &script)
        .arg("-c")
        .arg(EMIT_EVERY_HOOK)
        .output()
        .unwrap_or_else(|err| panic!("running {shell_name}: {err}"));

    assert!(
        output.status.success(),
        "{shell_name} exited with {:?}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.starts_with(ENVELOPE_PREFIX),
        "{shell_name} did not open with the DCS envelope: {:?}",
        String::from_utf8_lossy(&output.stdout)
    );

    let hooks = decode(&output.stdout);

    match hooks.first() {
        Some(HookEvent::InitShell(value)) => {
            assert_eq!(value.shell.as_deref(), Some(shell_name));
            assert!(
                value.cwd.as_deref().is_some_and(|pwd| !pwd.is_empty()),
                "InitShell must carry the working directory: {value:?}"
            );
        }
        other => panic!("first hook must be InitShell, got {other:?}"),
    }
    assert!(
        matches!(hooks.get(1), Some(HookEvent::Bootstrapped(_))),
        "second hook must be Bootstrapped: {hooks:?}"
    );
    assert!(
        matches!(hooks.get(2), Some(HookEvent::Precmd(_))),
        "third hook must be the first Precmd: {hooks:?}"
    );
    match hooks.get(3) {
        Some(HookEvent::Preexec(value)) => {
            assert_eq!(value.command.as_deref(), Some(r#"echo "hi""#));
        }
        other => panic!("fourth hook must be Preexec, got {other:?}"),
    }
    match hooks.get(4) {
        Some(HookEvent::CommandFinished(value)) => {
            assert_eq!(value.exit_code, 7, "the exit code must survive encoding");
            assert!(value.next_block_id.is_some());
        }
        other => panic!("fifth hook must be CommandFinished, got {other:?}"),
    }
    match hooks.get(5) {
        Some(HookEvent::InputBuffer(value)) => {
            assert_eq!(value.buffer, "a\tb");
            assert_eq!(value.cursor, Some(3));
        }
        other => panic!("sixth hook must be InputBuffer, got {other:?}"),
    }
    assert!(
        matches!(hooks.get(6), Some(HookEvent::Clear)),
        "seventh hook must be Clear: {hooks:?}"
    );
    match hooks.get(7) {
        Some(HookEvent::Precmd(value)) => {
            assert_eq!(value.honor_ps1, Some(false), "the integration hides PS1");
            assert!(value.pwd.as_deref().is_some_and(|pwd| !pwd.is_empty()));
        }
        other => panic!("eighth hook must be Precmd, got {other:?}"),
    }
    assert_eq!(
        hooks.len(),
        8,
        "exactly the handshake plus one of each hook: {hooks:?}"
    );
}

#[test]
fn bash_script_emits_the_hook_channel() {
    integration_round_trips("bash");
}

#[test]
fn zsh_script_emits_the_hook_channel() {
    integration_round_trips("zsh");
}
