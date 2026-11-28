//! 535/536: the side-panel Browser tab chrome.
//!
//! The composited native webview (WKWebView/WebView2/WebKitGTK) is a platform
//! surface owned outside this element tree; this module draws its chrome and an
//! honest placeholder when the current build has no native view. The URL is
//! per-conversation and lives in app state, so switching conversations keeps
//! each browser where it was.

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::{
    AppContext, Border, Container, CrossAxisAlignment, EdgeInsets, Element, Expanded, Fill, Flex,
    Icon, MainAxisSize, Text, TextInput,
};
use goble_ui::theme::ColorToken;

use super::UiSnapshot;

pub(crate) fn build_browser_tab(app: &AppContext, state: &UiSnapshot) -> Box<dyn Element> {
    let current_url = state.browser_url.borrow().clone();
    let shown_url = if current_url.trim().is_empty() {
        "https://www.google.com/".to_string()
    } else {
        current_url
    };
    let url = state.browser_url.clone();
    let url_for_field = url.clone();
    let url_for_buttons = url.clone();
    let field = TextInput::new()
        .with_value(shown_url)
        .with_placeholder("https://")
        .with_full_width(true)
        .with_on_change(move |value| *url_for_field.borrow_mut() = value)
        .finish();
    let button = |name: &'static str, tooltip: &str, value: Option<String>| {
        let url = url_for_buttons.clone();
        TooltipButton::new(app, name, tooltip, move || {
            if let Some(value) = value.clone() {
                *url.borrow_mut() = value;
            }
        })
    };
    let bar = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(4.0)
        .with_child(button("arrow-left", "Back", None))
        .with_child(button("arrow-right", "Forward", None))
        .with_child(button("refresh", "Reload", None))
        .with_child(button("x", "Stop", None))
        .with_child(Expanded::new(field).finish())
        .with_child(button("external-link", "Open in browser", None))
        .finish();
    let bar = Container::new(bar)
        .with_padding(EdgeInsets::new(6.0, 6.0, 6.0, 6.0))
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .finish();
    let placeholder = Container::new(
        Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(6.0)
            .with_child(
                Icon::new("globe")
                    .with_size(28.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_child(
                Text::new("Native web view unavailable in this build")
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            )
            .with_child(
                Text::new("The URL and browser state are preserved per conversation.")
                    .with_theme_color(ColorToken::Muted, app)
                    .with_font_size(11.0)
                    .finish(),
            )
            .finish(),
    )
    .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
    .finish();
    let column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(bar)
        .with_child(Container::new(Expanded::new(placeholder).finish()).finish())
        .finish();
    // 557: the page area is edge-to-edge; no frame, border, margin or inset.
    column
}

struct TooltipButton {
    inner: Box<dyn Element>,
}

impl TooltipButton {
    fn new(
        app: &AppContext,
        icon: &'static str,
        tooltip: &str,
        on_click: impl FnMut() + 'static,
    ) -> Box<dyn Element> {
        let icon = Icon::new(icon)
            .with_size(13.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish();
        let button = goble_ui::elements::TopbarButton::new(icon)
            .with_size(24.0)
            .with_corner_radius(6.0)
            .with_on_click(on_click)
            .finish();
        goble_ui::elements::Tooltip::new(button, tooltip.to_string())
            .with_position(goble_ui::elements::TooltipPosition::Below)
            .finish()
    }
}

impl Element for TooltipButton {
    fn layout(
        &mut self,
        constraint: goble_ui::elements::SizeConstraint,
        ctx: &mut goble_ui::elements::LayoutContext,
        app: &AppContext,
    ) -> goble_ui::geometry::Vector2F {
        self.inner.layout(constraint, ctx, app)
    }
    fn paint(
        &mut self,
        origin: goble_ui::geometry::Vector2F,
        ctx: &mut goble_ui::elements::PaintContext,
        app: &AppContext,
    ) {
        self.inner.paint(origin, ctx, app);
    }
    fn dispatch_event(
        &mut self,
        event: &goble_ui::event::DispatchedEvent,
        ctx: &mut goble_ui::elements::EventContext,
        app: &AppContext,
    ) -> bool {
        self.inner.dispatch_event(event, ctx, app)
    }
    fn size(&self) -> Option<goble_ui::geometry::Vector2F> {
        self.inner.size()
    }
    fn origin(&self) -> Option<goble_ui::elements::Point> {
        self.inner.origin()
    }
}
