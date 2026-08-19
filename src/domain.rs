use std::{fmt, path::PathBuf, str::FromStr};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;

use crate::sanitize::sanitize_bytes;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ObjectFormat {
    Sha1,
    Sha256,
    Unknown,
}

impl ObjectFormat {
    pub fn name(self) -> &'static str {
        match self {
            Self::Sha1 => "sha1",
            Self::Sha256 => "sha256",
            Self::Unknown => "unknown",
        }
    }

    pub fn from_git(value: &str) -> Self {
        match value.trim() {
            "sha1" => Self::Sha1,
            "sha256" => Self::Sha256,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Oid {
    pub algorithm: ObjectFormat,
    pub hex: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum OidError {
    #[error("object id is empty, oddly sized, or too long")]
    InvalidLength,
    #[error("object id contains a non-hexadecimal byte")]
    InvalidHex,
}

impl Oid {
    pub fn parse_with_format(value: &str, algorithm: ObjectFormat) -> Result<Self, OidError> {
        let value = value.trim();
        let valid_length = match algorithm {
            ObjectFormat::Sha1 => value.len() == 40,
            ObjectFormat::Sha256 => value.len() == 64,
            ObjectFormat::Unknown => (4..=128).contains(&value.len()) && value.len() % 2 == 0,
        };
        if !valid_length {
            return Err(OidError::InvalidLength);
        }
        if !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(OidError::InvalidHex);
        }
        let inferred = match value.len() {
            40 => ObjectFormat::Sha1,
            64 => ObjectFormat::Sha256,
            _ => ObjectFormat::Unknown,
        };
        Ok(Self {
            algorithm: if algorithm == ObjectFormat::Unknown {
                inferred
            } else {
                algorithm
            },
            hex: value.to_ascii_lowercase(),
        })
    }

    pub fn short(&self, width: usize) -> &str {
        &self.hex[..width.min(self.hex.len())]
    }
}

impl FromStr for Oid {
    type Err = OidError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse_with_format(value, ObjectFormat::Unknown)
    }
}

impl fmt::Display for Oid {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.hex)
    }
}

/// A repository path whose byte representation is authoritative.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct GitPath {
    #[serde(rename = "bytesBase64")]
    bytes_base64: String,
    pub display: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum GitPathError {
    #[error("path bytes are not valid base64")]
    InvalidBase64,
    #[error("native path conversion is unavailable for non-UTF-8 bytes on this platform")]
    UnsupportedNativeEncoding,
}

impl GitPath {
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        let bytes = bytes.into();
        Self {
            bytes_base64: STANDARD.encode(&bytes),
            display: sanitize_bytes(&bytes),
        }
    }

    pub fn bytes_base64(&self) -> &str {
        &self.bytes_base64
    }

    pub fn bytes(&self) -> Vec<u8> {
        STANDARD
            .decode(&self.bytes_base64)
            .expect("GitPath construction validates base64")
    }

    #[cfg(unix)]
    pub fn to_os_string(&self) -> Result<std::ffi::OsString, GitPathError> {
        use std::os::unix::ffi::OsStringExt;
        Ok(std::ffi::OsString::from_vec(self.bytes()))
    }

    #[cfg(not(unix))]
    pub fn to_os_string(&self) -> Result<std::ffi::OsString, GitPathError> {
        String::from_utf8(self.bytes())
            .map(std::ffi::OsString::from)
            .map_err(|_| GitPathError::UnsupportedNativeEncoding)
    }
}

impl<'de> Deserialize<'de> for GitPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct EncodedPath {
            #[serde(rename = "bytesBase64")]
            bytes_base64: String,
            #[serde(default)]
            display: String,
        }

        let encoded = EncodedPath::deserialize(deserializer)?;
        let bytes = STANDARD
            .decode(&encoded.bytes_base64)
            .map_err(|_| de::Error::custom(GitPathError::InvalidBase64))?;
        let path = GitPath::new(bytes);
        if !encoded.display.is_empty() && encoded.display != path.display {
            return Err(de::Error::custom(
                "path display does not match its authoritative bytes",
            ));
        }
        Ok(path)
    }
}

