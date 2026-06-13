//! Git-backed implementations of the facade operations: thin calls to the
//! `vcs-git` client plus pure mappers from its types into the facade DTOs.

use std::path::Path;

use processkit::ProcessRunner;
use vcs_git::{
    CommitEntry, CommitFileChange, Git, GitApi, GitPush, RemoteEntry, StatusEntry, WorktreeAdd,
};

use crate::dto::{
    ChangeKind, CreateOutcome, DiffFormat, DiffOutput, DiffSpec, DiffStat, FileChange, LogEntry,
    LogSpec, MergeProbe, OperationState, RemoteInfo, RefsSnapshot, RepoSnapshot,
    UpstreamTracking, WorktreeInfo,
};
use crate::error::Result;

pub(crate) async fn current_branch<R: ProcessRunner>(
    git: &Git<R>,
    dir: &Path,
) -> Result<Option<String>> {
    // `GitApi::current_branch` already maps a detached HEAD to `None`, mirroring
    // jj's `Option` bookmark — forward it directly.
    Ok(git.current_branch(dir).await?)
}

pub(crate) async fn trunk<R: ProcessRunner>(git: &Git<R>, dir: &Path) -> Result<Option<String>> {
    Ok(git.remote_head_branch(dir).await?)
}

pub(crate) async fn local_branches<R: ProcessRunner>(
    git: &Git<R>,
    dir: &Path,
) -> Result<Vec<String>> {
    Ok(git
        .branches(dir)
        .await?
        .into_iter()
        .map(|b| b.name)
        .collect())
}

pub(crate) async fn branch_exists<R: ProcessRunner>(
    git: &Git<R>,
    dir: &Path,
    name: &str,
) -> Result<bool> {
    Ok(git.branch_exists(dir, name).await?)
}

pub(crate) async fn has_uncommitted_changes<R: ProcessRunner>(
    git: &Git<R>,
    dir: &Path,
) -> Result<bool> {
    Ok(!git.status(dir).await?.is_empty())
}

pub(crate) async fn has_tracked_changes<R: ProcessRunner>(
    git: &Git<R>,
    dir: &Path,
) -> Result<bool> {
    Ok(!git.status_tracked(dir).await?.is_empty())
}

pub(crate) async fn conflicted_files<R: ProcessRunner>(
    git: &Git<R>,
    dir: &Path,
) -> Result<Vec<String>> {
    Ok(git.conflicted_files(dir).await?)
}

pub(crate) async fn delete_branch<R: ProcessRunner>(
    git: &Git<R>,
    dir: &Path,
    name: &str,
    force: bool,
) -> Result<()> {
    git.delete_branch(dir, name, force).await?;
    Ok(())
}

pub(crate) async fn rename_branch<R: ProcessRunner>(
    git: &Git<R>,
    dir: &Path,
    old: &str,
    new: &str,
) -> Result<()> {
    git.rename_branch(dir, old, new).await?;
    Ok(())
}

pub(crate) async fn changed_files<R: ProcessRunner>(
    git: &Git<R>,
    dir: &Path,
) -> Result<Vec<FileChange>> {
    let entries = git.status(dir).await?;
    Ok(entries.into_iter().map(file_change_from_status).collect())
}

pub(crate) async fn diff_stat<R: ProcessRunner>(git: &Git<R>, dir: &Path) -> Result<DiffStat> {
    // Working tree vs the last commit. On an unborn repo `HEAD` doesn't resolve
    // (`git diff HEAD` errors), so stat against the empty tree — a fresh repo's
    // working copy then reports its files as additions instead of hard-failing,
    // matching `changed_files()` (status-based) and `git.diff_text(WorkingTree)`.
    // `git.diff_stat` already returns the shared `vcs_diff::DiffStat` — no remap.
    let range = if git.is_unborn(dir).await? {
        vcs_git::EMPTY_TREE
    } else {
        "HEAD"
    };
    git.diff_stat(dir, range).await.map_err(Into::into)
}

