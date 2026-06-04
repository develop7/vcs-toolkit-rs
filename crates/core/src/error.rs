//! The facade's error type: a thin wrapper that adds repo-detection failures on
//! top of the underlying [`processkit::Error`] the per-tool clients return.

use std::path::PathBuf;

/// An error from a [`Repo`](crate::Repo) operation.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// [`Repo::open`](crate::Repo::open) found no `.git`/`.jj` from the start dir
    /// up to the filesystem root.
    NotARepository(PathBuf),
    /// A worktree/workspace lookup by path matched no attached worktree.
    WorktreeNotFound(PathBuf),
    /// A filesystem operation failed (e.g. removing a workspace directory).
    Io(std::io::Error),
    /// An underlying `vcs-git` / `vcs-jj` (i.e. `processkit`) error.
    Vcs(processkit::Error),
}

impl Error {
    /// Whether this wraps a merge/rebase **conflict** from the backend — so a
    /// caller can branch on "conflict, resolve it" vs. a hard failure without
    /// matching on [`processkit::Error`] internals. (Recognises git's conflict
    /// markers; jj surfaces conflicts as state, not errors — see
    /// [`Repo::in_progress_state`](crate::Repo::in_progress_state).)
    pub fn is_conflict(&self) -> bool {
        matches!(self, Error::Vcs(e) if vcs_git::is_merge_conflict(e))
    }

    /// Whether this is a benign "nothing to commit" — an empty commit attempt the
    /// caller likely wants to treat as a no-op.
    pub fn is_nothing_to_commit(&self) -> bool {
        matches!(self, Error::Vcs(e) if vcs_git::is_nothing_to_commit(e))
    }

    /// Whether this is a **transient** fetch/network failure worth retrying
    /// (DNS, connection reset, timeout). The underlying clients already retry
    /// their own fetches; this is for retrying higher-level flows.
    pub fn is_transient_fetch(&self) -> bool {
        matches!(self, Error::Vcs(e)
            if vcs_git::is_transient_fetch_error(e) || vcs_jj::is_transient_fetch_error(e))
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotARepository(p) => {
                write!(
                    f,
                    "no git or jj repository found at or above {}",
                    p.display()
                )
            }
            Error::WorktreeNotFound(p) => {
                write!(f, "no worktree found at {}", p.display())
            }
            Error::Io(e) => write!(f, "{e}"),
            Error::Vcs(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            Error::Vcs(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<processkit::Error> for Error {
    fn from(e: processkit::Error) -> Self {
        Error::Vcs(e)
    }
}

/// `Result` specialised to the facade [`Error`].
pub type Result<T> = std::result::Result<T, Error>;
