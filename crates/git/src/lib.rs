//! `vcs-git` — automate Git from Rust through CLI process execution.
//!
//! The API is built for **mockability**: consumers depend on the [`GitApi`]
//! trait (the interface) and, in their tests, substitute a mock for the real
//! [`Git`] client. Commands run inside an OS job (via [`vcs_process`]) so a
//! `git` subprocess is never orphaned.
//!
//! ```no_run
//! use vcs_git::{Git, GitApi};
//! use std::path::Path;
//!
//! fn report(git: &dyn GitApi) -> std::io::Result<String> {
//!     git.current_branch(Path::new("."))
//! }
//! report(&Git::new())?;
//! # Ok::<(), std::io::Error>(())
//! ```
//!
//! Two test seams:
//! - **Mock the interface** — with the `mock` feature, `mockall` generates
//!   `MockGitApi`; stub whole methods (`expect_status().returning(...)`).
//! - **Inject a runner** — `Git::with_runner(`[`ScriptedRunner`](vcs_process::ScriptedRunner)`)`
//!   feeds canned `git` output through the *real* argument-building and parsing.

use std::io;
use std::path::{Path, PathBuf};

use vcs_process::{Exec, JobRunner, Output, Runner};

mod parse;
pub use parse::{Branch, Commit, StatusEntry};

/// Name of the underlying CLI binary this crate drives.
pub const BINARY: &str = "git";

/// The Git operations this crate exposes — the interface consumers code against
/// and mock in tests.
#[cfg_attr(feature = "mock", mockall::automock)]
pub trait GitApi {
    /// Installed Git version (`git --version`).
    fn version(&self) -> io::Result<String>;
    /// Working-tree status (`git status --porcelain`).
    fn status(&self, dir: &Path) -> io::Result<Vec<StatusEntry>>;
    /// Current branch name (`git rev-parse --abbrev-ref HEAD`).
    fn current_branch(&self, dir: &Path) -> io::Result<String>;
    /// Local branches, current one flagged (`git branch`).
    fn branches(&self, dir: &Path) -> io::Result<Vec<Branch>>;
    /// Latest `max` commits, newest first (`git log`).
    fn log(&self, dir: &Path, max: usize) -> io::Result<Vec<Commit>>;
    /// Resolve a revision to a full hash (`git rev-parse <rev>`).
    fn rev_parse(&self, dir: &Path, rev: &str) -> io::Result<String>;
    /// Initialise a repository (`git init`).
    fn init(&self, dir: &Path) -> io::Result<()>;
    /// Stage `paths` (`git add -- <paths>`). Owned `PathBuf`s keep the trait
    /// object-safe and friendly to `mockall` (no nested-reference lifetimes).
    fn add(&self, dir: &Path, paths: &[PathBuf]) -> io::Result<()>;
    /// Commit staged changes (`git commit -m`).
    fn commit(&self, dir: &Path, message: &str) -> io::Result<()>;
    /// Whether the working tree has no unstaged changes (`git diff --quiet`).
    fn diff_is_empty(&self, dir: &Path) -> io::Result<bool>;
}

/// The real Git client. Generic over the [`Runner`] so tests can inject a fake
/// process executor; `Git::new()` uses the real job-backed runner.
pub struct Git<R: Runner = JobRunner> {
    runner: R,
}

impl Git<JobRunner> {
    /// A client backed by the real `git` binary.
    pub fn new() -> Self {
        Git { runner: JobRunner }
    }
}

impl Default for Git<JobRunner> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R: Runner> Git<R> {
    /// A client that runs commands through `runner` — pass a fake in tests.
    pub fn with_runner(runner: R) -> Self {
        Git { runner }
    }

    /// Build and run `git <args>` (in `dir` if given), returning raw [`Output`].
    fn out(&self, dir: Option<&Path>, args: &[&str]) -> io::Result<Output> {
        let mut exec = Exec::new(BINARY);
        if let Some(dir) = dir {
            exec = exec.current_dir(dir);
        }
        exec = exec.args(args);
        self.runner.run(&exec)
    }

