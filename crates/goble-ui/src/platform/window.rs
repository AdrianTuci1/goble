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
        modifiers: winit::keyboard::ModifiersState::empty(),
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
    modifiers: winit::keyboard::ModifiersState,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window_attributes = winit::window::WindowAttributes::default()
            .with_title("Goble")
            .with_inner_size(winit::dpi::LogicalSize::new(1024.0, 768.0));
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
                let size = window.inner_size();
                let constraint = SizeConstraint::loose(vec2f(size.width as f32, size.height as f32));
                let mut layout_ctx = LayoutContext::default();
                let app_context = self.app_context.borrow();
                let _ = self.root.layout(constraint, &mut layout_ctx, &*app_context);
                let mut renderer = Renderer::new();
                {
                    let mut paint_ctx = PaintContext::new(renderer);
                    self.root.paint(vec2f(0.0, 0.0), &mut paint_ctx, &*app_context);
                    renderer = paint_ctx.renderer.take().unwrap();
                }
                if let Some(surface_state) = self.surface_state.as_mut() {
                    if let Err(e) = surface_state.render(&renderer) {
                        log::error!("render error: {e}");
                    }
                }
            }
            winit::event::WindowEvent::MouseInput { state, button, .. } => {
                let button_id = match button {
                    winit::event::MouseButton::Left => 0,
                    winit::event::MouseButton::Right => 1,
                    _ => 2,
                };
                let position = self.cursor_position;
                let event = match state {
                    winit::event::ElementState::Pressed => DispatchedEvent::MouseDown { position, button: button_id },
                    winit::event::ElementState::Released => DispatchedEvent::MouseUp { position, button: button_id },
                };
                let mut event_ctx = crate::elements::EventContext::default();
                let app_context = self.app_context.borrow();
                let _ = self.root.dispatch_event(&event, &mut event_ctx, &*app_context);
                drop(app_context);
                window.request_redraw();
            }
            winit::event::WindowEvent::CursorMoved { position, .. } => {
                let pos = vec2f(position.x as f32, position.y as f32);
                self.cursor_position = pos;
                let event = DispatchedEvent::MouseMove { position: pos };
                let mut event_ctx = crate::elements::EventContext::default();
                let app_context = self.app_context.borrow();
                let _ = self.root.dispatch_event(&event, &mut event_ctx, &*app_context);
                drop(app_context);
            }
            winit::event::WindowEvent::MouseWheel { delta, .. } => {
                let delta = match delta {
                    winit::event::MouseScrollDelta::LineDelta(x, y) => vec2f(x * 20.0, y * 20.0),
                    winit::event::MouseScrollDelta::PixelDelta(p) => vec2f(p.x as f32, p.y as f32),
                };
                let event = DispatchedEvent::Scroll { delta };
                let mut event_ctx = crate::elements::EventContext::default();
                let app_context = self.app_context.borrow();
                let _ = self.root.dispatch_event(&event, &mut event_ctx, &*app_context);
                drop(app_context);
                window.request_redraw();
            }
            winit::event::WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            winit::event::WindowEvent::KeyboardInput { event, .. } => {
                if let winit::keyboard::Key::Character(c) = event.logical_key {
                    let shift = self.modifiers.shift_key();
                    let event = match event.state {
                        winit::event::ElementState::Pressed => DispatchedEvent::KeyDown { key: c.to_string(), shift },
                        winit::event::ElementState::Released => DispatchedEvent::KeyUp { key: c.to_string(), shift },
                    };
                    let mut event_ctx = crate::elements::EventContext::default();
                    let app_context = self.app_context.borrow();
                    let _ = self.root.dispatch_event(&event, &mut event_ctx, &*app_context);
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
                required_limits: wgpu::Limits::downlevel_defaults(),
                label: Some("goble-ui device"),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            })
            .await?;
        let size = window.inner_size();
        let config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or_else(|| anyhow::anyhow!("no surface config"))?;
        surface.configure(&device, &config);
        let engine = crate::platform::wgpu_render_engine::WgpuRenderEngine::new(&device, &queue);
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

    fn render(&mut self, renderer: &Renderer) -> anyhow::Result<()> {
        let output = self.surface.get_current_texture()?;
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.engine.render(
            &self.device,
            &self.queue,
            &view,
            (self.config.width, self.config.height),
            renderer,
        );
        output.present();
        Ok(())
    }
}
