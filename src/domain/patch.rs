//! Git patch records and source-coordinate lookups. No presentation state.

use serde::{Deserialize, Serialize};

use super::{GitPath, Oid};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiffLineKind {
    FileHeader,
    HunkHeader,
    Added,
    Removed,
    Context,
    Metadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hunk {
    pub header_line: usize,
    pub old_start: usize,
    pub old_lines: usize,
    pub new_start: usize,
    pub new_lines: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffFile {
    pub header_line: usize,
    pub old_path: Option<GitPath>,
    pub new_path: Option<GitPath>,
    /// Hunk headers in source order.
    pub hunks: Vec<Hunk>,
}

impl DiffFile {
    pub(crate) fn path(&self) -> Option<&GitPath> {
        self.new_path.as_ref().or(self.old_path.as_ref())
    }

    pub(crate) fn hunk_at(&self, raw: usize) -> Option<(usize, &Hunk)> {
        let index = self
            .hunks
            .partition_point(|hunk| hunk.header_line <= raw)
            .checked_sub(1)?;
        Some((index, &self.hunks[index]))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diff {
    pub lines: Vec<DiffLine>,
    /// File headers in source order.
    pub files: Vec<DiffFile>,
    pub truncated: bool,
}

impl Diff {
    pub(crate) fn file_at(&self, raw: usize) -> Option<(usize, &DiffFile)> {
        let index = self
            .files
            .partition_point(|file| file.header_line <= raw)
            .checked_sub(1)?;
        Some((index, &self.files[index]))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ComparisonMode {
    Exact,
    #[default]
    MergeBase,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comparison {
    pub mode: ComparisonMode,
    pub requested_base: String,
    pub requested_head: String,
    pub resolved_base: Oid,
    pub resolved_head: Oid,
    pub merge_base: Option<Oid>,
    pub ahead: usize,
    pub behind: usize,
    pub diff: Diff,
}
