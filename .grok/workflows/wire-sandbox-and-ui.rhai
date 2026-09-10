let meta = #{
    name: "wire-sandbox-and-ui",
    description: "Wire goble-sandbox into the harness command path and refine the pane/space UI",
    phases: [
        #{ title: "Sandbox", detail: "implement + verify graceful sandbox wiring" },
        #{ title: "UI", detail: "implement + verify space persistence and sidebar polish" },
    ],
};

phase("Sandbox");
let impl_sandbox = agent(
    "Wire goble-sandbox into goble-core SandboxedCommandRunner. Do this WITHOUT changing current default behavior: any sandboxing must degrade to direct execution when no sandbox backend is available (e.g. on macOS where bwrap is not installed), so the desktop app and the harness keep working exactly as before.\n\nSteps:\n1. Read crates/goble-sandbox/src (bwrap.rs, lib.rs) and crates/goble-core/src/harness.rs. The single injection chokepoint is SandboxedCommandRunner::run (around lines 71-94), where the child process is spawned (around lines 77-84).\n2. Add an async path to goble-sandbox: PreparedCommand::to_tokio_command and SandboxGuard::run_async if not present; add a tokio process feature to goble-sandbox/Cargo.toml if needed.\n3. In goble-core SandboxedCommandRunner add a field sandbox: Option<Box<dyn Sandbox>> and a with_sandbox(...) builder. At the spawn chokepoint, when sandbox is None OR the sandbox backend is unavailable on this platform, run the command directly (today's behavior). When a sandbox is configured and available, wrap the spawn via goble_sandbox::SandboxGuard / PreparedCommand.\n4. Keep the existing allowed_commands gate intact.\n5. Opt in at the composition roots ONLY in a way that preserves behavior when unavailable: goble-desktop-service/src/state.rs (~line 2202), goblin-worker/src/runner.rs (~line 293), goble-desktop-native/src/state_api.rs (~line 650), goble-desktop/src-tauri/src/lib.rs (~line 971). Default to NO sandbox (None) so macOS/dev behavior is unchanged; the builder must handle None gracefully.\n6. Choose a sandbox profile for the harness that keeps the workspace directory WRITABLE (so write_file/edit_file/delete_file/rename_file/git_commit keep working) while isolating the rest of the filesystem read-only and (on Linux) unsharing the network. Document this choice in a comment.\n7. Add a unit test proving that with no sandbox configured, SandboxedCommandRunner still runs a command directly (fallthrough), and that with_sandbox(None) is a no-op.\n8. Run cargo check --workspace and cargo test -p goble-core and cargo test -p goble-sandbox. Fix any failures you introduced. Do NOT weaken or ignore existing tests.\n\nReport a short summary of files changed and the test results.",
    #{ label: "sandbox-impl", capability_mode: "all" }
);
if !impl_sandbox.success { complete(#{ error: "sandbox impl agent failed", report: impl_sandbox.output }); }

let verify_sandbox = agent(
    "Verify the sandbox wiring changes in the workspace. Read the changed files to confirm: (a) SandboxedCommandRunner falls back to direct execution when sandbox is None or the backend is unavailable; (b) the existing allowed_commands gate is intact; (c) the goble-core harness tests still pass; (d) no existing test was weakened. Run: cargo test -p goble-core, cargo test -p goble-sandbox, and cargo check --workspace.\n\nReport success=true only if all pass and the constraints hold; otherwise success=false and list concrete failures. Do not modify any files; you are verification only.",
    #{ label: "sandbox-verify", capability_mode: "all" }
);

phase("UI");
let impl_ui = agent(
    "Refine the pane/space UI in the app (app/ + goble-ui). Scope is limited to the following; do not change default behavior elsewhere:\n\n1. Persist the spaces, active_space and active_pane_id so they survive an app restart: load them on startup into UiState and save them whenever they change. Choose the store yourself (a small JSON file under ~/.goble/ or via DesktopState); reuse existing persistence patterns in the app and crates/goble-persistence if appropriate.\n2. Make the medium selector fit the sidebar width (~300px) without overflowing: render the medium chips in a wrapping/vertical layout, or a compact segmented control, so it stays usable. Read app/src/ui/sidebar.rs and app/src/ui/media.rs.\n3. Ensure the composer path label (goble-ui chat_composer path_label) clips/ellipsizes at long paths instead of overflowing the composer.\n4. Add an unobtrusive keyboard-shortcut hint (e.g. in the pane header tooltip) documenting Ctrl/Cmd+Space (split right), Cmd+Shift+D (split down), Ctrl/Cmd+W (close pane).\n\nVerify with cargo check --workspace, cargo test -p goble-app, and cargo test -p goble-ui; fix any breakage you introduce. If you introduce space persistence, add or adjust a test that exercises it, and run the app render/integration tests.\n\nReport a short summary of files changed and the test results.",
    #{ label: "ui-impl", capability_mode: "all" }
);
if !impl_ui.success { complete(#{ error: "ui impl agent failed", report: impl_ui.output }); }

let verify_ui = agent(
    "Verify the pane/space UI refinements in the app. Read the changed files to confirm: (a) spaces/active_space/active_pane_id are persisted and reloaded on startup; (b) the medium selector fits the sidebar without overflowing; (c) the composer path label clips at long paths; (d) the keyboard-shortcut hint is present; (e) the app render/integration tests still pass. Run: cargo test -p goble-app, cargo test -p goble-ui, and cargo check --workspace.\n\nReport success=true only if all pass; otherwise success=false and list concrete failures. Do not modify any files; you are verification only.",
    #{ label: "ui-verify", capability_mode: "all" }
);

let sandbox_ok = verify_sandbox.success && verify_sandbox.output.success == true;
let ui_ok = verify_ui.success && verify_ui.output.success == true;
complete(#{ summary: "done", sandbox_ok: sandbox_ok, ui_ok: ui_ok, sandbox_report: verify_sandbox.output, ui_report: verify_ui.output });
