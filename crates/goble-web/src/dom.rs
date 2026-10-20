//! Browser shell: boot, worker list, tabs, terminal canvas, input and chrome.
//!
//! All DOM work lives here so the protocol/session/terminal logic stays
//! native-testable. The shell is built from one trusted markup string; worker
//! ids and terminal titles are always inserted with `textContent`, never as
//! HTML.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use serde::Deserialize;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{
    ClipboardEvent, CloseEvent, CompositionEvent, Document, DomRect, Element, Event,
    HtmlCanvasElement, HtmlElement, HtmlTextAreaElement, KeyboardEvent, MessageEvent, MouseEvent,
    Request, RequestCredentials, RequestInit, RequestMode, ResizeObserver, Response, WebSocket,
    WheelEvent, Window,
};

use crate::input::{self, KeyEvent};
use crate::layout::Tabs;
use crate::protocol::{self, ClientFrame};
use crate::render::{DrawOptions, RenderLoop, Renderer};
use crate::session::{Plan, Session, SessionEvent, SessionState};
use crate::theme::Theme;
use crate::vt::{CellPos, Selection, Terminal};

const TIMER_MS: i32 = 250;
const WORKER_REFRESH_MS: f64 = 5_000.0;
const PING_INTERVAL_MS: f64 = 5_000.0;
const BLINK_MS: f64 = 500.0;
const BELL_FLASH_MS: f64 = 140.0;

/// Trusted shell markup. Every id referenced by [`App::new`] lives here.
const SHELL: &str = r##"
<div class="app" id="goble-app">
  <header class="topbar">
    <div class="brand" aria-label="Goble">
      <span class="mark" aria-hidden="true"></span>
      <span class="brand-name">goble</span>
    </div>
    <button class="icon drawer-toggle" type="button" data-action="toggle-drawer" aria-label="Show workers" title="Workers">☰</button>
    <div class="tabstrip" id="tabstrip" role="tablist" aria-label="Terminal tabs"></div>
    <button class="new-tab" type="button" data-action="new-tab" aria-label="New terminal" title="New tab (Ctrl/Cmd+Shift+T)">+</button>
    <div class="top-right">
      <span class="dot off" id="conn-dot" aria-hidden="true"></span>
      <span class="active-worker" id="active-worker-label">no worker</span>
      <button class="icon" type="button" data-action="help" aria-label="Keyboard shortcuts" title="Keyboard shortcuts">?</button>
      <form method="post" action="/logout"><button class="ghost logout" type="submit">Log out</button></form>
    </div>
  </header>
  <div class="body">
    <aside class="sidebar" id="sidebar" aria-label="Workers">
      <div class="sidebar-head">
        <span>Workers</span>
        <button class="ghost" type="button" data-action="refresh-workers" title="Refresh now">Refresh</button>
      </div>
      <div class="worker-list" id="worker-list"></div>
      <div class="sidebar-note" id="workers-updated">Not refreshed yet</div>
    </aside>
    <main class="main">
      <div class="terminal-wrap" id="terminal-wrap">
        <canvas id="terminal" tabindex="0" aria-label="Terminal screen"></canvas>
        <textarea id="key-input" class="key-input" aria-label="Terminal input" autocomplete="off"
          autocapitalize="off" autocorrect="off" spellcheck="false" enterkeyhint="send"></textarea>
        <div class="overlay" id="overlay" hidden>
          <div class="overlay-card">
            <div class="overlay-spinner" aria-hidden="true"></div>
            <h2 id="overlay-title">Connecting…</h2>
            <p id="overlay-detail"></p>
            <button class="primary" type="button" data-action="retry" id="overlay-retry">Retry now</button>
          </div>
        </div>
        <div class="empty-state" id="empty-state">
          <div class="empty-mark" aria-hidden="true"></div>
          <h2 id="empty-title">No terminal open</h2>
          <p id="empty-hint">Pick a connected worker in the sidebar, or press Ctrl/Cmd + Shift + T.</p>
          <button class="primary" type="button" data-action="new-tab">Open a terminal</button>
        </div>
      </div>
      <footer class="statusbar">
        <span class="status-state" id="status-state">idle</span>
        <span class="sep" aria-hidden="true"></span>
        <span id="status-size">—</span>
        <span class="sep" aria-hidden="true"></span>
        <span id="status-pane">pane —</span>
        <span class="sep" aria-hidden="true"></span>
        <span id="status-scroll">-0</span>
        <span class="grow"></span>
        <span id="status-worker" class="status-worker">no worker</span>
        <span class="sep" aria-hidden="true"></span>
        <span id="status-latency">—</span>
      </footer>
      <p class="sr-only" id="sr-status" aria-live="polite"></p>
    </main>
  </div>
  <div class="scrim" id="scrim" data-action="close-drawer" aria-hidden="true"></div>
  <div class="toast" id="toast" hidden></div>
  <div class="help" id="help" hidden role="dialog" aria-label="Keyboard shortcuts">
    <div class="help-head"><h2>Keyboard</h2><button class="icon" type="button" data-action="help" aria-label="Close">×</button></div>
    <dl>
      <div><dt>New tab</dt><dd><kbd>Ctrl</kbd><kbd>Shift</kbd><kbd>T</kbd></dd></div>
      <div><dt>Close tab</dt><dd><kbd>Ctrl</kbd><kbd>Shift</kbd><kbd>W</kbd></dd></div>
      <div><dt>Next / previous tab</dt><dd><kbd>Ctrl</kbd><kbd>Tab</kbd> / <kbd>Ctrl</kbd><kbd>Shift</kbd><kbd>Tab</kbd></dd></div>
      <div><dt>Select tab 1–9</dt><dd><kbd>Alt</kbd><kbd>1</kbd>…<kbd>9</kbd></dd></div>
      <div><dt>Reconnect</dt><dd><kbd>Ctrl</kbd><kbd>Shift</kbd><kbd>R</kbd></dd></div>
      <div><dt>Scroll history</dt><dd><kbd>Shift</kbd><kbd>PgUp</kbd> / <kbd>Shift</kbd><kbd>PgDn</kbd></dd></div>
      <div><dt>Copy selection</dt><dd><kbd>Ctrl</kbd><kbd>Shift</kbd><kbd>C</kbd></dd></div>
      <div><dt>Paste</dt><dd><kbd>Ctrl</kbd><kbd>Shift</kbd><kbd>V</kbd></dd></div>
      <div><dt>Open link</dt><dd><kbd>Ctrl</kbd> + click</dd></div>
    </dl>
    <p class="help-note">On macOS use ⌘ where Ctrl is shown. Everything else goes to the shell.</p>
  </div>
</div>
"##;

#[derive(Debug, Clone, Deserialize)]
struct WorkerInfo {
    id: String,
    connected_at: String,
    last_seen: String,
}

#[derive(Debug, Deserialize)]
struct WorkersResponse {
    workers: Vec<WorkerInfo>,
}

struct TabData {
    session: Session,
    terminal: Terminal,
    socket: Option<WebSocket>,
    listeners: Vec<Closure<dyn FnMut(Event)>>,
    pending: Vec<ClientFrame>,
    selection: Option<Selection>,
    failure: Option<String>,
    last_ping_at: f64,
    last_ping_sent: f64,
    latency_ms: Option<f64>,
}

enum FetchOutcome {
    Workers(Vec<WorkerInfo>),
    Unauthorized,
}

struct App {
    window: Window,
    document: Document,
    me: std::rc::Weak<RefCell<App>>,
    theme: Theme,
    tabs: Tabs,
    tab_data: HashMap<u64, TabData>,
    workers: Vec<WorkerInfo>,
    workers_error: Option<String>,
    workers_loaded: bool,
    workers_updated: f64,
    fetching: bool,
    selected_worker: Option<String>,
    renderer: Renderer,
    render_loop: RenderLoop,
    canvas: HtmlCanvasElement,
    input: HtmlTextAreaElement,
    viewport: HtmlElement,
    app_el: HtmlElement,
    tabstrip: Element,
    worker_list: Element,
    workers_updated_el: HtmlElement,
    conn_dot: HtmlElement,
    active_worker_label: HtmlElement,
    overlay: HtmlElement,
    overlay_title: HtmlElement,
    overlay_detail: HtmlElement,
    overlay_retry: HtmlElement,
    empty_state: HtmlElement,
    empty_title: HtmlElement,
    empty_hint: HtmlElement,
    status_state: HtmlElement,
    status_size: HtmlElement,
    status_pane: HtmlElement,
    status_scroll: HtmlElement,
    status_worker: HtmlElement,
    status_latency: HtmlElement,
    sr_status: HtmlElement,
    toast: HtmlElement,
    help: HtmlElement,
    scrim: HtmlElement,
    help_open: bool,
    drawer_open: bool,
    composing: bool,
    just_composed: bool,
    dragging: bool,
    drag_anchor: CellPos,
    drag_moved: bool,
    blink_on: bool,
    next_blink: f64,
    bell_until: f64,
    toast_until: f64,
    _handlers: Vec<Closure<dyn FnMut(Event)>>,
    /// Socket listeners stay alive after their socket is closed: a close/error
    /// event may already be queued when a tab is closed or reconnected, and
    /// dropping the closure first aborts that callback.
    _retired_listeners: Vec<Closure<dyn FnMut(Event)>>,
    _observer: Option<ResizeObserver>,
    _resize_handler: Option<Closure<dyn FnMut(js_sys::Array)>>,
    _interval_handler: Option<Closure<dyn FnMut()>>,
}

