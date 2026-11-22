//! Derived source coordinates and bounded replacement emphasis, never protocol data.

mod words;

use std::sync::{Arc, Mutex, OnceLock};

use crate::domain::{Diff, DiffLineKind};
use words::WordChanges;

#[derive(Debug, Clone, Default)]
pub(crate) struct PatchLine {
    pub old: Option<usize>,
    pub new: Option<usize>,
    pub pair: Option<usize>,
}

/// Added and removed line counts for one changed file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct FileStats {
    pub added: usize,
    pub removed: usize,
}

/// Screen rows for one diff presentation and each raw line's row.
///
/// A raw line that is not drawn (a paired addition in split mode, or Git's
/// `index`/`---`/`+++` header lines that the file banner replaces) maps to
/// the next drawn row, so any raw scroll target still lands on screen.
#[derive(Debug, Clone, Default)]
pub(crate) struct RowMap {
    pub rows: Vec<usize>,
    pub positions: Vec<usize>,
}

impl RowMap {
    pub fn first_row(&self, anchor: usize, height: usize) -> usize {
        self.position(anchor)
            .min(self.rows.len().saturating_sub(height))
    }

    pub fn position(&self, raw: usize) -> usize {
        self.positions
            .get(raw)
            .copied()
            .unwrap_or(0)
            .min(self.rows.len().saturating_sub(1))
    }

