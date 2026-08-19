//! Pure adaptive geometry shared by drawing and navigation page sizing.

use ratatui::layout::Rect;

use crate::app::{App, Focus, View};

use super::history::metadata_height;

pub(super) const COMPARE_HEADER_ROWS: u16 = 3;
pub(super) const STATUS_DIFF_HEADER_ROWS: u16 = 1;

/// Rows available to patch content after reserving the truncation notice.
pub(super) fn diff_content_rows(height: u16, truncated: bool) -> u16 {
    if truncated && height > 1 {
        height - 1
    } else {
        height.max(1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SplitDirection {
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PaneLayout {
    pub primary: Rect,
    pub divider: Option<Rect>,
    pub secondary: Option<Rect>,
    pub direction: Option<SplitDirection>,
}

impl PaneLayout {
    fn single(area: Rect) -> Self {
        Self {
            primary: area,
            divider: None,
            secondary: None,
            direction: None,
        }
    }
}

pub(crate) fn preview_focus_available(app: &App, width: u16, height: u16) -> bool {
    let body_height = height.saturating_sub(2);
    app.show_preview && width >= 72 && body_height >= 16
}

pub(super) fn pane_layout(app: &App, area: Rect, stacked_percent: u16) -> PaneLayout {
    if !app.show_preview || area.height < 16 || area.width < 72 {
        return PaneLayout::single(area);
    }
    if area.width >= 110 {
        let content = area.width.saturating_sub(1);
        let primary_width = content.saturating_mul(42) / 100;
        let secondary_width = content.saturating_sub(primary_width);
        PaneLayout {
            primary: Rect::new(area.x, area.y, primary_width, area.height),
            divider: Some(Rect::new(area.x + primary_width, area.y, 1, area.height)),
            secondary: Some(Rect::new(
                area.x + primary_width + 1,
                area.y,
                secondary_width,
                area.height,
            )),
            direction: Some(SplitDirection::Vertical),
        }
    } else {
        let content = area.height.saturating_sub(1);
        let primary_height = content.saturating_mul(stacked_percent) / 100;
        let secondary_height = content.saturating_sub(primary_height);
        PaneLayout {
            primary: Rect::new(area.x, area.y, area.width, primary_height),
            divider: Some(Rect::new(area.x, area.y + primary_height, area.width, 1)),
            secondary: Some(Rect::new(
                area.x,
                area.y + primary_height + 1,
                area.width,
                secondary_height,
            )),
            direction: Some(SplitDirection::Horizontal),
        }
    }
}

pub(super) fn log_layout(app: &App, area: Rect) -> PaneLayout {
    pane_layout(app, area, 45)
}

pub(super) fn list_preview_layout(app: &App, area: Rect) -> PaneLayout {
    pane_layout(app, area, 50)
}

pub(crate) fn page_rows(app: &App, width: u16, height: u16) -> usize {
    let body = Rect::new(0, 1, width, height.saturating_sub(2));
    if app.view == View::Log && app.focus == Focus::List {
        return usize::from(log_layout(app, body).primary.height.max(1));
    }
    if matches!(
        app.view,
        View::Refs | View::Status | View::Blame | View::Stash
    ) {
        return usize::from(list_preview_layout(app, body).primary.height.max(1));
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
        log_layout(app, body).secondary.unwrap_or(body)
    };
    let persistent_header = match app.view {
        View::Compare => COMPARE_HEADER_ROWS,
        View::StatusDiff => STATUS_DIFF_HEADER_ROWS,
        _ => metadata_height(app, preview.height),
    };
    let content_height = preview.height.saturating_sub(persistent_header);
    let truncated = app.active_diff().is_some_and(|diff| diff.truncated);
    usize::from(diff_content_rows(content_height, truncated))
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
