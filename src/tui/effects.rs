//! Bridge between application effects and bounded Git worker queries.

use std::collections::HashMap;

use crate::{
    app::{App, Effect, RequestKind, View},
    runtime::{Coordinator, CoordinatorError, GitQuery, GitResult, RequestKey, Response},
};

use super::TuiError;

const REQUEST_KEYS: [RequestKey; 8] = [
    RequestKey::History,
    RequestKey::Preview,
    RequestKey::Refs,
    RequestKey::Status,
    RequestKey::Tree,
    RequestKey::Blame,
    RequestKey::Stashes,
    RequestKey::Compare,
];

pub(super) fn invalidate_for_transition(
    coordinator: &Coordinator,
    pending: &mut HashMap<RequestKey, GitQuery>,
    previous: View,
    current: View,
) {
    if view_context(previous) == view_context(current) {
        return;
    }
    for key in REQUEST_KEYS {
        coordinator.invalidate(key);
        pending.remove(&key);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewContext {
    History,
    Status,
    Tree,
    Compare,
    Refs,
    Blame,
    Stash,
}

fn view_context(view: View) -> ViewContext {
    match view {
        View::Log | View::Detail => ViewContext::History,
        View::Status | View::StatusDiff => ViewContext::Status,
        View::Tree | View::Blob => ViewContext::Tree,
        View::Compare => ViewContext::Compare,
        View::Refs => ViewContext::Refs,
        View::Blame => ViewContext::Blame,
        View::Stash => ViewContext::Stash,
    }
}

pub(super) fn apply_response(
    app: &mut App,
    coordinator: &Coordinator,
    response: Response,
    pending: &mut HashMap<RequestKey, GitQuery>,
) -> Result<(), TuiError> {
    if !coordinator.is_current(response.key, response.generation) {
        return Ok(());
    }
    match response.result {
        Ok(GitResult::History(page)) => {
            let effects = app.apply_history(page);
            dispatch_effects(coordinator, app, effects, pending)?;
        }
        Ok(GitResult::CommitDetail(detail)) => app.apply_preview(detail),
        Ok(GitResult::Refs(refs)) => {
            let effects = app.apply_refs(refs);
            dispatch_effects(coordinator, app, effects, pending)?;
        }
        Ok(GitResult::Status(status)) => {
            let effects = app.apply_status(status);
            dispatch_effects(coordinator, app, effects, pending)?;
        }
        Ok(GitResult::Tree(tree)) => app.apply_tree(tree),
        Ok(GitResult::Blob(blob)) => app.apply_blob(blob),
        Ok(GitResult::Blame(blame)) => {
            let effects = app.apply_blame(blame);
            dispatch_effects(coordinator, app, effects, pending)?;
        }
        Ok(GitResult::Stashes(stashes)) => {
            let effects = app.apply_stashes(stashes);
            dispatch_effects(coordinator, app, effects, pending)?;
        }
        Ok(GitResult::Compare(comparison)) => app.apply_comparison(comparison),
        Ok(GitResult::Diff(diff)) => app.apply_working_diff(diff),
        Err(error) => {
            let request = match response.key {
                RequestKey::History => RequestKind::History,
                RequestKey::Preview => RequestKind::Preview,
                _ => RequestKind::Inspect,
            };
            app.apply_error(request, &error);
        }
    }
    Ok(())
}

pub(super) fn dispatch_effects(
    coordinator: &Coordinator,
    app: &App,
    effects: Vec<Effect>,
    pending: &mut HashMap<RequestKey, GitQuery>,
) -> Result<(), TuiError> {
    for effect in effects {
        match effect {
            Effect::LoadHistory { offset, limit } => {
                let query = GitQuery::History {
                    repository: app.repository.clone(),
                    range: app.history_range(),
                    paths: if app.show_mode {
                        Vec::new()
                    } else {
                        app.paths.clone()
                    },
                    offset,
                    limit,
                };
                submit_or_defer(coordinator, pending, RequestKey::History, query)?;
            }
            Effect::LoadPreview {
                revision,
                parent_index,
            } => {
                let query = GitQuery::CommitDetail {
                    repository: app.repository.clone(),
                    revision,
                    parent_index,
                    paths: app.preview_paths(),
                };
                submit_or_defer(coordinator, pending, RequestKey::Preview, query)?;
            }
            Effect::LoadRefs => submit_or_defer(
                coordinator,
                pending,
                RequestKey::Refs,
                GitQuery::Refs {
                    repository: app.repository.clone(),
                },
            )?,
            Effect::LoadStatus => submit_or_defer(
                coordinator,
                pending,
                RequestKey::Status,
                GitQuery::Status {
                    repository: app.repository.clone(),
                    include_ignored: false,
                },
            )?,
            Effect::LoadTree { revision, path } => submit_or_defer(
                coordinator,
                pending,
                RequestKey::Tree,
                GitQuery::Tree {
                    repository: app.repository.clone(),
                    revision,
                    path,
                },
            )?,
            Effect::LoadBlob { id, path } => submit_or_defer(
                coordinator,
                pending,
                RequestKey::Tree,
                GitQuery::Blob {
                    repository: app.repository.clone(),
                    id,
                    path,
                },
            )?,
            Effect::LoadBlame { revision, path } => submit_or_defer(
                coordinator,
                pending,
                RequestKey::Blame,
                GitQuery::Blame {
                    repository: app.repository.clone(),
                    revision,
                    path,
                },
            )?,
            Effect::LoadStashes => submit_or_defer(
                coordinator,
                pending,
                RequestKey::Stashes,
                GitQuery::Stashes {
                    repository: app.repository.clone(),
                },
            )?,
            Effect::LoadCompare => submit_or_defer(
                coordinator,
                pending,
                RequestKey::Compare,
                GitQuery::Compare {
                    repository: app.repository.clone(),
                    base: app.inspect.compare_base.clone(),
                    head: app.inspect.compare_head.clone(),
                    mode: app.inspect.compare_mode,
                    paths: app.paths.clone(),
                },
            )?,
            Effect::LoadWorkingDiff { path, staged } => submit_or_defer(
                coordinator,
                pending,
                RequestKey::Status,
                GitQuery::WorkingDiff {
                    repository: app.repository.clone(),
                    path,
                    staged,
                },
            )?,
        }
    }
    Ok(())
}

pub(super) fn submit_or_defer(
    coordinator: &Coordinator,
    pending: &mut HashMap<RequestKey, GitQuery>,
    key: RequestKey,
    query: GitQuery,
) -> Result<(), TuiError> {
    match coordinator.submit(key, query.clone()) {
        Ok(_) => {
            pending.remove(&key);
            Ok(())
        }
        Err(CoordinatorError::Busy) => {
            // Same-key work is coalesced: invalidate the prior accepted
            // generation before retaining only the newest desired query. A
            // response racing queue saturation must not repopulate stale data.
            coordinator.invalidate(key);
            pending.insert(key, query);
            Ok(())
        }
        Err(error @ CoordinatorError::Stopped) => Err(error.into()),
    }
}

pub(super) fn retry_pending(
    coordinator: &Coordinator,
    pending: &mut HashMap<RequestKey, GitQuery>,
) -> Result<(), TuiError> {
    let queued: Vec<_> = pending
        .iter()
        .map(|(key, query)| (*key, query.clone()))
        .collect();
    for (key, query) in queued {
        submit_or_defer(coordinator, pending, key, query)?;
    }
    Ok(())
}
