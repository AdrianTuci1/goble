//! The settings surface's app tests.
//!
//! These drive the real path: a real `~/.goble`-shaped temp directory (the
//! service's `set_config_path` pins it), the real `goble-core::config` editor
//! behind the service, the real page models and renderer, and the real settings
//! dispatch. Nothing here is a mock of the settings store — the store is the
//! files, so the tests write and read them.
//!
//! What is pinned:
//!
//! - every page renders from a real temp config file, and its copy reaches the
//!   render commands;
//! - a UI action writes the file and **preserves comments and unknown keys**;
//! - a reload reflects an edit made outside the app;
//! - a provider can be added, edited and tested against a local mock HTTP
//!   server, and the probe result reaches the model's row;
//! - the keyboard model navigates the rail and the page;
//! - rows this build cannot perform are disabled with an honest status line.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use goble_core::store::Store;
use goble_desktop_service::{DesktopState, ThreadStore};
use goble_ui::elements::{AppContext, Element};
use goble_ui::geometry::vec2f;
use goble_ui::test_util::render_element;

use crate::state::UiState;
use crate::ui::settings::model::ControlModel;
use crate::ui::settings::pages;
use crate::ui::settings::render;
use crate::ui::settings::state::{
    SettingsAction, SettingsForm, SettingsRegion, SettingsUiState, StatusKind,
};
use crate::ui::SettingsCategory;

const SEEDED_CONFIG: &str = r#"# goble configuration: this comment is the file's own documentation.
# It must survive every write the settings page makes.

[app]
friendly_name = "Ada"
# the telemetry choice is opt-in
telemetry = false

# a section this build does not know, which must be kept byte for byte
[future_section]
kept = "yes"
nested = { a = 1 }

[unknown_table]
value = "still here"

[model.seeded]
model = "seeded-model"
name = "Seeded Model"
base_url = "https://seeded.example.invalid"
api_key = "sk-seeded"
"#;

struct Fixture {
    state: std::rc::Rc<std::cell::RefCell<UiState>>,
    desktop: Arc<DesktopState>,
    _dir: tempfile::TempDir,
}

