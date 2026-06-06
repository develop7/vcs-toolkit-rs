//! Backend-agnostic data types the facade returns, generalising the per-tool
//! shapes of `vcs-git` and `vcs-jj` into one set a consumer can use without
//! knowing which backend is in play.

use std::path::PathBuf;

/// Which version-control tool backs a [`Repo`](crate::Repo).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BackendKind {
    /// A plain Git repository.
    Git,
    /// A Jujutsu repository (possibly colocated with Git).
    Jj,
}

impl BackendKind {
    /// The tool's short name (`"git"` / `"jj"`).
    pub fn as_str(self) -> &'static str {
        match self {
            BackendKind::Git => "git",
            BackendKind::Jj => "jj",
        }
    }
}

/// How a file changed in the working copy — the shared [`vcs_diff::ChangeKind`]
/// (one type across the wrappers and the facade, no remapping). The status-code
/// mappers in the backends turn git's `XY` codes / jj's letters into it.
pub use vcs_diff::ChangeKind;

/// One changed path in the working copy, unified across `git status` /
/// `jj diff --summary`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FileChange {
    /// The path (the *new* path for a rename).
    pub path: String,
    /// The original path for a rename; `None` otherwise (jj never supplies it).
    pub old_path: Option<String>,
    /// How the file changed.
    pub kind: ChangeKind,
}

/// Aggregate insertion/deletion counts for the working copy — the shared
/// [`vcs_diff::DiffStat`], returned by the backends directly (no remapping).
pub use vcs_diff::DiffStat;

/// One attached worktree (git) / workspace (jj).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct WorktreeInfo {
    /// Filesystem path of the worktree's working copy.
    pub path: PathBuf,
    /// The branch (git) or first bookmark (jj) on it; `None` when detached/none.
    pub branch: Option<String>,
    /// The checked-out commit; `None` when unavailable (e.g. a bare git entry).
    pub commit: Option<String>,
    /// A bare git worktree entry (always `false` for jj).
    pub is_bare: bool,
}

/// Whether the working copy is mid-operation, unified across the backends'
/// different models: git exposes an in-progress merge or rebase as on-disk state
/// (`MERGE_HEAD` / a `rebase-*` dir), while jj has no multi-step operations — it
/// records a conflict directly on the working-copy change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum OperationState {
    /// No operation in progress and no conflict.
    Clear,
    /// A git merge is in progress (`MERGE_HEAD` present).
    Merge,
    /// A git rebase is in progress (a `rebase-merge`/`rebase-apply` dir present).
    Rebase,
    /// The working copy has an unresolved conflict (chiefly jj, which records
    /// conflicts on the change rather than pausing an operation).
    Conflict,
}

/// A one-shot snapshot of the common repository state — branch, upstream
/// tracking, ahead/behind, dirtiness, and operation state — gathered in **one or
/// two** process spawns instead of a call per field. The data a prompt, status
/// line, or TUI refresh needs. See [`Repo::snapshot`](crate::Repo::snapshot).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RepoSnapshot {
    /// The working-copy commit's **full** object id (git `HEAD` oid / jj `@`
    /// commit id) on both backends; `None` on an unborn git repo. Truncate for
    /// display.
    pub head: Option<String>,
    /// Current branch (git) / bookmark (jj); `None` when detached or unset.
    pub branch: Option<String>,
    /// Upstream tracking branch; `None` when unset, and **always `None` on jj**
    /// (jj has no git-style upstream tracking).
    pub upstream: Option<String>,
    /// Commits ahead of the upstream; `None` with no upstream (always on jj).
    pub ahead: Option<usize>,
    /// Commits behind the upstream; `None` with no upstream (always on jj).
    pub behind: Option<usize>,
    /// Whether the working copy has any uncommitted change (tracked or untracked).
    pub dirty: bool,
    /// Number of changed paths (tracked + untracked on git; the `@` change's
    /// files on jj).
    pub change_count: usize,
    /// Whether the working copy has an unresolved conflict.
    pub conflicted: bool,
    /// In-progress operation / conflict state (see [`OperationState`]).
    pub operation: OperationState,
}

/// The outcome of a [`try_merge`](crate::Repo::try_merge) probe. The probe
/// itself is rolled back before it returns, whatever the outcome — this only
/// *reports* what a real merge would do.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MergeProbe {
    /// The merge would apply without conflicts.
    Clean,
    /// The merge would conflict in these paths (repo-relative, `/` separators —
    /// the same contract as [`conflicted_files`](crate::Repo::conflicted_files)).
    Conflicts(Vec<String>),
}

impl MergeProbe {
    /// Whether the probe found no conflicts.
    pub fn is_clean(&self) -> bool {
        matches!(self, MergeProbe::Clean)
    }
}

/// How a worktree was materialised. The facade always reports
/// [`Plain`](CreateOutcome::Plain); the [`CowCloned`](CreateOutcome::CowCloned)
/// variant exists so a consumer that layers a copy-on-write strategy on top can
/// reuse this type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CreateOutcome {
    /// The tool materialised the working copy itself.
    Plain,
    /// A copy-on-write clone populated the working copy (consumer-supplied).
    CowCloned,
}
