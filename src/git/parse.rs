use std::str;

use memchr::memchr;

use crate::{
    domain::{
        BlameLine, Commit, ConflictStage, ConflictStages, Diff, DiffFile, DiffLine, DiffLineKind,
        GitPath, HistoryPage, Hunk, ObjectFormat, Oid, RefInfo, RefKind, RefName, Signature,
        StashEntry, Status, StatusCode, StatusEntry, TreeEntry, TreeEntryKind,
    },
    git::process::GitError,
    sanitize::sanitize_bytes,
};

const MAX_STRUCTURED_RECORDS: usize = 65_536;
const MAX_PARSER_LINES: usize = 1_048_576;
const MAX_DIFF_LINES: usize = 262_144;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiffFileIdentity {
    pub old_path: Option<GitPath>,
    pub new_path: Option<GitPath>,
}

pub fn parse_history(
    output: &[u8],
    format: ObjectFormat,
    offset: usize,
    limit: usize,
) -> Result<HistoryPage, GitError> {
    let records = parse_fixed_nul_records(output, 12, "history")?;
    let mut commits = Vec::with_capacity(records.len().min(limit));
    for fields in records.iter().take(limit) {
        commits.push(parse_commit_fields(fields, format, false)?);
    }
    Ok(HistoryPage {
        has_more: records.len() > limit,
        commits,
        offset,
        limit,
    })
}

pub fn parse_commit(output: &[u8], format: ObjectFormat) -> Result<Commit, GitError> {
    let records = parse_fixed_nul_records(output, 13, "commit-detail")?;
    let fields = records
        .first()
        .ok_or_else(|| parse_failure("commit-detail", 0, "empty commit metadata"))?;
    parse_commit_fields(fields, format, true)
}

fn parse_fixed_nul_records<'a>(
    output: &'a [u8],
    field_count: usize,
    operation: &'static str,
) -> Result<Vec<Vec<&'a [u8]>>, GitError> {
    let mut records = Vec::new();
    let mut cursor = 0;
    while cursor < output.len() {
        let mut fields = Vec::with_capacity(field_count);
        for _ in 0..field_count {
            let relative = memchr(0, &output[cursor..]).ok_or_else(|| {
                parse_failure(operation, cursor, "record ended before all fields")
            })?;
            let end = cursor + relative;
            fields.push(&output[cursor..end]);
            cursor = end + 1;
        }
        if output.get(cursor) != Some(&0) {
            return parse_error(operation, cursor, "record terminator is missing");
        }
        cursor += 1;
        if records.len() >= MAX_STRUCTURED_RECORDS {
            return parse_error(operation, cursor, "record count exceeds safety limit");
        }
        records.push(fields);
    }
    Ok(records)
}

fn parse_commit_fields(
    fields: &[&[u8]],
    format: ObjectFormat,
    with_body: bool,
) -> Result<Commit, GitError> {
    let required = if with_body { 13 } else { 12 };
    if fields.len() != required {
        return parse_error(
            "history",
            0,
            format!("expected {required} fields, got {}", fields.len()),
        );
    }
    let oid = parse_oid(fields[0], format, "history")?;
    let parents = field_str(fields[1])?
        .split_ascii_whitespace()
        .map(|value| Oid::parse_with_format(value, format))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| parse_failure("history", 0, error.to_string()))?;
    let author = Signature {
        name: sanitize_bytes(fields[2]),
        email: sanitize_bytes(fields[3]),
        timestamp: parse_i64(fields[4], "history")?,
        timezone: timezone_from_iso(fields[5]),
    };
    let committer = Signature {
        name: sanitize_bytes(fields[6]),
        email: sanitize_bytes(fields[7]),
        timestamp: parse_i64(fields[8], "history")?,
        timezone: timezone_from_iso(fields[9]),
    };
    let decorations = fields[10]
        .split(|byte| *byte == 0x1f)
        .filter(|item| !item.is_empty())
        .map(sanitize_bytes)
        .collect();
    Ok(Commit {
        id: oid,
        parents,
        author,
        committer,
        decorations,
        subject: sanitize_bytes(fields[11]),
        body: if with_body {
            fields[12]
                .split(|byte| *byte == b'\n')
                .map(sanitize_bytes)
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            String::new()
        },
    })
}

