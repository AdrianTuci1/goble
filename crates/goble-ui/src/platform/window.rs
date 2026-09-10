use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use crate::elements::{AppContext, Element, LayoutContext, PaintContext, SizeConstraint};
use crate::event::DispatchedEvent;
use crate::geometry::{vec2f, Vector2F};
use crate::render::Renderer;
use winit::application::ApplicationHandler;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

/// A handle the UI uses to request window-level changes (e.g. toggling
/// fullscreen). The winit event loop owns the real [`Window`] and installs a
/// handler here once the window is created; the app layer can safely call
/// [`WindowControl::set_fullscreen`] before/after that (no-ops until a handler
/// is installed, so it can be built without a live window).
///
/// This is how the app crate (which constructs the element tree) reaches the
/// platform window: `AppContext` carries a `WindowControl` clone.
#[derive(Clone, Default)]
pub struct WindowControl {
    inner: Rc<RefCell<Option<Box<dyn FnMut(bool)>>>>,
    /// Registered app commands (name -> callback). Rebuilt by the app layer each
    /// frame (the closures are fresh per frame); the native menu bar and the
    /// command palette dispatch into this table by name.
    pub commands: Rc<RefCell<HashMap<String, Rc<RefCell<dyn FnMut()>>>>>,
}

impl WindowControl {
    /// Request the window to enter (`true`) or leave (`false`) fullscreen.
    /// No-op until the event loop has created the window and installed a
    /// handler via [`WindowControl::install`].
    pub fn set_fullscreen(&self, fullscreen: bool) {
        if let Some(handler) = self.inner.borrow_mut().as_mut() {
            handler(fullscreen);
        }
    }

    /// Install a handler that applies fullscreen state to a real window. Called
    /// by the platform event loop once the window exists.
    pub fn install<F: FnMut(bool) + 'static>(&self, handler: F) {
        *self.inner.borrow_mut() = Some(Box::new(handler));
    }

    /// Invoke a registered command by name (e.g. a menu item or the command
    /// palette). No-op if no callback is registered for that name.
    pub fn run_command(&self, name: &str) {
        if let Some(cb) = self.commands.borrow().get(name) {
            (cb.borrow_mut())();
        }
    }
}

/// Whole-app zoom clamps, so a runaway Cmd+Plus never collapses the UI.
pub const ZOOM_MIN: f32 = 0.5;
pub const ZOOM_MAX: f32 = 2.0;
pub const ZOOM_STEP: f32 = 0.1;

/// Clamp a zoom value into `[ZOOM_MIN, ZOOM_MAX]`.
pub fn clamp_zoom(zoom: f32) -> f32 {
    zoom.clamp(ZOOM_MIN, ZOOM_MAX)
}

/// Apply the whole-app zoom to the device-pixel-ratio: the render scale used to
/// lay out (in logical points) and paint. At `zoom > 1` the logical viewport
/// shrinks while the paint scale grows, so everything (fonts included) appears
/// larger.
pub fn render_scale_for(scale: f64, zoom: f32) -> f64 {
    scale * clamp_zoom(zoom) as f64
}

/// The logical size corresponding to `px` physical pixels at `scale`/`zoom`.
pub fn logical_dim(px: u32, scale: f64, zoom: f32) -> f32 {
    px as f32 / render_scale_for(scale, zoom) as f32
}

/// Adjust a zoom value by `delta` (steps), clamped. Returns the new value.
fn adjust_zoom(current: f32, delta: f32) -> f32 {
    clamp_zoom(current + delta)
}

