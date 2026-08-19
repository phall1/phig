use std::{
    ffi::OsStr,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError},
    },
    thread,
    time::{Duration, Instant},
};

use command_group::{CommandGroup, GroupChild};
use thiserror::Error;

use crate::{domain::GitPathError, sanitize::sanitize_bytes};

#[derive(Debug, Clone)]
pub struct GitLimits {
    pub stdout_bytes: usize,
    pub stderr_bytes: usize,
    pub timeout: Duration,
}

impl Default for GitLimits {
    fn default() -> Self {
        Self {
            stdout_bytes: 32 * 1024 * 1024,
            stderr_bytes: 64 * 1024,
            timeout: Duration::from_secs(30),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Debug, Error)]
pub enum GitError {
    #[error("failed to execute Git for {operation}: {source}")]
    Io {
        operation: &'static str,
        #[source]
        source: io::Error,
    },
    #[error(
        "Git operation {operation} failed with exit code {code:?} (output truncated: {truncated}): {stderr}"
    )]
    Failed {
        operation: &'static str,
        code: Option<i32>,
        stderr: String,
        truncated: bool,
    },
    #[error("Git operation {operation} exceeded its {stream} output limit")]
    OutputLimit {
        operation: &'static str,
        stream: &'static str,
    },
    #[error("Git operation {0} timed out")]
    Timeout(&'static str),
    #[error("Git operation {0} was cancelled")]
    Cancelled(&'static str),
    #[error("could not parse {operation} output at byte {offset}: {message}")]
    Parse {
        operation: &'static str,
        offset: usize,
        message: String,
    },
    #[error("{0} is not a Git repository")]
    NotRepository(PathBuf),
    #[error("Git {found} is too old; phig requires Git 2.45.1 or newer")]
    UnsupportedGit { found: String },
    #[error("unsupported repository state: {0}")]
    Unsupported(String),
    #[error("the selected commits have no merge base")]
    NoMergeBase,
    #[error("{platform} is unsupported: {guidance}")]
    UnsupportedPlatform {
        platform: &'static str,
        guidance: &'static str,
    },
    #[error("comparison has {count} merge bases; select an exact base explicitly")]
    AmbiguousMergeBase { count: usize },
    #[error("parent index {requested} is invalid for a commit with {available} parent(s)")]
    InvalidParentIndex { requested: usize, available: usize },
    #[error(transparent)]
    InvalidPath(#[from] GitPathError),
}

#[derive(Debug, Clone)]
pub struct GitOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

#[derive(Debug, Clone)]
pub struct GitRunner {
    executable: PathBuf,
    limits: GitLimits,
}

impl Default for GitRunner {
    fn default() -> Self {
        Self::new(PathBuf::from("git"), GitLimits::default())
    }
}

impl GitRunner {
    pub fn new(executable: PathBuf, limits: GitLimits) -> Self {
        Self { executable, limits }
    }

    pub fn executable(&self) -> &Path {
        &self.executable
    }

    pub fn limits(&self) -> &GitLimits {
        &self.limits
    }

    pub fn run<I, S>(
        &self,
        directory: Option<&Path>,
        operation: &'static str,
        args: I,
        cancellation: &CancellationToken,
    ) -> Result<GitOutput, GitError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        if cancellation.is_cancelled() {
            return Err(GitError::Cancelled(operation));
        }

        let mut command = Command::new(&self.executable);
        if let Some(directory) = directory {
            command.arg("-C").arg(directory);
        }
        command
            .arg("--no-pager")
            .arg("--literal-pathspecs")
            .args([
                "-c",
                "color.ui=false",
                "-c",
                "core.pager=cat",
                "-c",
                "core.fsmonitor=false",
                "-c",
                "core.quotePath=false",
                "-c",
                "diff.mnemonicPrefix=false",
                "-c",
                "diff.noprefix=false",
                "-c",
                "diff.srcPrefix=a/",
                "-c",
                "diff.dstPrefix=b/",
                "-c",
                "diff.external=",
                "-c",
                "diff.trustExitCode=false",
                "-c",
                "log.showSignature=false",
                "-c",
                "fetch.writeCommitGraph=false",
            ])
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("GIT_PAGER", "cat")
            .env("PAGER", "cat")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_NO_LAZY_FETCH", "1")
            .env("GIT_LITERAL_PATHSPECS", "1")
            .env_remove("GIT_GLOB_PATHSPECS")
            .env_remove("GIT_NOGLOB_PATHSPECS")
            .env_remove("GIT_ICASE_PATHSPECS")
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .env("GIT_EXTERNAL_DIFF", "")
            .env("LC_ALL", "C")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_OBJECT_DIRECTORY")
            .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
            .env_remove("GIT_CONFIG_COUNT")
            .env_remove("GIT_CONFIG_KEY_0")
            .env_remove("GIT_CONFIG_VALUE_0");

        let mut child = command
            .group_spawn()
            .map_err(|source| GitError::Io { operation, source })?;
        let stdout = child.inner().stdout.take().ok_or_else(|| GitError::Io {
            operation,
            source: io::Error::other("Git stdout pipe was unavailable"),
        })?;
        let stderr = child.inner().stderr.take().ok_or_else(|| GitError::Io {
            operation,
            source: io::Error::other("Git stderr pipe was unavailable"),
        })?;
        let stdout_reader = spawn_reader(stdout, self.limits.stdout_bytes, false);
        let stderr_reader = spawn_reader(stderr, self.limits.stderr_bytes, true);

        let started = Instant::now();
        let termination = loop {
            if cancellation.is_cancelled() {
                break Some(GitError::Cancelled(operation));
            }
            if started.elapsed() > self.limits.timeout {
                break Some(GitError::Timeout(operation));
            }
            match child.try_wait() {
                Ok(Some(status)) => {
                    break if status.success() {
                        None
                    } else {
                        Some(GitError::Failed {
                            operation,
                            code: status.code(),
                            stderr: String::new(),
                            truncated: false,
                        })
                    };
                }
                Ok(None) => thread::sleep(Duration::from_millis(10)),
                Err(source) => break Some(GitError::Io { operation, source }),
            }
        };

        // One shared deadline covers process-group kill, guaranteed child
        // reaping, and both pipe drains. The reaper owns the child until wait.
        let cleanup_deadline = Instant::now() + Duration::from_secs(1);
        let reaper = spawn_reaper(child);
        let stdout_result = receive_reader(stdout_reader, operation, cleanup_deadline);
        let stderr_result = receive_reader(stderr_reader, operation, cleanup_deadline);
        let reaper_result = receive_reaper(reaper, operation, cleanup_deadline);
        let (stdout, stdout_truncated) = stdout_result?;
        let (stderr, stderr_truncated) = stderr_result?;
        reaper_result?;

        if let Some(error) = termination {
            return Err(match error {
                GitError::Failed {
                    operation, code, ..
                } => GitError::Failed {
                    operation,
                    code,
                    stderr: sanitize_bytes(&stderr),
                    truncated: stdout_truncated || stderr_truncated,
                },
                other => other,
            });
        }
        Ok(GitOutput {
            stdout,
            stderr,
            stdout_truncated,
            stderr_truncated,
        })
    }

    pub fn run_complete<I, S>(
        &self,
        directory: Option<&Path>,
        operation: &'static str,
        args: I,
        cancellation: &CancellationToken,
    ) -> Result<GitOutput, GitError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = self.run(directory, operation, args, cancellation)?;
        if output.stdout_truncated {
            return Err(GitError::OutputLimit {
                operation,
                stream: "stdout",
            });
        }
        if output.stderr_truncated {
            return Err(GitError::OutputLimit {
                operation,
                stream: "stderr",
            });
        }
        Ok(output)
    }

    pub fn run_simple(
        &self,
        directory: Option<&Path>,
        operation: &'static str,
        args: &[&str],
    ) -> Result<GitOutput, GitError> {
        self.run_complete(directory, operation, args, &CancellationToken::new())
    }
}

