//! The crate's error type: filesystem-watcher setup failures plus the underlying
//! `vcs-core` re-query errors.

/// An error from setting up or running a [`RepoWatcher`](crate::RepoWatcher).
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The `notify` filesystem watcher failed to start or register a path.
    Notify(notify::Error),
    /// A `vcs-core` query (detection / `snapshot` / `local_branches`) failed —
    /// chiefly while *building* the watcher (capturing the baseline state). A
    /// re-query failure *during* watching is skipped and retried, not surfaced
    /// here (see [`RepoWatcher`](crate::RepoWatcher)).
    Vcs(vcs_core::Error),
    /// A filesystem operation failed (e.g. resolving a worktree gitlink).
    Io(std::io::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Notify(e) => write!(f, "filesystem watch failed: {e}"),
            Error::Vcs(e) => write!(f, "{e}"),
            Error::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Notify(e) => Some(e),
            Error::Vcs(e) => Some(e),
            Error::Io(e) => Some(e),
        }
    }
}

impl From<notify::Error> for Error {
    fn from(e: notify::Error) -> Self {
        Error::Notify(e)
    }
}

impl From<vcs_core::Error> for Error {
    fn from(e: vcs_core::Error) -> Self {
        Error::Vcs(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

/// `Result` specialised to the watcher [`Error`].
pub type Result<T> = std::result::Result<T, Error>;