thread_local! {
    /// The app is a page-lifetime singleton. Event handlers and the render loop
    /// only hold `Weak` references so a future teardown can drop it; this slot is
    /// the one strong owner.
    static INSTANCE: RefCell<Option<Rc<RefCell<App>>>> = const { RefCell::new(None) };
}

/// Entry point called from the wasm-bindgen glue.
pub fn start() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    let Some(mount) = document.get_element_by_id("goble-boot") else {
        return;
    };
    mount.set_inner_html(SHELL);

    let app = match App::new(window, document.clone()) {
        Ok(app) => Rc::new(RefCell::new(app)),
        Err(error) => {
            show_fatal(&document, &format!("The client could not start: {error:?}"));
            return;
        }
    };
    app.borrow_mut().me = Rc::downgrade(&app);
    INSTANCE.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&app)));

    install_handlers(&app);
    {
        let mut app = app.borrow_mut();
        app.resize_to_viewport();
        app.render_sidebar();
        app.render_tabs();
        app.update_chrome();
        app.update_empty();
        app.update_overlay();
        app.update_status();
        app.render_drawer();
        app.render_help();
        app.schedule_draw();
        app.refresh_workers();
    }
}

impl App {
    fn new(window: Window, document: Document) -> Result<Self, JsValue> {
        let canvas: HtmlCanvasElement = by_id(&document, "terminal")?;
        let input: HtmlTextAreaElement = by_id(&document, "key-input")?;
        let viewport: HtmlElement = by_id(&document, "terminal-wrap")?;
        let now = now_ms(&window);
        let theme = Theme::dark();
        let renderer = Renderer::new(canvas.clone())?;
        let render_loop = RenderLoop::new()?;

        Ok(Self {
            window,
            document: document.clone(),
            me: std::rc::Weak::new(),
            theme,
            tabs: Tabs::new(),
            tab_data: HashMap::new(),
            workers: Vec::new(),
            workers_error: None,
            workers_loaded: false,
            workers_updated: 0.0,
            fetching: false,
            selected_worker: None,
            renderer,
            render_loop,
            canvas,
            input,
            viewport,
            app_el: by_id(&document, "goble-app")?,
            tabstrip: by_id(&document, "tabstrip")?,
            worker_list: by_id(&document, "worker-list")?,
            workers_updated_el: by_id(&document, "workers-updated")?,
            conn_dot: by_id(&document, "conn-dot")?,
            active_worker_label: by_id(&document, "active-worker-label")?,
            overlay: by_id(&document, "overlay")?,
            overlay_title: by_id(&document, "overlay-title")?,
            overlay_detail: by_id(&document, "overlay-detail")?,
            overlay_retry: by_id(&document, "overlay-retry")?,
            empty_state: by_id(&document, "empty-state")?,
            empty_title: by_id(&document, "empty-title")?,
            empty_hint: by_id(&document, "empty-hint")?,
            status_state: by_id(&document, "status-state")?,
            status_size: by_id(&document, "status-size")?,
            status_pane: by_id(&document, "status-pane")?,
            status_scroll: by_id(&document, "status-scroll")?,
            status_worker: by_id(&document, "status-worker")?,
            status_latency: by_id(&document, "status-latency")?,
            sr_status: by_id(&document, "sr-status")?,
            toast: by_id(&document, "toast")?,
            help: by_id(&document, "help")?,
            scrim: by_id(&document, "scrim")?,
            help_open: false,
            drawer_open: false,
            composing: false,
            just_composed: false,
            dragging: false,
            drag_anchor: (0, 0),
            drag_moved: false,
            blink_on: true,
            next_blink: now + BLINK_MS,
            bell_until: 0.0,
            toast_until: 0.0,
            _handlers: Vec::new(),
            _retired_listeners: Vec::new(),
            _observer: None,
            _resize_handler: None,
            _interval_handler: None,
        })
    }

    fn now(&self) -> f64 {
        now_ms(&self.window)
    }

    // ----------------------------------------------------------------- tabs

    fn new_tab(&mut self) {
        let worker = self
            .tabs
            .active()
            .map(|tab| tab.worker_id.clone())
            .or_else(|| self.selected_worker.clone())
            .or_else(|| self.workers.first().map(|worker| worker.id.clone()));
        match worker {
            Some(worker) => self.open_worker(&worker, true),
            None => self.toast("No workers connected. Start a worker, then refresh."),
        }
    }

    fn open_worker(&mut self, worker_id: &str, force_new: bool) {
        if !self.workers.iter().any(|worker| worker.id == worker_id) {
            self.toast("That worker is no longer connected.");
            return;
        }
        self.selected_worker = Some(worker_id.to_string());
        if !force_new {
            if let Some(existing) = self.tabs.for_worker(worker_id).first().copied() {
                self.select_tab(existing);
                self.drawer_open = false;
                self.render_drawer();
                return;
            }
        }

        let grid = self.renderer.grid();
        let id = self.tabs.open(worker_id.to_string(), worker_id.to_string());
        let tag = format!("tab{id}");
        let mut session = Session::new(worker_id, tag, terminal_cwd(), grid.cols, grid.rows);
        let plans = session.start();
        let terminal = Terminal::new(grid.cols, grid.rows);
        self.tab_data.insert(
            id,
            TabData {
                session,
                terminal,
                socket: None,
                listeners: Vec::new(),
                pending: Vec::new(),
                selection: None,
                failure: None,
                last_ping_at: 0.0,
                last_ping_sent: 0.0,
                latency_ms: None,
            },
        );
        self.apply_plans(id, plans);
        self.connect_socket(id);
        self.drawer_open = false;
        self.render_drawer();
        self.after_change();
        self.focus_input();
    }

    fn select_tab(&mut self, id: u64) {
        if !self.tabs.select(id) {
            return;
        }
        self.sync_active_size();
        self.after_change();
        self.focus_input();
    }

    fn close_tab(&mut self, id: u64) {
        let plans = self
            .tab_data
            .get_mut(&id)
            .and_then(|tab| tab.session.close());
        if let Some(plan) = plans {
            self.apply_plans(id, vec![plan]);
        }
        self.close_socket(id);
        self.retire_tab(id);
        self.tabs.close(id);
        self.after_change();
        self.focus_input();
    }

    fn retry_active(&mut self) {
        let Some(id) = self.tabs.active_id() else {
            self.toast("No terminal to reconnect.");
            return;
        };
        let plans = match self.tab_data.get_mut(&id) {
            Some(tab) => {
                tab.failure = None;
                tab.session.retry_now()
            }
            None => return,
        };
        if plans.is_empty() {
            return;
        }
        self.apply_plans(id, plans);
        let needs_socket = self
            .tab_data
            .get(&id)
            .and_then(|tab| tab.socket.as_ref())
            .map_or(true, |socket| {
                matches!(socket.ready_state(), WebSocket::CLOSING | WebSocket::CLOSED)
            });
        if needs_socket {
            self.connect_socket(id);
        }
        self.after_change();
        self.focus_input();
    }

    fn destroy_tab(&mut self, id: u64) {
        self.retire_tab(id);
    }

    /// Remove a tab's data while keeping its socket callbacks alive.
    fn retire_tab(&mut self, id: u64) {
        if let Some(mut tab) = self.tab_data.remove(&id) {
            if let Some(socket) = tab.socket.take() {
                let _ = socket.close();
            }
            self._retired_listeners.append(&mut tab.listeners);
        }
    }

