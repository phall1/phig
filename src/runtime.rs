use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use crossbeam_channel::{Receiver, SendTimeoutError, Sender, TrySendError, bounded};

use crate::{
    domain::{
        BlameLine, Blob, CommitDetail, Comparison, ComparisonMode, Diff, GitPath, HistoryPage,
        HistoryRange, Oid, RefInfo, Repository, StashEntry, Status, TreeEntry,
    },
    git::{CancellationToken, GitClient, GitError},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RequestKey {
    History,
    Preview,
    Refs,
    Status,
    Tree,
    Blame,
    Stashes,
    Compare,
    Custom(u16),
}

#[derive(Debug, Clone)]
pub enum GitQuery {
    History {
        repository: Repository,
        range: HistoryRange,
        paths: Vec<GitPath>,
        offset: usize,
        limit: usize,
    },
    CommitDetail {
        repository: Repository,
        revision: String,
        parent_index: usize,
        paths: Vec<GitPath>,
    },
    Refs {
        repository: Repository,
    },
    Status {
        repository: Repository,
        include_ignored: bool,
    },
    Tree {
        repository: Repository,
        revision: String,
        path: Option<GitPath>,
    },
    Blob {
        repository: Repository,
        id: Oid,
        path: Option<GitPath>,
    },
    Blame {
        repository: Repository,
        revision: String,
        path: GitPath,
    },
    Stashes {
        repository: Repository,
    },
    Compare {
        repository: Repository,
        base: String,
        head: String,
        mode: ComparisonMode,
        paths: Vec<GitPath>,
    },
    WorkingDiff {
        repository: Repository,
        path: GitPath,
        staged: bool,
    },
}

#[derive(Debug)]
pub enum GitResult {
    History(HistoryPage),
    CommitDetail(CommitDetail),
    Refs(Vec<RefInfo>),
    Status(Status),
    Tree(Vec<TreeEntry>),
    Blob(Blob),
    Blame(Vec<BlameLine>),
    Stashes(Vec<StashEntry>),
    Compare(Comparison),
    Diff(Diff),
}

#[derive(Debug)]
pub struct Response {
    pub key: RequestKey,
    pub generation: u64,
    pub result: Result<GitResult, GitError>,
}

#[derive(Debug, thiserror::Error)]
pub enum CoordinatorError {
    #[error("the Git worker service has stopped")]
    Stopped,
    #[error("the bounded Git request queue is full")]
    Busy,
}

#[derive(Debug)]
struct Request {
    key: RequestKey,
    generation: u64,
    cancellation: CancellationToken,
    query: GitQuery,
}

type ActiveMap = Arc<Mutex<HashMap<RequestKey, (u64, CancellationToken)>>>;

pub struct Coordinator {
    sender: Option<Sender<Request>>,
    responses: Receiver<Response>,
    active: ActiveMap,
    workers: Vec<thread::JoinHandle<()>>,
}

impl Coordinator {
    pub fn new(client: GitClient, worker_count: usize, capacity: usize) -> Self {
        let worker_count = worker_count.clamp(1, 8);
        let capacity = capacity.clamp(1, 128);
        let (sender, requests) = bounded::<Request>(capacity);
        let (response_sender, responses) = bounded::<Response>(capacity);
        let active = Arc::new(Mutex::new(HashMap::new()));
        let workers = (0..worker_count)
            .map(|_| {
                let client = client.clone();
                let requests = requests.clone();
                let response_sender = response_sender.clone();
                let active = Arc::clone(&active);
                thread::spawn(move || worker_loop(client, requests, response_sender, active))
            })
            .collect();
        Self {
            sender: Some(sender),
            responses,
            active,
            workers,
        }
    }

    pub fn submit(&self, key: RequestKey, query: GitQuery) -> Result<u64, CoordinatorError> {
        let mut active = self.active.lock().map_err(|_| CoordinatorError::Stopped)?;
        let generation = active
            .get(&key)
            .map_or(1, |(generation, _)| generation.saturating_add(1));
        let previous = active.get(&key).map(|(_, token)| token.clone());
        let cancellation = CancellationToken::new();
        let request = Request {
            key,
            generation,
            cancellation: cancellation.clone(),
            query,
        };
        match self
            .sender
            .as_ref()
            .ok_or(CoordinatorError::Stopped)?
            .try_send(request)
        {
            Ok(()) => {
                if let Some(previous) = previous {
                    previous.cancel();
                }
                active.insert(key, (generation, cancellation));
                Ok(generation)
            }
            Err(TrySendError::Full(request)) => {
                request.cancellation.cancel();
                Err(CoordinatorError::Busy)
            }
            Err(TrySendError::Disconnected(request)) => {
                request.cancellation.cancel();
                Err(CoordinatorError::Stopped)
            }
        }
    }

    pub fn cancel(&self, key: RequestKey) {
        if let Ok(active) = self.active.lock()
            && let Some((_, token)) = active.get(&key)
        {
            token.cancel();
        }
    }

    /// Cancel a request slot and make every response from its current
    /// generation stale, even if the process races cancellation.
    pub fn invalidate(&self, key: RequestKey) {
        if let Ok(mut active) = self.active.lock()
            && let Some((generation, token)) = active.get_mut(&key)
        {
            token.cancel();
            *generation = generation.saturating_add(1);
        }
    }

    pub fn responses(&self) -> &Receiver<Response> {
        &self.responses
    }

    pub fn try_recv(&self) -> Option<Response> {
        self.responses.try_recv().ok()
    }

    pub fn is_current(&self, key: RequestKey, generation: u64) -> bool {
        self.active
            .lock()
            .ok()
            .and_then(|active| active.get(&key).map(|(current, _)| *current == generation))
            .unwrap_or(false)
    }
}

impl Drop for Coordinator {
    fn drop(&mut self) {
        if let Ok(active) = self.active.lock() {
            for (_, token) in active.values() {
                token.cancel();
            }
        }
        self.sender.take();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

fn worker_loop(
    client: GitClient,
    requests: Receiver<Request>,
    responses: Sender<Response>,
    active: ActiveMap,
) {
    while let Ok(request) = requests.recv() {
        if request.cancellation.is_cancelled() {
            continue;
        }
        let result = execute(&client, &request.query, &request.cancellation);
        let current = active
            .lock()
            .ok()
            .and_then(|active| {
                active
                    .get(&request.key)
                    .map(|(generation, _)| *generation == request.generation)
            })
            .unwrap_or(false);
        if current {
            let mut response = Response {
                key: request.key,
                generation: request.generation,
                result,
            };
            loop {
                match responses.send_timeout(response, Duration::from_millis(50)) {
                    Ok(()) | Err(SendTimeoutError::Disconnected(_)) => break,
                    Err(SendTimeoutError::Timeout(returned)) => {
                        if request.cancellation.is_cancelled() {
                            break;
                        }
                        response = returned;
                    }
                }
            }
        }
    }
}

fn execute(
    client: &GitClient,
    query: &GitQuery,
    cancellation: &CancellationToken,
) -> Result<GitResult, GitError> {
    match query {
        GitQuery::History {
            repository,
            range,
            paths,
            offset,
            limit,
        } => client
            .history(repository, range, paths, *offset, *limit, cancellation)
            .map(GitResult::History),
        GitQuery::CommitDetail {
            repository,
            revision,
            parent_index,
            paths,
        } => client
            .commit_detail(repository, revision, *parent_index, paths, cancellation)
            .map(GitResult::CommitDetail),
        GitQuery::Refs { repository } => client.refs(repository, cancellation).map(GitResult::Refs),
        GitQuery::Status {
            repository,
            include_ignored,
        } => client
            .status(repository, *include_ignored, cancellation)
            .map(GitResult::Status),
        GitQuery::Tree {
            repository,
            revision,
            path,
        } => client
            .tree(repository, revision, path.as_ref(), cancellation)
            .map(GitResult::Tree),
        GitQuery::Blob {
            repository,
            id,
            path,
        } => client
            .blob(repository, id, path.clone(), cancellation)
            .map(GitResult::Blob),
        GitQuery::Blame {
            repository,
            revision,
            path,
        } => client
            .blame(repository, revision, path, cancellation)
            .map(GitResult::Blame),
        GitQuery::Stashes { repository } => client
            .stashes(repository, cancellation)
            .map(GitResult::Stashes),
        GitQuery::Compare {
            repository,
            base,
            head,
            mode,
            paths,
        } => client
            .compare(repository, base, head, *mode, paths, cancellation)
            .map(GitResult::Compare),
        GitQuery::WorkingDiff {
            repository,
            path,
            staged,
        } => client
            .working_diff(repository, path, *staged, cancellation)
            .map(GitResult::Diff),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_increments_and_prior_request_is_cancelled() {
        let coordinator = Coordinator::new(GitClient::default(), 1, 2);
        // Discovery isn't part of the worker; use a repository value that will
        // fail quickly if executed. The generation contract is independent.
        let repository = Repository {
            root: "/definitely/missing".into(),
            worktree: None,
            git_dir: "/definitely/missing".into(),
            bare: true,
            object_format: crate::domain::ObjectFormat::Sha1,
            git_version: "2.45.1".into(),
            head: None,
            branch: None,
        };
        let query = || GitQuery::Refs {
            repository: repository.clone(),
        };
        let first = coordinator.submit(RequestKey::Refs, query()).unwrap();
        let second = coordinator.submit(RequestKey::Refs, query()).unwrap();
        assert_eq!(first, 1);
        assert_eq!(second, 2);
        assert!(!coordinator.is_current(RequestKey::Refs, first));
        assert!(coordinator.is_current(RequestKey::Refs, second));
    }

    #[test]
    fn invalidation_makes_an_in_flight_generation_stale() {
        let coordinator = Coordinator::new(GitClient::default(), 1, 2);
        let repository = Repository {
            root: "/definitely/missing".into(),
            worktree: None,
            git_dir: "/definitely/missing".into(),
            bare: true,
            object_format: crate::domain::ObjectFormat::Sha1,
            git_version: "2.45.1".into(),
            head: None,
            branch: None,
        };
        let generation = coordinator
            .submit(
                RequestKey::Status,
                GitQuery::Status {
                    repository,
                    include_ignored: false,
                },
            )
            .unwrap();
        assert!(coordinator.is_current(RequestKey::Status, generation));
        coordinator.invalidate(RequestKey::Status);
        assert!(!coordinator.is_current(RequestKey::Status, generation));
    }

    #[cfg(unix)]
    #[test]
    fn rejected_submission_preserves_the_last_accepted_generation() {
        use std::{fs, os::unix::fs::PermissionsExt, time::Duration};

        let directory = tempfile::tempdir().unwrap();
        let helper = directory.path().join("fake-git");
        let marker = directory.path().join("started");
        fs::write(
            &helper,
            format!("#!/bin/sh\ntouch '{}'\nsleep 30\n", marker.display()),
        )
        .unwrap();
        fs::set_permissions(&helper, fs::Permissions::from_mode(0o755)).unwrap();
        let runner = crate::git::GitRunner::new(
            helper,
            crate::git::GitLimits {
                stdout_bytes: 1024,
                stderr_bytes: 1024,
                timeout: Duration::from_secs(10),
            },
        );
        let coordinator = Coordinator::new(GitClient::new(runner), 1, 1);
        let repository = Repository {
            root: directory.path().to_path_buf(),
            worktree: None,
            git_dir: directory.path().to_path_buf(),
            bare: true,
            object_format: crate::domain::ObjectFormat::Sha1,
            git_version: "2.45.1".into(),
            head: None,
            branch: None,
        };
        let query = |repository: &Repository| GitQuery::Refs {
            repository: repository.clone(),
        };
        coordinator
            .submit(RequestKey::Custom(1), query(&repository))
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !marker.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(marker.exists(), "fake Git worker did not start");
        let accepted = coordinator
            .submit(RequestKey::Refs, query(&repository))
            .unwrap();
        let rejected = coordinator.submit(RequestKey::Refs, query(&repository));
        assert!(matches!(rejected, Err(CoordinatorError::Busy)));
        assert!(coordinator.is_current(RequestKey::Refs, accepted));
    }
}
