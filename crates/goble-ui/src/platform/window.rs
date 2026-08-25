use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::elements::{AppContext, Element, LayoutContext, PaintContext, SizeConstraint};
use crate::event::DispatchedEvent;
use crate::geometry::{vec2f, Vector2F};
use crate::render::Renderer;
use winit::application::ApplicationHandler;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

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
                // which would otherwise shrink every element on screen.
                let scale = window.scale_factor();
                let constraint = SizeConstraint::loose(vec2f(
                    width as f32 / scale as f32,
                    height as f32 / scale as f32,
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
                    if let Err(e) = surface_state.render(&renderer, scale) {
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
                // so hit-testing against the logical-layout tree stays aligned.
                let scale = window.scale_factor();
                let logical = position.to_logical::<f64>(scale);
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
                let scale = window.scale_factor();
                let delta = match delta {
                    winit::event::MouseScrollDelta::LineDelta(x, y) => vec2f(x * 20.0, y * 20.0),
                    // Pixel deltas are physical; divide by scale for logical px.
                    winit::event::MouseScrollDelta::PixelDelta(p) => {
                        let logical = p.to_logical::<f64>(scale);
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
                    let event = match event.state {
                        winit::event::ElementState::Pressed => DispatchedEvent::KeyDown { key },
                        winit::event::ElementState::Released => DispatchedEvent::KeyUp { key },
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