    // -------------------------------------------------------------- workers

    fn refresh_workers(&mut self) {
        if self.fetching {
            return;
        }
        self.fetching = true;
        refresh_workers_async(self.me.clone());
    }

    fn set_workers(&mut self, workers: Vec<WorkerInfo>) {
        self.fetching = false;
        self.workers_error = None;
        self.workers_loaded = true;
        self.workers_updated = self.now();
        self.workers = workers;

        let connected: Vec<String> = self
            .workers
            .iter()
            .map(|worker| worker.id.clone())
            .collect();
        let removed = self.tabs.retain_workers(&connected);
        for tab in removed {
            self.destroy_tab(tab.id);
        }
        if self
            .selected_worker
            .as_ref()
            .is_some_and(|selected| !connected.iter().any(|id| id == selected))
        {
            self.selected_worker = connected.first().cloned();
        }
        if self.selected_worker.is_none() {
            self.selected_worker = connected.first().cloned();
        }

        self.render_sidebar();
        self.after_change();
    }

    fn set_workers_error(&mut self, message: String) {
        self.fetching = false;
        self.workers_error = Some(message);
        self.workers_loaded = true;
        self.workers_updated = self.now();
        self.render_sidebar();
        self.after_change();
    }

    // -------------------------------------------------------------- sockets

    fn connect_socket(&mut self, id: u64) {
        let Some(worker_id) = self.tabs.get(id).map(|tab| tab.worker_id.clone()) else {
            return;
        };
        let previous = self
            .tab_data
            .get_mut(&id)
            .map(|tab| (tab.socket.take(), std::mem::take(&mut tab.listeners)));
        if let Some((socket, listeners)) = previous {
            if let Some(socket) = socket {
                let _ = socket.close();
            }
            self._retired_listeners.extend(listeners);
        }

        let url = match self.socket_url(&worker_id) {
            Ok(url) => url,
            Err(message) => {
                self.fail_tab(id, message);
                return;
            }
        };
        let socket = match WebSocket::new(&url) {
            Ok(socket) => socket,
            Err(error) => {
                self.fail_tab(id, format!("WebSocket: {error:?}"));
                return;
            }
        };

        let weak = self.me.clone();
        let open = {
            let weak = weak.clone();
            Closure::wrap(Box::new(move |_event: Event| {
                if let Some(app) = weak.upgrade() {
                    app.borrow_mut().on_socket_open(id);
                }
            }) as Box<dyn FnMut(Event)>)
        };
        let message = {
            let weak = weak.clone();
            Closure::wrap(Box::new(move |event: Event| {
                let Some(event) = event.dyn_ref::<MessageEvent>() else {
                    return;
                };
                let Some(text) = event.data().as_string() else {
                    return;
                };
                if let Some(app) = weak.upgrade() {
                    app.borrow_mut().on_socket_message(id, &text);
                }
            }) as Box<dyn FnMut(Event)>)
        };
        let close = {
            let weak = weak.clone();
            Closure::wrap(Box::new(move |event: Event| {
                let (code, reason) = event
                    .dyn_ref::<CloseEvent>()
                    .map(|event| (event.code(), event.reason()))
                    .unwrap_or((0, String::new()));
                if let Some(app) = weak.upgrade() {
                    app.borrow_mut().on_socket_close(id, code, &reason);
                }
            }) as Box<dyn FnMut(Event)>)
        };
        let error = {
            let weak = weak.clone();
            Closure::wrap(Box::new(move |_event: Event| {
                web_sys::console::warn_1(&JsValue::from_str("goble-web: websocket error"));
                if let Some(app) = weak.upgrade() {
                    app.borrow_mut().on_socket_error(id);
                }
            }) as Box<dyn FnMut(Event)>)
        };

        let mut listeners = Vec::with_capacity(4);
        for (kind, callback) in [
            ("open", open),
            ("message", message),
            ("close", close),
            ("error", error),
        ] {
            let _ =
                socket.add_event_listener_with_callback(kind, callback.as_ref().unchecked_ref());
            listeners.push(callback);
        }

        if let Some(tab) = self.tab_data.get_mut(&id) {
            tab.socket = Some(socket);
            tab.listeners = listeners;
        }
        self.update_overlay();
        self.update_status();
    }

    fn socket_url(&self, worker_id: &str) -> Result<String, String> {
        let location = self.window.location();
        let host = location
            .host()
            .map_err(|_| "no location host".to_string())?;
        let protocol = location
            .protocol()
            .map_err(|_| "no location protocol".to_string())?;
        let scheme = if protocol == "https:" { "wss" } else { "ws" };
        let encoded = js_sys::encode_uri_component(worker_id)
            .as_string()
            .unwrap_or_else(|| worker_id.to_string());
        Ok(format!("{scheme}://{host}/ws/client?worker={encoded}"))
    }

    fn close_socket(&mut self, id: u64) {
        if let Some(tab) = self.tab_data.get_mut(&id) {
            if let Some(socket) = tab.socket.take() {
                let _ = socket.close();
            }
        }
    }

    fn fail_tab(&mut self, id: u64, message: String) {
        if let Some(tab) = self.tab_data.get_mut(&id) {
            let events = tab.session.socket_failed(message.clone());
            tab.failure = Some(message);
            for event in events {
                if let SessionEvent::Failed(message) = event {
                    tab.failure = Some(message);
                }
            }
        }
        self.after_change();
    }

    fn on_socket_open(&mut self, id: u64) {
        let Some(tab) = self.tab_data.get_mut(&id) else {
            return;
        };
        tab.session.socket_opened();
        tab.failure = None;
        if let Some(socket) = tab.socket.as_ref() {
            if socket.ready_state() == WebSocket::OPEN {
                for frame in std::mem::take(&mut tab.pending) {
                    if let Ok(json) = protocol::encode(&frame) {
                        let _ = socket.send_with_str(&json);
                    }
                }
            }
        }
        self.after_change();
    }

    fn on_socket_message(&mut self, id: u64, text: &str) {
        let Ok(frame) = protocol::decode(text) else {
            web_sys::console::warn_1(&JsValue::from_str("goble-web: malformed worker frame"));
            return;
        };
        let now = self.now();
        let mut title = None;
        let mut dirty = false;
        let bell;
        let mut exited = false;
        {
            let Some(tab) = self.tab_data.get_mut(&id) else {
                return;
            };
            for event in tab.session.on_frame(&frame) {
                match event {
                    SessionEvent::Output(bytes) => {
                        tab.terminal.process(&bytes);
                        dirty = true;
                    }
                    SessionEvent::Opened => {
                        tab.failure = None;
                        dirty = true;
                    }
                    SessionEvent::Exited(code) => {
                        tab.failure = Some(match code {
                            Some(code) => format!("Shell exited with status {code}."),
                            None => "Shell exited.".to_string(),
                        });
                        exited = true;
                        dirty = true;
                    }
                    SessionEvent::Failed(message) => {
                        tab.failure = Some(message);
                        dirty = true;
                    }
                    SessionEvent::Pong => {
                        if tab.last_ping_sent > 0.0 {
                            tab.latency_ms = Some((now - tab.last_ping_sent).max(0.0));
                        }
                    }
                }
            }
            let terminal_title = tab.terminal.title().to_string();
            if !terminal_title.is_empty() {
                title = Some(terminal_title);
            }
            bell = tab.terminal.take_bell();
        }

        let mut redraw_chrome = false;
        if let Some(title) = title {
            redraw_chrome |= self.tabs.set_title(id, title);
        }
        if bell {
            self.bell_until = now + BELL_FLASH_MS;
            dirty = true;
        }
        if exited {
            self.close_socket(id);
        }
        if redraw_chrome {
            self.render_tabs();
        }
        self.update_overlay();
        self.update_status();
        if dirty {
            self.schedule_draw();
        }
    }

    fn on_socket_close(&mut self, id: u64, code: u16, reason: &str) {
        let now_ms = self.now() as u64;
        let Some(tab) = self.tab_data.get_mut(&id) else {
            return;
        };
        tab.socket = None;
        if tab.failure.is_none() && !reason.is_empty() {
            tab.failure = Some(format!("Connection closed ({code}): {reason}"));
        }
        let plans = tab.session.socket_lost(now_ms);
        if !plans.is_empty() {
            tab.failure = None;
        }
        self.apply_plans(id, plans);
        self.after_change();
    }

