//! Canvas 2D renderer for the active terminal.
//!
//! One canvas is shared by every tab; switching tabs just redraws from the
//! newly active [`Terminal`]. Drawing is coalesced through
//! [`RenderLoop`], which schedules at most one `requestAnimationFrame` per
//! frame even when output arrives in bursts.
//!
//! Layout is in CSS pixels on a DPR-scaled canvas. Rect edges are snapped to
//! device pixels so cell borders and the block cursor stay crisp.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, TextMetrics};

use crate::theme::{Rgb, Theme};
use crate::vt::{Selection, Terminal};

/// Same monospace stack as the native terminal UI.
pub const FONT_STACK: &str =
    "ui-monospace, SFMono-Regular, \"SF Mono\", Menlo, Consolas, monospace";
pub const FONT_SIZE: f64 = 13.0;

/// Current grid geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    pub cols: u16,
    pub rows: u16,
    pub cell_w: f64,
    pub cell_h: f64,
    pub width: f64,
    pub height: f64,
    pub dpr: f64,
}

/// Everything one frame needs.
pub struct DrawOptions<'a> {
    pub terminal: &'a Terminal,
    pub theme: &'a Theme,
    pub selection: Option<Selection>,
    pub cursor_visible: bool,
    pub bell_flash: bool,
}

/// Canvas-backed renderer.
pub struct Renderer {
    ctx: CanvasRenderingContext2d,
    canvas: HtmlCanvasElement,
    cell_w: f64,
    cell_h: f64,
    ascent: f64,
    grid: Grid,
}

impl Renderer {
    pub fn new(canvas: HtmlCanvasElement) -> Result<Self, JsValue> {
        let context = canvas
            .get_context("2d")?
            .ok_or_else(|| JsValue::from_str("2d canvas context unavailable"))?;
        let ctx: CanvasRenderingContext2d = context.dyn_into()?;
        let font = font_for(false, false);
        ctx.set_font(&font);
        ctx.set_text_baseline("alphabetic");

        let metrics = ctx.measure_text("M")?;
        let cell_w = sane_width(&metrics);
        let cell_h = cell_height(&ctx, cell_w);
        let ascent = ascent(&ctx, cell_h);

        Ok(Self {
            ctx,
            canvas,
            cell_w,
            cell_h,
            ascent,
            grid: Grid {
                cols: 1,
                rows: 1,
                cell_w,
                cell_h,
                width: 0.0,
                height: 0.0,
                dpr: 1.0,
            },
        })
    }

    #[must_use]
    pub fn grid(&self) -> Grid {
        self.grid
    }

    /// Resize the backing store and recompute the grid.
    ///
    /// Returns `true` when the number of columns or rows changed, which is the
    /// caller's cue to resize the pty.
    pub fn resize(&mut self, width: f64, height: f64, dpr: f64) -> bool {
        let dpr = if dpr.is_finite() && dpr > 0.0 {
            dpr
        } else {
            1.0
        };
        let width = width.max(1.0);
        let height = height.max(1.0);
        self.canvas.set_width((width * dpr).round().max(1.0) as u32);
        self.canvas
            .set_height((height * dpr).round().max(1.0) as u32);
        let _ = self.ctx.set_transform(dpr, 0.0, 0.0, dpr, 0.0, 0.0);

        let cols = ((width / self.cell_w).floor().max(1.0) as u16).max(1);
        let rows = ((height / self.cell_h).floor().max(1.0) as u16).max(1);
        let changed = cols != self.grid.cols || rows != self.grid.rows;
        self.grid = Grid {
            cols,
            rows,
            cell_w: self.cell_w,
            cell_h: self.cell_h,
            width,
            height,
            dpr,
        };
        changed
    }

    /// Fill the canvas with the theme background when no tab is active.
    pub fn clear(&mut self, theme: &Theme) {
        self.ctx.set_global_alpha(1.0);
        self.ctx.set_fill_style_str(&theme.background.hex());
        self.ctx
            .fill_rect(0.0, 0.0, self.grid.width, self.grid.height);
    }