pub fn parse_refs(output: &[u8], format: ObjectFormat) -> Result<Vec<RefInfo>, GitError> {
    let mut refs = Vec::new();
    for (line_number, line) in output.split(|byte| *byte == b'\n').enumerate() {
        if line_number >= MAX_PARSER_LINES {
            return parse_error("refs", line_number, "line count exceeds safety limit");
        }
        if line.is_empty() {
            continue;
        }
        if refs.len() >= MAX_STRUCTURED_RECORDS {
            return parse_error("refs", line_number, "record count exceeds safety limit");
        }
        let fields: Vec<&[u8]> = line.split(|byte| *byte == 0).take(10).collect();
        if fields.len() != 9 {
            return parse_error("refs", line_number, "ref record has wrong field count");
        }
        let kind = if fields[0].starts_with(b"refs/heads/") {
            RefKind::LocalBranch
        } else if fields[0].starts_with(b"refs/remotes/") {
            RefKind::RemoteBranch
        } else if fields[0].starts_with(b"refs/tags/") {
            RefKind::Tag
        } else if fields[0].starts_with(b"refs/stash") {
            RefKind::Stash
        } else {
            RefKind::Other
        };
        refs.push(RefInfo {
            full_name: RefName::new(fields[0].to_vec()),
            short_name: RefName::new(fields[1].to_vec()),
            kind,
            target: parse_oid(fields[2], format, "refs")?,
            peeled: optional_oid(fields[3], format, "refs")?,
            upstream: (!fields[4].is_empty()).then(|| RefName::new(fields[4].to_vec())),
            subject: sanitize_bytes(fields[5]),
            timestamp: optional_i64(fields[6], "refs")?,
            is_head: fields[7] == b"*",
        });
    }
    Ok(refs)
}

pub fn parse_status(output: &[u8], format: ObjectFormat) -> Result<Status, GitError> {
    let mut records = output.split(|byte| *byte == 0);
    let mut status = Status::default();
    let mut index = 0;
    while let Some(record) = records.next() {
        if record.is_empty() {
            continue;
        }
        index += 1;
        if index > MAX_STRUCTURED_RECORDS {
            return parse_error("status", index, "record count exceeds safety limit");
        }
        if let Some(header) = record.strip_prefix(b"# ") {
            parse_status_header(header, format, &mut status)?;
            continue;
        }
        match record[0] {
            b'1' => status.entries.push(parse_status_ordinary(record, format)?),
            b'2' => {
                let original = records.next().ok_or_else(|| {
                    parse_failure("status", index, "rename record is missing original path")
                })?;
                index += 1;
                if index > MAX_STRUCTURED_RECORDS {
                    return parse_error("status", index, "record count exceeds safety limit");
                }
                let mut entry = parse_status_ordinary(record, format)?;
                entry.original_path = Some(GitPath::new(original.to_vec()));
                status.entries.push(entry);
            }
            b'u' => status.entries.push(parse_status_unmerged(record, format)?),
            b'?' | b'!' => {
                let code = StatusCode::from_byte(record[0]);
                let path = record.get(2..).ok_or_else(|| {
                    parse_failure("status", index, "short status record has no path")
                })?;
                status.entries.push(StatusEntry {
                    index: code,
                    worktree: code,
                    path: GitPath::new(path.to_vec()),
                    original_path: None,
                    submodule: String::new(),
                    head_mode: None,
                    index_mode: None,
                    worktree_mode: None,
                    head_oid: None,
                    index_oid: None,
                    conflict: None,
                });
            }
            kind => {
                return parse_error(
                    "status",
                    index,
                    format!("unknown status record type {kind:#x}"),
                );
            }
        }
    }
    Ok(status)
}

fn parse_status_header(
    header: &[u8],
    format: ObjectFormat,
    status: &mut Status,
) -> Result<(), GitError> {
    if let Some(value) = header.strip_prefix(b"branch.oid ") {
        if value != b"(initial)" {
            status.oid = Some(parse_oid(value, format, "status")?);
        }
    } else if let Some(value) = header.strip_prefix(b"branch.head ") {
        if value != b"(detached)" {
            status.branch = Some(sanitize_bytes(value));
        }
    } else if let Some(value) = header.strip_prefix(b"branch.upstream ") {
        status.upstream = Some(sanitize_bytes(value));
    } else if let Some(value) = header.strip_prefix(b"branch.ab ") {
        let text = field_str(value)?;
        let fields: Vec<&str> = text.split_ascii_whitespace().take(3).collect();
        if fields.len() != 2 {
            return parse_error("status", 0, "branch.ab must contain exactly +ahead -behind");
        }
        let ahead = fields[0]
            .strip_prefix('+')
            .ok_or_else(|| parse_failure("status", 0, "branch.ab ahead lacks + prefix"))?;
        let behind = fields[1]
            .strip_prefix('-')
            .ok_or_else(|| parse_failure("status", 0, "branch.ab behind lacks - prefix"))?;
        status.ahead = parse_unsigned_ascii(ahead, "ahead")?;
        status.behind = parse_unsigned_ascii(behind, "behind")?;
    }
    Ok(())
}

