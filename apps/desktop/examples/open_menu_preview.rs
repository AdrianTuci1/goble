//! Visual verification of the window's open-menu behaviour: the pointer over an
//! open menu must belong to the menu, and the rows/pills/tabs behind it must not
//! read as hovered.
//!
//! Each scene renders twice through the real app root (`RootView`), with the
//! pointer parked over the open menu:
//!
//! * `*-before.png` — the hover state the unrestricted pointer left behind: the
//!   row underneath the open menu is marked hovered (band, title/time swap and
//!   its hover card), which is the behaviour the owner reported.
//! * `*-after.png` — the same frame with the shared open-menu gate in force: the
//!   pointer on the menu's surface no longer touches the row behind it, so the
//!   row rests while the menu keeps its own highlight.
//!
//! Usage: cargo run -p goble-app --example open_menu_preview

#[path = "../../../crates/goble-ui/examples/composer_preview/raster.rs"]
mod raster;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_core::store::Store;
use goble_desktop_service::{DesktopState, ThreadStore};
use goble_ui::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, SizeConstraint,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::{vec2f, Vector2F};
use goble_ui::render::{RenderCommand, Renderer};

use goble_app::root_view::RootView;
use goble_app::state::{SpaceMenu, UiState};

const OUT_DIR: &str = "/tmp/goble-work/out";
const WINDOW_W: f32 = 1024.0;
const WINDOW_H: f32 = 768.0;

fn window() -> Vector2F {
    vec2f(WINDOW_W, WINDOW_H)
}

struct Scene {
    root: Box<dyn Element>,
    state: Rc<RefCell<UiState>>,
    /// The commands of the last painted frame, so a scene can ask where the
    /// rows landed.
    last_commands: Vec<RenderCommand>,
    _desktop: Arc<DesktopState>,
    _dir: tempfile::TempDir,
}

impl Scene {
    fn new(app: &AppContext) -> Self {
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        );
        let first = desktop
            .create_chat("Redesign the sidebar", None, None)
            .expect("create the first conversation");
        desktop
            .add_chat_message(&first, "user", "salut")
            .expect("the user's prompt");
        desktop
            .add_chat_message(&first, "assistant", "Bine ai venit!")
            .expect("the agent's reply");
        desktop
            .set_chat_working_dir(&first, "/tmp/goble-work")
            .expect("the conversation's directory");
        let second = desktop
            .create_chat("Automate Instagram posting", None, None)
            .expect("create the second conversation");
        desktop
            .add_chat_message(&second, "assistant", "done")
            .expect("its reply");
        let view = RootView::new(app, &desktop, None);
        let state = view.state_rc();
        {
            let mut s = state.borrow_mut();
            s.show_workspace_choice = false;
            s.show_llm_key_banner = false;
            s.show_onboarding_tip = false;
            s.right_sidebar_open = false;
            s.crons_open = false;
            s.refresh_conversations(&desktop);
            // Two more workspace tabs, so the tab strip has a tab that is not
            // the selected one for the tab-menu scene's hover comparison.
            for (id, label) in [(2u64, "Second workspace"), (3, "Third workspace")] {
                s.spaces.push(goble_app::ui::Space::new(
                    label,
                    goble_app::ui::Pane::Leaf {
                        id,
                        kind: goble_app::ui::PaneKind::Chat,
                    },
                ));
            }
        }
        Self {
            root: Box::new(view),
            state,
            last_commands: Vec::new(),
            _desktop: desktop,
            _dir: dir,
        }
    }

    /// Lay out and paint the whole app, with the pointer parked at `cursor`
    /// (outside the window when `None`), exactly as one frame of the real loop.
    fn frame(&mut self, app: &AppContext, cursor: Option<Vector2F>) -> Vec<RenderCommand> {
        let _ = self.root.layout(
            SizeConstraint::loose(window()),
            &mut LayoutContext::default(),
            app,
        );
        let mut ctx = PaintContext::new(Renderer::new());
        if let Some(point) = cursor {
            ctx.cursor_position = point;
            ctx.cursor_inside = true;
        }
        self.root.paint(vec2f(0.0, 0.0), &mut ctx, app);
        let commands = ctx
            .renderer
            .take()
            .map(|renderer| renderer.commands().to_vec())
            .unwrap_or_default();
        self.last_commands = commands.clone();
        commands
    }

    fn dispatch(&mut self, app: &AppContext, event: DispatchedEvent) {
        let mut ctx = EventContext::default();
        self.root.dispatch_event(&event, &mut ctx, app);
    }

    fn click(&mut self, app: &AppContext, at: Vector2F) {
        for event in [
            DispatchedEvent::MouseDown {
                position: at,
                button: 0,
            },
            DispatchedEvent::MouseUp {
                position: at,
                button: 0,
            },
        ] {
            self.dispatch(app, event);
        }
    }

    /// The centre of a drawn glyph with that name.
    fn icon(&self, commands: &[RenderCommand], name: &str) -> Vector2F {
        commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawIcon {
                    name: drawn,
                    origin,
                    size,
                    ..
                } if drawn == name => Some(*origin + vec2f(*size / 2.0, *size / 2.0)),
                _ => None,
            })
            .unwrap_or_else(|| panic!("the frame draws the {name:?} glyph"))
    }

    /// The id of the first listed conversation.
    fn first_conversation(&self) -> String {
        self.state
            .borrow()
            .conversations
            .first()
            .map(|entry| entry.id.clone())
            .expect("a conversation is listed")
    }

    /// The conversation whose title the last frame drew on the tray's own
    /// height, starting left of the tray: that is the row the tray covers. Its
    /// row is still painted (the tray draws over it), so the hover state the
    /// old unrestricted pointer left on it is what the before image shows.
    fn row_behind(&self, surface: goble_ui::RectF) -> String {
        let state = self.state.borrow();
        let names: Vec<(String, String)> = state
            .conversations
            .iter()
            .map(|entry| (entry.name.clone(), entry.id.clone()))
            .collect();
        drop(state);
        self.last_commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } => {
                    let on_row = origin.y >= surface.min_y()
                        && origin.y <= surface.max_y()
                        && origin.x < surface.min_x();
                    on_row
                        .then(|| names.iter().find(|(name, _)| name == text))
                        .flatten()
                        .map(|(_, id)| id.clone())
                }
                _ => None,
            })
            .unwrap_or_else(|| self.first_conversation())
    }

    /// Mark the first conversation's row as hovered for this one frame, with its
    /// hover card open — the state an unrestricted hover over the row leaves.
    fn mark_row_hovered(&self, id: &str) {
        let mut state = self.state.borrow_mut();
        *state.sidebar_hover_row.borrow_mut() = Some(format!("session:{id}"));
        if let Some(card) = state.agent_cards.get(id).cloned() {
            let mut card = card.borrow_mut();
            card.hover = true;
            card.hover_card_since = Some(std::time::Instant::now());
            card.hover_card_open = true;
            card.marquee_offset = 14.0;
        }
    }

    /// Force the active pane header's hover cell for the next frame.
    fn set_pane_hover(&self, value: bool) {
        let state = self.state.borrow();
        let pane_id = state.active_pane_id;
        if let Some(hover) = state.pane_hover.get(&pane_id) {
            *hover.borrow_mut() = value;
        }
    }

    fn clear_row_hovered(&self) {
        let mut state = self.state.borrow_mut();
        *state.sidebar_hover_row.borrow_mut() = None;
        for card in state.agent_cards.values() {
            let mut card = card.borrow_mut();
            card.hover = false;
            card.hover_card_since = None;
            card.hover_card_open = false;
            card.marquee_offset = 0.0;
        }
    }
}