fn spawn_reader<R: Read + Send + 'static>(
    reader: R,
    limit: usize,
    keep_tail: bool,
) -> Receiver<io::Result<(Vec<u8>, bool)>> {
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let _ = sender.send(read_bounded(reader, limit, keep_tail));
    });
    receiver
}

fn receive_reader(
    receiver: Receiver<io::Result<(Vec<u8>, bool)>>,
    operation: &'static str,
    deadline: Instant,
) -> Result<(Vec<u8>, bool), GitError> {
    match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        Ok(result) => result.map_err(|source| GitError::Io { operation, source }),
        Err(RecvTimeoutError::Timeout) => Err(GitError::Timeout(operation)),
        Err(RecvTimeoutError::Disconnected) => Err(GitError::Io {
            operation,
            source: io::Error::other("Git pipe reader stopped unexpectedly"),
        }),
    }
}

fn spawn_reaper(mut child: GroupChild) -> Receiver<io::Result<()>> {
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let kill = child.kill();
        let waited = child.wait();
        // A failed kill is harmless only when wait proves the child was already
        // reaped. Otherwise retain the most direct cleanup failure.
        let result = match (kill, waited) {
            (_, Ok(_)) => Ok(()),
            (Err(kill_error), Err(_)) => Err(kill_error),
            (Ok(()), Err(wait_error)) => Err(wait_error),
        };
        let _ = sender.send(result);
    });
    receiver
}

fn receive_reaper(
    receiver: Receiver<io::Result<()>>,
    operation: &'static str,
    deadline: Instant,
) -> Result<(), GitError> {
    match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        Ok(result) => result.map_err(|source| GitError::Io { operation, source }),
        Err(RecvTimeoutError::Timeout) => Err(GitError::Timeout(operation)),
        Err(RecvTimeoutError::Disconnected) => Err(GitError::Io {
            operation,
            source: io::Error::other("Git child reaper stopped unexpectedly"),
        }),
    }
}

fn read_bounded<R: Read>(
    mut reader: R,
    limit: usize,
    keep_tail: bool,
) -> io::Result<(Vec<u8>, bool)> {
    let mut output = Vec::with_capacity(limit.min(8192));
    let mut buffer = [0_u8; 8192];
    let mut truncated = false;
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        if keep_tail {
            if count >= limit {
                output.clear();
                output.extend_from_slice(&buffer[count - limit..count]);
                truncated = true;
            } else {
                let overflow = output.len().saturating_add(count).saturating_sub(limit);
                if overflow > 0 {
                    output.drain(..overflow);
                    truncated = true;
                }
                output.extend_from_slice(&buffer[..count]);
            }
        } else if output.len() < limit {
            let remaining = limit - output.len();
            let retained = count.min(remaining);
            output.extend_from_slice(&buffer[..retained]);
            truncated |= retained < count;
        } else {
            truncated = true;
        }
    }
    Ok((output, truncated))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_reader_keeps_prefix_or_tail() {
        let (prefix, truncated) = read_bounded(&b"0123456789"[..], 4, false).unwrap();
        assert_eq!(prefix, b"0123");
        assert!(truncated);
        let (tail, truncated) = read_bounded(&b"0123456789"[..], 4, true).unwrap();
        assert_eq!(tail, b"6789");
        assert!(truncated);
    }
}