fn parse_status_ordinary(record: &[u8], format: ObjectFormat) -> Result<StatusEntry, GitError> {
    let parts: Vec<&[u8]> = record.splitn(10, |byte| *byte == b' ').collect();
    let rename = record.first() == Some(&b'2');
    let expected = if rename { 10 } else { 9 };
    if parts.len() != expected {
        return parse_error("status", 0, format!("expected {expected} status fields"));
    }
    let path_index = expected - 1;
    let xy = parts[1];
    if xy.len() != 2 {
        return parse_error("status", 0, "status XY field is malformed");
    }
    Ok(StatusEntry {
        index: StatusCode::from_byte(xy[0]),
        worktree: StatusCode::from_byte(xy[1]),
        path: GitPath::new(parts[path_index].to_vec()),
        original_path: None,
        submodule: sanitize_bytes(parts[2]),
        head_mode: Some(sanitize_bytes(parts[3])),
        index_mode: Some(sanitize_bytes(parts[4])),
        worktree_mode: Some(sanitize_bytes(parts[5])),
        head_oid: optional_oid(parts[6], format, "status")?,
        index_oid: optional_oid(parts[7], format, "status")?,
        conflict: None,
    })
}

fn parse_status_unmerged(record: &[u8], format: ObjectFormat) -> Result<StatusEntry, GitError> {
    let parts: Vec<&[u8]> = record.splitn(11, |byte| *byte == b' ').collect();
    if parts.len() != 11 {
        return parse_error("status", 0, "unmerged status record is malformed");
    }
    let xy = parts[1];
    if xy.len() != 2 {
        return parse_error("status", 0, "unmerged XY field is malformed");
    }
    Ok(StatusEntry {
        index: StatusCode::from_byte(xy[0]),
        worktree: StatusCode::from_byte(xy[1]),
        path: GitPath::new(parts[10].to_vec()),
        original_path: None,
        submodule: sanitize_bytes(parts[2]),
        head_mode: Some(sanitize_bytes(parts[3])),
        index_mode: Some(sanitize_bytes(parts[4])),
        worktree_mode: Some(sanitize_bytes(parts[6])),
        head_oid: optional_oid(parts[7], format, "status")?,
        index_oid: optional_oid(parts[8], format, "status")?,
        conflict: Some(ConflictStages {
            base: ConflictStage {
                mode: sanitize_bytes(parts[3]),
                oid: optional_oid(parts[7], format, "status")?,
            },
            ours: ConflictStage {
                mode: sanitize_bytes(parts[4]),
                oid: optional_oid(parts[8], format, "status")?,
            },
            theirs: ConflictStage {
                mode: sanitize_bytes(parts[5]),
                oid: optional_oid(parts[9], format, "status")?,
            },
            worktree_mode: sanitize_bytes(parts[6]),
        }),
    })
}

pub fn parse_tree(output: &[u8], format: ObjectFormat) -> Result<Vec<TreeEntry>, GitError> {
    let mut entries = Vec::new();
    for (record_number, record) in output.split(|byte| *byte == 0).enumerate() {
        if record.is_empty() {
            continue;
        }
        if entries.len() >= MAX_STRUCTURED_RECORDS {
            return parse_error("tree", record_number, "record count exceeds safety limit");
        }
        let (metadata, path) = split_once_byte(record, b'\t').ok_or_else(|| {
            parse_failure("tree", record_number, "tree record has no path separator")
        })?;
        let fields: Vec<&[u8]> = metadata
            .split(|byte| *byte == b' ')
            .filter(|field| !field.is_empty())
            .collect();
        if fields.len() != 4 {
            return parse_error("tree", record_number, "tree metadata has wrong field count");
        }
        entries.push(TreeEntry {
            mode: sanitize_bytes(fields[0]),
            kind: match fields[1] {
                b"blob" => TreeEntryKind::Blob,
                b"tree" => TreeEntryKind::Tree,
                b"commit" => TreeEntryKind::Commit,
                _ => TreeEntryKind::Unknown,
            },
            id: parse_oid(fields[2], format, "tree")?,
            size: if fields[3] == b"-" {
                None
            } else {
                Some(parse_u64(fields[3], "tree")?)
            },
            path: GitPath::new(path.to_vec()),
        });
    }
    Ok(entries)
}

pub fn parse_stashes(output: &[u8], format: ObjectFormat) -> Result<Vec<StashEntry>, GitError> {
    let mut entries = Vec::new();
    for (record_number, record) in output.split(|byte| *byte == 0).enumerate() {
        if record.is_empty() {
            continue;
        }
        if entries.len() >= MAX_STRUCTURED_RECORDS {
            return parse_error("stash", record_number, "record count exceeds safety limit");
        }
        let fields: Vec<&[u8]> = record.splitn(5, |byte| *byte == 0x1f).collect();
        if fields.len() != 5 {
            return parse_error("stash", record_number, "stash record has wrong field count");
        }
        entries.push(StashEntry {
            selector: sanitize_bytes(fields[0]),
            id: parse_oid(fields[1], format, "stash")?,
            parents: field_str(fields[2])?
                .split_ascii_whitespace()
                .map(|value| Oid::parse_with_format(value, format))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| parse_failure("stash", record_number, error.to_string()))?,
            timestamp: optional_i64(fields[3], "stash")?,
            subject: sanitize_bytes(fields[4]),
        });
    }
    Ok(entries)
}