pub(crate) async fn snapshot<R: ProcessRunner>(git: &Git<R>, dir: &Path) -> Result<RepoSnapshot> {
    // 1 spawn: branch + upstream + ahead/behind + change counts (porcelain v2).
    let bs = git.branch_status(dir).await?;
    // 1 spawn: resolve the git dir, then a filesystem probe for an interrupted
    // merge/rebase (porcelain v2 doesn't report it). A git conflict is part of
    // that paused state, so `operation` is Merge/Rebase/Clear here (matching
    // `in_progress_state`); the unresolved-files signal is `conflicted`. Mirrors
    // the client's private `resolved_git_dir` (relative `--git-dir` → join `dir`).
    let raw = git.git_dir(dir).await?;
    let git_dir = if raw.is_absolute() {
        raw
    } else {
        dir.join(raw)
    };
    let operation = if git_dir.join("MERGE_HEAD").exists() {
        OperationState::Merge
    } else if git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists() {
        OperationState::Rebase
    } else {
        OperationState::Clear
    };
    // Derive before moving the String fields out of `bs`.
    let dirty = bs.is_dirty();
    let change_count = bs.tracked_changes + bs.untracked;
    let conflicted = bs.conflicts > 0;
    // Upstream + ahead/behind travel together: git reports the counts only when an
    // upstream is set. Bundle them into one `UpstreamTracking`.
    let ahead = bs.ahead.unwrap_or(0);
    let behind = bs.behind.unwrap_or(0);
    let tracking = bs.upstream.map(|branch| UpstreamTracking {
        branch,
        ahead,
        behind,
    });
    Ok(RepoSnapshot {
        head: bs.head,
        branch: bs.branch,
        tracking,
        dirty,
        change_count,
        conflicted,
        operation,
    })
}

pub(crate) async fn commit_paths<R: ProcessRunner>(
    git: &Git<R>,
    dir: &Path,
    paths: &[String],
    message: &str,
) -> Result<()> {
    git.commit_paths(
        dir,
        vcs_git::CommitPaths::new(paths.iter().map(String::as_str), message),
    )
    .await?;
    Ok(())
}

pub(crate) async fn fetch<R: ProcessRunner>(git: &Git<R>, dir: &Path) -> Result<()> {
    git.fetch(dir).await?;
    Ok(())
}

pub(crate) async fn fetch_from<R: ProcessRunner>(
    git: &Git<R>,
    dir: &Path,
    remote: &str,
) -> Result<()> {
    git.fetch_from(dir, remote).await?;
    Ok(())
}

pub(crate) async fn fetch_remote_branch<R: ProcessRunner>(
    git: &Git<R>,
    dir: &Path,
    branch: &str,
) -> Result<()> {
    git.fetch_remote_branch(dir, branch).await?;
    Ok(())
}

pub(crate) async fn push<R: ProcessRunner>(git: &Git<R>, dir: &Path, branch: &str) -> Result<()> {
    // `-u` so the first facade push also records the upstream — the facade has
    // no separate set-upstream step, and `-u` on later pushes is idempotent.
    git.push(dir, GitPush::branch(branch).set_upstream())
        .await?;
    Ok(())
}

pub(crate) async fn checkout<R: ProcessRunner>(
    git: &Git<R>,
    dir: &Path,
    reference: &str,
) -> Result<()> {
    git.checkout(dir, reference).await?;
    Ok(())
}

pub(crate) async fn rebase<R: ProcessRunner>(git: &Git<R>, dir: &Path, onto: &str) -> Result<()> {
    git.rebase(dir, onto).await?;
    Ok(())
}

pub(crate) async fn try_merge<R: ProcessRunner>(
    git: &Git<R>,
    dir: &Path,
    source: &str,
) -> Result<MergeProbe> {
    // `--no-ff` so even a fast-forwardable merge stages a real (abortable) merge
    // instead of moving HEAD; `--no-commit` so nothing is committed either way.
    let merged = git
        .merge_no_commit(dir, vcs_git::MergeNoCommit::branch(source).no_ff())
        .await;
    match merged {
        Ok(()) => {
            // "Already up to date." exits 0 *without* MERGE_HEAD — `merge
            // --abort` would then fail, so only abort an actually-started merge.
            if git.is_merge_in_progress(dir).await? {
                git.merge_abort(dir).await?;
            }
            Ok(MergeProbe::Clean)
        }
        Err(err) if vcs_git::is_merge_conflict(&err) => {
            // Collect the conflicted paths BEFORE aborting — `merge --abort`
            // clears the unmerged index entries this reads.
            let files = git.conflicted_files(dir).await?;
            // A failed abort breaks the guaranteed-rollback contract → propagate
            // rather than return a `Conflicts` that lies about the tree state.
            git.merge_abort(dir).await?;
            Ok(MergeProbe::Conflicts(files))
        }
        Err(err) => {
            // E.g. a dirty-tree refusal or an unknown ref — the merge usually
            // never started, but clean up if it did.
            if git.is_merge_in_progress(dir).await? {
                git.merge_abort(dir).await?;
            }
            Err(err.into())
        }
    }
}

