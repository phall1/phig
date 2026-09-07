//! Semantic presentation changes; terminal geometry stays in the TUI adapter.

use super::{App, Effect, Focus, View};

impl App {
    pub(super) fn reset_view_presentation(&mut self) {
        self.diff_fullscreen = false;
        self.focus = if matches!(
            self.view,
            View::Detail | View::Compare | View::StatusDiff | View::Blob
        ) {
            Focus::Preview
        } else {
            Focus::List
        };
    }

    pub(super) fn scrolls_document(&self) -> bool {
        self.diff_fullscreen
            || self.focus == Focus::Preview
            || matches!(
                self.view,
                View::Detail | View::Compare | View::StatusDiff | View::Blob
            )
    }

    pub(super) fn toggle_diff_fullscreen(&mut self) -> Vec<Effect> {
        if self.diff_fullscreen {
            self.restore_diff_layout();
            return Vec::new();
        }
        if matches!(self.view, View::Tree | View::Blob) {
            self.notice = Some("Open a patch to expand its diff".into());
            return Vec::new();
        }
        if self.active_diff().is_none() && !self.preview_loading && !self.inspect.loading {
            self.notice = Some("No diff to expand".into());
            return Vec::new();
        }
        self.fullscreen_previous_focus = self.focus;
        self.diff_fullscreen = true;
        self.focus = Focus::Preview;
        Vec::new()
    }

    pub(super) fn restore_diff_layout(&mut self) {
        self.diff_fullscreen = false;
        self.focus = self.fullscreen_previous_focus;
        if !self.preview_focus_available && self.view == View::Log {
            self.focus = Focus::List;
        }
    }

    pub(super) fn toggle_preview(&mut self) -> Vec<Effect> {
        self.show_preview = !self.show_preview;
        if !self.show_preview && !self.diff_fullscreen {
            self.focus = Focus::List;
        }
        Vec::new()
    }

    pub(super) fn toggle_focus(&mut self) -> Vec<Effect> {
        if self.diff_fullscreen {
            return Vec::new();
        }
        if self.view == View::Log && self.show_preview && self.preview_focus_available {
            self.focus = match self.focus {
                Focus::List => Focus::Preview,
                Focus::Preview => Focus::List,
            };
        }
        Vec::new()
    }
}