pub fn parse_blame(output: &[u8], format: ObjectFormat) -> Result<Vec<BlameLine>, GitError> {
    let mut lines = Vec::new();
    let records: Vec<&[u8]> = output
        .split(|byte| *byte == b'\n')
        .take(MAX_PARSER_LINES + 1)
        .collect();
    if records.len() > MAX_PARSER_LINES {
        return parse_error("blame", 0, "line count exceeds safety limit");
    }
    let mut index = 0;
    while index < records.len() {
        let header = records[index];
        index += 1;
        if header.is_empty() {
            continue;
        }
        let header_parts: Vec<&[u8]> = header.split(|byte| *byte == b' ').collect();
        if header_parts.len() < 3 || !looks_like_oid(header_parts[0]) {
            return parse_error("blame", index, "invalid blame group header");
        }
        let id = parse_oid(header_parts[0], format, "blame")?;
        let original_line = parse_usize(header_parts[1], "blame")?;
        let final_line = parse_usize(header_parts[2], "blame")?;
        let mut author = String::new();
        let mut author_mail = String::new();
        let mut author_time = None;
        let mut summary = String::new();
        let mut filename = GitPath::new(Vec::new());
        let mut boundary = false;
        let mut previous = None;
        let content = loop {
            let record = records.get(index).ok_or_else(|| {
                parse_failure("blame", index, "blame group ended before source line")
            })?;
            index += 1;
            if let Some(value) = record.strip_prefix(b"\t") {
                break sanitize_bytes(value);
            }
            if let Some(value) = record.strip_prefix(b"author ") {
                author = sanitize_bytes(value);
            } else if let Some(value) = record.strip_prefix(b"author-mail ") {
                author_mail = sanitize_bytes(value);
            } else if let Some(value) = record.strip_prefix(b"author-time ") {
                author_time = optional_i64(value, "blame")?;
            } else if let Some(value) = record.strip_prefix(b"summary ") {
                summary = sanitize_bytes(value);
            } else if let Some(value) = record.strip_prefix(b"filename ") {
                filename = GitPath::new(decode_git_path(value));
            } else if *record == b"boundary" {
                boundary = true;
            } else if let Some(value) = record.strip_prefix(b"previous ") {
                if let Some((oid, path)) = split_once_byte(value, b' ') {
                    previous = Some((
                        parse_oid(oid, format, "blame")?,
                        GitPath::new(decode_git_path(path)),
                    ));
                }
            }
        };
        lines.push(BlameLine {
            final_line,
            original_line,
            id,
            author,
            author_mail,
            author_time,
            summary,
            filename,
            content,
            boundary,
            previous,
        });
    }
    Ok(lines)
}

pub(crate) fn parse_raw_diff_metadata(output: &[u8]) -> Result<Vec<DiffFileIdentity>, GitError> {
    if !output.is_empty() && !output.ends_with(b"\0") {
        return parse_error("diff-metadata", output.len(), "missing final NUL delimiter");
    }
    let mut records = output.split(|byte| *byte == 0);
    let mut files = Vec::new();
    loop {
        let Some(header) = records.next() else {
            break;
        };
        if header.is_empty() {
            if records.any(|record| !record.is_empty()) {
                return parse_error(
                    "diff-metadata",
                    files.len(),
                    "records follow the final NUL delimiter",
                );
            }
            break;
        }
        if files.len() >= MAX_STRUCTURED_RECORDS {
            return parse_error(
                "diff-metadata",
                files.len(),
                "file count exceeds safety limit",
            );
        }
        let mut fields = header.split(|byte| *byte == b' ');
        let old_mode = fields.next();
        let new_mode = fields.next();
        let old_oid = fields.next();
        let new_oid = fields.next();
        let status = fields.next();
        if fields.next().is_some()
            || !old_mode.is_some_and(|field| field.starts_with(b":"))
            || new_mode.is_none_or(<[u8]>::is_empty)
            || old_oid.is_none_or(<[u8]>::is_empty)
            || new_oid.is_none_or(<[u8]>::is_empty)
            || status.is_none_or(<[u8]>::is_empty)
        {
            return parse_error("diff-metadata", files.len(), "malformed raw diff header");
        }
        let status = status.expect("validated above");
        let kind = *status
            .first()
            .ok_or_else(|| parse_failure("diff-metadata", files.len(), "missing status"))?;
        if !matches!(kind, b'A' | b'C' | b'D' | b'M' | b'R' | b'T' | b'U' | b'X')
            || status[1..].iter().any(|byte| !byte.is_ascii_digit())
            || (!matches!(kind, b'C' | b'R') && status.len() != 1)
        {
            return parse_error("diff-metadata", files.len(), "invalid raw diff status");
        }
        let first_path = records
            .next()
            .ok_or_else(|| parse_failure("diff-metadata", files.len(), "missing raw diff path"))?;
        if first_path.is_empty() {
            return parse_error("diff-metadata", files.len(), "empty raw diff path");
        }
        let identity = match kind {
            b'A' => DiffFileIdentity {
                old_path: None,
                new_path: Some(GitPath::new(first_path.to_vec())),
            },
            b'D' => DiffFileIdentity {
                old_path: Some(GitPath::new(first_path.to_vec())),
                new_path: None,
            },
            b'C' | b'R' => {
                let second_path = records.next().ok_or_else(|| {
                    parse_failure(
                        "diff-metadata",
                        files.len(),
                        "rename/copy is missing its destination path",
                    )
                })?;
                if second_path.is_empty() {
                    return parse_error(
                        "diff-metadata",
                        files.len(),
                        "empty rename/copy destination path",
                    );
                }
                DiffFileIdentity {
                    old_path: Some(GitPath::new(first_path.to_vec())),
                    new_path: Some(GitPath::new(second_path.to_vec())),
                }
            }
            _ => DiffFileIdentity {
                old_path: Some(GitPath::new(first_path.to_vec())),
                new_path: Some(GitPath::new(first_path.to_vec())),
            },
        };
        files.push(identity);
    }
    Ok(files)
}