fn save(
    assets: &raster::PreviewAssets,
    app: &AppContext,
    commands: &[RenderCommand],
    name: &str,
) -> anyhow::Result<()> {
    let out = std::path::Path::new(OUT_DIR).join(name);
    raster::rasterize_to_png(assets, app, commands, window(), 1.0, None, &out)
}

fn main() -> anyhow::Result<()> {
    std::fs::create_dir_all(OUT_DIR)?;
    let assets = raster::PreviewAssets::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/goble-ui/assets"),
    )?;

    // 1. The sidebar's view-options menu over the session rows: the row behind
    //    the open panel must not hover (its hover card used to pop out beside
    //    the panel while the pointer was on the menu).
    {
        let app = AppContext::default();
        let mut scene = Scene::new(&app);
        let commands = scene.frame(&app, None);
        let trigger = scene.icon(&commands, "sliders-two");
        scene.click(&app, trigger);
        // A frame with no pointer so the registry learns the panel's rectangle.
        let _ = scene.frame(&app, None);
        let surface = app
            .open_menus
            .surfaces()
            .into_iter()
            .next()
            .expect("the view-options panel reported its surface");
        let point = vec2f(
            surface.min_x() + surface.width() * 0.5,
            surface.min_y() + surface.height() * 0.35,
        );
        println!(
            "sidebar view-options: open menu {:?} at {:?}",
            app.open_menus.id(),
            surface
        );

        let id = scene.first_conversation();
        scene.mark_row_hovered(&id);
        let commands = scene.frame(&app, Some(point));
        save(
            &assets,
            &app,
            &commands,
            "open-menu-sidebar-view-options-before.png",
        )?;
        // The row the unrestricted pointer left hovered is the one under the
        // panel; after the frame the gate clears it.
        assert!(
            scene.state.borrow().sidebar_hover_row.borrow().is_none(),
            "the row behind the open menu is not left hovered once the menu owns the pointer"
        );

        scene.clear_row_hovered();
        let commands = scene.frame(&app, Some(point));
        save(
            &assets,
            &app,
            &commands,
            "open-menu-sidebar-view-options-after.png",
        )?;
    }

    // 2. A session row's "..." tray: the row behind the open tray must rest.
    {
        let app = AppContext::default();
        let mut scene = Scene::new(&app);
        let id = scene.first_conversation();
        scene.mark_row_hovered(&id);
        let commands = scene.frame(&app, None);
        let dots = scene.icon(&commands, "dots-horizontal");
        scene.click(&app, dots);
        let _ = scene.frame(&app, None);
        let surface = app
            .open_menus
            .surfaces()
            .into_iter()
            .next()
            .expect("the tray reported its surface");
        let point = vec2f(
            surface.min_x() + surface.width() * 0.5,
            surface.min_y() + surface.height() * 0.5,
        );
        println!(
            "session tray: open menu {:?} at {:?}",
            app.open_menus.id(),
            surface
        );

        // The row state the tray opening left behind (the pointer walked from the
        // row to the tray), as the unrestricted hover left it: the row the tray
        // covers at its own height.
        let behind = scene.row_behind(surface);
        let mut state = scene.state.borrow_mut();
        *state.sidebar_hover_row.borrow_mut() = Some(format!("session:{behind}"));
        if let Some(card) = state.agent_cards.get(&behind).cloned() {
            let mut card = card.borrow_mut();
            card.hover = true;
            card.hover_card_since = Some(std::time::Instant::now());
            card.hover_card_open = true;
        }
        drop(state);
        let commands = scene.frame(&app, Some(point));
        save(
            &assets,
            &app,
            &commands,
            "open-menu-session-tray-before.png",
        )?;

        scene.clear_row_hovered();
        let commands = scene.frame(&app, Some(point));
        save(&assets, &app, &commands, "open-menu-session-tray-after.png")?;
    }

    // 3. The workspace tab's context menu: the tab strip under the open menu
    //    must not hover.
    {
        let app = AppContext::default();
        let mut scene = Scene::new(&app);
        let _ = scene.frame(&app, None);
        // Three tabs; the menu hangs over the second one, which is not the
        // selected tab (so its hover pill is visible when the pointer used to
        // reach it).
        *scene.state.borrow_mut().space_menu.borrow_mut() = Some(SpaceMenu {
            index: 1,
            at: vec2f(600.0, 20.0),
        });
        let _ = scene.frame(&app, None);
        let surface = app
            .open_menus
            .surfaces()
            .into_iter()
            .next()
            .expect("the tab menu reported its surface");
        let point = vec2f(
            surface.min_x() + surface.width() * 0.5,
            surface.min_y() + surface.height() * 0.5,
        );
        println!(
            "tab menu: open menu {:?} at {:?}",
            app.open_menus.id(),
            surface
        );

        // The pane-header hover the unrestricted pointer left under the open
        // menu (read while the header is built, so the before frame draws it).
        scene.set_pane_hover(true);
        let commands = scene.frame(&app, Some(point));
        save(&assets, &app, &commands, "open-menu-tab-context-before.png")?;

        scene.set_pane_hover(false);
        let commands = scene.frame(&app, Some(point));
        save(&assets, &app, &commands, "open-menu-tab-context-after.png")?;
        scene.state.borrow_mut().space_hover = None;
    }

    // 4. The composer's model card: the pointer over the card is the card's.
    {
        let app = AppContext::default();
        let mut scene = Scene::new(&app);
        let _ = scene.frame(&app, None);
        let pane_id = scene.state.borrow().active_pane_id;
        *scene
            .state
            .borrow_mut()
            .pane_controls_mut(pane_id)
            .model_tray_open
            .borrow_mut() = true;
        let _ = scene.frame(&app, None);
        let surface = app
            .open_menus
            .surfaces()
            .into_iter()
            .next()
            .expect("the model card reported its surface");
        let point = vec2f(
            surface.min_x() + surface.width() * 0.5,
            surface.min_y() + surface.height() * 0.4,
        );
        println!(
            "composer model card: open menu {:?} at {:?}",
            app.open_menus.id(),
            surface
        );

        // The pane header's hover state that the pointer used to leave behind.
        *scene
            .state
            .borrow_mut()
            .pane_hover
            .get(&pane_id)
            .expect("the pane owns a hover cell")
            .borrow_mut() = true;
        let commands = scene.frame(&app, Some(point));
        save(
            &assets,
            &app,
            &commands,
            "open-menu-composer-model-before.png",
        )?;

        *scene
            .state
            .borrow_mut()
            .pane_hover
            .get(&pane_id)
            .expect("the pane owns a hover cell")
            .borrow_mut() = false;
        let commands = scene.frame(&app, Some(point));
        save(
            &assets,
            &app,
            &commands,
            "open-menu-composer-model-after.png",
        )?;
    }

    Ok(())
}