pub(crate) async fn abort_in_progress<R: ProcessRunner>(
    git: &Git<R>,
    dir: &Path,
) -> Result<OperationState> {
    match in_progress_state(git, dir).await? {
        OperationState::Merge => git.merge_abort(dir).await?,
        OperationState::Rebase => git.rebase_abort(dir).await?,
        _ => {}
    }
    // Recompute rather than assume `Clear` — the return is the *post-call* state.
    in_progress_state(git, dir).await
}

pub(crate) async fn continue_in_progress<R: ProcessRunner>(
    git: &Git<R>,
    dir: &Path,
) -> Result<OperationState> {
    // git refuses to continue while unmerged paths remain; report instead of
    // tripping over the hard error.
    if !git.conflicted_files(dir).await?.is_empty() {
        return Ok(OperationState::Conflict);
    }
    match in_progress_state(git, dir).await? {
        OperationState::Merge => git.merge_continue(dir).await?,
        OperationState::Rebase => {
            // `rebase --continue` exits non-zero when it stops on the NEXT
            // patch's conflict — that's the `Conflict` outcome, not an error.
            if let Err(err) = git.rebase_continue(dir).await {
                if !git.conflicted_files(dir).await?.is_empty() {
                    return Ok(OperationState::Conflict);
                }
                return Err(err.into());
            }
        }
        _ => {}
    }
    // Belt and braces: report any unresolved paths the continue left behind.
    if !git.conflicted_files(dir).await?.is_empty() {
        return Ok(OperationState::Conflict);
    }
    in_progress_state(git, dir).await
}

pub(crate) async fn in_progress_state<R: ProcessRunner>(
    git: &Git<R>,
    dir: &Path,
) -> Result<OperationState> {
    // git surfaces an interrupted operation as on-disk state; a merge and a rebase
    // can't both be live, so report whichever is present.
    if git.is_merge_in_progress(dir).await? {
        Ok(OperationState::Merge)
    } else if git.is_rebase_in_progress(dir).await? {
        Ok(OperationState::Rebase)
    } else {
        Ok(OperationState::Clear)
    }
}

pub(crate) async fn list_worktrees<R: ProcessRunner>(
    git: &Git<R>,
    dir: &Path,
) -> Result<Vec<WorktreeInfo>> {
    let worktrees = git.worktree_list(dir).await?;
    Ok(worktrees
        .into_iter()
        .map(|w| WorktreeInfo {
            path: w.path,
            branch: w.branch,
            commit: w.head,
            is_bare: w.bare,
        })
        .collect())
}

pub(crate) async fn create_worktree<R: ProcessRunner>(
    git: &Git<R>,
    dir: &Path,
    path: &Path,
    branch: &str,
    base: &str,
) -> Result<CreateOutcome> {
    git.worktree_add(dir, WorktreeAdd::create_branch(path, branch, base))
        .await?;
    Ok(CreateOutcome::Plain)
}

pub(crate) async fn remove_worktree<R: ProcessRunner>(
    git: &Git<R>,
    dir: &Path,
    path: &Path,
    force: bool,
) -> Result<()> {
    git.worktree_remove(dir, path, force).await?;
    Ok(())
}

/// Project a `git status --porcelain` entry into a [`FileChange`].
fn file_change_from_status(entry: StatusEntry) -> FileChange {
    FileChange {
        kind: change_kind_from_code(&entry.code),
        path: entry.path,
        old_path: entry.old_path,
    }
}

/// Map a porcelain `XY` status code to a [`ChangeKind`]. Rename wins over the
/// others; an untracked (`??`) or copied (`C`) entry counts as added (a copy is a
/// new file — `parse_porcelain` even records its source as `old_path`, like a
/// rename); unmerged states (`UU`/`AA`/`DD`/…) fold into their underlying kind —
/// use [`conflicted_files`](crate::Repo::conflicted_files) for the conflict signal.
fn change_kind_from_code(code: &str) -> ChangeKind {
    if code.contains('R') {
        ChangeKind::Renamed
    } else if code.contains('D') {
        ChangeKind::Deleted
    } else if code.contains('A') || code.contains('?') || code.contains('C') {
        ChangeKind::Added
    } else {
        ChangeKind::Modified
    }
}