    fn on_socket_error(&mut self, id: u64) {
        let should_handle = self
            .tab_data
            .get(&id)
            .and_then(|tab| tab.socket.as_ref())
            .is_some_and(|socket| socket.ready_state() != WebSocket::OPEN);
        if should_handle {
            self.on_socket_close(id, 0, "socket error");
        }
    }

    // ---------------------------------------------------------------- plans

    fn apply_plans(&mut self, id: u64, plans: Vec<Plan>) {
        let mut dirty = false;
        let mut reconnect_ms = None;
        {
            let Some(tab) = self.tab_data.get_mut(&id) else {
                return;
            };
            for plan in plans {
                let frame = match plan {
                    Plan::OpenPty {
                        pane_id,
                        cwd,
                        cols,
                        rows,
                    } => {
                        tab.pending.clear();
                        ClientFrame::OpenPty {
                            pane_id,
                            cwd,
                            cols,
                            rows,
                        }
                    }
                    Plan::Resize {
                        pane_id,
                        cols,
                        rows,
                    } => ClientFrame::PtyResize {
                        pane_id,
                        cols,
                        rows,
                    },
                    Plan::Input { pane_id, bytes } => protocol::input_frame(&pane_id, &bytes),
                    Plan::Close { pane_id } => ClientFrame::ClosePty { pane_id },
                    Plan::Ping => ClientFrame::Ping,
                    Plan::ReconnectIn { ms } => {
                        reconnect_ms = Some(ms);
                        continue;
                    }
                    Plan::Reconnected { pane_id } => {
                        let marker =
                            format!("\r\n\x1b[38;5;245m── reconnected · {pane_id} ──\x1b[0m\r\n");
                        tab.terminal.process(marker.as_bytes());
                        tab.terminal.scroll_to_bottom();
                        tab.selection = None;
                        dirty = true;
                        continue;
                    }
                };
                queue_or_send(tab, frame);
            }
        }
        if let Some(ms) = reconnect_ms {
            self.toast(format!("Reconnecting in {:.1}s", ms as f64 / 1000.0));
        }
        if dirty {
            self.schedule_draw();
            self.update_status();
        }
    }

    fn send_bytes(&mut self, bytes: &[u8]) {
        let Some(id) = self.tabs.active_id() else {
            return;
        };
        let plan = self
            .tab_data
            .get_mut(&id)
            .and_then(|tab| tab.session.input(bytes));
        if let Some(plan) = plan {
            self.apply_plans(id, vec![plan]);
        }
    }

    fn send_paste(&mut self, text: &str) {
        let Some(id) = self.tabs.active_id() else {
            return;
        };
        let bracketed = self
            .tab_data
            .get(&id)
            .map(|tab| tab.terminal.bracketed_paste())
            .unwrap_or(false);
        let bytes = input::paste_bytes(text, bracketed);
        self.send_bytes(&bytes);
    }

    // -------------------------------------------------------------- sizing

    fn resize_to_viewport(&mut self) -> bool {
        let width = self.viewport.client_width() as f64;
        let height = self.viewport.client_height() as f64;
        if width <= 1.0 || height <= 1.0 {
            return false;
        }
        let changed = self
            .renderer
            .resize(width, height, self.window.device_pixel_ratio());
        if changed {
            self.sync_active_size();
            self.update_status();
            self.schedule_draw();
        }
        changed
    }

    fn sync_active_size(&mut self) {
        let grid = self.renderer.grid();
        let Some(id) = self.tabs.active_id() else {
            return;
        };
        let plan = match self.tab_data.get_mut(&id) {
            Some(tab) => {
                tab.terminal.resize(grid.cols, grid.rows);
                tab.session.resize(grid.cols, grid.rows)
            }
            None => None,
        };
        if let Some(plan) = plan {
            self.apply_plans(id, vec![plan]);
        }
    }

    // ------------------------------------------------------------ rendering

    fn schedule_draw(&mut self) {
        if self.me.strong_count() == 0 {
            return;
        }
        let weak = self.me.clone();
        self.render_loop.schedule(move || {
            if let Some(app) = weak.upgrade() {
                app.borrow_mut().draw();
            }
        });
    }

    fn draw(&mut self) {
        let now = self.now();
        let bell_flash = now < self.bell_until;
        let active = self.tabs.active_id();
        if let Some(tab) = active.and_then(|id| self.tab_data.get(&id)) {
            let options = DrawOptions {
                terminal: &tab.terminal,
                theme: &self.theme,
                selection: tab.selection,
                cursor_visible: self.blink_on,
                bell_flash,
            };
            self.renderer.draw(options);
        } else {
            self.renderer.clear(&self.theme);
        }
        self.update_status();
    }

    // ------------------------------------------------------------------ ui

    fn after_change(&mut self) {
        self.render_tabs();
        self.update_chrome();
        self.update_empty();
        self.update_overlay();
        self.update_status();
        self.schedule_draw();
    }

    fn render_tabs(&mut self) {
        self.tabstrip.set_inner_html("");
        let active = self.tabs.active_id();
        for tab in self.tabs.tabs() {
            let element = match self.document.create_element("button") {
                Ok(element) => element,
                Err(_) => continue,
            };
            let class = if Some(tab.id) == active {
                "tab active"
            } else {
                "tab"
            };
            element.set_class_name(class);
            let _ = element.set_attribute("type", "button");
            let _ = element.set_attribute("data-action", "select-tab");
            let _ = element.set_attribute("data-tab", &tab.id.to_string());
            let _ = element.set_attribute("role", "tab");
            let _ = element.set_attribute(
                "aria-selected",
                if Some(tab.id) == active {
                    "true"
                } else {
                    "false"
                },
            );
            let _ =
                element.set_attribute("title", &format!("{} · pane {}", tab.worker_id, tab.title));

            if let Ok(label) = self.document.create_element("span") {
                label.set_class_name("tab-label");
                label.set_text_content(Some(&short_id(&tab.title)));
                let _ = element.append_child(&label);
            }

            if let Ok(close) = self.document.create_element("span") {
                close.set_class_name("tab-close");
                let _ = close.set_attribute("data-action", "close-tab");
                let _ = close.set_attribute("data-tab", &tab.id.to_string());
                let _ = close.set_attribute("role", "button");
                let _ = close.set_attribute("aria-label", "Close tab");
                close.set_text_content(Some("×"));
                let _ = element.append_child(&close);
            }
            let _ = self.tabstrip.append_child(&element);
        }
    }

    fn render_sidebar(&mut self) {
        self.worker_list.set_inner_html("");
        if !self.workers_loaded {
            append_text(
                &self.document,
                &self.worker_list,
                "p",
                "worker-empty",
                "Loading workers…",
            );
            return;
        }
        if let Some(error) = self.workers_error.clone() {
            append_text(
                &self.document,
                &self.worker_list,
                "p",
                "worker-empty",
                "Cannot reach the gateway.",
            );
            append_text(
                &self.document,
                &self.worker_list,
                "p",
                "worker-hint",
                &error,
            );
            if let Ok(button) = self.document.create_element("button") {
                button.set_class_name("ghost retry-workers");
                let _ = button.set_attribute("data-action", "refresh-workers");
                button.set_text_content(Some("Retry"));
                let _ = self.worker_list.append_child(&button);
            }
            return;
        }
        if self.workers.is_empty() {
            append_text(
                &self.document,
                &self.worker_list,
                "p",
                "worker-empty",
                "No workers connected.",
            );
            append_text(
                &self.document,
                &self.worker_list,
                "p",
                "worker-hint",
                "Start goblin-worker against this gateway. It appears here within a few seconds.",
            );
            return;
        }

        for worker in self.workers.clone() {
            let button = match self.document.create_element("button") {
                Ok(button) => button,
                Err(_) => continue,
            };
            button.set_class_name("worker");
            let _ = button.set_attribute("type", "button");
            let _ = button.set_attribute("data-action", "worker");
            let _ = button.set_attribute("data-worker", &worker.id);
            let _ = button.set_attribute(
                "title",
                &format!(
                    "{}\nconnected {}\nlast seen {}",
                    worker.id, worker.connected_at, worker.last_seen
                ),
            );

            if let Ok(dot) = self.document.create_element("span") {
                dot.set_class_name(if worker_age_seconds(&worker.last_seen) < 60.0 {
                    "dot online"
                } else {
                    "dot idle"
                });
                let _ = button.append_child(&dot);
            }
            if let Ok(body) = self.document.create_element("span") {
                body.set_class_name("worker-body");
                if let Ok(id) = self.document.create_element("span") {
                    id.set_class_name("worker-id");
                    id.set_text_content(Some(&short_id(&worker.id)));
                    let _ = body.append_child(&id);
                }
                if let Ok(meta) = self.document.create_element("span") {
                    meta.set_class_name("worker-meta");
                    meta.set_text_content(Some(&format!(
                        "connected {} · {}",
                        clock_time(&worker.connected_at),
                        age_label(&worker.last_seen)
                    )));
                    let _ = body.append_child(&meta);
                }
                let _ = button.append_child(&body);
            }
            let _ = self.worker_list.append_child(&button);
        }
    }