    /// Redraw one frame.
    pub fn draw(&mut self, options: DrawOptions<'_>) {
        let DrawOptions {
            terminal,
            theme,
            selection,
            cursor_visible,
            bell_flash,
        } = options;
        let grid = self.grid;
        if grid.width <= 0.0 || grid.height <= 0.0 {
            return;
        }

        self.ctx.set_global_alpha(1.0);
        self.ctx.set_fill_style_str(&theme.background.hex());
        self.ctx.fill_rect(0.0, 0.0, grid.width, grid.height);

        let cols = grid.cols.min(terminal.cols());
        let rows = grid.rows.min(terminal.rows());

        for row in 0..rows {
            let y = self.snap(row as f64 * self.cell_h);
            self.draw_row(row, y, cols, terminal, theme, selection);
        }

        if cursor_visible && terminal.scroll_offset() == 0 {
            self.draw_cursor(terminal, theme, cols, rows);
        }

        self.draw_scrollbar(terminal, theme);
        if bell_flash {
            self.ctx.set_global_alpha(0.08);
            self.ctx.set_fill_style_str(&theme.accent.hex());
            self.ctx.fill_rect(0.0, 0.0, grid.width, grid.height);
            self.ctx.set_global_alpha(1.0);
        }
    }

    fn draw_row(
        &mut self,
        row: u16,
        y: f64,
        cols: u16,
        terminal: &Terminal,
        theme: &Theme,
        selection: Option<Selection>,
    ) {
        let cell_h = self.cell_h;
        let mut col = 0;
        while col < cols {
            let Some(cell) = terminal.cell_at(row, col) else {
                break;
            };
            let wide = cell.is_wide();
            let width = if wide { 2.0 } else { 1.0 };
            let x = self.snap(col as f64 * self.cell_w);
            let w = self.snap_x(col as f64 * self.cell_w + width * self.cell_w) - x;

            let selected = selection.is_some_and(|selection| {
                selection.contains(row, col) || (wide && selection.contains(row, col + 1))
            });

            let bold = cell.bold();
            let mut fg = theme.foreground_for(cell.fgcolor(), bold);
            let mut bg = theme.background_for(cell.bgcolor());
            if cell.dim() {
                fg = theme.dimmed(fg);
            }
            if cell.inverse() {
                std::mem::swap(&mut fg, &mut bg);
            }
            if selected {
                bg = theme.selection;
            }

            if bg != theme.background {
                self.fill(bg, x, y, w, cell_h);
            }
            if cell.underline() {
                self.fill(fg, x, y + cell_h - 1.0, w, 1.0);
            }

            let contents = cell.contents();
            if !contents.is_empty() {
                let font = font_for(bold, cell.italic());
                self.ctx.set_font(&font);
                self.ctx.set_fill_style_str(&fg.hex());
                let _ = self.ctx.fill_text(contents, x, y + self.ascent);
            }

            col += if wide { 2 } else { 1 };
        }
    }

    fn draw_cursor(&mut self, terminal: &Terminal, theme: &Theme, cols: u16, rows: u16) {
        let cursor = terminal.cursor();
        if !cursor.visible || cursor.row >= rows || cursor.col >= cols {
            return;
        }
        let x = self.snap(cursor.col as f64 * self.cell_w);
        let y = self.snap(cursor.row as f64 * self.cell_h);
        let w = self.snap_x((cursor.col as f64 + 1.0) * self.cell_w) - x;
        self.fill(theme.cursor, x, y, w, self.cell_h);

        let Some(cell) = terminal.cell_at(cursor.row, cursor.col) else {
            return;
        };
        let contents = cell.contents();
        if !contents.is_empty() {
            self.ctx.set_font(&font_for(cell.bold(), false));
            self.ctx.set_fill_style_str(&theme.background.hex());
            let _ = self.ctx.fill_text(contents, x, y + self.ascent);
        }
    }