/// Map a `git log --name-status` `XY` code to a [`ChangeKind`]. Same rule as
/// the porcelain mapper but applied to the wrapper's `CommitFileChange`
/// records (the leading `X` carries the index status, the trailing `Y` the
/// worktree — for `--name-status` they tend to agree for non-merge commits).
fn change_kind_from_name_status_code(code: &str) -> ChangeKind {
    if code.contains('R') {
        ChangeKind::Renamed
    } else if code.contains('D') {
        ChangeKind::Deleted
    } else if code.contains('A') || code.contains('C') || code.contains('?') {
        ChangeKind::Added
    } else {
        ChangeKind::Modified
    }
}

fn file_change_from_commit(c: CommitFileChange) -> FileChange {
    FileChange {
        path: c.path,
        old_path: c.old_path,
        kind: change_kind_from_name_status_code(&c.code),
    }
}

pub(crate) async fn log<R: ProcessRunner>(
    git: &Git<R>,
    dir: &Path,
    spec: LogSpec<'_>,
) -> Result<Vec<LogEntry>> {
    let entries = git
        .log_enriched(dir, spec.range, spec.max_count, spec.since, spec.with_files)
        .await?;
    Ok(entries.into_iter().map(log_entry_from_commit).collect())
}

fn log_entry_from_commit(c: CommitEntry) -> LogEntry {
    LogEntry {
        sha: c.hash,
        parents: c.parents,
        author: c.author,
        authored_at: c.authored_at,
        committer: c.committer,
        committed_at: c.committed_at,
        summary: c.summary,
        body: c.body,
        files: c.files.into_iter().map(file_change_from_commit).collect(),
    }
}

pub(crate) async fn diff<R: ProcessRunner>(
    git: &Git<R>,
    dir: &Path,
    spec: DiffSpec<'_>,
) -> Result<DiffOutput> {
    match spec.format {
        DiffFormat::Names => {
            let names = git.diff_names(dir, spec.range, spec.paths).await?;
            Ok(DiffOutput::Names(names))
        }
        DiffFormat::Stat => {
            // The wrapper's `diff_stat` takes a `&str` range; the facade's
            // `None` means "working tree" which the wrapper handles via
            // `git diff --shortstat` (no range arg) on git's side. To keep
            // the call site simple, fall back to `HEAD` (matching the
            // `diff_stat` precedent at `Repo::diff_stat`).
            let range = spec.range.unwrap_or("HEAD");
            let stat = git.diff_stat(dir, range).await?;
            Ok(DiffOutput::Stat(stat))
        }
        DiffFormat::Unified => {
            let text = match spec.range {
                Some(r) => git
                    .diff_text(
                        dir,
                        vcs_git::DiffSpec::Rev(r.to_string()),
                    )
                    .await?,
                None => git.diff_text(dir, vcs_git::DiffSpec::WorkingTree).await?,
            };
            apply_max_bytes_pub(text, spec.max_bytes)
        }
    }
}

pub(crate) fn apply_max_bytes_pub(text: String, max_bytes: Option<usize>) -> Result<DiffOutput> {
    let Some(limit) = max_bytes else {
        return Ok(DiffOutput::Unified {
            text,
            truncated: false,
            omitted_bytes: 0,
        });
    };
    if text.len() <= limit {
        return Ok(DiffOutput::Unified {
            text,
            truncated: false,
            omitted_bytes: 0,
        });
    }
    // The truncation marker is appended at the limit, leaving the cut point
    // *before* the marker (so a caller reading the first N bytes still
    // sees a valid diff prefix). The marker is included in the returned
    // text so a downstream `len() == text.len()` check still holds.
    let mut end = limit;
    // Walk back to a UTF-8 char boundary so the cut doesn't split a
    // multi-byte sequence. The boundary is at-or-before `end`.
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let head = &text[..end];
    let omitted = (text.len() - end) as u64;
    let marker = format!(
        "\n…\n[truncated, {omitted} more bytes]\n"
    );
    Ok(DiffOutput::Unified {
        text: format!("{head}{marker}"),
        truncated: true,
        omitted_bytes: omitted,
    })
}

pub(crate) async fn refs<R: ProcessRunner>(
    git: &Git<R>,
    dir: &Path,
) -> Result<RefsSnapshot> {
    let head_sha = git.rev_parse(dir, "HEAD").await?;
    let current_branch = current_branch(git, dir).await?;
    let default_branch = trunk(git, dir).await?;
    let remotes: Vec<RemoteInfo> = git
        .remote_url_list(dir)
        .await?
        .into_iter()
        .map(remote_info_from_git)
        .collect();
    Ok(RefsSnapshot {
        head_sha,
        current_branch,
        default_branch,
        remotes,
    })
}