impl Fixture {
    fn new(config: &str) -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let home = dir.path();
        std::fs::create_dir_all(home.join(".goble")).expect("home");
        let path = home.join("config.toml");
        std::fs::write(&path, config).expect("seed config");
        let desktop = DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(home).expect("thread store"),
        );
        // The config path pinned here is the one the service reads and writes;
        // environment.toml and hosts.toml resolve beside it.
        desktop.set_config_path(path.clone());
        let state = std::rc::Rc::new(std::cell::RefCell::new(UiState::mock()));
        {
            let mut state = state.borrow_mut();
            state.settings.ssh_home_override = Some(home.to_path_buf());
            state.refresh_settings(&desktop);
        }
        Self {
            state,
            desktop,
            _dir: dir,
        }
    }

    fn config_path(&self) -> std::path::PathBuf {
        self.desktop
            .settings_path(goble_core::config::ConfigFile::Config)
            .expect("a config path is pinned")
    }

    fn text(&self) -> String {
        std::fs::read_to_string(self.config_path()).expect("read config")
    }

    fn dispatch(&self, action: SettingsAction) {
        let actions = crate::actions::settings_actions::make_settings_actions(
            std::rc::Rc::clone(&self.state),
            Some(Arc::clone(&self.desktop)),
            std::rc::Rc::new(std::cell::RefCell::new(crate::media::MediaState::mock())),
        );
        (actions.dispatch.borrow_mut())(action);
    }

    fn model(&self, page: SettingsCategory) -> pages::PageModel {
        let state = self.state.borrow();
        pages::page_model(page, &state.settings)
    }

    fn row(&self, page: SettingsCategory, id: &str) -> crate::ui::settings::RowModel {
        self.model(page)
            .interactive_rows()
            .into_iter()
            .find(|row| row.id == id)
            .cloned()
            .or_else(|| {
                self.model(page)
                    .sections
                    .iter()
                    .flat_map(|section| section.rows.iter())
                    .find(|row| row.id == id)
                    .cloned()
            })
            .unwrap_or_else(|| panic!("page {:?} has a row {id}", page))
    }

    fn drawn(&self, page: SettingsCategory) -> Vec<goble_ui::render::RenderCommand> {
        let app = AppContext::default();
        let state = self.state.borrow();
        let model = pages::page_model(page, &state.settings);
        let actions = crate::actions::settings_actions::make_settings_actions(
            std::rc::Rc::clone(&self.state),
            Some(Arc::clone(&self.desktop)),
            std::rc::Rc::new(std::cell::RefCell::new(crate::media::MediaState::mock())),
        );
        let mut element: Box<dyn Element> =
            render::render_page(&app, &model, &state.settings, &actions);
        render_element(&mut element, vec2f(900.0, 2400.0), &app)
    }

    fn drawn_text(&self, page: SettingsCategory) -> Vec<String> {
        self.drawn(page)
            .into_iter()
            .filter_map(|command| match command {
                goble_ui::render::RenderCommand::DrawText { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    }
}

fn drawn_contains(commands: &[String], needle: &str) -> bool {
    commands.iter().any(|text| text == needle)
}

/// Every one of the twelve pages builds from a real, freshly written config file
/// and draws its own copy.
#[test]
fn every_page_renders_from_a_real_config_file() {
    let fixture = Fixture::new(SEEDED_CONFIG);
    let expected: &[(SettingsCategory, &str)] = &[
        (SettingsCategory::General, "Friendly name"),
        (SettingsCategory::Appearance, "Colors & themes"),
        (SettingsCategory::Providers, "Models"),
        (SettingsCategory::Connectors, "MCP servers"),
        (SettingsCategory::Agent, "Approval"),
        (SettingsCategory::Environments, "Add a group"),
        (SettingsCategory::Hosts, "Saved hosts"),
        (SettingsCategory::Secrets, "Vault"),
        (SettingsCategory::Shortcuts, "Keyboard"),
        (SettingsCategory::Updates, "This build"),
        (SettingsCategory::Advanced, "config.toml"),
        (SettingsCategory::About, "Goble"),
    ];
    for (page, needle) in expected {
        let commands = fixture.drawn_text(*page);
        assert!(
            drawn_contains(&commands, needle),
            "page {page:?} draws {needle:?}; got {commands:?}"
        );
    }
    // The values the file held reach the page, not placeholder data.
    let commands = fixture.drawn_text(SettingsCategory::General);
    assert!(
        commands
            .iter()
            .any(|text| text == "Ada" || text.contains("Ada")),
        "the friendly name from the file is drawn: {commands:?}"
    );
}

/// A write is a document op: the header comment, an unknown table and an
/// unknown nested key all survive, and the changed key is exactly the one the
/// control named.
#[test]
fn a_ui_action_writes_the_file_and_keeps_comments_and_unknown_keys() {
    let fixture = Fixture::new(SEEDED_CONFIG);
    fixture.dispatch(SettingsAction::SetToggle {
        path: "app.telemetry".to_string(),
        value: true,
    });
    let text = fixture.text();
    assert!(
        text.contains("this comment is the file's own documentation"),
        "{text}"
    );
    assert!(text.contains("the telemetry choice is opt-in"), "{text}");
    assert!(text.contains("[future_section]"), "{text}");
    assert!(text.contains("kept = \"yes\""), "{text}");
    assert!(text.contains("[unknown_table]"), "{text}");
    assert!(text.contains("value = \"still here\""), "{text}");
    assert!(text.contains("telemetry = true"), "{text}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(fixture.config_path())
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "the write keeps the settings file private");
    }
    // The view the page now holds is the one the service returned: the toggle
    // row shows the new value without a reload.
    let row = fixture.row(SettingsCategory::General, "app.telemetry");
    assert!(matches!(
        row.control,
        ControlModel::Toggle { value: true, .. }
    ));
}

/// A reset unsets the key instead of writing the default into the file, so the
/// file keeps saying "unset means default" and the comments stay.
#[test]
fn a_reset_removes_the_key_and_keeps_the_file_intact() {
    let fixture = Fixture::new(SEEDED_CONFIG);
    fixture.dispatch(SettingsAction::SetToggle {
        path: "app.telemetry".to_string(),
        value: true,
    });
    assert!(fixture.text().contains("telemetry = true"));
    fixture.dispatch(SettingsAction::Reset {
        path: "app.telemetry".to_string(),
    });
    let text = fixture.text();
    assert!(!text.contains("telemetry = true"), "{text}");
    assert!(text.contains("this comment is the file's own documentation"));
    assert!(text.contains("[future_section]"));
}

/// A text field commits through the same document editor: the name changes and
/// nothing else does.
#[test]
fn a_committed_text_field_writes_the_file() {
    let fixture = Fixture::new(SEEDED_CONFIG);
    fixture.dispatch(SettingsAction::SetDraft {
        id: "app.friendly_name".to_string(),
        value: "Grace".to_string(),
    });
    fixture.dispatch(SettingsAction::CommitField {
        id: "app.friendly_name".to_string(),
    });
    let text = fixture.text();
    assert!(text.contains("friendly_name = \"Grace\""), "{text}");
    assert!(text.contains("this comment is the file's own documentation"));
    let row = fixture.row(SettingsCategory::General, "app.friendly_name");
    assert!(matches!(
        row.control,
        ControlModel::Text { ref value, .. } if value == "Grace"
    ));
}

/// A reload re-reads the file: an edit made outside the app reaches the page,
/// and a stale in-memory view cannot win.
#[test]
fn a_reload_reflects_an_external_edit() {
    let fixture = Fixture::new(SEEDED_CONFIG);
    let path = fixture.config_path();
    let edited = SEEDED_CONFIG.replace("friendly_name = \"Ada\"", "friendly_name = \"Outside\"");
    std::fs::write(&path, edited).expect("external edit");
    fixture.dispatch(SettingsAction::Reload);
    let row = fixture.row(SettingsCategory::General, "app.friendly_name");
    assert!(
        matches!(row.control, ControlModel::Text { ref value, .. } if value == "Outside"),
        "the reload shows the file's value: {row:?}"
    );
}

/// A stale revision is a conflict, not an overwrite: the other writer's change
/// stays and the page says the file changed.
#[test]
fn a_stale_write_is_a_conflict_and_does_not_overwrite() {
    let fixture = Fixture::new(SEEDED_CONFIG);
    // Render from the current revision, then edit the file behind the page.
    let path = fixture.config_path();
    let edited = SEEDED_CONFIG.replace(
        "friendly_name = \"Ada\"",
        "friendly_name = \"Other writer\"",
    );
    std::fs::write(&path, edited).expect("external edit");
    fixture.dispatch(SettingsAction::SetToggle {
        path: "app.telemetry".to_string(),
        value: true,
    });
    let text = fixture.text();
    assert!(text.contains("friendly_name = \"Other writer\""), "{text}");
    let state = fixture.state.borrow();
    let notice = state.settings.notice.as_ref().expect("a notice");
    assert_eq!(notice.kind, StatusKind::Warning, "{notice:?}");
    assert!(notice.text.contains("changed on disk"), "{notice:?}");
}

/// The keyboard model: the rail moves page to page, Enter opens the page, and
/// the page's rows step.
#[test]
fn the_keyboard_model_navigates_the_rail_and_the_page() {
    let fixture = Fixture::new(SEEDED_CONFIG);
    {
        let mut state = fixture.state.borrow_mut();
        state.settings_set_page(SettingsCategory::General);
        state.settings.region = SettingsRegion::Rail;
        state.settings.nav_index = 0;
    }
    fixture.dispatch(SettingsAction::Step(1));
    {
        let state = fixture.state.borrow();
        assert_eq!(state.settings.page, SettingsCategory::Appearance);
        assert_eq!(state.settings.nav_index, 1);
    }
    fixture.dispatch(SettingsAction::Activate);
    {
        let state = fixture.state.borrow();
        assert_eq!(state.settings.region, SettingsRegion::Page);
        assert_eq!(state.settings.row_index, 0);
    }
    fixture.dispatch(SettingsAction::Step(1));
    assert_eq!(fixture.state.borrow().settings.row_index, 1);
    // Enter on a toggle row flips it through the file. The page's first
    // interactive row is the friendly-name field, whose Enter puts the caret
    // in rather than writing; find the first toggle row and focus that.
    let toggle_index = {
        let state = fixture.state.borrow();
        let model = pages::page_model(SettingsCategory::General, &state.settings);
        model
            .interactive_rows()
            .iter()
            .position(|row| {
                matches!(
                    row.control,
                    crate::ui::settings::model::ControlModel::Toggle { .. }
                )
            })
            .expect("a toggle row")
    };
    {
        let mut state = fixture.state.borrow_mut();
        state.settings.region = SettingsRegion::Page;
        state.settings.row_index = toggle_index;
    }
    let before = fixture.text();
    fixture.dispatch(SettingsAction::Activate);
    let after = fixture.text();
    assert_ne!(before, after, "Enter on a toggle row writes the file");
}

/// Rows this build cannot perform are still drawn: disabled, with the honest
/// sentence in the row itself rather than hidden or tooltip-only.
#[test]
fn unavailable_rows_are_disabled_and_honest() {
    let fixture = Fixture::new(SEEDED_CONFIG);
    let shortcuts = fixture.drawn_text(SettingsCategory::Shortcuts);
    assert!(drawn_contains(&shortcuts, "Remap a shortcut"));
    assert!(
        shortcuts
            .iter()
            .any(|text| text.contains("Not available in this build")),
        "the disabled row says why: {shortcuts:?}"
    );
    let providers = fixture.drawn_text(SettingsCategory::Providers);
    assert!(drawn_contains(&providers, "Sign in with Codex"));
    assert!(providers
        .iter()
        .any(|text| text.contains("no OAuth sign-in")));
    let secrets = fixture.drawn_text(SettingsCategory::Secrets);
    assert!(drawn_contains(&secrets, "1Password"));
    // The model is honest about the binding, not just the render: a disabled
    // row reports `disabled`.
    let model = fixture.model(SettingsCategory::Shortcuts);
    let row = model
        .sections
        .iter()
        .flat_map(|section| section.rows.iter())
        .find(|row| row.id == "remap")
        .expect("the remap row exists");
    assert!(row.disabled);
}

/// The focus order the keyboard walks is the render order: the focused slot
/// names a row that is actually drawn and interactive.
#[test]
fn the_focus_order_is_the_page_order() {
    let fixture = Fixture::new(SEEDED_CONFIG);
    let model = fixture.model(SettingsCategory::Providers);
    assert!(model.control_count() > 0);
    let first = model.row_at(0).expect("a first row");
    assert!(!first.disabled);
    assert!(first.primary_action().is_some());
    // The page's value rows are not focus stops; the list rows and buttons are.
    let ids: Vec<&str> = model
        .interactive_rows()
        .iter()
        .map(|row| row.id.as_str())
        .collect();
    assert!(ids.contains(&"models.default"), "{ids:?}");
}

/// The rail's search filters pages by the schema's own labels: typing a setting's
/// name shows the page it lives on.
#[test]
fn the_search_matches_schema_rows() {
    assert!(crate::ui::settings::page_matches(
        SettingsCategory::General,
        "scroll"
    ));
    assert!(crate::ui::settings::page_matches(
        SettingsCategory::Appearance,
        "palette"
    ));
    assert!(!crate::ui::settings::page_matches(
        SettingsCategory::About,
        "palette"
    ));
}

// ---------------------------------------------------------------------------
// Providers against a local mock HTTP endpoint
// ---------------------------------------------------------------------------

/// A minimal HTTP/1.1 server that answers every request with a model list, so
/// the provider probe is a real network round trip to a local socket.
fn mock_models_server() -> (u16, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
    let port = listener.local_addr().expect("addr").port();
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&hits);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buffer = [0u8; 8192];
            let _ = stream.read(&mut buffer);
            counter.fetch_add(1, Ordering::SeqCst);
            let body = r#"{"object":"list","data":[{"id":"mock-model","object":"model"}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });
    (port, hits)
}