    fn update_chrome(&mut self) {
        let active = self.tabs.active().cloned();
        match active {
            Some(tab) => {
                let tab_data = self.tab_data.get(&tab.id);
                let (class, label) = match tab_data.map(|tab| tab.session.state()) {
                    Some(SessionState::Live) => ("dot online", "live"),
                    Some(SessionState::Connecting | SessionState::Open) => {
                        ("dot warn", "connecting")
                    }
                    Some(SessionState::Closed) => ("dot off", "closed"),
                    Some(SessionState::Failed) => ("dot error", "failed"),
                    None => ("dot off", "idle"),
                };
                self.conn_dot.set_class_name(class);
                self.active_worker_label
                    .set_text_content(Some(&format!("{} · {label}", short_id(&tab.worker_id))));
            }
            None => {
                self.conn_dot.set_class_name("dot off");
                self.active_worker_label.set_text_content(Some("no worker"));
            }
        }
    }

    fn update_empty(&mut self) {
        let empty = self.tabs.is_empty();
        self.empty_state.set_hidden(!empty);
        if !empty {
            return;
        }
        let (title, hint) = if !self.workers_loaded {
            ("Loading workers…", "Contacting the gateway.".to_string())
        } else if let Some(error) = self.workers_error.clone() {
            ("Cannot reach the gateway", error)
        } else if self.workers.is_empty() {
            (
                "No workers connected",
                "Start a worker and point it at this gateway. It will appear in the sidebar within a few seconds."
                    .to_string(),
            )
        } else {
            (
                "No terminal open",
                "Pick a connected worker in the sidebar, or press Ctrl/Cmd + Shift + T."
                    .to_string(),
            )
        };
        self.empty_title.set_text_content(Some(title));
        self.empty_hint.set_text_content(Some(&hint));
    }

    fn update_overlay(&mut self) {
        let now = self.now();
        let active = self.tabs.active_id().and_then(|id| self.tab_data.get(&id));
        let Some(tab) = active else {
            self.overlay.set_hidden(true);
            return;
        };
        if tab.session.state() == SessionState::Live {
            self.overlay.set_hidden(true);
            return;
        }
        let worker = short_id(tab.session.worker_id());
        let (title, detail) = match tab.session.state() {
            SessionState::Connecting => (
                "Connecting…".to_string(),
                format!("Opening a terminal on {worker}."),
            ),
            SessionState::Open => (
                "Opening terminal…".to_string(),
                format!("Waiting for {worker} to start the shell."),
            ),
            SessionState::Closed => match tab.session.reconnect_at() {
                Some(deadline) => {
                    let remaining = ((deadline as f64 - now) / 1000.0).max(0.0);
                    (
                        "Reconnecting…".to_string(),
                        format!("Retrying in {remaining:.0}s · {worker}"),
                    )
                }
                None => (
                    "Session ended".to_string(),
                    tab.failure
                        .clone()
                        .unwrap_or_else(|| "The shell is no longer running.".to_string()),
                ),
            },
            SessionState::Failed => (
                "Connection failed".to_string(),
                tab.failure
                    .clone()
                    .unwrap_or_else(|| "The worker rejected the terminal.".to_string()),
            ),
            SessionState::Live => unreachable!(),
        };
        self.overlay_title.set_text_content(Some(&title));
        self.overlay_detail.set_text_content(Some(&detail));
        self.overlay_retry.set_hidden(false);
        self.overlay.set_hidden(false);
    }

    fn update_status(&mut self) {
        let active = self.tabs.active_id().and_then(|id| self.tab_data.get(&id));
        let (state, size, pane, scroll, worker, latency, sr) = match active {
            Some(tab) => {
                let state = match tab.session.state() {
                    SessionState::Live => "live",
                    SessionState::Connecting => "connecting",
                    SessionState::Open => "opening",
                    SessionState::Closed => {
                        if tab.session.reconnect_at().is_some() {
                            "reconnecting"
                        } else {
                            "closed"
                        }
                    }
                    SessionState::Failed => "failed",
                };
                let (cols, rows) = tab.terminal.size();
                let pane = tab
                    .session
                    .pane_id()
                    .map(|pane| format!("pane {pane}"))
                    .unwrap_or_else(|| "pane —".to_string());
                let latency = tab
                    .latency_ms
                    .map(|ms| format!("{ms:.0} ms"))
                    .unwrap_or_else(|| "—".to_string());
                let sr = format!(
                    "{} on {} at {} by {}, scrollback {}",
                    state,
                    tab.session.worker_id(),
                    cols,
                    rows,
                    tab.terminal.scroll_offset()
                );
                (
                    state,
                    format!("{cols}×{rows}"),
                    pane,
                    format!("-{}", tab.terminal.scroll_offset()),
                    tab.session.worker_id().to_string(),
                    latency,
                    sr,
                )
            }
            None => {
                let grid = self.renderer.grid();
                (
                    "idle",
                    format!("{}×{}", grid.cols, grid.rows),
                    "pane —".to_string(),
                    "-0".to_string(),
                    "no worker".to_string(),
                    "—".to_string(),
                    "No terminal open".to_string(),
                )
            }
        };
        self.status_state.set_text_content(Some(state));
        self.status_state
            .set_class_name(&format!("status-state {state}"));
        self.status_size.set_text_content(Some(&size));
        self.status_pane.set_text_content(Some(&pane));
        self.status_scroll.set_text_content(Some(&scroll));
        self.status_worker.set_text_content(Some(&worker));
        self.status_latency.set_text_content(Some(&latency));
        self.sr_status.set_text_content(Some(&sr));
        let note = if self.fetching {
            "Refreshing…".to_string()
        } else if self.workers_updated <= 0.0 {
            "Not refreshed yet".to_string()
        } else {
            format!(
                "Updated {:.0}s ago",
                ((self.now() - self.workers_updated) / 1000.0).max(0.0)
            )
        };
        self.workers_updated_el.set_text_content(Some(&note));
    }

    fn render_help(&mut self) {
        self.help.set_hidden(!self.help_open);
    }

    fn render_drawer(&mut self) {
        let _ = self
            .app_el
            .class_list()
            .toggle_with_force("drawer-open", self.drawer_open);
        self.scrim.set_hidden(!self.drawer_open);
    }

    fn toast(&mut self, message: impl Into<String>) {
        self.toast.set_text_content(Some(&message.into()));
        self.toast.set_hidden(false);
        self.toast_until = self.now() + 3_000.0;
    }

    fn focus_input(&mut self) {
        let _ = self.input.focus();
    }

    // --------------------------------------------------------------- events

    fn on_click(&mut self, event: Event) {
        let Some(target) = event
            .target()
            .and_then(|target| target.dyn_into::<Element>().ok())
        else {
            return;
        };
        let on_help_button = target
            .closest("[data-action=\"help\"]")
            .ok()
            .flatten()
            .is_some();
        if self.help_open
            && !on_help_button
            && !self.help.contains(target.dyn_ref::<web_sys::Node>())
        {
            self.help_open = false;
            self.render_help();
        }

        let element = target.closest("[data-action]").ok().flatten();
        let Some(element) = element else {
            return;
        };
        let action = element.get_attribute("data-action").unwrap_or_default();
        let tab_id = element
            .get_attribute("data-tab")
            .and_then(|value| value.parse::<u64>().ok());
        match action.as_str() {
            "new-tab" => self.new_tab(),
            "select-tab" => {
                if let Some(id) = tab_id {
                    self.select_tab(id);
                }
            }
            "close-tab" => {
                if let Some(id) = tab_id {
                    self.close_tab(id);
                }
            }
            "worker" => {
                if let Some(worker) = element.get_attribute("data-worker") {
                    self.open_worker(&worker, false);
                }
            }
            "refresh-workers" => {
                self.workers_error = None;
                self.workers_loaded = false;
                self.render_sidebar();
                self.refresh_workers();
            }
            "retry" => self.retry_active(),
            "toggle-drawer" => {
                self.drawer_open = !self.drawer_open;
                self.render_drawer();
            }
            "close-drawer" => {
                self.drawer_open = false;
                self.render_drawer();
            }
            "help" => {
                self.help_open = !self.help_open;
                self.render_help();
            }
            _ => {}
        }
    }