    fn draw_scrollbar(&mut self, terminal: &Terminal, theme: &Theme) {
        let history = terminal.scrollback_len();
        if history == 0 || terminal.alternate_screen() {
            return;
        }
        let width = 4.0;
        let x = self.snap(self.grid.width - width);
        self.ctx.set_global_alpha(0.55);
        self.fill(theme.panel, x, 0.0, width, self.grid.height);
        let visible = f64::from(self.grid.rows);
        let total = visible + history as f64;
        let thumb = (self.grid.height * visible / total)
            .max(18.0)
            .min(self.grid.height);
        let progress = if history == 0 {
            1.0
        } else {
            1.0 - terminal.scroll_offset() as f64 / history as f64
        };
        let y = (self.grid.height - thumb) * progress;
        self.fill(theme.muted, x, self.snap(y), width, thumb);
        self.ctx.set_global_alpha(1.0);
    }

    fn fill(&mut self, color: Rgb, x: f64, y: f64, w: f64, h: f64) {
        self.ctx.set_fill_style_str(&color.hex());
        self.ctx.fill_rect(x, y, w, h);
    }

    fn snap(&self, value: f64) -> f64 {
        (value * self.grid.dpr).round() / self.grid.dpr
    }

    fn snap_x(&self, value: f64) -> f64 {
        self.snap(value)
    }
}

fn font_for(bold: bool, italic: bool) -> String {
    let mut font = String::new();
    if italic {
        font.push_str("italic ");
    }
    if bold {
        font.push_str("bold ");
    }
    font.push_str(&format!("{FONT_SIZE}px {FONT_STACK}"));
    font
}

fn sane_width(metrics: &TextMetrics) -> f64 {
    let width = metrics.width();
    let width = if width.is_finite() && width > 1.0 {
        width
    } else {
        FONT_SIZE * 0.6
    };
    (width * 4.0).round() / 4.0
}

fn cell_height(ctx: &CanvasRenderingContext2d, cell_w: f64) -> f64 {
    // A glyph's bounding box plus a little leading is close to what terminal
    // emulators use. Fall back to a ratio when the font is not measurable.
    ctx.set_font(&font_for(false, false));
    if let Ok(metrics) = ctx.measure_text("Mg") {
        let height = metrics.font_bounding_box_ascent() + metrics.font_bounding_box_descent();
        if height.is_finite() && height > 0.0 {
            return (height + 2.0).ceil().max(FONT_SIZE + 3.0);
        }
    }
    (cell_w * 1.9).max(FONT_SIZE + 3.0)
}

fn ascent(ctx: &CanvasRenderingContext2d, cell_h: f64) -> f64 {
    if let Ok(metrics) = ctx.measure_text("Mg") {
        let ascent = metrics.font_bounding_box_ascent();
        if ascent.is_finite() && ascent > 0.0 {
            return ascent + (cell_h - (ascent + metrics.font_bounding_box_descent())) / 2.0;
        }
    }
    cell_h * 0.8
}

/// Coalesces draw requests into one `requestAnimationFrame` callback.
pub struct RenderLoop {
    window: web_sys::Window,
    pending: Rc<Cell<bool>>,
    action: Rc<RefCell<Option<Box<dyn FnOnce()>>>>,
    callback: Closure<dyn FnMut()>,
}

impl RenderLoop {
    pub fn new() -> Result<Self, JsValue> {
        let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
        let pending = Rc::new(Cell::new(false));
        let action: Rc<RefCell<Option<Box<dyn FnOnce()>>>> = Rc::new(RefCell::new(None));

        let callback = {
            let pending = Rc::clone(&pending);
            let action = Rc::clone(&action);
            Closure::wrap(Box::new(move || {
                pending.set(false);
                let mut slot = action.borrow_mut();
                if let Some(draw) = slot.take() {
                    draw();
                }
            }) as Box<dyn FnMut()>)
        };

        Ok(Self {
            window,
            pending,
            action,
            callback,
        })
    }

    /// Request a draw. Calling this repeatedly before the next frame replaces
    /// the pending action instead of queueing more frames.
    pub fn schedule<F>(&self, draw: F)
    where
        F: FnOnce() + 'static,
    {
        *self.action.borrow_mut() = Some(Box::new(draw));
        if self.pending.replace(true) {
            return;
        }
        let _ = self
            .window
            .request_animation_frame(self.callback.as_ref().unchecked_ref());
    }
}