pub fn run_with_root(
    root: Box<dyn Element>,
    app_context: Rc<RefCell<AppContext>>,
) -> anyhow::Result<()> {
    let event_loop = EventLoop::new()?;
    let mut app = App {
        window: None,
        surface_state: None,
        root,
        app_context,
        cursor_position: vec2f(0.0, 0.0),
        cursor_inside: false,
        modifiers: winit::event::Modifiers::default(),
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}

struct App {
    window: Option<Arc<Window>>,
    surface_state: Option<SurfaceState>,
    root: Box<dyn Element>,
    app_context: Rc<RefCell<AppContext>>,
    cursor_position: Vector2F,
    cursor_inside: bool,
    modifiers: winit::event::Modifiers,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let mut window_attributes = winit::window::WindowAttributes::default()
            .with_title("Goble")
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 800.0));

        // macOS: use the real OS titlebar, but make it transparent so the app
        // content (topbar surface) doubles as the titlebar background. The
        // traffic lights stay real and overlay the top-left of our content.
        #[cfg(target_os = "macos")]
        {
            use winit::platform::macos::WindowAttributesExtMacOS;
            window_attributes = window_attributes
                .with_titlebar_transparent(true)
                .with_title_hidden(true)
                .with_fullsize_content_view(true);
        }

        let window = Arc::new(event_loop.create_window(window_attributes).unwrap());
        let surface_state = pollster::block_on(SurfaceState::new(Arc::clone(&window))).unwrap();
        // Expose a fullscreen control to the UI: the app layer requests the real
        // window to enter/leave borderless fullscreen through
        // `AppContext::window_control`. The handler is installed now, when the
        // window first exists.
        let window_control = self.app_context.borrow().window_control.clone();
        let fullscreen_window = Arc::clone(&window);
        window_control.install(move |fullscreen: bool| {
            if fullscreen {
                fullscreen_window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
            } else {
                fullscreen_window.set_fullscreen(None);
            }
        });

        // macOS: install the native application menu bar (`NSMenu`), following
        // warp-new. The menu dispatches into the app command registry via
        // `WindowControl::run_command`, and adjusts the whole-app zoom.
        #[cfg(target_os = "macos")]
        {
            let ui_zoom = self.app_context.borrow().ui_zoom.clone();
            unsafe {
                crate::platform::mac::menus::install_main_menu(window_control, ui_zoom);
            }
        }

        self.window = Some(window);
        self.surface_state = Some(surface_state);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: winit::event::WindowEvent,
    ) {
        let window = match self.window.as_ref() {
            Some(w) => w,
            None => return,
        };
        if window_id != window.id() {
            return;
        }

        match event {
            winit::event::WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            winit::event::WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers;
            }
            winit::event::WindowEvent::Resized(new_size) => {
                if let Some(surface_state) = self.surface_state.as_mut() {
                    surface_state.resize(new_size.width, new_size.height);
                }
                window.request_redraw();
            }
            winit::event::WindowEvent::RedrawRequested => {
                let (width, height) = self
                    .surface_state
                    .as_ref()
                    .map(|s| (s.config.width, s.config.height))
                    .unwrap_or_else(|| {
                        let size = window.inner_size();
                        (size.width, size.height)
                    });
                // Layout in logical points; the render pass scales by the device
                // pixel ratio so 1 point == `scale` physical pixels. On HiDPI
                // displays `inner_size()`/surface config are physical pixels,
                // which would otherwise shrink every element on screen. The
                // whole-app zoom factor scales both axes so everything (fonts
                // included) appears larger uniformly.
                let scale = window.scale_factor();
                let zoom = *self.app_context.borrow().ui_zoom.borrow();
                let render_scale = render_scale_for(scale, zoom);
                let constraint = SizeConstraint::loose(vec2f(
                    logical_dim(width, scale, zoom),
                    logical_dim(height, scale, zoom),
                ));
                let mut layout_ctx = LayoutContext::default();
                let app_context = self.app_context.borrow().clone();
                let _ = self.root.layout(constraint, &mut layout_ctx, &app_context);
                let mut renderer = Renderer::new();
                {
                    let mut paint_ctx = PaintContext::new(renderer);
                    paint_ctx.cursor_position = self.cursor_position;
                    paint_ctx.cursor_inside = self.cursor_inside;
                    self.root
                        .paint(vec2f(0.0, 0.0), &mut paint_ctx, &app_context);
                    renderer = paint_ctx.renderer.take().unwrap();
                }
                if let Some(surface_state) = self.surface_state.as_mut() {
                    if let Err(e) = surface_state.render(&renderer, render_scale) {
                        log::error!("render error: {e}");
                    }
                }
            }
            winit::event::WindowEvent::CursorEntered { .. } => {
                self.cursor_inside = true;
            }
            winit::event::WindowEvent::CursorLeft { .. } => {
                self.cursor_inside = false;
            }
            // A terminal tells the program it hosts whether the user is looking
            // at it (mode 1004), so the focus has to reach the element tree.
            winit::event::WindowEvent::Focused(gained) => {
                let event = DispatchedEvent::Focus { gained };
                let mut event_ctx = crate::elements::EventContext::default();
                let app_context = self.app_context.borrow().clone();
                let _ = self
                    .root
                    .dispatch_event(&event, &mut event_ctx, &app_context);
                drop(app_context);
            }
            winit::event::WindowEvent::MouseInput { state, button, .. } => {
                let button_id = match button {
                    winit::event::MouseButton::Left => 0,
                    winit::event::MouseButton::Right => 1,
                    _ => 2,
                };
                let position = self.cursor_position;
                let event = match state {
                    winit::event::ElementState::Pressed => DispatchedEvent::MouseDown {
                        position,
                        button: button_id,
                    },
                    winit::event::ElementState::Released => DispatchedEvent::MouseUp {
                        position,
                        button: button_id,
                    },
                };
                let mut event_ctx = crate::elements::EventContext::default();
                let app_context = self.app_context.borrow().clone();
                let _ = self
                    .root
                    .dispatch_event(&event, &mut event_ctx, &app_context);
                drop(app_context);
                window.request_redraw();
            }
            winit::event::WindowEvent::CursorMoved { position, .. } => {
                // Cursor events arrive in physical pixels; convert to logical
                // (using the same zoomed render scale as layout) so hit-testing
                // against the logical-layout tree stays aligned.
                let zoom = *self.app_context.borrow().ui_zoom.borrow();
                let render_scale = render_scale_for(window.scale_factor(), zoom);
                let logical = position.to_logical::<f64>(render_scale);
                let pos = vec2f(logical.x as f32, logical.y as f32);
                self.cursor_position = pos;
                self.cursor_inside = true;
                let event = DispatchedEvent::MouseMove { position: pos };
                let mut event_ctx = crate::elements::EventContext::default();
                let app_context = self.app_context.borrow().clone();
                let _ = self
                    .root
                    .dispatch_event(&event, &mut event_ctx, &app_context);
                drop(app_context);
            }
            winit::event::WindowEvent::MouseWheel { delta, .. } => {
                let zoom = *self.app_context.borrow().ui_zoom.borrow();
                let render_scale = render_scale_for(window.scale_factor(), zoom);
                let delta = match delta {
                    winit::event::MouseScrollDelta::LineDelta(x, y) => vec2f(x * 20.0, y * 20.0),
                    // Pixel deltas are physical; divide by the zoomed scale for
                    // logical px (so the scroll amount matches the layout).
                    winit::event::MouseScrollDelta::PixelDelta(p) => {
                        let logical = p.to_logical::<f64>(render_scale);
                        vec2f(logical.x as f32, logical.y as f32)
                    }
                };
                let event = DispatchedEvent::Scroll { delta };
                let mut event_ctx = crate::elements::EventContext::default();
                let app_context = self.app_context.borrow().clone();
                let _ = self
                    .root
                    .dispatch_event(&event, &mut event_ctx, &app_context);
                drop(app_context);
                window.request_redraw();
            }
            winit::event::WindowEvent::KeyboardInput { event, .. } => {
                if let Some(key) = logical_key_string(&event.logical_key) {
                    // Whole-app zoom (Cmd/Ctrl+Plus/Minus/0). Handled before the
                    // tree so a focused composer cannot swallow the keys. This is
                    // the app-level zoom; on macOS it is also reachable via the
                    // native menubar's View → Zoom items (see `mac/menus.rs`).
                    let cmd =
                        self.modifiers.state().super_key() || self.modifiers.state().control_key();
                    if event.state == winit::event::ElementState::Pressed && cmd {
                        let mut applied = false;
                        {
                            let ui_zoom = self.app_context.borrow().ui_zoom.clone();
                            let mut zoom = ui_zoom.borrow_mut();
                            if key == "+" || key == "=" {
                                *zoom = adjust_zoom(*zoom, ZOOM_STEP);
                                applied = true;
                            } else if key == "-" || key == "_" {
                                *zoom = adjust_zoom(*zoom, -ZOOM_STEP);
                                applied = true;
                            } else if key == "0" {
                                *zoom = 1.0;
                                applied = true;
                            }
                        }
                        if applied {
                            window.request_redraw();
                            return;
                        }
                    }
                    let modifiers = map_modifiers(&self.modifiers);
                    let event = match event.state {
                        winit::event::ElementState::Pressed => {
                            DispatchedEvent::KeyDown { key, modifiers }
                        }
                        winit::event::ElementState::Released => {
                            DispatchedEvent::KeyUp { key, modifiers }
                        }
                    };
                    let mut event_ctx = crate::elements::EventContext::default();
                    let app_context = self.app_context.borrow().clone();
                    let _ = self
                        .root
                        .dispatch_event(&event, &mut event_ctx, &app_context);
                    drop(app_context);
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }
}

/// Map winit modifier bits onto the UI [`ModifiersState`].
fn map_modifiers(modifiers: &winit::event::Modifiers) -> crate::event::ModifiersState {
    crate::event::ModifiersState {
        alt: modifiers.state().alt_key(),
        ctrl: modifiers.state().control_key(),
        command: modifiers.state().super_key(),
        shift: modifiers.state().shift_key(),
    }
}

/// Convert a winit logical key to the string form used by `DispatchedEvent::KeyDown`.
///
/// Printable keys arrive as `Key::Character`; navigation/editing keys arrive as
/// `Key::Named` and would otherwise be swallowed by the input elements.
fn logical_key_string(key: &winit::keyboard::Key) -> Option<String> {
    match key {
        winit::keyboard::Key::Character(c) => Some(c.to_string()),
        winit::keyboard::Key::Named(named) => {
            let s = match named {
                winit::keyboard::NamedKey::Backspace => "Backspace",
                winit::keyboard::NamedKey::Enter => "Enter",
                winit::keyboard::NamedKey::Tab => "Tab",
                winit::keyboard::NamedKey::Escape => "Escape",
                winit::keyboard::NamedKey::Delete => "Delete",
                winit::keyboard::NamedKey::ArrowLeft => "ArrowLeft",
                winit::keyboard::NamedKey::ArrowRight => "ArrowRight",
                winit::keyboard::NamedKey::ArrowUp => "ArrowUp",
                winit::keyboard::NamedKey::ArrowDown => "ArrowDown",
                winit::keyboard::NamedKey::Space => " ",
                winit::keyboard::NamedKey::Home => "Home",
                winit::keyboard::NamedKey::End => "End",
                winit::keyboard::NamedKey::PageUp => "PageUp",
                winit::keyboard::NamedKey::PageDown => "PageDown",
                winit::keyboard::NamedKey::Insert => "Insert",
                winit::keyboard::NamedKey::F1 => "F1",
                winit::keyboard::NamedKey::F2 => "F2",
                winit::keyboard::NamedKey::F3 => "F3",
                winit::keyboard::NamedKey::F4 => "F4",
                winit::keyboard::NamedKey::F5 => "F5",
                winit::keyboard::NamedKey::F6 => "F6",
                winit::keyboard::NamedKey::F7 => "F7",
                winit::keyboard::NamedKey::F8 => "F8",
                winit::keyboard::NamedKey::F9 => "F9",
                winit::keyboard::NamedKey::F10 => "F10",
                winit::keyboard::NamedKey::F11 => "F11",
                winit::keyboard::NamedKey::F12 => "F12",
                winit::keyboard::NamedKey::F13 => "F13",
                winit::keyboard::NamedKey::F14 => "F14",
                winit::keyboard::NamedKey::F15 => "F15",
                winit::keyboard::NamedKey::F16 => "F16",
                winit::keyboard::NamedKey::F17 => "F17",
                winit::keyboard::NamedKey::F18 => "F18",
                winit::keyboard::NamedKey::F19 => "F19",
                winit::keyboard::NamedKey::F20 => "F20",
                winit::keyboard::NamedKey::F21 => "F21",
                winit::keyboard::NamedKey::F22 => "F22",
                winit::keyboard::NamedKey::F23 => "F23",
                winit::keyboard::NamedKey::F24 => "F24",
                winit::keyboard::NamedKey::F25 => "F25",
                winit::keyboard::NamedKey::F26 => "F26",
                winit::keyboard::NamedKey::F27 => "F27",
                winit::keyboard::NamedKey::F28 => "F28",
                winit::keyboard::NamedKey::F29 => "F29",
                winit::keyboard::NamedKey::F30 => "F30",
                winit::keyboard::NamedKey::F31 => "F31",
                winit::keyboard::NamedKey::F32 => "F32",
                winit::keyboard::NamedKey::F33 => "F33",
                winit::keyboard::NamedKey::F34 => "F34",
                winit::keyboard::NamedKey::F35 => "F35",
                _ => return None,
            };
            Some(s.to_string())
        }
        _ => None,
    }
}

struct SurfaceState {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    engine: crate::platform::wgpu_render_engine::WgpuRenderEngine,
}

impl SurfaceState {
    async fn new(window: Arc<winit::window::Window>) -> anyhow::Result<Self> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });
        let surface = instance.create_surface(Arc::clone(&window))?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|_| anyhow::anyhow!("no wgpu adapter"))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                label: Some("goble-ui device"),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            })
            .await?;
        let size = window.inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);
        let mut config = surface
            .get_default_config(&adapter, width, height)
            .ok_or_else(|| anyhow::anyhow!("no surface config"))?;
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&device, &config);
        let engine = crate::platform::wgpu_render_engine::WgpuRenderEngine::new(
            &device,
            &queue,
            config.format,
        );
        Ok(Self {
            surface,
            device,
            queue,
            config,
            engine,
        })
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(&self.device, &self.config);
    }

    fn render(&mut self, renderer: &Renderer, scale: f64) -> anyhow::Result<()> {
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.engine.render(
            &self.device,
            &self.queue,
            &view,
            (self.config.width, self.config.height),
            renderer,
            scale as f32,
        );
        output.present();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_respects_min_and_max_clamp() {
        assert_eq!(clamp_zoom(0.1), ZOOM_MIN);
        assert_eq!(clamp_zoom(2.4), ZOOM_MAX);
        assert_eq!(clamp_zoom(1.0), 1.0);
    }

    #[test]
    fn render_scale_scales_by_zoom() {
        assert_eq!(render_scale_for(2.0, 1.0), 2.0);
        assert!((render_scale_for(2.0, 2.0) - 4.0).abs() < 1e-6);
        assert!((render_scale_for(2.0, 0.5) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn logical_dim_shrinks_as_zoom_grows() {
        let base = logical_dim(1280, 2.0, 1.0);
        let zoomed = logical_dim(1280, 2.0, 2.0);
        assert!(zoomed < base);
        assert!((zoomed - base / 2.0).abs() < 1e-6);
    }

    #[test]
    fn run_command_invokes_registered_callback() {
        let wc = WindowControl::default();
        let calls = Rc::new(RefCell::new(0i32));
        let calls2 = Rc::clone(&calls);
        wc.commands.borrow_mut().insert(
            "ping".to_string(),
            Rc::new(RefCell::new(move || {
                *calls2.borrow_mut() += 1;
            })),
        );
        wc.run_command("ping");
        wc.run_command("ping");
        assert_eq!(*calls.borrow(), 2);
        // Unknown commands are a no-op.
        wc.run_command("missing");
        assert_eq!(*calls.borrow(), 2);
    }

    #[test]
    fn app_context_default_zoom_is_one() {
        let app = crate::elements::AppContext::default();
        assert_eq!(*app.ui_zoom.borrow(), 1.0);
    }
}
