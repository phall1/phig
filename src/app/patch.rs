//! Derived source coordinates and bounded replacement emphasis, never protocol data.

use crate::domain::{Diff, DiffLineKind};

#[derive(Debug, Clone, Default)]
pub(crate) struct PatchLine {
    pub old: Option<usize>,
    pub new: Option<usize>,
    pub pair: Option<usize>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PatchIndex {
    pub lines: Vec<PatchLine>,
    pub split_rows: Vec<usize>,
    pub split_positions: Vec<usize>,
    kinds: Vec<DiffLineKind>,
    hunks: Vec<crate::domain::Hunk>,
}

impl PatchIndex {
    pub fn new(diff: &Diff) -> Self {
        let mut index = Self {
            lines: vec![PatchLine::default(); diff.lines.len()],
            split_rows: Vec::new(),
            split_positions: vec![0; diff.lines.len()],
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
        index.index_split_rows(diff);
        index
    }

    fn index_split_rows(&mut self, diff: &Diff) {
        for (i, line) in diff.lines.iter().enumerate() {
            if line.kind == DiffLineKind::Added
                && let Some(pair) = self.lines[i].pair
            {
                self.split_positions[i] = self.split_positions[pair];
                continue;
            }
            self.split_positions[i] = self.split_rows.len();
            self.split_rows.push(i);
        }
    }

    pub fn move_split(&self, scroll: usize, delta: i32) -> usize {
        let row = self.split_positions.get(scroll).copied().unwrap_or(0);
        let next = (row as i64 + i64::from(delta))
            .clamp(0, self.split_rows.len().saturating_sub(1) as i64) as usize;
        self.split_rows.get(next).copied().unwrap_or(0)
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
        assert_eq!(index.move_split(5, 1), 7);
        assert_eq!(index.move_split(6, -1), 4);
        assert_eq!(index.split_positions[5], index.split_positions[6]);
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
