//! The facade's error type: the underlying [`processkit::Error`] the wrapper
//! clients return, plus an [`Unsupported`](Error::Unsupported) variant for an
//! operation a given forge's CLI does not provide.

use crate::ForgeKind;

/// An error from a [`Forge`](crate::Forge) operation.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// An underlying `vcs-github` / `vcs-gitlab` / `vcs-gitea` (i.e. `processkit`)
    /// error.
    Forge(processkit::Error),
    /// The operation isn't available on this forge's CLI — e.g. `repo_view`,
    /// `pr_mark_ready`, and `pr_checks` on Gitea, whose `tea` has no command for
    /// them. The `operation` is the [`ForgeApi`](crate::ForgeApi) method name.
    Unsupported {
        /// Which forge lacks the operation.
        forge: ForgeKind,
        /// The [`ForgeApi`](crate::ForgeApi) method that isn't supported.
        operation: &'static str,
    },
}

impl Error {
    /// Whether this is a **transient** network failure worth retrying (DNS,
    /// connection reset, timeout) — forge commands are network-bound, so a higher
    /// flow may want to retry. Named to match the wrapper classifiers
    /// ([`vcs_cli_support::is_transient_fetch_error`]).
    pub fn is_transient_fetch_error(&self) -> bool {
        matches!(self, Error::Forge(e) if vcs_cli_support::is_transient_fetch_error(e))
    }

    /// Whether this is an [`Unsupported`](Error::Unsupported) operation (rather
    /// than a forge/network failure).
    pub fn is_unsupported(&self) -> bool {
        matches!(self, Error::Unsupported { .. })
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Forge(e) => write!(f, "{e}"),
            Error::Unsupported { forge, operation } => {
                write!(f, "{} does not support `{operation}`", forge.as_str())
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Forge(e) => Some(e),
            Error::Unsupported { .. } => None,
        }
    }
}

impl From<processkit::Error> for Error {
    fn from(e: processkit::Error) -> Self {
        Error::Forge(e)
    }
}

/// `Result` specialised to the facade [`Error`].
pub type Result<T> = std::result::Result<T, Error>;