fn remote_info_from_git(r: RemoteEntry) -> RemoteInfo {
    RemoteInfo { name: r.name, url: r.url }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_code_maps_to_change_kind() {
        assert_eq!(change_kind_from_code(" M"), ChangeKind::Modified);
        assert_eq!(change_kind_from_code("??"), ChangeKind::Added);
        assert_eq!(change_kind_from_code("A "), ChangeKind::Added);
        assert_eq!(change_kind_from_code(" D"), ChangeKind::Deleted);
        assert_eq!(change_kind_from_code("R "), ChangeKind::Renamed);
        // A copy (only emitted with copy detection on) is a new file, not a modify.
        assert_eq!(change_kind_from_code("C "), ChangeKind::Added);
    }

    #[test]
    fn name_status_code_maps_to_change_kind() {
        assert_eq!(
            change_kind_from_name_status_code("M"),
            ChangeKind::Modified
        );
        assert_eq!(change_kind_from_name_status_code("A"), ChangeKind::Added);
        assert_eq!(change_kind_from_name_status_code("D"), ChangeKind::Deleted);
        assert_eq!(change_kind_from_name_status_code("R"), ChangeKind::Renamed);
        assert_eq!(change_kind_from_name_status_code("C"), ChangeKind::Added);
    }

    #[test]
    fn apply_max_bytes_truncates_at_utf8_boundary() {
        let text = "abcdefghij".to_string();
        let out = apply_max_bytes_pub(text.clone(), Some(5)).expect("truncate");
        match out {
            DiffOutput::Unified { text, truncated, omitted_bytes } => {
                assert!(truncated);
                assert_eq!(omitted_bytes, 5);
                assert!(text.starts_with("abcde"));
                assert!(text.contains("[truncated, 5 more bytes]"));
            }
            other => panic!("expected Unified, got {other:?}"),
        }
    }

    #[test]
    fn apply_max_bytes_no_truncation_when_under_limit() {
        let out = apply_max_bytes_pub("hello".to_string(), Some(100)).expect("no-trunc");
        match out {
            DiffOutput::Unified { text, truncated, omitted_bytes } => {
                assert!(!truncated);
                assert_eq!(omitted_bytes, 0);
                assert_eq!(text, "hello");
            }
            other => panic!("expected Unified, got {other:?}"),
        }
    }

    #[test]
    fn apply_max_bytes_respects_utf8_boundaries() {
        // "αβγδ" is 8 bytes (2 each). Char boundaries: 0, 2, 4, 6, 8.
        // Cutting at byte 3: byte 3 is NOT a boundary (mid-β). The
        // function walks back to byte 2 (start of β), so the head is "α".
        // The test asserts that the cut happens at the *previous* boundary,
        // not at the requested byte.
        let text = "αβγδ".to_string();
        let out = apply_max_bytes_pub(text, Some(3)).expect("utf8");
        match out {
            DiffOutput::Unified { text, truncated, omitted_bytes } => {
                assert!(truncated);
                assert!(text.starts_with("α"));
                // β is dropped (we cut mid-`β`, walked back to "α").
                assert!(!text.contains("β"), "β should be dropped: {text:?}");
                // 6 bytes dropped.
                assert_eq!(omitted_bytes, 6);
            }
            other => panic!("expected Unified, got {other:?}"),
        }
    }

    #[test]
    fn apply_max_bytes_walks_back_when_cut_splits_a_codepoint() {
        // "αβγδ" is 8 bytes (2 each). Char boundaries: 0, 2, 4, 6, 8.
        // Cutting at byte 5: byte 5 is mid-γ, NOT a boundary. The function
        // walks back to byte 4 (start of γ), so the head is "αβ".
        let text = "αβγδ".to_string();
        let out = apply_max_bytes_pub(text, Some(5)).expect("utf8");
        match out {
            DiffOutput::Unified { text, truncated, omitted_bytes } => {
                assert!(truncated);
                assert!(text.starts_with("αβ"));
                // γ and δ are dropped; 4 bytes dropped (γ+δ).
                assert!(!text.contains("γ"), "γ should be dropped: {text:?}");
                assert!(!text.contains("δ"), "δ should be dropped: {text:?}");
                // The head is 4 bytes; the rest is 4 bytes.
                assert_eq!(omitted_bytes, 4);
            }
            other => panic!("expected Unified, got {other:?}"),
        }
    }
}