    fn on_keydown(&mut self, event: KeyboardEvent) {
        if event.is_composing() {
            return;
        }
        let key = event.key();
        let ctrl = event.ctrl_key();
        let meta = event.meta_key();
        let shift = event.shift_key();
        let alt = event.alt_key();
        let command = ctrl || meta;

        if command && shift {
            match key.to_ascii_lowercase().as_str() {
                "t" => {
                    event.prevent_default();
                    self.new_tab();
                    return;
                }
                "w" => {
                    event.prevent_default();
                    if let Some(id) = self.tabs.active_id() {
                        self.close_tab(id);
                    }
                    return;
                }
                "r" => {
                    event.prevent_default();
                    self.retry_active();
                    return;
                }
                "c" => {
                    event.prevent_default();
                    self.copy_active_selection();
                    return;
                }
                "v" => {
                    event.prevent_default();
                    self.paste_from_clipboard();
                    return;
                }
                _ => {}
            }
        }

        if ctrl && !meta && key == "Tab" {
            event.prevent_default();
            let next = if shift {
                self.tabs.previous_tab()
            } else {
                self.tabs.next_tab()
            };
            if let Some(id) = next {
                self.select_tab(id);
            }
            return;
        }

        if alt && !ctrl && !meta && !shift {
            if let Some(digit) = key.chars().next().and_then(|c| c.to_digit(10)) {
                if digit > 0 && key.chars().count() == 1 {
                    if self.tabs.select_index(digit as usize - 1) {
                        event.prevent_default();
                        let id = self.tabs.active_id();
                        if let Some(id) = id {
                            self.select_tab(id);
                        }
                        return;
                    }
                }
            }
        }

        if shift && matches!(key.as_str(), "PageUp" | "PageDown") {
            event.prevent_default();
            let page = i32::from(self.renderer.grid().rows.saturating_sub(1)).max(1);
            let delta = if key == "PageUp" { page } else { -page };
            if let Some(tab) = self
                .tabs
                .active_id()
                .and_then(|id| self.tab_data.get_mut(&id))
            {
                tab.terminal.scroll(delta);
            }
            self.schedule_draw();
            self.update_status();
            return;
        }

        if key == "Escape" && (self.help_open || self.drawer_open) {
            event.prevent_default();
            self.help_open = false;
            self.drawer_open = false;
            self.render_help();
            self.render_drawer();
            return;
        }

        let key_event = KeyEvent {
            key,
            code: event.code(),
            ctrl,
            alt,
            shift,
            meta,
            repeat: event.repeat(),
        };
        if let Some(bytes) = key_event.encode() {
            event.prevent_default();
            self.send_bytes(&bytes);
        }
    }

    fn on_input(&mut self) {
        if self.composing {
            return;
        }
        if std::mem::take(&mut self.just_composed) {
            self.input.set_value("");
            return;
        }
        let value = self.input.value();
        self.input.set_value("");
        if let Some(bytes) = input::text_bytes(&value) {
            self.send_bytes(&bytes);
        }
    }

    fn on_composition_end(&mut self, event: &CompositionEvent) {
        self.composing = false;
        self.just_composed = true;
        self.input.set_value("");
        if let Some(text) = event.data() {
            if let Some(bytes) = input::text_bytes(&text) {
                self.send_bytes(&bytes);
            }
        }
    }

    fn on_paste(&mut self, event: Event) {
        let Some(event) = event.dyn_ref::<ClipboardEvent>() else {
            return;
        };
        let text = event
            .clipboard_data()
            .and_then(|data| data.get_data("text/plain").ok())
            .unwrap_or_default();
        if text.is_empty() {
            return;
        }
        event.prevent_default();
        self.input.set_value("");
        self.send_paste(&text);
    }

    fn paste_from_clipboard(&mut self) {
        let promise = self.window.navigator().clipboard().read_text();
        let weak = self.me.clone();
        spawn_local(async move {
            match JsFuture::from(promise).await {
                Ok(value) => {
                    if let Some(text) = value.as_string() {
                        if let Some(app) = weak.upgrade() {
                            app.borrow_mut().send_paste(&text);
                        }
                    }
                }
                Err(_) => {
                    if let Some(app) = weak.upgrade() {
                        let mut app = app.borrow_mut();
                        app.toast("Clipboard blocked — use your paste shortcut.");
                        app.focus_input();
                    }
                }
            }
        });
    }