pub(crate) fn parse_diff(
    output: &[u8],
    identities: &[DiffFileIdentity],
    mut truncated: bool,
) -> Result<Diff, GitError> {
    let mut lines = Vec::new();
    let mut files: Vec<DiffFile> = Vec::new();
    let mut in_hunk = false;
    for raw_line in output.split_inclusive(|byte| *byte == b'\n') {
        if lines.len() >= MAX_DIFF_LINES {
            truncated = true;
            break;
        }
        let raw_line = raw_line.strip_suffix(b"\n").unwrap_or(raw_line);
        let kind = if raw_line.starts_with(b"diff --git ") {
            in_hunk = false;
            let identity = identities.get(files.len()).ok_or_else(|| {
                parse_failure(
                    "diff",
                    lines.len(),
                    "patch contains more file headers than raw metadata",
                )
            })?;
            files.push(DiffFile {
                header_line: lines.len(),
                old_path: identity.old_path.clone(),
                new_path: identity.new_path.clone(),
                hunks: Vec::new(),
            });
            DiffLineKind::FileHeader
        } else if raw_line.starts_with(b"@@ ") || raw_line.starts_with(b"@@@ ") {
            if let Some(hunk) = parse_hunk_header(raw_line, lines.len())
                && let Some(file) = files.last_mut()
            {
                file.hunks.push(hunk);
            }
            in_hunk = true;
            DiffLineKind::HunkHeader
        } else if in_hunk {
            match raw_line.first() {
                Some(b'+') => DiffLineKind::Added,
                Some(b'-') => DiffLineKind::Removed,
                Some(b' ') => DiffLineKind::Context,
                _ => DiffLineKind::Metadata,
            }
        } else if raw_line.starts_with(b"--- ") || raw_line.starts_with(b"+++ ") {
            DiffLineKind::FileHeader
        } else {
            DiffLineKind::Metadata
        };
        lines.push(DiffLine {
            kind,
            text: sanitize_bytes(raw_line),
        });
    }
    if !truncated && files.len() != identities.len() {
        return parse_error(
            "diff",
            lines.len(),
            "raw metadata and patch file counts differ",
        );
    }
    Ok(Diff {
        lines,
        files,
        truncated,
    })
}

fn decode_git_path(value: &[u8]) -> Vec<u8> {
    decode_quoted_token(value)
        .filter(|(_, consumed)| *consumed == value.len())
        .map_or_else(|| value.to_vec(), |(decoded, _)| decoded)
}

fn decode_quoted_token(value: &[u8]) -> Option<(Vec<u8>, usize)> {
    if value.first() != Some(&b'\"') {
        return None;
    }
    let mut output = Vec::with_capacity(value.len());
    let mut index = 1;
    while index < value.len() {
        match value[index] {
            b'\"' => return Some((output, index + 1)),
            b'\\' => {
                index += 1;
                let escaped = *value.get(index)?;
                match escaped {
                    b'a' => output.push(0x07),
                    b'b' => output.push(0x08),
                    b't' => output.push(b'\t'),
                    b'n' => output.push(b'\n'),
                    b'v' => output.push(0x0b),
                    b'f' => output.push(0x0c),
                    b'r' => output.push(b'\r'),
                    b'\\' | b'\"' => output.push(escaped),
                    b'0'..=b'7' => {
                        let mut octal = u16::from(escaped - b'0');
                        let mut count = 1;
                        while count < 3
                            && value
                                .get(index + 1)
                                .is_some_and(|byte| (b'0'..=b'7').contains(byte))
                        {
                            index += 1;
                            octal = octal * 8 + u16::from(value[index] - b'0');
                            count += 1;
                        }
                        output.push(octal.min(255) as u8);
                    }
                    other => output.push(other),
                }
            }
            byte => output.push(byte),
        }
        index += 1;
    }
    None
}

