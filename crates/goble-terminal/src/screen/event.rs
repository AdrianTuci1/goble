use std::fmt;
use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{
    Event as AlacrittyEvent, EventListener, WindowSize as AlacrittyWindowSize,
};
use alacritty_terminal::vte::ansi::Rgb;

/// Events the emulator surfaces to the application. Anything the grid cannot
/// act on itself travels through here instead.
///
/// Three of the emulator's requests — clipboard read, colour report, text-area
/// size — arrive with a formatter closure rather than a value, so they cannot
/// live in this `PartialEq` enum. They are queued as [`ScreenQuery`] and
/// announced by [`ScreenEvent::QueryPending`]; take them with
/// [`Screen::drain_queries`](crate::screen::Screen::drain_queries).
#[derive(Debug, Clone, PartialEq)]
pub enum ScreenEvent {
    Title(String),
    ResetTitle,
    Bell,
    CursorBlinkingChange,
    ClipboardStore(String),
    /// Bytes the emulator wants written back to the PTY (query replies).
    PtyWrite(String),
    /// The emulator is waiting for an answer; see
    /// [`Screen::drain_queries`](crate::screen::Screen::drain_queries).
    QueryPending,
    MouseCursorDirty,
    Wakeup,
    Exit,
    ChildExit(i32),
}

/// A question from the emulator that only the application can answer.
///
/// The answer goes back to the PTY in the escape-sequence form the emulator
/// expects, which is what the formatter produces.
#[derive(Clone)]
pub enum ScreenQuery {
    /// Write the current clipboard contents back, in the requested encoding.
    ClipboardLoad {
        format: Arc<dyn Fn(&str) -> String + Send + Sync>,
    },
    /// Report a palette colour (index, usually an OSC 4 index) as an RGB triple.
    ColorRequest {
        index: usize,
        format: Arc<dyn Fn(Rgb) -> String + Send + Sync>,
    },
    /// Report the text area size.
    TextAreaSize {
        format: Arc<dyn Fn(AlacrittyWindowSize) -> String + Send + Sync>,
    },
}

impl fmt::Debug for ScreenQuery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScreenQuery::ClipboardLoad { .. } => f.write_str("ClipboardLoad"),
            ScreenQuery::ColorRequest { index, .. } => f
                .debug_struct("ColorRequest")
                .field("index", index)
                .finish(),
            ScreenQuery::TextAreaSize { .. } => f.write_str("TextAreaSize"),
        }
    }
}

impl ScreenQuery {
    /// The bytes to write back for a clipboard read, or `None` if this is a
    /// different kind of query.
    pub fn clipboard_reply(&self, text: &str) -> Option<String> {
        match self {
            ScreenQuery::ClipboardLoad { format } => Some(format(text)),
            _ => None,
        }
    }

    /// The bytes to write back for a colour report.
    pub fn color_reply(&self, index: usize, rgb: (u8, u8, u8)) -> Option<String> {
        match self {
            ScreenQuery::ColorRequest {
                index: wanted,
                format,
            } if *wanted == index => {
                let (r, g, b) = rgb;
                Some(format(Rgb { r, g, b }))
            }
            _ => None,
        }
    }

    /// The bytes to write back for a text-area size request. Cell metrics are
    /// ours, not the emulator's, so they are passed in.
    pub fn text_area_reply(
        &self,
        columns: u16,
        screen_lines: u16,
        cell_width: u16,
        cell_height: u16,
    ) -> Option<String> {
        match self {
            ScreenQuery::TextAreaSize { format } => Some(format(AlacrittyWindowSize {
                num_cols: columns,
                num_lines: screen_lines,
                cell_width,
                cell_height,
            })),
            _ => None,
        }
    }

    /// The palette index a colour report is about.
    pub fn color_index(&self) -> Option<usize> {
        match self {
            ScreenQuery::ColorRequest { index, .. } => Some(*index),
            _ => None,
        }
    }
}

/// Collects [`ScreenEvent`]s from the emulator.
///
/// The listener is handed to the emulator by value and must be `&self`-callable,
/// so the queue lives behind a shared handle that the caller also holds.
#[derive(Debug, Clone, Default)]
pub struct ScreenEvents {
    queue: Arc<Mutex<Vec<ScreenEvent>>>,
    queries: Arc<Mutex<Vec<ScreenQuery>>>,
}

impl ScreenEvents {
    pub fn new() -> Self {
        Self::default()
    }

    /// Take everything collected so far.
    pub fn drain(&self) -> Vec<ScreenEvent> {
        let mut queue = self.queue.lock().expect("screen event queue poisoned");
        std::mem::take(&mut *queue)
    }

    /// Take the pending questions, oldest first.
    pub fn drain_queries(&self) -> Vec<ScreenQuery> {
        let mut queries = self.queries.lock().expect("screen query queue poisoned");
        std::mem::take(&mut *queries)
    }

    pub fn query_count(&self) -> usize {
        self.queries.lock().map(|q| q.len()).unwrap_or(0)
    }

    fn push(&self, event: ScreenEvent) {
        if let Ok(mut queue) = self.queue.lock() {
            queue.push(event);
        }
    }

    fn push_query(&self, query: ScreenQuery) {
        if let Ok(mut queries) = self.queries.lock() {
            queries.push(query);
        }
        self.push(ScreenEvent::QueryPending);
    }
}

impl EventListener for ScreenEvents {
    fn send_event(&self, event: AlacrittyEvent) {
        let mapped = match event {
            AlacrittyEvent::Title(title) => ScreenEvent::Title(title),
            AlacrittyEvent::ResetTitle => ScreenEvent::ResetTitle,
            AlacrittyEvent::ClipboardStore(_, text) => ScreenEvent::ClipboardStore(text),
            AlacrittyEvent::ClipboardLoad(_, format) => {
                self.push_query(ScreenQuery::ClipboardLoad { format });
                return;
            }
            AlacrittyEvent::PtyWrite(text) => ScreenEvent::PtyWrite(text),
            AlacrittyEvent::TextAreaSizeRequest(format) => {
                self.push_query(ScreenQuery::TextAreaSize { format });
                return;
            }
            AlacrittyEvent::ColorRequest(index, format) => {
                self.push_query(ScreenQuery::ColorRequest { index, format });
                return;
            }
            AlacrittyEvent::MouseCursorDirty => ScreenEvent::MouseCursorDirty,
            AlacrittyEvent::Wakeup => ScreenEvent::Wakeup,
            AlacrittyEvent::Bell => ScreenEvent::Bell,
            AlacrittyEvent::CursorBlinkingChange => ScreenEvent::CursorBlinkingChange,
            AlacrittyEvent::Exit => ScreenEvent::Exit,
            AlacrittyEvent::ChildExit(status) => {
                ScreenEvent::ChildExit(status.code().unwrap_or(-1))
            }
        };
        self.push(mapped);
    }
}
