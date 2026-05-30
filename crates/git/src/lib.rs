//! `vcs-git` — automate Git from Rust through CLI process execution.
//!
//! Thin wrappers that shell out to the `git` binary and capture its output.
//! Commands run inside an OS job (via [`vcs_process`]) so a `git` subprocess is
//! never orphaned.
//!
//! Three layers, from raw to typed:
//! - [`run`] / [`version`] — the original thin string helpers.
//! - [`exec`] — a [`vcs_process::Exec`] preset on the `git` binary, for a
//!   working directory, env vars, or stdin.
//! - typed, repo-scoped commands ([`status`], [`log`], [`branches`], …) that
//!   take a `dir` and return parsed structs ([`StatusEntry`], [`Commit`],
//!   [`Branch`]). Pass `"."` to operate on the current directory.

use std::ffi::OsStr;
use std::io;
use std::path::Path;

use vcs_process::Exec;

mod parse;
pub use parse::{Branch, Commit, StatusEntry};

/// Name of the underlying CLI binary this crate drives.
pub const BINARY: &str = "git";

/// Run `git <args>` and return trimmed stdout on success.
///
/// Fails if the process can't be spawned (e.g. `git` not on `PATH`) or exits
/// with a non-zero status — stderr is surfaced in the error message.
pub fn run<I, S>(args: I) -> io::Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    vcs_process::run(BINARY, args)
}

/// Return the installed Git version (`git --version`).
pub fn version() -> io::Result<String> {
    run(["--version"])
}

/// A [`vcs_process::Exec`] builder preset to the `git` binary — set a working
/// directory, env vars, or stdin before running.
pub fn exec() -> Exec {
    Exec::new(BINARY)
}

/// `git` in `dir` (internal builder for the typed commands below).
fn at(dir: impl AsRef<Path>) -> Exec {
    Exec::new(BINARY).current_dir(dir)
}

/// Initialise a new repository in `dir` (`git init`).
pub fn init(dir: impl AsRef<Path>) -> io::Result<()> {
    at(dir).arg("init").run().map(drop)
}

/// Working-tree status as parsed `git status --porcelain` entries.
pub fn status(dir: impl AsRef<Path>) -> io::Result<Vec<StatusEntry>> {
    let out = at(dir).args(["status", "--porcelain"]).run()?;
    Ok(parse::parse_porcelain(&out))
}

/// The current branch name (`git rev-parse --abbrev-ref HEAD`).
pub fn current_branch(dir: impl AsRef<Path>) -> io::Result<String> {
    at(dir).args(["rev-parse", "--abbrev-ref", "HEAD"]).run()
}

/// Local branches, with the checked-out one flagged (`git branch`).
pub fn branches(dir: impl AsRef<Path>) -> io::Result<Vec<Branch>> {
    let out = at(dir).arg("branch").run()?;
    Ok(parse::parse_branches(&out))
}

/// The latest `max` commits (`git log -n<max>`), newest first.
pub fn log(dir: impl AsRef<Path>, max: usize) -> io::Result<Vec<Commit>> {
    let out = at(dir)
        .args(["log", &format!("-n{max}"), "--format=%H%x1f%an%x1f%s"])
        .run()?;
    Ok(parse::parse_log(&out))
}

/// Resolve a revision to a full hash (`git rev-parse <rev>`).
pub fn rev_parse(dir: impl AsRef<Path>, rev: &str) -> io::Result<String> {
    at(dir).args(["rev-parse", rev]).run()
}

/// Stage `paths` (`git add -- <paths>`).
pub fn add<I, S>(dir: impl AsRef<Path>, paths: I) -> io::Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    at(dir).arg("add").arg("--").args(paths).run().map(drop)
}

/// Commit staged changes with `message` (`git commit -m`).
pub fn commit(dir: impl AsRef<Path>, message: &str) -> io::Result<()> {
    at(dir).args(["commit", "-m", message]).run().map(drop)
}

/// Whether the working tree has no unstaged changes (`git diff --quiet`).
///
/// `git diff --quiet` exits 0 when clean and 1 when there are differences; any
/// other code (e.g. 128 outside a repository) is surfaced as an error rather
/// than misreported as "dirty".
pub fn diff_is_empty(dir: impl AsRef<Path>) -> io::Result<bool> {
    let out = at(dir).args(["diff", "--quiet"]).output()?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_name_is_git() {
        assert_eq!(BINARY, "git");
    }

    // Requires the `git` binary on PATH, so it's ignored by default and not
    // exercised in CI. Run locally with `cargo test -- --ignored`.
    #[test]
    #[ignore = "requires the git binary to be installed"]
    fn version_mentions_git() {
        let v = version().expect("git should be installed");
        assert!(v.to_lowercase().contains("git"), "unexpected output: {v}");
    }
}