fn parse_hunk_header(line: &[u8], header_line: usize) -> Option<Hunk> {
    let text = str::from_utf8(line).ok()?;
    let mut pieces = text.split_ascii_whitespace();
    let marker = pieces.next()?;
    if marker != "@@" {
        return None;
    }
    let old = pieces.next()?.strip_prefix('-')?;
    let new = pieces.next()?.strip_prefix('+')?;
    let (old_start, old_lines) = parse_range(old)?;
    let (new_start, new_lines) = parse_range(new)?;
    Some(Hunk {
        header_line,
        old_start,
        old_lines,
        new_start,
        new_lines,
    })
}

fn parse_range(value: &str) -> Option<(usize, usize)> {
    if let Some((start, count)) = value.split_once(',') {
        Some((start.parse().ok()?, count.parse().ok()?))
    } else {
        Some((value.parse().ok()?, 1))
    }
}

fn split_once_byte(value: &[u8], delimiter: u8) -> Option<(&[u8], &[u8])> {
    let index = value.iter().position(|byte| *byte == delimiter)?;
    Some((&value[..index], &value[index + 1..]))
}

fn looks_like_oid(value: &[u8]) -> bool {
    value.len() >= 4 && value.iter().all(u8::is_ascii_hexdigit)
}

fn parse_oid(value: &[u8], format: ObjectFormat, operation: &'static str) -> Result<Oid, GitError> {
    let value = field_str(value)?;
    Oid::parse_with_format(value, format)
        .map_err(|error| parse_failure(operation, 0, error.to_string()))
}

fn optional_oid(
    value: &[u8],
    format: ObjectFormat,
    operation: &'static str,
) -> Result<Option<Oid>, GitError> {
    if value.is_empty() || value.iter().all(|byte| *byte == b'0') {
        Ok(None)
    } else {
        parse_oid(value, format, operation).map(Some)
    }
}

fn field_str(value: &[u8]) -> Result<&str, GitError> {
    str::from_utf8(value)
        .map_err(|error| parse_failure("git", error.valid_up_to(), error.to_string()))
}

fn parse_unsigned_ascii(value: &str, label: &'static str) -> Result<usize, GitError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return parse_error("status", 0, format!("invalid {label} count"));
    }
    value
        .parse()
        .map_err(|error| parse_failure("status", 0, format!("invalid {label} count: {error}")))
}

fn timezone_from_iso(value: &[u8]) -> String {
    if value.ends_with(b"Z") {
        return "Z".to_owned();
    }
    if value.len() >= 6 {
        let suffix = &value[value.len() - 6..];
        if matches!(suffix[0], b'+' | b'-') && suffix[3] == b':' {
            return sanitize_bytes(suffix);
        }
    }
    sanitize_bytes(value)
}

fn parse_i64(value: &[u8], operation: &'static str) -> Result<i64, GitError> {
    field_str(value)?
        .parse()
        .map_err(|error| parse_failure(operation, 0, format!("invalid integer: {error}")))
}

fn optional_i64(value: &[u8], operation: &'static str) -> Result<Option<i64>, GitError> {
    if value.is_empty() {
        Ok(None)
    } else {
        parse_i64(value, operation).map(Some)
    }
}

fn parse_u64(value: &[u8], operation: &'static str) -> Result<u64, GitError> {
    field_str(value)?
        .parse()
        .map_err(|error| parse_failure(operation, 0, format!("invalid integer: {error}")))
}

fn parse_usize(value: &[u8], operation: &'static str) -> Result<usize, GitError> {
    field_str(value)?
        .parse()
        .map_err(|error| parse_failure(operation, 0, format!("invalid integer: {error}")))
}

fn parse_failure(operation: &'static str, offset: usize, message: impl Into<String>) -> GitError {
    GitError::Parse {
        operation,
        offset,
        message: message.into(),
    }
}

