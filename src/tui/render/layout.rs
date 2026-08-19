//! Pure adaptive geometry shared by drawing and navigation page sizing.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::app::{App, Focus, View};

use super::history::metadata_height;

pub(crate) fn preview_focus_available(app: &App, width: u16, height: u16) -> bool {
    let body_height = height.saturating_sub(2);
    app.show_preview && width >= 72 && body_height >= 16
}

pub(super) fn log_areas(app: &App, area: Rect) -> (Rect, Option<Rect>) {
    let can_preview = app.show_preview && area.height >= 16;
    if can_preview && area.width >= 110 {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
            .split(area);
        (columns[0], Some(columns[1]))
    } else if can_preview && area.width >= 72 {
        // The 72–109 column contract is intentionally stacked.
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
            .split(area);
        (rows[0], Some(rows[1]))
    } else {
        (area, None)
    }
}

pub(crate) fn page_rows(app: &App, width: u16, height: u16) -> usize {
    let body = Rect::new(0, 1, width, height.saturating_sub(2));
    if app.view == View::Log && app.focus == Focus::List {
        return usize::from(log_areas(app, body).0.height.max(1));
    }
    if matches!(
        app.view,
        View::Refs | View::Status | View::Blame | View::Stash
    ) {
        return usize::from(list_preview_areas(app, body).0.height.max(1));
    }
    if app.view == View::Tree {
        return usize::from(body.height.max(1));
    }
    let preview = if matches!(
        app.view,
        View::Detail | View::Compare | View::StatusDiff | View::Blob
    ) {
        body
    } else {
        log_areas(app, body).1.unwrap_or(body)
    };
    usize::from(
        preview
            .height
            .saturating_sub(metadata_height(app, preview.height))
            .max(1),
    )
}

pub(super) fn list_preview_areas(app: &App, area: Rect) -> (Rect, Rect) {
    if !app.show_preview || area.height < 16 || area.width < 72 {
        return (area, Rect::default());
    }
    let parts = if area.width >= 110 {
        Layout::horizontal([Constraint::Percentage(42), Constraint::Percentage(58)]).split(area)
    } else {
        Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).split(area)
    };
    (parts[0], parts[1])
}

pub(super) fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.max(1).min(area.width);
    let height = height.max(1).min(area.height);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}