    /// Whether raw line `raw` has a row of its own.
    pub fn is_drawn(&self, raw: usize) -> bool {
        self.positions
            .get(raw)
            .and_then(|position| self.rows.get(*position))
            == Some(&raw)
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PatchIndex {
    pub lines: Vec<PatchLine>,
    pub unified: RowMap,
    pub split: RowMap,
    changes: OnceLock<RowMap>,
    split_changes: OnceLock<RowMap>,
    has_context: bool,
    pub file_stats: Vec<FileStats>,
    /// Widest old/new line number, so gutters fit the file instead of a guess.
    pub number_width: usize,
    kinds: Vec<DiffLineKind>,
    hunks: Vec<crate::domain::Hunk>,
    file_headers: Vec<usize>,
    words: Arc<Mutex<words::Cache>>,
}

impl PatchIndex {
    pub fn new(diff: &Diff) -> Self {
        let mut index = Self {
            lines: vec![PatchLine::default(); diff.lines.len()],
            unified: RowMap::default(),
            split: RowMap::default(),
            changes: OnceLock::new(),
            split_changes: OnceLock::new(),
            has_context: diff
                .lines
                .iter()
                .any(|line| line.kind == DiffLineKind::Context),
            file_stats: file_stats(diff),
            file_headers: diff.files.iter().map(|file| file.header_line).collect(),
            words: Arc::default(),
            number_width: 0,
            kinds: diff.lines.iter().map(|line| line.kind).collect(),
            hunks: diff
                .files
                .iter()
                .flat_map(|file| &file.hunks)
                .cloned()
                .collect(),
        };
        for file in &diff.files {
            for hunk in &file.hunks {
                index.number_hunk(diff, hunk);
            }
        }
        index.replacements(diff);
        index.number_width = index
            .lines
            .iter()
            .flat_map(|line| [line.old, line.new])
            .flatten()
            .max()
            .map_or(1, |n| n.to_string().len())
            .max(3);
        let hidden = banner_replaced_lines(diff);
        index.unified = index.row_map(false, |raw, _| hidden[raw]);
        index.split = index.row_map(true, |raw, _| hidden[raw]);
        index
    }

    fn row_map(&self, split: bool, hidden: impl Fn(usize, DiffLineKind) -> bool) -> RowMap {
        let mut map = RowMap {
            rows: Vec::new(),
            positions: vec![0; self.lines.len()],
        };
        for (i, &kind) in self.kinds.iter().enumerate() {
            if split
                && kind == DiffLineKind::Added
                && let Some(pair) = self.lines[i].pair
            {
                map.positions[i] = map.positions[pair];
                continue;
            }
            map.positions[i] = map.rows.len();
            if !hidden(i, kind) {
                map.rows.push(i);
            }
        }
        map
    }

    pub fn rows(&self, split: bool, changes: bool) -> &RowMap {
        match (split, changes && self.has_context) {
            (false, false) => &self.unified,
            (true, false) => &self.split,
            (false, true) => self.changes.get_or_init(|| self.compact_rows(false)),
            (true, true) => self.split_changes.get_or_init(|| self.compact_rows(true)),
        }
    }

    fn compact_rows(&self, split: bool) -> RowMap {
        let mut map = self.row_map(split, |raw, kind| {
            kind == DiffLineKind::Context || !self.unified.is_drawn(raw)
        });
        for (&kind, position) in self.kinds.iter().zip(&mut map.positions) {
            if kind != DiffLineKind::Context {
                continue;
            }
            // Trailing context stays with its hunk instead of jumping across
            // a file or hunk boundary when the presentation changes.
            let next = map.rows.get(*position).map(|&raw| self.kinds[raw]);
            if matches!(
                next,
                None | Some(DiffLineKind::FileHeader | DiffLineKind::HunkHeader)
            ) {
                *position = position.saturating_sub(1);
            }
        }
        map
    }

    pub fn word_changes(&self, diff: &Diff, raw: usize) -> Option<Arc<WordChanges>> {
        let pair = self.lines.get(raw)?.pair?;
        let (old, new) = (raw.min(pair), raw.max(pair));
        self.words
            .lock()
            .ok()?
            .get(old, &diff.lines[old].text, &diff.lines[new].text)
    }

    fn matches_source(&self, diff: &Diff) -> bool {
        // App's public records may be edited by embedding callers. Validate only
        // compact structural metadata; text edits do not invalidate coordinates.
        self.kinds
            .iter()
            .copied()
            .eq(diff.lines.iter().map(|line| line.kind))
            && self
                .hunks
                .iter()
                .eq(diff.files.iter().flat_map(|file| &file.hunks))
            && self
                .file_headers
                .iter()
                .copied()
                .eq(diff.files.iter().map(|file| file.header_line))
    }

    fn number_hunk(&mut self, diff: &Diff, hunk: &crate::domain::Hunk) {
        let (mut old, mut new) = (hunk.old_start, hunk.new_start);
        for (line, source) in diff
            .lines
            .iter()
            .zip(&mut self.lines)
            .skip(hunk.header_line + 1)
        {
            match line.kind {
                DiffLineKind::Context => {
                    source.old = Some(old);
                    source.new = Some(new);
                    old = old.saturating_add(1);
                    new = new.saturating_add(1);
                }
                DiffLineKind::Removed => {
                    source.old = Some(old);
                    old = old.saturating_add(1);
                }
                DiffLineKind::Added => {
                    source.new = Some(new);
                    new = new.saturating_add(1);
                }
                DiffLineKind::Metadata => {}
                _ => break,
            }
        }
    }

    fn replacements(&mut self, diff: &Diff) {
        let mut cursor = 0;
        while cursor < diff.lines.len() {
            if diff.lines[cursor].kind != DiffLineKind::Removed {
                cursor += 1;
                continue;
            }
            let start = cursor;
            cursor = run_end(diff, cursor, DiffLineKind::Removed);
            let added = cursor;
            cursor = run_end(diff, cursor, DiffLineKind::Added);
            if added - start != cursor - added {
                continue;
            }
            for (old, new) in (start..added).zip(added..cursor) {
                self.lines[old].pair = Some(new);
                self.lines[new].pair = Some(old);
            }
        }
    }
}

pub(super) fn file_stats(diff: &Diff) -> Vec<FileStats> {
    let len = diff.lines.len();
    if diff
        .files
        .is_sorted_by(|a, b| a.header_line < b.header_line)
    {
        let starts = diff.files.iter().map(|file| file.header_line.min(len));
        let ends = starts.clone().skip(1).chain(std::iter::once(len));
        return starts
            .zip(ends)
            .map(|(start, end)| count_changes(&diff.lines[start..end]))
            .collect();
    }
    // Git emits ordered file headers. Keep the public-API fallback bounded
    // and count duplicate or out-of-order anchors by their source ranges.
    let mut order: Vec<_> = (0..diff.files.len()).collect();
    let header = |ordinal: usize| diff.files[ordinal].header_line.min(len);
    order.sort_unstable_by_key(|ordinal| header(*ordinal));
    let mut stats = vec![FileStats::default(); order.len()];
    let mut groups = order.chunk_by(|a, b| header(*a) == header(*b)).peekable();
    while let Some(group) = groups.next() {
        let start = header(group[0]);
        let end = groups.peek().map_or(len, |next| header(next[0]));
        let count = count_changes(&diff.lines[start..end]);
        for &ordinal in group {
            stats[ordinal] = count;
        }
    }
    stats
}

pub(crate) fn count_changes(lines: &[crate::domain::DiffLine]) -> FileStats {
    lines
        .iter()
        .fold(FileStats::default(), |stats, line| match line.kind {
            DiffLineKind::Added => FileStats {
                added: stats.added + 1,
                ..stats
            },
            DiffLineKind::Removed => FileStats {
                removed: stats.removed + 1,
                ..stats
            },
            _ => stats,
        })
}

/// Git's `index`, `---`, and `+++` lines restate what a file banner shows.
/// Only lines between a file header and its first hunk are candidates.
fn banner_replaced_lines(diff: &Diff) -> Vec<bool> {
    let mut hidden = vec![false; diff.lines.len()];
    for (ordinal, file) in diff.files.iter().enumerate() {
        let end = file
            .hunks
            .first()
            .map(|hunk| hunk.header_line)
            .or_else(|| diff.files.get(ordinal + 1).map(|next| next.header_line))
            .unwrap_or(diff.lines.len())
            .min(diff.lines.len());
        let start = file.header_line.saturating_add(1).min(end);
        for (line, hide) in diff.lines[start..end].iter().zip(&mut hidden[start..end]) {
            *hide = (line.kind == DiffLineKind::FileHeader
                && (line.text.starts_with("--- ") || line.text.starts_with("+++ ")))
                || (line.kind == DiffLineKind::Metadata && line.text.starts_with("index "));
        }
    }
    hidden
}

fn run_end(diff: &Diff, start: usize, kind: DiffLineKind) -> usize {
    start
        + diff.lines[start..]
            .iter()
            .take_while(|line| line.kind == kind)
            .count()
}

impl super::App {
    pub(crate) fn patch_index(&self) -> std::borrow::Cow<'_, PatchIndex> {
        use super::View;
        let cached = match self.view {
            View::Compare => &self.comparison_index,
            View::Status | View::StatusDiff => &self.working_index,
            _ => &self.preview_index,
        };
        match cached {
            Some(index)
                if self
                    .active_diff()
                    .is_some_and(|diff| index.matches_source(diff)) =>
            {
                std::borrow::Cow::Borrowed(index)
            }
            _ => std::borrow::Cow::Owned(
                self.active_diff()
                    .map_or_else(PatchIndex::default, PatchIndex::new),
            ),
        }
    }
}

#[cfg(test)]
pub(crate) fn fixture() -> Diff {
    use crate::{
        domain::GitPath,
        git::parse::{DiffFileIdentity, parse_diff},
    };
    let paths = ["src/main.rs", "src/ui/view.rs", "README.md"];
    let identities: Vec<_> = paths
        .iter()
        .map(|path| DiffFileIdentity {
            old_path: Some(GitPath::new(path.as_bytes().to_vec())),
            new_path: Some(GitPath::new(path.as_bytes().to_vec())),
        })
        .collect();
    parse_diff(
        concat!(
            "diff --git a/src/main.rs b/src/main.rs\n",
            "--- a/src/main.rs\n+++ b/src/main.rs\n@@ -10,3 +10,3 @@ fn main()\n",
            " let label = \"café\";\n",
            "-let timeout = 100; // old value\n",
            "+let timeout = 250; // new value\n",
            " render(label);\n",
            "diff --git a/src/ui/view.rs b/src/ui/view.rs\n",
            "--- a/src/ui/view.rs\n+++ b/src/ui/view.rs\n@@ -0,0 +1,2 @@\n",
            "+fn draw() {}\n+// 界面\n",
            "diff --git a/README.md b/README.md\n",
            "old mode 100644\nnew mode 100755\n",
        )
        .as_bytes(),
        &identities,
        false,
    )
    .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The raw line `delta` drawn rows away from `scroll`.
    fn step(map: &RowMap, scroll: usize, delta: i64) -> usize {
        let row = (map.position(scroll) as i64 + delta).clamp(0, map.rows.len() as i64 - 1);
        map.rows[row as usize]
    }

    #[test]
    fn parsed_hunks_have_independent_source_numbers_and_split_anchors() {
        let diff = fixture();
        let index = PatchIndex::new(&diff);
        assert_eq!(
            (index.lines[4].old, index.lines[4].new),
            (Some(10), Some(10))
        );
        assert_eq!((index.lines[5].old, index.lines[5].new), (Some(11), None));
        assert_eq!((index.lines[6].old, index.lines[6].new), (None, Some(11)));
        assert_eq!(
            (index.lines[7].old, index.lines[7].new),
            (Some(12), Some(12))
        );
        assert_eq!((index.lines[12].old, index.lines[12].new), (None, Some(1)));
        assert_eq!(step(&index.split, 5, 1), 7);
        assert_eq!(step(&index.split, 6, -1), 4);
        assert_eq!(index.split.positions[5], index.split.positions[6]);
        assert_eq!(index.number_width, 3);
        assert_eq!(
            index.file_stats[0],
            FileStats {
                added: 1,
                removed: 1
            }
        );
    }

    #[test]
    fn banner_replaced_headers_are_skipped_but_still_reachable() {
        let diff = fixture();
        let index = PatchIndex::new(&diff);
        // Raw lines 1 and 2 are `---`/`+++` for the first file.
        assert!(!index.unified.rows.contains(&1));
        assert!(!index.unified.rows.contains(&2));
        assert_eq!(index.unified.rows[..2], [0, 3]);
        // Scrolling onto a hidden line lands on the next drawn row.
        assert_eq!(index.unified.position(1), index.unified.position(3));
        assert_eq!(step(&index.unified, 0, 1), 3);
        assert_eq!(step(&index.unified, 3, -1), 0);
        // Mode changes are real information and stay visible.
        let mode = diff
            .lines
            .iter()
            .position(|line| line.text.starts_with("old mode"))
            .unwrap();
        assert!(index.unified.rows.contains(&mode));
    }

    #[test]
    fn compact_context_anchors_stay_in_their_hunk() {
        let index = PatchIndex::new(&fixture());
        for split in [false, true] {
            let compact = index.rows(split, true);
            assert_eq!(compact.position(4), compact.position(5));
            assert_eq!(compact.position(7), compact.position(6));
            assert_ne!(compact.position(7), compact.position(8));
        }
    }

    #[test]
    fn file_counts_hold_for_missing_and_unordered_headers() {
        let mut diff = fixture();
        let expected = file_stats(&diff);
        diff.files.swap(0, 2);
        diff.files.push(diff.files[2].clone());
        diff.files.push(crate::domain::DiffFile {
            header_line: usize::MAX,
            old_path: None,
            new_path: None,
            hunks: Vec::new(),
        });
        assert_eq!(
            file_stats(&diff),
            [
                expected[2],
                expected[1],
                expected[0],
                expected[0],
                FileStats::default()
            ]
        );
    }

    #[test]
    fn unbalanced_changes_and_no_newline_markers_never_pair_across_boundaries() {
        let mut diff = fixture();
        diff.lines[6].kind = DiffLineKind::Metadata;
        let index = PatchIndex::new(&diff);
        assert_eq!(index.lines[5].pair, None);
        assert_eq!(
            (index.lines[7].old, index.lines[7].new),
            (Some(12), Some(11))
        );
        assert!(index.lines[14].old.is_none());
    }
}
