use crate::color::ColorU;
use crate::elements::{AppContext, Element, LayoutContext, PaintContext, Point, SizeConstraint};
use crate::geometry::{vec2f, Vector2F};
use crate::theme::ColorToken;

const DEFAULT_ICON_SIZE: f32 = 16.0;

pub type IconName = &'static str;

pub struct Icon {
    name: IconName,
    size: f32,
    color: ColorU,
    layout_size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Icon {
    pub fn new(name: IconName) -> Self {
        Self {
            name,
            size: DEFAULT_ICON_SIZE,
            color: ColorU::default(),
            layout_size: None,
            origin: None,
        }
    }

    pub fn with_size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    pub fn with_color(mut self, color: impl Into<ColorU>) -> Self {
        self.color = color.into();
        self
    }

    pub fn with_theme_color(mut self, token: ColorToken, app: &AppContext) -> Self {
        self.color = app.theme.color(token);
        self
    }

    pub fn name(&self) -> IconName {
        self.name
    }

    pub fn icon_size(&self) -> f32 {
        self.size
    }
}

impl Element for Icon {
    fn layout(
        &mut self,
        _constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let size = vec2f(self.size, self.size);
        self.layout_size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let color = if self.color.a == 0 {
            app.theme.color(ColorToken::Text)
        } else {
            self.color
        };
        let atlas_name = icon_atlas_name(self.name);
        if let Some(renderer) = ctx.renderer.as_mut() {
            renderer.draw_icon(origin, atlas_name, self.size, color);
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.layout_size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}

/// Maps logical icon names used by components to canonical SVG file names in the icon atlas.
fn icon_atlas_name(name: &str) -> &'static str {
    match name {
        "close" => "close",
        "x" => "x-close",
        "minimize" => "minimize-01",
        "maximize" => "maximize-01",
        "menu" | "hamburger" => "menu-01",
        "search" => "search",
        "bell" | "notification" => "bell",
        "user" | "account" => "user",
        "user-02" => "user-02",
        "gear" | "settings" => "settings",
        "chat" | "chat-dashed" => "message-chat-square",
        "agent" | "agents" => "agentmode",
        "threads" => "message-chat-square",
        "drive" | "plug" | "layers" => "layers-three-01",
        "users" | "team" => "users-02",
        "chevron-down" => "chevron-down",
        "chevron-left" => "chevron-left",
        "chevron-right" => "chevron-right",
        "plus" | "add" => "plus",
        "new-conversation" => "new-conversation",
        "message-plus-square" => "message-plus-square",
        "circle" => "x-circle",
        "circle-outline" => "x-circle",
        "check" => "check",
        "x-circle" => "x-circle",
        "cancelled" => "cancelled",
        "left-panel-close" => "left-panel-close",
        "left-panel-open" => "left-panel-open",
        "dots" | "dots-horizontal" => "dots-horizontal",
        "trash" | "delete" => "trash-02",
        "paperclip" | "attach" => "paperclip",
        "send" => "send",
        "inbox" | "inbox-01" | "mail" => "inbox-01",
        "computer" | "monitor" | "agentmode" => "agentmode",
        _ => {
            log::warn!("unknown icon name: {name}");
            "x-close"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_size_defaults_to_square() {
        let app = AppContext::default();
        let mut icon = Icon::new("send");
        let size = icon.layout(
            SizeConstraint::loose(vec2f(200.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert_eq!(size.x, size.y);
        assert_eq!(size.x, DEFAULT_ICON_SIZE);
    }

    #[test]
    fn icon_atlas_name_resolves_aliases() {
        assert_eq!(icon_atlas_name("threads"), "message-chat-square");
        assert_eq!(icon_atlas_name("x"), "x-close");
        assert_eq!(icon_atlas_name("settings"), "settings");
    }
}
