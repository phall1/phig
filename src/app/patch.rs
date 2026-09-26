//! Derived source coordinates and bounded replacement emphasis, never protocol data.

use crate::domain::{Diff, DiffLineKind};

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
    pub fn position(&self, raw: usize) -> usize {
        self.positions
            .get(raw)
            .copied()
            .unwrap_or(0)
            .min(self.rows.len().saturating_sub(1))
    }

    pub fn move_by(&self, scroll: usize, delta: i32) -> usize {
        let next = (self.position(scroll) as i64 + i64::from(delta))
            .clamp(0, self.rows.len().saturating_sub(1) as i64) as usize;
        self.rows.get(next).copied().unwrap_or(0)
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PatchIndex {
    pub lines: Vec<PatchLine>,
    pub unified: RowMap,
    pub split: RowMap,
    pub file_stats: Vec<FileStats>,
    /// Widest old/new line number, so gutters fit the file instead of a guess.
    pub number_width: usize,
    kinds: Vec<DiffLineKind>,
    hunks: Vec<crate::domain::Hunk>,
}

impl PatchIndex {
    pub fn new(diff: &Diff) -> Self {
        let mut index = Self {
            lines: vec![PatchLine::default(); diff.lines.len()],
            unified: RowMap::default(),
            split: RowMap::default(),
            file_stats: diff
                .files
                .iter()
                .map(|file| file_stats(diff, file))
                .collect(),
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
        index.unified = index.row_map(diff, &hidden, false);
        index.split = index.row_map(diff, &hidden, true);
        index
    }

    fn row_map(&self, diff: &Diff, hidden: &[bool], split: bool) -> RowMap {
        let mut map = RowMap {
            rows: Vec::new(),
            positions: vec![0; diff.lines.len()],
        };
        for (i, line) in diff.lines.iter().enumerate() {
            if split
                && line.kind == DiffLineKind::Added
                && let Some(pair) = self.lines[i].pair
            {
                map.positions[i] = map.positions[pair];
                continue;
            }
            map.positions[i] = map.rows.len();
            if !hidden[i] {
                map.rows.push(i);
            }
        }
        map
    }

    pub fn rows(&self, split: bool) -> &RowMap {
        if split { &self.split } else { &self.unified }
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

fn file_stats(diff: &Diff, file: &crate::domain::DiffFile) -> FileStats {
    let end = diff
        .files
        .iter()
        .find(|next| next.header_line > file.header_line)
        .map_or(diff.lines.len(), |next| next.header_line);
    diff.lines[file.header_line.min(end)..end]
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
        assert_eq!(index.split.move_by(5, 1), 7);
        assert_eq!(index.split.move_by(6, -1), 4);
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
        assert_eq!(index.unified.move_by(0, 1), 3);
        assert_eq!(index.unified.move_by(3, -1), 0);
        // Mode changes are real information and stay visible.
        let mode = diff
            .lines
            .iter()
            .position(|line| line.text.starts_with("old mode"))
            .unwrap();
        assert!(index.unified.rows.contains(&mode));
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