fn parse_error<T>(
    operation: &'static str,
    offset: usize,
    message: impl Into<String>,
) -> Result<T, GitError> {
    Err(parse_failure(operation, offset, message))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "0123456789012345678901234567890123456789";

    fn same_path_identity(path: &[u8]) -> Vec<DiffFileIdentity> {
        vec![DiffFileIdentity {
            old_path: Some(GitPath::new(path.to_vec())),
            new_path: Some(GitPath::new(path.to_vec())),
        }]
    }

    #[test]
    fn strictly_parses_branch_counts() {
        assert!(parse_status(b"# branch.ab +2 -3\0", ObjectFormat::Sha1).is_ok());
        assert!(parse_status(b"# branch.ab ++2 -3\0", ObjectFormat::Sha1).is_err());
        assert!(parse_status(b"# branch.ab +2 -+3\0", ObjectFormat::Sha1).is_err());
        assert!(parse_status(b"# branch.ab +2 -3 extra\0", ObjectFormat::Sha1).is_err());
    }

    #[test]
    fn parses_rename_status_with_spaces() {
        let input = format!(
            "# branch.oid {SHA}\0# branch.head main\02 R. N... 100644 100644 100644 {SHA} {SHA} R100 new name\0old name\0"
        );
        let status = parse_status(input.as_bytes(), ObjectFormat::Sha1).unwrap();
        assert_eq!(status.entries.len(), 1);
        assert_eq!(status.entries[0].path.bytes(), b"new name");
        assert_eq!(
            status.entries[0].original_path.as_ref().unwrap().bytes(),
            b"old name"
        );
    }

    #[test]
    fn parses_all_unmerged_stages() {
        let input = format!(
            "u UU N... 100644 100755 100600 100640 {0} {1} {2} conflict file\0",
            "1".repeat(40),
            "2".repeat(40),
            "3".repeat(40)
        );
        let status = parse_status(input.as_bytes(), ObjectFormat::Sha1).unwrap();
        let stages = status.entries[0].conflict.as_ref().unwrap();
        assert_eq!(stages.base.mode, "100644");
        assert_eq!(stages.ours.mode, "100755");
        assert_eq!(stages.theirs.mode, "100600");
        assert_eq!(stages.worktree_mode, "100640");
        assert_eq!(stages.base.oid.as_ref().unwrap().hex, "1".repeat(40));
        assert_eq!(stages.ours.oid.as_ref().unwrap().hex, "2".repeat(40));
        assert_eq!(stages.theirs.oid.as_ref().unwrap().hex, "3".repeat(40));
    }

    #[test]
    fn parses_patch_anchors_and_sanitizes_escapes() {
        let input = b"diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1,2 @@\n-old\n+new\x1b\n+x\n";
        let diff = parse_diff(input, &same_path_identity(b"a"), false).unwrap();
        assert_eq!(diff.files.len(), 1);
        assert_eq!(diff.files[0].hunks[0].new_lines, 2);
        assert!(diff.lines[5].text.contains("\\e"));
    }

    #[test]
    fn hunk_content_that_looks_like_file_headers_stays_content() {
        let input = b"diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1,2 +1,2 @@\n--- removed content\n+++ added content\n";
        let diff = parse_diff(input, &same_path_identity(b"a"), false).unwrap();
        assert_eq!(diff.files[0].old_path.as_ref().unwrap().bytes(), b"a");
        assert_eq!(diff.files[0].new_path.as_ref().unwrap().bytes(), b"a");
        assert_eq!(diff.lines[4].kind, DiffLineKind::Removed);
        assert_eq!(diff.lines[5].kind, DiffLineKind::Added);
    }

    #[test]
    fn records_multiple_hunks_after_entering_hunk_state() {
        let input = b"diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n-old\n+new\n@@ -10 +10 @@\n-old ten\n+new ten\n";
        let diff = parse_diff(input, &same_path_identity(b"a"), false).unwrap();
        assert_eq!(diff.files[0].hunks.len(), 2);
        assert_eq!(diff.files[0].hunks[1].old_start, 10);
        assert_eq!(diff.lines[6].kind, DiffLineKind::HunkHeader);
    }

    #[test]
    fn parses_authoritative_raw_diff_paths_including_rename() {
        let ordinary = format!(":100644 100755 {SHA} {SHA} M\0dir b/file\0");
        let files = parse_raw_diff_metadata(ordinary.as_bytes()).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].old_path.as_ref().unwrap().bytes(), b"dir b/file");
        assert_eq!(files[0].new_path.as_ref().unwrap().bytes(), b"dir b/file");

        let rename = format!(":100644 100644 {SHA} {SHA} R087\0old b/name\0new b/name\0");
        let files = parse_raw_diff_metadata(rename.as_bytes()).unwrap();
        assert_eq!(files[0].old_path.as_ref().unwrap().bytes(), b"old b/name");
        assert_eq!(files[0].new_path.as_ref().unwrap().bytes(), b"new b/name");

        let added = format!(":000000 100644 {} {SHA} A\0added\0", "0".repeat(40));
        let files = parse_raw_diff_metadata(added.as_bytes()).unwrap();
        assert!(files[0].old_path.is_none());
        assert_eq!(files[0].new_path.as_ref().unwrap().bytes(), b"added");
        assert!(parse_raw_diff_metadata(b":bad M\0path\0").is_err());
        assert!(parse_raw_diff_metadata(b":100644 100644 a b R100\0old\0").is_err());
    }

    #[test]
    fn aligns_patch_anchors_with_authoritative_file_order() {
        let metadata = format!(
            ":100644 100644 {SHA} {SHA} M\0first b/path\0:100644 100644 {SHA} {SHA} M\0second\0"
        );
        let identities = parse_raw_diff_metadata(metadata.as_bytes()).unwrap();
        let patch = b"diff --git a/ambiguous b/path b/first b/path\nold mode 100644\nnew mode 100755\ndiff --git a/second b/second\nold mode 100644\nnew mode 100755\n";
        let diff = parse_diff(patch, &identities, false).unwrap();
        assert_eq!(diff.files.len(), 2);
        assert_eq!(diff.files[0].header_line, 0);
        assert_eq!(diff.files[1].header_line, 3);
        assert_eq!(
            diff.files[0].old_path.as_ref().unwrap().bytes(),
            b"first b/path"
        );
        assert_eq!(diff.files[1].new_path.as_ref().unwrap().bytes(), b"second");
        let second_header = patch
            .windows(b"diff --git a/second".len())
            .position(|window| window == b"diff --git a/second")
            .unwrap();
        assert!(parse_diff(&patch[..second_header], &identities, false).is_err());
    }

    #[test]
    fn decodes_git_quoted_paths_in_blame() {
        let blame = format!(
            "{SHA} 1 1 1\nauthor A\nauthor-mail <a@b>\nauthor-time 1\nsummary s\nfilename \"bad\\377\"\n\tcontent\n"
        );
        let lines = parse_blame(blame.as_bytes(), ObjectFormat::Sha1).unwrap();
        assert_eq!(lines[0].filename.bytes(), b"bad\xff");
    }

    #[test]
    fn extracts_timezone_from_iso_timestamp() {
        assert_eq!(timezone_from_iso(b"2026-08-19T03:07:02-04:00"), "-04:00");
        assert_eq!(timezone_from_iso(b"2026-08-19T07:07:02Z"), "Z");
    }

    #[test]
    fn caps_adversarial_record_and_diff_line_counts() {
        let mut status = Vec::new();
        for _ in 0..MAX_STRUCTURED_RECORDS {
            status.extend_from_slice(b"? x\0");
        }
        assert_eq!(
            parse_status(&status, ObjectFormat::Sha1)
                .unwrap()
                .entries
                .len(),
            MAX_STRUCTURED_RECORDS
        );
        status.extend_from_slice(b"? x\0");
        assert!(parse_status(&status, ObjectFormat::Sha1).is_err());

        let mut patch = b"diff --git a/a b/a\n@@ -1 +1 @@\n".to_vec();
        for _ in 0..=MAX_DIFF_LINES {
            patch.extend_from_slice(b"+x\n");
        }
        let diff = parse_diff(&patch, &same_path_identity(b"a"), false).unwrap();
        assert!(diff.truncated);
        assert_eq!(diff.lines.len(), MAX_DIFF_LINES);
    }

    #[test]
    fn refs_preserve_bytes_and_sanitize_every_display_field() {
        let line = format!(
            "refs/heads/bad\x1bname\0bad\x1bname\0{}\0\0up\x1bstream\0subject\x1b\01\0*\0\n",
            "a".repeat(40)
        );
        let refs = parse_refs(line.as_bytes(), ObjectFormat::Sha1).unwrap();
        assert_eq!(refs[0].full_name.bytes(), b"refs/heads/bad\x1bname");
        assert!(refs[0].full_name.display().contains("\\e"));
        assert!(refs[0].short_name.display().contains("\\e"));
        assert!(refs[0].upstream.as_ref().unwrap().display().contains("\\e"));
        assert!(refs[0].subject.contains("\\e"));
    }

    #[test]
    fn commit_body_preserves_safe_line_boundaries() {
        let fields: [&[u8]; 13] = [
            b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            b"",
            b"Pat",
            b"pat@example.invalid",
            b"1700000000",
            b"2023-11-14T22:13:20+00:00",
            b"Pat",
            b"pat@example.invalid",
            b"1700000000",
            b"2023-11-14T22:13:20+00:00",
            b"",
            b"subject",
            b"first line\nsecond\x1bline",
        ];
        let mut input = Vec::new();
        for field in fields {
            input.extend_from_slice(field);
            input.push(0);
        }
        input.push(0);
        let commit = parse_commit(&input, ObjectFormat::Sha1).unwrap();
        assert_eq!(commit.body, "first line\nsecond\\eline");
    }

    #[test]
    fn malformed_parsers_return_errors() {
        assert!(parse_status(b"2 broken\0", ObjectFormat::Sha1).is_err());
        assert!(parse_tree(b"bad\0", ObjectFormat::Sha1).is_err());
        assert!(parse_blame(b"nonsense\n", ObjectFormat::Sha1).is_err());
    }
}