/// Add a provider through the form, save it to the file, and probe it against
/// the local mock endpoint: the round trip is real and the row reports it.
#[test]
fn a_model_is_added_saved_and_tested_against_a_mock_endpoint() {
    let runtime = tokio::runtime::Runtime::new().expect("test runtime");
    let _entered = runtime.enter();
    let fixture = Fixture::new(SEEDED_CONFIG);
    let (port, hits) = mock_models_server();
    let base_url = format!("http://127.0.0.1:{port}");
    fixture.dispatch(SettingsAction::NewModel);
    assert!(matches!(
        fixture.state.borrow().settings.form,
        Some(SettingsForm::Model(_))
    ));
    for (id, value) in [
        ("model-form.slug", "mock-provider"),
        ("model-form.model_id", "mock-model"),
        ("model-form.name", "Mock Provider"),
        ("model-form.base_url", base_url.as_str()),
        ("model-form.api_key", "test-key"),
    ] {
        fixture.dispatch(SettingsAction::SetDraft {
            id: id.to_string(),
            value: value.to_string(),
        });
    }
    fixture.dispatch(SettingsAction::SaveModelForm);
    let text = fixture.text();
    assert!(text.contains("[model.mock-provider]"), "{text}");
    assert!(text.contains("base_url = \"http://127.0.0.1:"), "{text}");
    assert!(text.contains("model = \"mock-model\""), "{text}");
    assert!(
        text.contains("this comment is the file's own documentation"),
        "{text}"
    );
    // The saved provider appears on the page.
    let commands = fixture.drawn_text(SettingsCategory::Providers);
    assert!(
        commands
            .iter()
            .any(|text| text.contains("Mock Provider") || text == "Mock Provider"),
        "{commands:?}"
    );
    // Test it: a real HTTP request to the mock server, and the report reaches
    // the row's status line.
    fixture.dispatch(SettingsAction::TestModel("mock-provider".to_string()));
    assert!(
        hits.load(Ordering::SeqCst) >= 1,
        "the probe reached the mock server"
    );
    let state = fixture.state.borrow();
    let (kind, line) = state
        .settings
        .test_results
        .get("model.mock-provider")
        .expect("the probe result is stored")
        .clone();
    assert_eq!(kind, StatusKind::Success, "{line}");
    assert!(line.contains("connected"), "{line}");
}

