//! Literal search in the active semantic surface, independent of layout.

use super::{App, Effect, Focus, View};

impl App {
    pub(super) fn seek_match(&mut self, forward: bool, include_current: bool) -> Vec<Effect> {
        if self.search_query.is_empty() {
            return Vec::new();
        }
        let needle = self.search_query.to_lowercase();
        if self.diff_fullscreen {
            return self.seek_diff_match(&needle, forward, include_current);
        }
        if matches!(
            self.view,
            View::Refs | View::Status | View::Tree | View::Blame | View::Stash
        ) {
            return self.seek_inspect_match(&needle, forward, include_current);
        }
        if self.view == View::Log && self.focus == Focus::List {
            self.seek_history_match(&needle, forward, include_current)
        } else {
            self.seek_diff_match(&needle, forward, include_current)
        }
    }

    fn seek_inspect_match(
        &mut self,
        needle: &str,
        forward: bool,
        include_current: bool,
    ) -> Vec<Effect> {
        let index = search_indices(
            self.inspect.selected,
            self.active_len(),
            forward,
            include_current,
        )
        .find(|index| {
            self.inspect_search_text(*index)
                .to_lowercase()
                .contains(needle)
        });
        if let Some(index) = index {
            self.inspect.selected = index;
            return self.inspect_selection_effects();
        }
        Vec::new()
    }

    fn inspect_search_text(&self, index: usize) -> String {
        match self.view {
            View::Refs => self.inspect.refs.get(index).map(|item| {
                format!(
                    "{} {} {}",
                    item.short_name.display(),
                    item.full_name.display(),
                    item.subject
                )
            }),
            View::Status => self
                .inspect
                .status_entries()
                .get(index)
                .map(|item| item.path.display.clone()),
            View::Tree => self
                .inspect
                .tree
                .get(index)
                .map(|item| item.path.display.clone()),
            View::Blame => self
                .inspect
                .blame
                .get(index)
                .map(|item| format!("{} {} {}", item.author, item.summary, item.content)),
            View::Stash => self
                .inspect
                .stashes
                .get(index)
                .map(|item| format!("{} {}", item.selector, item.subject)),
            _ => None,
        }
        .unwrap_or_default()
    }

    fn seek_history_match(
        &mut self,
        needle: &str,
        forward: bool,
        include_current: bool,
    ) -> Vec<Effect> {
        if self.commits.is_empty() {
            return Vec::new();
        }
        let index = search_indices(self.selected, self.commits.len(), forward, include_current)
            .find(|index| {
                let commit = &self.commits[*index];
                format!(
                    "{} {} {} {}",
                    commit.id.hex, commit.subject, commit.author.name, commit.author.email
                )
                .to_lowercase()
                .contains(needle)
            });
        if let Some(index) = index {
            return self.select_index(index);
        }
        if !self.has_more {
            return Vec::new();
        }
        self.search_pending = Some(forward);
        if self.history_loading {
            return Vec::new();
        }
        self.history_loading = true;
        self.history_error = None;
        vec![Effect::LoadHistory {
            offset: self.commits.len(),
            limit: self.history_page_size,
        }]
    }

    fn seek_diff_match(
        &mut self,
        needle: &str,
        forward: bool,
        include_current: bool,
    ) -> Vec<Effect> {
        let Some(diff) = self.active_diff() else {
            return Vec::new();
        };
        let index = search_indices(self.diff_scroll, diff.lines.len(), forward, include_current)
            .find(|index| diff.lines[*index].text.to_lowercase().contains(needle));
        if let Some(index) = index {
            self.diff_scroll = index;
        }
        Vec::new()
    }
}

fn search_indices(
    start: usize,
    len: usize,
    forward: bool,
    include_current: bool,
) -> impl Iterator<Item = usize> {
    let offset = usize::from(!include_current);
    (offset..len + offset).map(move |step| {
        if forward {
            (start + step) % len
        } else {
            (start + len - step % len) % len
        }
    })
}
