//! Structured error for command execution — carries the program, arguments,
//! exit code, stderr, and timeout flag so callers can react programmatically
//! (the analogue of .NET's `GitCliException`, as a Rust enum).

use std::time::Duration;

/// Why a command run failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CommandError {
    /// The process could not be started (e.g. the binary is not on `PATH`).
    #[error("could not start `{program}`: {source}")]
    Spawn {
        /// The program that failed to launch.
        program: String,
        /// The underlying OS error.
        #[source]
        source: std::io::Error,
    },

    /// The process ran but exited with a non-zero status.
    #[error("`{program} {args}` exited with code {code}: {stderr}")]
    Exit {
        /// The program that was run.
        program: String,
        /// The arguments, space-joined.
        args: String,
        /// The exit code (`-1` if the process was terminated by a signal).
        code: i32,
        /// Trimmed standard error.
        stderr: String,
    },

    /// The process exceeded its timeout and was killed.
    #[error("`{program} {args}` timed out after {timeout:?}")]
    Timeout {
        /// The program that was run.
        program: String,
        /// The arguments, space-joined.
        args: String,
        /// The timeout that elapsed.
        timeout: Duration,
    },

    /// The command succeeded but its output could not be parsed.
    #[error("failed to parse `{program}` output: {message}")]
    Parse {
        /// The program whose output was being parsed.
        program: String,
        /// What went wrong.
        message: String,
    },
}

/// Convenience alias for results that fail with [`CommandError`].
pub type Result<T> = std::result::Result<T, CommandError>;