/// Editing a provider keeps its inline key when the form's key field is left
/// empty: the redacted projection never hands the key back, so an empty save
/// must not delete it.
#[test]
fn editing_a_model_keeps_its_key_when_the_field_is_empty() {
    let fixture = Fixture::new(SEEDED_CONFIG);
    fixture.dispatch(SettingsAction::NewModel);
    for (id, value) in [
        ("model-form.slug", "kept-key"),
        ("model-form.base_url", "https://example.invalid"),
        ("model-form.api_key", "sk-keep-me"),
    ] {
        fixture.dispatch(SettingsAction::SetDraft {
            id: id.to_string(),
            value: value.to_string(),
        });
    }
    fixture.dispatch(SettingsAction::SaveModelForm);
    assert!(fixture.text().contains("api_key = \"sk-keep-me\""));
    // Edit the display name only, leaving the key field empty.
    fixture.dispatch(SettingsAction::EditModel("kept-key".to_string()));
    fixture.dispatch(SettingsAction::SetDraft {
        id: "model-form.name".to_string(),
        value: "Renamed".to_string(),
    });
    fixture.dispatch(SettingsAction::SaveModelForm);
    let text = fixture.text();
    assert!(text.contains("name = \"Renamed\""), "{text}");
    assert!(text.contains("api_key = \"sk-keep-me\""), "{text}");
}

