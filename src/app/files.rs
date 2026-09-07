//! An overlay-scoped file snapshot and ranked query results, reused on movement.

use std::borrow::Cow;

use super::{App, Overlay};

#[derive(Debug)]
pub(super) struct FilePickerCache {
    files: Vec<(String, usize)>,
    query: String,
    matches: Vec<(String, usize)>,
}

impl FilePickerCache {
    fn matches_source(&self, app: &App) -> bool {
        app.file_picker_source()
            .eq(self.files.iter().map(|(path, line)| (path.as_str(), *line)))
    }

    fn new(files: Vec<(String, usize)>, query: &str) -> Self {
        let mut cache = Self {
            files,
            query: String::new(),
            matches: Vec::new(),
        };
        cache.filter(query);
        cache
    }

    fn filter(&mut self, query: &str) {
        self.matches = crate::fuzzy::ranked(&self.files, query, |(path, _)| path)
            .into_iter()
            .cloned()
            .collect();
        self.query = query.to_owned();
    }
}

impl App {
    pub(super) fn file_picker_source(&self) -> impl Iterator<Item = (&str, usize)> {
        self.active_diff()
            .into_iter()
            .flat_map(|diff| &diff.files)
            .filter_map(|file| {
                let path = file.new_path.as_ref().or(file.old_path.as_ref())?;
                Some((path.display.as_str(), file.header_line))
            })
    }

    pub(crate) fn cached_file_picker_entries(&self, query: &str) -> Cow<'_, [(String, usize)]> {
        if let Some(cache) = &self.file_picker_cache
            && cache.query == query
            && cache.matches_source(self)
        {
            return Cow::Borrowed(&cache.matches);
        }
        Cow::Owned(self.file_picker_entries(query))
    }

    pub(super) fn prepare_file_picker(&mut self) {
        let Overlay::FilePicker { draft, .. } = &self.overlay else {
            self.file_picker_cache = None;
            return;
        };
        // App exposes mutable public records. Compare borrowed metadata rather
        // than assuming callers used apply_preview; no path allocation or
        // fuzzy scoring is needed while the source is unchanged.
        if self
            .file_picker_cache
            .as_ref()
            .is_some_and(|cache| !cache.matches_source(self))
        {
            self.file_picker_cache = None;
        }
        if let Some(cache) = &mut self.file_picker_cache {
            if cache.query != *draft {
                cache.filter(draft);
            }
            return;
        }
        self.file_picker_cache = Some(FilePickerCache::new(self.file_picker_entries(""), draft));
    }

    pub(super) fn invalidate_file_picker(&mut self) {
        self.file_picker_cache = None;
        self.prepare_file_picker();
    }
}
