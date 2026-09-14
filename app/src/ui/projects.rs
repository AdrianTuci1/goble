//! Per-project observability panel: sessions grouped per project + a
//! "what's running" status. Read-only, driven from [`crate::projects::ProjectsState`].

use goble_ui::elements::{
    AppContext, Axis, Container, CrossAxisAlignment, Divider, EdgeInsets, Element, Fill, Flex, Icon,
    MainAxisSize, Rect, Scrollable, Spacer, Text, TopbarButton,
};
use goble_ui::geometry::vec2f;
use goble_ui::theme::{ColorToken, SpacingToken};

use super::{ProjectEntry, ProjectsActions, ProjectsSnapshot};

/// Main-area projects view: header + scrollable list of per-project cards.
pub fn build_projects_view(
    app: &AppContext,
    projects: &ProjectsSnapshot,
    projects_actions: &ProjectsActions,
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Md);
    let sm = app.theme.spacing_px(SpacingToken::Sm);

    let on_refresh = projects_actions.on_refresh.clone();
    let refresh_button = TopbarButton::new(
        Icon::new("refresh")
            .with_size(16.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(28.0)
    .with_on_click(move || (on_refresh.borrow_mut())())
    .finish();

    let header = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(
            Text::new("Projects")
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_child(Spacer::new().finish())
        .with_child(refresh_button)
        .finish();

    let mut list = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(sm);
    if projects.projects.is_empty() {
        list = list.with_child(
            Text::new("No sessions yet. Start a turn to group it under a project.")
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        );
    }
    for entry in &projects.projects {
        list = list.with_child(build_project_card(app, entry));
    }

    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(spacing);
    column = column.with_child(header);
    column = column.with_child(Divider::horizontal().finish());
    column = column.with_child(Scrollable::new(list.finish(), Axis::Vertical).finish());

    Container::new(column.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .with_padding(EdgeInsets::uniform(spacing))
        .finish()
}

/// One per-project card: status dot, project name, "what's running" status,
/// and the sessions grouped under it.
fn build_project_card(app: &AppContext, entry: &ProjectEntry) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Sm);
    let radius = app.theme.radius_px();

    let (status_color, status) = project_status(entry.status.as_str(), app);

    let name = Text::new(project_label(entry))
        .with_font_size(12.0)
        .with_theme_color(ColorToken::Text, app)
        .finish();

    let mut session_col = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(4.0);
    for session in &entry.sessions {
        session_col = session_col.with_child(
            Text::new(format!(
                "{} @ {} · {} · {}",
                session.session_id, session.medium, session.created_at, session.status
            ))
            .with_font_size(11.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
        );
    }

    let info = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Start)
        .with_spacing(6.0)
        .with_child(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(spacing)
                .with_child(
                    Container::new(Rect::new().with_size(vec2f(8.0, 8.0)).finish())
                        .with_background(Fill::Solid(app.theme.color(status_color)))
                        .with_corner_radius(4.0)
                        .finish(),
                )
                .with_child(name)
                .with_child(Spacer::new().finish())
                .with_child(status)
                .finish(),
        )
        .with_child(session_col.finish())
        .finish();

    Container::new(info)
        .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
        .with_corner_radius(radius)
        .with_padding(EdgeInsets::uniform(spacing))
        .finish()
}

/// Resolve a per-project status string to a dot color + label element.
fn project_status(status: &str, app: &AppContext) -> (ColorToken, Box<dyn Element>) {
    let (color, label): (ColorToken, &str) = match status {
        "running" => (ColorToken::Success, "running"),
        "scheduled" => (ColorToken::Warning, "scheduled"),
        _ => (ColorToken::Muted, "idle"),
    };
    (color, Text::new(label).with_font_size(11.0).with_theme_color(ColorToken::Muted, app).finish())
}

/// Human label for a project card: prefer the directory, fall back to the id.
fn project_label(entry: &ProjectEntry) -> String {
    if entry.directory.is_empty() {
        entry.project_id.clone()
    } else {
        format!(
            "{} · {}",
            crate::state::display_path(&entry.directory),
            entry.project_id
        )
    }
}