/// The form's Test button probes the endpoint before anything is saved, so a
/// bad URL is caught before it reaches the file.
#[test]
fn the_model_form_probes_before_saving() {
    let runtime = tokio::runtime::Runtime::new().expect("test runtime");
    let _entered = runtime.enter();
    let fixture = Fixture::new(SEEDED_CONFIG);
    let (port, hits) = mock_models_server();
    fixture.dispatch(SettingsAction::NewModel);
    let fields: Vec<(String, String)> = vec![
        ("model-form.slug".to_string(), "draft-provider".to_string()),
        (
            "model-form.base_url".to_string(),
            format!("http://127.0.0.1:{port}"),
        ),
        ("model-form.api_key".to_string(), "test-key".to_string()),
    ];
    for (id, value) in fields {
        fixture.dispatch(SettingsAction::SetDraft { id, value });
    }
    fixture.dispatch(SettingsAction::TestModelDraft);
    assert!(hits.load(Ordering::SeqCst) >= 1);
    let state = fixture.state.borrow();
    let Some(SettingsForm::Model(form)) = state.settings.form.as_ref() else {
        panic!("the form is still open");
    };
    let (kind, line) = form.probe.clone().expect("the form shows the probe");
    assert_eq!(kind, StatusKind::Success, "{line}");
    // Nothing was written yet.
    assert_eq!(fixture.text(), SEEDED_CONFIG);
    let _: &SettingsUiState = &state.settings;
}