    fn copy_active_selection(&mut self) {
        let Some(id) = self.tabs.active_id() else {
            self.toast("Nothing to copy.");
            return;
        };
        let text = self
            .tab_data
            .get(&id)
            .and_then(|tab| tab.selection)
            .map(|selection| {
                self.tab_data
                    .get(&id)
                    .map(|tab| tab.terminal.selection_text(selection))
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        if text.is_empty() {
            self.toast("Select text with the mouse first.");
            return;
        }
        self.write_clipboard(text);
    }

    fn write_clipboard(&mut self, text: String) {
        if text.is_empty() {
            return;
        }
        let promise = self.window.navigator().clipboard().write_text(&text);
        let document = self.document.clone();
        spawn_local(async move {
            if JsFuture::from(promise).await.is_err() {
                fallback_copy(&document, &text);
            }
        });
        self.toast("Copied selection.");
    }

    fn on_mouse_down(&mut self, event: MouseEvent) {
        self.focus_input();
        if event.button() != 0 {
            return;
        }
        let Some(cell) = self.cell_at_mouse(&event) else {
            return;
        };
        if event.ctrl_key() || event.meta_key() {
            if let Some(id) = self.tabs.active_id() {
                if let Some(tab) = self.tab_data.get(&id) {
                    if let Some(url) = tab.terminal.link_at(cell.0, cell.1) {
                        let _ = self.window.open_with_url_and_target(&url, "_blank");
                        event.prevent_default();
                        return;
                    }
                }
            }
        }
        event.prevent_default();
        if let Some(tab) = self
            .tabs
            .active_id()
            .and_then(|id| self.tab_data.get_mut(&id))
        {
            tab.selection = Some(Selection::new(cell, cell));
        }
        self.dragging = true;
        self.drag_moved = false;
        self.drag_anchor = cell;
        self.schedule_draw();
    }

    fn on_mouse_move(&mut self, event: MouseEvent) {
        if !self.dragging {
            return;
        }
        let Some(cell) = self.cell_at_mouse(&event) else {
            return;
        };
        if cell != self.drag_anchor {
            self.drag_moved = true;
        }
        if let Some(tab) = self
            .tabs
            .active_id()
            .and_then(|id| self.tab_data.get_mut(&id))
        {
            tab.selection = Some(Selection::new(self.drag_anchor, cell));
        }
        self.schedule_draw();
    }

    fn on_mouse_up(&mut self, _event: MouseEvent) {
        if !self.dragging {
            return;
        }
        self.dragging = false;
        if self.drag_moved {
            self.copy_active_selection();
        } else if let Some(tab) = self
            .tabs
            .active_id()
            .and_then(|id| self.tab_data.get_mut(&id))
        {
            tab.selection = None;
        }
        self.drag_moved = false;
        self.schedule_draw();
    }

    fn on_double_click(&mut self, event: MouseEvent) {
        let Some(cell) = self.cell_at_mouse(&event) else {
            return;
        };
        let Some(id) = self.tabs.active_id() else {
            return;
        };
        let Some(tab) = self.tab_data.get(&id) else {
            return;
        };
        let (start, end) = word_bounds(&tab.terminal, cell.0, cell.1);
        if let Some(tab) = self.tab_data.get_mut(&id) {
            tab.selection = Some(Selection::new(start, end));
        }
        event.prevent_default();
        self.copy_active_selection();
        self.schedule_draw();
    }

    fn on_wheel(&mut self, event: WheelEvent) {
        let Some(id) = self.tabs.active_id() else {
            return;
        };
        let cell_h = self.renderer.grid().cell_h.max(1.0);
        let delta = event.delta_y();
        let mut lines = (delta / cell_h).round() as i32;
        if lines == 0 && delta != 0.0 {
            lines = if delta > 0.0 { 1 } else { -1 };
        }
        if lines == 0 {
            return;
        }
        if let Some(tab) = self.tab_data.get_mut(&id) {
            tab.terminal.scroll(-lines);
        }
        event.prevent_default();
        self.update_status();
        self.schedule_draw();
    }

    fn cell_at_mouse(&self, event: &MouseEvent) -> Option<CellPos> {
        let grid = self.renderer.grid();
        if grid.width <= 0.0 || grid.height <= 0.0 {
            return None;
        }
        let rect: DomRect = self.canvas.get_bounding_client_rect();
        let x = f64::from(event.client_x()) - rect.left();
        let y = f64::from(event.client_y()) - rect.top();
        if x < 0.0 || y < 0.0 || x >= grid.width || y >= grid.height {
            return None;
        }
        let col = (x / grid.cell_w).floor() as u16;
        let row = (y / grid.cell_h).floor() as u16;
        Some((
            row.min(grid.rows.saturating_sub(1)),
            col.min(grid.cols.saturating_sub(1)),
        ))
    }

    fn on_timer(&mut self) {
        let now = self.now();
        let now_ms = now as u64;
        let mut dirty = false;

        if now >= self.next_blink {
            self.blink_on = !self.blink_on;
            self.next_blink = now + BLINK_MS;
            dirty = true;
        }
        if self.toast_until > 0.0 && now >= self.toast_until {
            self.toast_until = 0.0;
            self.toast.set_hidden(true);
        }

        let ids: Vec<u64> = self.tab_data.keys().copied().collect();
        for id in ids {
            let plans = self
                .tab_data
                .get_mut(&id)
                .map(|tab| tab.session.tick(now_ms))
                .unwrap_or_default();
            if !plans.is_empty() {
                let opens = plans
                    .iter()
                    .any(|plan| matches!(plan, Plan::OpenPty { .. }));
                self.apply_plans(id, plans);
                if opens {
                    self.connect_socket(id);
                }
                self.after_change();
            }

            let ping_due = self.tab_data.get(&id).is_some_and(|tab| {
                tab.session.state() == SessionState::Live
                    && now - tab.last_ping_at >= PING_INTERVAL_MS
            });
            if ping_due {
                let plan = self.tab_data.get(&id).and_then(|tab| tab.session.ping());
                if let Some(plan) = plan {
                    self.apply_plans(id, vec![plan]);
                    if let Some(tab) = self.tab_data.get_mut(&id) {
                        tab.last_ping_at = now;
                        tab.last_ping_sent = now;
                    }
                }
            }
        }

        if now - self.workers_updated >= WORKER_REFRESH_MS {
            self.refresh_workers();
        }

        self.update_chrome();
        self.update_overlay();
        self.update_status();
        if dirty {
            self.schedule_draw();
        }
    }
}

// ------------------------------------------------------------------ helpers

fn now_ms(window: &Window) -> f64 {
    window.performance().map(|perf| perf.now()).unwrap_or(0.0)
}

fn by_id<T: JsCast>(document: &Document, id: &str) -> Result<T, JsValue> {
    document
        .get_element_by_id(id)
        .ok_or_else(|| JsValue::from_str(&format!("missing #{id}")))?
        .dyn_into::<T>()
        .map_err(|_| JsValue::from_str(&format!("#{id} has an unexpected type")))
}

fn queue_or_send(tab: &mut TabData, frame: ClientFrame) {
    if let Some(socket) = tab.socket.as_ref() {
        if socket.ready_state() == WebSocket::OPEN {
            if let Ok(json) = protocol::encode(&frame) {
                let _ = socket.send_with_str(&json);
            }
            return;
        }
    }
    match frame {
        ClientFrame::PtyResize { .. } => {
            tab.pending
                .retain(|queued| !matches!(queued, ClientFrame::PtyResize { .. }));
            tab.pending.push(frame);
        }
        ClientFrame::PtyInput { .. } | ClientFrame::OpenPty { .. } => tab.pending.push(frame),
        ClientFrame::ClosePty { .. } | ClientFrame::Ping => {}
    }
}

fn short_id(id: &str) -> String {
    let count = id.chars().count();
    if count <= 18 {
        id.to_string()
    } else {
        let tail: String = id.chars().skip(count - 12).collect();
        format!("…{tail}")
    }
}

fn clock_time(rfc3339: &str) -> String {
    let time = match rfc3339.split_once('T') {
        Some((_, time)) => time.trim_end_matches('Z').split('.').next().unwrap_or(time),
        None => return rfc3339.to_string(),
    };
    time.get(..5).unwrap_or(time).to_string()
}

fn age_label(last_seen: &str) -> String {
    let parsed = js_sys::Date::parse(last_seen);
    if parsed.is_nan() {
        return "seen recently".to_string();
    }
    let seconds = ((js_sys::Date::now() - parsed) / 1000.0).max(0.0);
    if seconds < 60.0 {
        format!("seen {:.0}s ago", seconds)
    } else if seconds < 3_600.0 {
        format!("seen {:.0}m ago", seconds / 60.0)
    } else {
        format!("seen {:.0}h ago", seconds / 3_600.0)
    }
}

fn worker_age_seconds(last_seen: &str) -> f64 {
    let parsed = js_sys::Date::parse(last_seen);
    if parsed.is_nan() {
        return 0.0;
    }
    ((js_sys::Date::now() - parsed) / 1000.0).max(0.0)
}

fn append_text(document: &Document, parent: &Element, tag: &str, class: &str, text: &str) {
    if let Ok(element) = document.create_element(tag) {
        element.set_class_name(class);
        element.set_text_content(Some(text));
        let _ = parent.append_child(&element);
    }
}

fn word_bounds(terminal: &Terminal, row: u16, col: u16) -> (CellPos, CellPos) {
    let cols = terminal.cols();
    let mut flags = Vec::with_capacity(usize::from(cols));
    let mut previous = false;
    for current in 0..cols {
        let Some(cell) = terminal.cell_at(row, current) else {
            flags.push(false);
            previous = false;
            continue;
        };
        let mut word = if cell.is_wide_continuation() {
            previous
        } else {
            let contents = cell.contents();
            !contents.is_empty()
                && contents.chars().all(|character| {
                    character.is_alphanumeric() || "_-./:@~#%+=?".contains(character)
                })
        };
        if cell.contents().is_empty() && !cell.is_wide_continuation() {
            word = false;
        }
        flags.push(word);
        previous = word;
    }
    let index = usize::from(col).min(flags.len().saturating_sub(1));
    if !flags.get(index).copied().unwrap_or(false) {
        return ((row, col), (row, (col + 1).min(cols.saturating_sub(1))));
    }
    let mut start = index;
    while start > 0 && flags[start - 1] {
        start -= 1;
    }
    let mut end = index;
    while end + 1 < flags.len() && flags[end + 1] {
        end += 1;
    }
    ((row, start as u16), (row, end as u16))
}

fn fallback_copy(document: &Document, text: &str) {
    let Some(body) = document.body() else {
        return;
    };
    let Ok(element) = document.create_element("textarea") else {
        return;
    };
    let _ = element.set_attribute(
        "style",
        "position:fixed;top:-1000px;left:-1000px;opacity:0;",
    );
    let Ok(area) = element.clone().dyn_into::<HtmlTextAreaElement>() else {
        return;
    };
    area.set_value(text);
    let _ = body.append_child(&element);
    area.select();
    if let Some(html_document) = document.dyn_ref::<web_sys::HtmlDocument>() {
        let _ = html_document.exec_command("copy");
    }
    element.remove();
}

fn show_fatal(document: &Document, message: &str) {
    let escaped = message
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    if let Some(mount) = document.get_element_by_id("goble-boot") {
        mount.set_inner_html(&format!(
            r#"<div class="fatal"><h1>Goble could not start</h1><p>{escaped}</p>
<p class="hint">Reload the page. If this persists, rebuild the client with <code>scripts/build-web.sh</code>.</p></div>"#
        ));
    }
}

// ------------------------------------------------------------ async fetching

fn refresh_workers_async(weak: std::rc::Weak<RefCell<App>>) {
    spawn_local(async move {
        let Some(window) = weak.upgrade().map(|app| app.borrow().window.clone()) else {
            return;
        };
        let result = fetch_workers(&window).await;
        let Some(app) = weak.upgrade() else {
            return;
        };
        let mut app = app.borrow_mut();
        match result {
            Ok(FetchOutcome::Workers(workers)) => app.set_workers(workers),
            Ok(FetchOutcome::Unauthorized) => {
                let _ = window.location().set_href("/");
            }
            Err(message) => app.set_workers_error(message),
        }
    });
}

/// The directory a new terminal opens in.
///
/// An empty value lets the worker use the sandbox's `GOBLE_WORKSPACE_PATH`,
/// which is the shared project directory. `?cwd=/absolute/path` overrides it
/// for a single browser session.
fn terminal_cwd() -> String {
    let search = web_sys::window()
        .and_then(|window| window.location().search().ok())
        .unwrap_or_default();
    search
        .trim_start_matches('?')
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == "cwd")
        .and_then(|(_, value)| js_sys::decode_uri_component(value).ok()?.as_string())
        .filter(|cwd| cwd.starts_with('/'))
        .unwrap_or_default()
}

async fn fetch_workers(window: &Window) -> Result<FetchOutcome, String> {
    let init = RequestInit::new();
    init.set_method("GET");
    init.set_credentials(RequestCredentials::SameOrigin);
    init.set_mode(RequestMode::SameOrigin);
    let request = Request::new_with_str_and_init("/api/workers", &init)
        .map_err(|error| format!("Could not build the request: {error:?}"))?;
    let response_value = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|_| "Cannot reach the gateway.".to_string())?;
    let response: Response = response_value
        .dyn_into()
        .map_err(|_| "Invalid gateway response.".to_string())?;
    if response.status() == 401 {
        return Ok(FetchOutcome::Unauthorized);
    }
    if !response.ok() {
        return Err(format!("Gateway returned HTTP {}.", response.status()));
    }
    let text_value = JsFuture::from(
        response
            .text()
            .map_err(|_| "Gateway response had no body.".to_string())?,
    )
    .await
    .map_err(|_| "Could not read the gateway response.".to_string())?;
    let text = text_value
        .as_string()
        .ok_or_else(|| "Gateway response was not text.".to_string())?;
    let parsed: WorkersResponse = serde_json::from_str(&text)
        .map_err(|error| format!("Bad /api/workers response: {error}"))?;
    Ok(FetchOutcome::Workers(parsed.workers))
}

