//! `vcs-jj` — automate Jujutsu (`jj`) from Rust through CLI process execution.
//!
//! The API is built for **mockability**: consumers depend on the [`JjApi`] trait
//! and substitute a mock for the real [`Jj`] client in their tests. Commands run
//! inside an OS job (via [`vcs_process`]) so a `jj` subprocess is never orphaned.
//!
//! Two test seams: mock the interface (`mock` feature → `MockJjApi`), or inject
//! a [`ScriptedRunner`](vcs_process::ScriptedRunner) via [`Jj::with_runner`] to
//! drive the real argument-building and parsing against canned output.

use std::io;
use std::path::Path;

use vcs_process::{Exec, JobRunner, Output, Runner};

mod parse;
pub use parse::{Bookmark, Change};

/// Name of the underlying CLI binary this crate drives.
pub const BINARY: &str = "jj";

/// The jj operations this crate exposes — the interface consumers code against
/// and mock in tests.
#[cfg_attr(feature = "mock", mockall::automock)]
pub trait JjApi {
    /// Installed Jujutsu version (`jj --version`).
    fn version(&self) -> io::Result<String>;
    /// Working-copy status (`jj status`).
    fn status(&self, dir: &Path) -> io::Result<String>;
    /// Changes matching `revset`, newest first, up to `max` (`jj log`).
    fn log(&self, dir: &Path, revset: &str, max: usize) -> io::Result<Vec<Change>>;
    /// The working-copy change (`jj log -r @`).
    fn current_change(&self, dir: &Path) -> io::Result<Change>;
    /// Set the working-copy change's description (`jj describe -m`).
    fn describe(&self, dir: &Path, message: &str) -> io::Result<()>;
    /// Start a new change on top of the working copy (`jj new -m`).
    fn new_change(&self, dir: &Path, message: &str) -> io::Result<()>;
    /// Local bookmarks (`jj bookmark list`).
    fn bookmarks(&self, dir: &Path) -> io::Result<Vec<Bookmark>>;
}

/// The real jj client. Generic over the [`Runner`] so tests can inject a fake
/// process executor; `Jj::new()` uses the real job-backed runner.
pub struct Jj<R: Runner = JobRunner> {
    runner: R,
}

impl Jj<JobRunner> {
    /// A client backed by the real `jj` binary.
    pub fn new() -> Self {
        Jj { runner: JobRunner }
    }
}

impl Default for Jj<JobRunner> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R: Runner> Jj<R> {
    /// A client that runs commands through `runner` — pass a fake in tests.
    pub fn with_runner(runner: R) -> Self {
        Jj { runner }
    }

    /// Run `jj <args>` in `dir`, returning **raw** stdout on success (no trimming
    /// — separators in templated output are significant) or an error otherwise.
    fn stdout(&self, dir: Option<&Path>, args: &[&str]) -> io::Result<String> {
        let mut exec = Exec::new(BINARY);
        if let Some(dir) = dir {
            exec = exec.current_dir(dir);
        }
        exec = exec.args(args);
        let out: Output = self.runner.run(&exec)?;
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

impl<R: Runner> JjApi for Jj<R> {
    fn version(&self) -> io::Result<String> {
        Ok(self.stdout(None, &["--version"])?.trim().to_string())
    }

    fn status(&self, dir: &Path) -> io::Result<String> {
        Ok(self.stdout(Some(dir), &["status"])?.trim().to_string())
    }

    fn log(&self, dir: &Path, revset: &str, max: usize) -> io::Result<Vec<Change>> {
        let n = format!("-n{max}");
        let out = self.stdout(
            Some(dir),
            &[
                "log",
                "-r",
                revset,
                &n,
                "--no-graph",
                "-T",
                parse::CHANGE_TEMPLATE,
            ],
        )?;
        Ok(parse::parse_changes(&out))
    }

    fn current_change(&self, dir: &Path) -> io::Result<Change> {
        self.log(dir, "@", 1)?
            .pop()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no working-copy change found"))
    }

    fn describe(&self, dir: &Path, message: &str) -> io::Result<()> {
        self.stdout(Some(dir), &["describe", "-m", message])
            .map(drop)
    }

    fn new_change(&self, dir: &Path, message: &str) -> io::Result<()> {
        self.stdout(Some(dir), &["new", "-m", message]).map(drop)
    }

    fn bookmarks(&self, dir: &Path) -> io::Result<Vec<Bookmark>> {
        let out = self.stdout(Some(dir), &["bookmark", "list"])?;
        Ok(parse::parse_bookmarks(&out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vcs_process::{Output, ScriptedRunner};

    #[test]
    fn binary_name_is_jj() {
        assert_eq!(BINARY, "jj");
    }

    // Hermetic: real log() arg-building + template parsing against canned output.
    #[test]
    fn current_change_parses_scripted_output() {
        let jj = Jj::with_runner(
            ScriptedRunner::new().on(["log"], Output::ok("kztuxlro\t38e00654\thello jj\n")),
        );
        let change = jj.current_change(Path::new(".")).expect("current_change");
        assert_eq!(change.change_id, "kztuxlro");
        assert_eq!(change.description, "hello jj");
    }

    #[cfg(feature = "mock")]
    #[test]
    fn consumer_mocks_the_interface() {
        let mut mock = MockJjApi::new();
        mock.expect_describe().returning(|_, _| Ok(()));
        assert!(mock.describe(Path::new("."), "msg").is_ok());
    }
}