    /// Run and return **raw** stdout on success (no trimming — leading
    /// whitespace is significant for `--porcelain` codes and `branch` markers),
    /// or an `io::Error` carrying stderr on a non-zero exit.
    fn stdout(&self, dir: Option<&Path>, args: &[&str]) -> io::Result<String> {
        let out = self.out(dir, args)?;
        if out.success() {
            Ok(out.stdout)
        } else {
            Err(io::Error::other(format!(
                "`{BINARY}` exited with {}: {}",
                out.status,
                out.stderr.trim()
            )))
        }
    }
}

impl<R: Runner> GitApi for Git<R> {
    fn version(&self) -> io::Result<String> {
        Ok(self.stdout(None, &["--version"])?.trim().to_string())
    }

    fn status(&self, dir: &Path) -> io::Result<Vec<StatusEntry>> {
        let out = self.stdout(Some(dir), &["status", "--porcelain"])?;
        Ok(parse::parse_porcelain(&out))
    }

    fn current_branch(&self, dir: &Path) -> io::Result<String> {
        Ok(self
            .stdout(Some(dir), &["rev-parse", "--abbrev-ref", "HEAD"])?
            .trim()
            .to_string())
    }

    fn branches(&self, dir: &Path) -> io::Result<Vec<Branch>> {
        let out = self.stdout(Some(dir), &["branch"])?;
        Ok(parse::parse_branches(&out))
    }

    fn log(&self, dir: &Path, max: usize) -> io::Result<Vec<Commit>> {
        let n = format!("-n{max}");
        let out = self.stdout(Some(dir), &["log", &n, "--format=%H%x1f%an%x1f%s"])?;
        Ok(parse::parse_log(&out))
    }

    fn rev_parse(&self, dir: &Path, rev: &str) -> io::Result<String> {
        Ok(self
            .stdout(Some(dir), &["rev-parse", rev])?
            .trim()
            .to_string())
    }

    fn init(&self, dir: &Path) -> io::Result<()> {
        self.stdout(Some(dir), &["init"]).map(drop)
    }

    fn add(&self, dir: &Path, paths: &[PathBuf]) -> io::Result<()> {
        let mut exec = Exec::new(BINARY).current_dir(dir).arg("add").arg("--");
        for path in paths {
            exec = exec.arg(path);
        }
        let out = self.runner.run(&exec)?;
        if out.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "`{BINARY}` exited with {}: {}",
                out.status,
                out.stderr.trim()
            )))
        }
    }

    fn commit(&self, dir: &Path, message: &str) -> io::Result<()> {
        self.stdout(Some(dir), &["commit", "-m", message]).map(drop)
    }

    fn diff_is_empty(&self, dir: &Path) -> io::Result<bool> {
        let out = self.out(Some(dir), &["diff", "--quiet"])?;
        match out.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(io::Error::other(format!(
                "`git diff --quiet` failed ({}): {}",
                out.status,
                out.stderr.trim()
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vcs_process::{Output, ScriptedRunner};

    #[test]
    fn binary_name_is_git() {
        assert_eq!(BINARY, "git");
    }

    // Hermetic: the real status() command-building + porcelain parsing run
    // against a scripted runner — no `git` binary needed, so this runs on CI.
    #[test]
    fn status_parses_scripted_output() {
        let git = Git::with_runner(
            ScriptedRunner::new().on(["status", "--porcelain"], Output::ok(" M a.rs\n?? b.rs\n")),
        );
        let entries = git.status(Path::new(".")).expect("status");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].code, " M");
        assert_eq!(entries[1].path, "b.rs");
    }

    // Hermetic: a non-zero exit from a scripted runner surfaces as an error.
    #[test]
    fn nonzero_exit_becomes_error() {
        let git = Git::with_runner(
            ScriptedRunner::new().on(["status"], Output::fail(128, "not a git repository")),
        );
        let err = git.status(Path::new(".")).unwrap_err();
        assert!(err.to_string().contains("not a git repository"), "{err}");
    }

    // Demonstrates the consumer-facing mock seam: a function that depends on
    // `&dyn GitApi` is tested with a generated mock.
    #[cfg(feature = "mock")]
    #[test]
    fn consumer_mocks_the_interface() {
        fn on_branch(git: &dyn GitApi, want: &str) -> io::Result<bool> {
            Ok(git.current_branch(Path::new("."))? == want)
        }
        let mut mock = MockGitApi::new();
        mock.expect_current_branch()
            .returning(|_| Ok("main".to_string()));
        assert!(on_branch(&mock, "main").unwrap());
    }
}