// -------------------------------------------------------------- event setup

fn install_handlers(app: &Rc<RefCell<App>>) {
    let (window, document, viewport) = {
        let app = app.borrow();
        (
            app.window.clone(),
            app.document.clone(),
            app.viewport.clone(),
        )
    };

    let mut handlers = Vec::new();
    macro_rules! listen {
        ($target:expr, $event:expr, $closure:expr) => {{
            let closure: Closure<dyn FnMut(Event)> = $closure;
            let _ =
                $target.add_event_listener_with_callback($event, closure.as_ref().unchecked_ref());
            handlers.push(closure);
        }};
    }

    listen!(document, "click", {
        let weak = Rc::downgrade(app);
        Closure::wrap(Box::new(move |event: Event| {
            if let Some(app) = weak.upgrade() {
                app.borrow_mut().on_click(event);
            }
        }) as Box<dyn FnMut(Event)>)
    });

    listen!(document, "keydown", {
        let weak = Rc::downgrade(app);
        Closure::wrap(Box::new(move |event: Event| {
            let Some(event) = event.dyn_ref::<KeyboardEvent>() else {
                return;
            };
            if let Some(app) = weak.upgrade() {
                app.borrow_mut().on_keydown(event.clone());
            }
        }) as Box<dyn FnMut(Event)>)
    });

    listen!(&document, "mousemove", {
        let weak = Rc::downgrade(app);
        Closure::wrap(Box::new(move |event: Event| {
            let Some(event) = event.dyn_ref::<MouseEvent>() else {
                return;
            };
            if let Some(app) = weak.upgrade() {
                app.borrow_mut().on_mouse_move(event.clone());
            }
        }) as Box<dyn FnMut(Event)>)
    });

    listen!(document, "mouseup", {
        let weak = Rc::downgrade(app);
        Closure::wrap(Box::new(move |event: Event| {
            let Some(event) = event.dyn_ref::<MouseEvent>() else {
                return;
            };
            if let Some(app) = weak.upgrade() {
                app.borrow_mut().on_mouse_up(event.clone());
            }
        }) as Box<dyn FnMut(Event)>)
    });

    let (canvas, input) = {
        let app = app.borrow();
        (app.canvas.clone(), app.input.clone())
    };

    listen!(canvas, "mousedown", {
        let weak = Rc::downgrade(app);
        Closure::wrap(Box::new(move |event: Event| {
            let Some(event) = event.dyn_ref::<MouseEvent>() else {
                return;
            };
            if let Some(app) = weak.upgrade() {
                app.borrow_mut().on_mouse_down(event.clone());
            }
        }) as Box<dyn FnMut(Event)>)
    });

    listen!(canvas, "dblclick", {
        let weak = Rc::downgrade(app);
        Closure::wrap(Box::new(move |event: Event| {
            let Some(event) = event.dyn_ref::<MouseEvent>() else {
                return;
            };
            if let Some(app) = weak.upgrade() {
                app.borrow_mut().on_double_click(event.clone());
            }
        }) as Box<dyn FnMut(Event)>)
    });

    listen!(canvas, "wheel", {
        let weak = Rc::downgrade(app);
        Closure::wrap(Box::new(move |event: Event| {
            let Some(event) = event.dyn_ref::<WheelEvent>() else {
                return;
            };
            if let Some(app) = weak.upgrade() {
                app.borrow_mut().on_wheel(event.clone());
            }
        }) as Box<dyn FnMut(Event)>)
    });

    listen!(input, "input", {
        let weak = Rc::downgrade(app);
        Closure::wrap(Box::new(move |_event: Event| {
            if let Some(app) = weak.upgrade() {
                app.borrow_mut().on_input();
            }
        }) as Box<dyn FnMut(Event)>)
    });

    listen!(input, "compositionstart", {
        let weak = Rc::downgrade(app);
        Closure::wrap(Box::new(move |_event: Event| {
            if let Some(app) = weak.upgrade() {
                app.borrow_mut().composing = true;
            }
        }) as Box<dyn FnMut(Event)>)
    });

    listen!(input, "compositionend", {
        let weak = Rc::downgrade(app);
        Closure::wrap(Box::new(move |event: Event| {
            let Some(event) = event.dyn_ref::<CompositionEvent>() else {
                return;
            };
            if let Some(app) = weak.upgrade() {
                app.borrow_mut().on_composition_end(event);
            }
        }) as Box<dyn FnMut(Event)>)
    });

    listen!(input, "paste", {
        let weak = Rc::downgrade(app);
        Closure::wrap(Box::new(move |event: Event| {
            if let Some(app) = weak.upgrade() {
                app.borrow_mut().on_paste(event);
            }
        }) as Box<dyn FnMut(Event)>)
    });

    // Resize observer on the terminal viewport.
    let resize_callback = {
        let weak = Rc::downgrade(app);
        Closure::wrap(Box::new(move |_entries: js_sys::Array| {
            if let Some(app) = weak.upgrade() {
                app.borrow_mut().resize_to_viewport();
            }
        }) as Box<dyn FnMut(js_sys::Array)>)
    };
    let observer = ResizeObserver::new(resize_callback.as_ref().unchecked_ref()).ok();
    if let Some(observer) = observer.as_ref() {
        observer.observe(&viewport);
    }

    // One timer drives blinking, reconnect deadlines, keepalives and refresh.
    let timer_callback = {
        let weak = Rc::downgrade(app);
        Closure::wrap(Box::new(move || {
            if let Some(app) = weak.upgrade() {
                app.borrow_mut().on_timer();
            }
        }) as Box<dyn FnMut()>)
    };
    let _ = window.set_interval_with_callback_and_timeout_and_arguments_0(
        timer_callback.as_ref().unchecked_ref(),
        TIMER_MS,
    );

    let mut app = app.borrow_mut();
    app._handlers = handlers;
    app._observer = observer;
    app._resize_handler = Some(resize_callback);
    app._interval_handler = Some(timer_callback);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_ids_keep_the_tail() {
        assert_eq!(short_id("worker-1"), "worker-1");
        assert_eq!(short_id("1234567890abcdefghij"), "…4567890abcdefghij");
    }

    #[test]
    fn clock_time_extracts_hours_and_minutes() {
        assert_eq!(clock_time("2026-10-21T14:03:22.123456Z"), "14:03");
        assert_eq!(clock_time("not-a-date"), "not-a-date");
    }
}