impl From<&[u8]> for GitPath {
    fn from(value: &[u8]) -> Self {
        Self::new(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RefName(pub GitPath);

impl RefName {
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self(GitPath::new(bytes))
    }

    pub fn bytes(&self) -> Vec<u8> {
        self.0.bytes()
    }

    pub fn display(&self) -> &str {
        &self.0.display
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repository {
    pub root: PathBuf,
    pub worktree: Option<PathBuf>,
    pub git_dir: PathBuf,
    pub bare: bool,
    pub object_format: ObjectFormat,
    pub git_version: String,
    pub head: Option<Oid>,
    pub branch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signature {
    pub name: String,
    pub email: String,
    pub timestamp: i64,
    pub timezone: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Commit {
    pub id: Oid,
    pub parents: Vec<Oid>,
    pub author: Signature,
    pub committer: Signature,
    pub decorations: Vec<String>,
    pub subject: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitDetail {
    pub commit: Commit,
    pub diff: Diff,
    pub selected_parent: Option<Oid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryPage {
    pub commits: Vec<Commit>,
    pub offset: usize,
    pub limit: usize,
    pub has_more: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RefKind {
    LocalBranch,
    RemoteBranch,
    Tag,
    Stash,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefInfo {
    pub full_name: RefName,
    pub short_name: RefName,
    pub kind: RefKind,
    pub target: Oid,
    pub peeled: Option<Oid>,
    pub upstream: Option<RefName>,
    pub subject: String,
    pub timestamp: Option<i64>,
    pub is_head: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatusCode {
    Unmodified,
    Modified,
    Added,
    Deleted,
    Renamed,
    Copied,
    UpdatedButUnmerged,
    Untracked,
    Ignored,
    Unknown(char),
}

impl StatusCode {
    pub fn from_byte(byte: u8) -> Self {
        match byte {
            b'.' | b' ' => Self::Unmodified,
            b'M' => Self::Modified,
            b'A' => Self::Added,
            b'D' => Self::Deleted,
            b'R' => Self::Renamed,
            b'C' => Self::Copied,
            b'U' => Self::UpdatedButUnmerged,
            b'?' => Self::Untracked,
            b'!' => Self::Ignored,
            value => Self::Unknown(char::from(value)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictStage {
    pub mode: String,
    pub oid: Option<Oid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictStages {
    pub base: ConflictStage,
    pub ours: ConflictStage,
    pub theirs: ConflictStage,
    pub worktree_mode: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusEntry {
    pub index: StatusCode,
    pub worktree: StatusCode,
    pub path: GitPath,
    pub original_path: Option<GitPath>,
    pub submodule: String,
    pub head_mode: Option<String>,
    pub index_mode: Option<String>,
    pub worktree_mode: Option<String>,
    pub head_oid: Option<Oid>,
    pub index_oid: Option<Oid>,
    pub conflict: Option<ConflictStages>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Status {
    pub branch: Option<String>,
    pub oid: Option<Oid>,
    pub upstream: Option<String>,
    pub ahead: usize,
    pub behind: usize,
    pub entries: Vec<StatusEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TreeEntryKind {
    Blob,
    Tree,
    Commit,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeEntry {
    pub mode: String,
    pub kind: TreeEntryKind,
    pub id: Oid,
    pub size: Option<u64>,
    pub path: GitPath,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Blob {
    pub id: Oid,
    pub path: Option<GitPath>,
    pub bytes_base64: String,
    pub size: usize,
    /// `None` means the retained prefix was too short to classify safely.
    pub binary: Option<bool>,
    pub truncated: bool,
}

impl Blob {
    pub fn bytes(&self) -> Vec<u8> {
        STANDARD.decode(&self.bytes_base64).unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlameLine {
    pub final_line: usize,
    pub original_line: usize,
    pub id: Oid,
    pub author: String,
    pub author_mail: String,
    pub author_time: Option<i64>,
    pub summary: String,
    pub filename: GitPath,
    pub content: String,
    pub boundary: bool,
    pub previous: Option<(Oid, GitPath)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StashEntry {
    pub selector: String,
    pub id: Oid,
    pub parents: Vec<Oid>,
    pub timestamp: Option<i64>,
    pub subject: String,
}

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
    pub hunks: Vec<Hunk>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diff {
    pub lines: Vec<DiffLine>,
    pub files: Vec<DiffFile>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ComparisonMode {
    Exact,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oid_does_not_assume_sha1() {
        let sha256 = Oid::from_str(&"a".repeat(64)).unwrap();
        assert_eq!(sha256.algorithm, ObjectFormat::Sha256);
        let unusual = Oid::from_str("abcd1234").unwrap();
        assert_eq!(unusual.algorithm, ObjectFormat::Unknown);
        assert!(Oid::parse_with_format(&"a".repeat(40), ObjectFormat::Sha256).is_err());
    }

    #[test]
    fn git_path_round_trips_invalid_utf8() {
        let path = GitPath::new(vec![b'a', 0xff, b'\n']);
        assert_eq!(path.bytes(), vec![b'a', 0xff, b'\n']);
        assert!(!path.display.contains('\n'));
        let json = serde_json::to_string(&path).unwrap();
        assert_eq!(serde_json::from_str::<GitPath>(&json).unwrap(), path);
    }

    #[test]
    fn git_path_rejects_invalid_or_spoofed_serialization() {
        assert!(serde_json::from_str::<GitPath>(r#"{"bytesBase64":"%%%","display":"x"}"#).is_err());
        assert!(
            serde_json::from_str::<GitPath>(r#"{"bytesBase64":"eA==","display":"y"}"#).is_err()
        );
    }
}
