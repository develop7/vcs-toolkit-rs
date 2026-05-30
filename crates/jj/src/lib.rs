//! `vcs-jj` — automate Jujutsu (`jj`) from Rust through CLI process execution.
//!
//! Thin wrappers that shell out to the `jj` binary and capture its output.
//! Commands run inside an OS job (via [`vcs_process`]) so a `jj` subprocess is
//! never orphaned.
//!
//! Three layers, from raw to typed:
//! - [`run`] / [`version`] — the original thin string helpers.
//! - [`exec`] — a [`vcs_process::Exec`] preset on the `jj` binary.
//! - typed, repo-scoped commands ([`log`], [`current_change`], [`describe`], …)
//!   that take a `dir` and return parsed structs ([`Change`], [`Bookmark`]).
//!   Pass `"."` to operate on the current directory.

use std::ffi::OsStr;
use std::io;
use std::path::Path;

use vcs_process::Exec;

mod parse;
pub use parse::{Bookmark, Change};

/// Name of the underlying CLI binary this crate drives.
pub const BINARY: &str = "jj";

/// Run `jj <args>` and return trimmed stdout on success.
///
/// Fails if the process can't be spawned (e.g. `jj` not on `PATH`) or exits
/// with a non-zero status — stderr is surfaced in the error message.
pub fn run<I, S>(args: I) -> io::Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    vcs_process::run(BINARY, args)
}

/// Return the installed Jujutsu version (`jj --version`).
pub fn version() -> io::Result<String> {
    run(["--version"])
}

/// A [`vcs_process::Exec`] builder preset to the `jj` binary — set a working
/// directory, env vars, or stdin before running.
pub fn exec() -> Exec {
    Exec::new(BINARY)
}

/// `jj` in `dir` (internal builder for the typed commands below).
fn at(dir: impl AsRef<Path>) -> Exec {
    Exec::new(BINARY).current_dir(dir)
}

/// Working-copy status as jj prints it (`jj status`).
pub fn status(dir: impl AsRef<Path>) -> io::Result<String> {
    at(dir).arg("status").run()
}

/// Changes matching `revset` (newest first), up to `max` (`jj log`).
pub fn log(dir: impl AsRef<Path>, revset: &str, max: usize) -> io::Result<Vec<Change>> {
    let out = at(dir)
        .args([
            "log",
            "-r",
            revset,
            &format!("-n{max}"),
            "--no-graph",
            "-T",
            parse::CHANGE_TEMPLATE,
        ])
        .run()?;
    Ok(parse::parse_changes(&out))
}

/// The working-copy change (`jj log -r @`).
pub fn current_change(dir: impl AsRef<Path>) -> io::Result<Change> {
    let mut changes = log(dir, "@", 1)?;
    changes
        .pop()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no working-copy change found"))
}

/// Set the description of the working-copy change (`jj describe -m`).
pub fn describe(dir: impl AsRef<Path>, message: &str) -> io::Result<()> {
    at(dir).args(["describe", "-m", message]).run().map(drop)
}

/// Start a new change on top of the working copy (`jj new -m`).
pub fn new_change(dir: impl AsRef<Path>, message: &str) -> io::Result<()> {
    at(dir).args(["new", "-m", message]).run().map(drop)
}

/// Local bookmarks (`jj bookmark list`).
pub fn bookmarks(dir: impl AsRef<Path>) -> io::Result<Vec<Bookmark>> {
    let out = at(dir).args(["bookmark", "list"]).run()?;
    Ok(parse::parse_bookmarks(&out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_name_is_jj() {
        assert_eq!(BINARY, "jj");
    }

    // Requires the `jj` binary on PATH, so it's ignored by default and not
    // exercised in CI. Run locally with `cargo test -- --ignored`.
    #[test]
    #[ignore = "requires the jj binary to be installed"]
    fn version_mentions_jj() {
        let v = version().expect("jj should be installed");
        assert!(v.to_lowercase().contains("jj"), "unexpected output: {v}");
    }
}
