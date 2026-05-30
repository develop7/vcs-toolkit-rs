//! `vcs-github` — automate GitHub from Rust through the `gh` CLI.
//!
//! Thin wrappers that shell out to the GitHub CLI (`gh`) and capture its output.
//! Commands run inside an OS job (via [`vcs_process`]) so a `gh` subprocess is
//! never orphaned.
//!
//! Three layers, from raw to typed:
//! - [`run`] / [`version`] — the original thin string helpers.
//! - [`exec`] — a [`vcs_process::Exec`] preset on the `gh` binary.
//! - typed commands ([`pr_list`], [`issue_list`], [`repo_view`], …) that
//!   deserialize `gh … --json` output into structs ([`PullRequest`], [`Issue`],
//!   [`Repo`]). The repo-scoped ones take a `dir` (`"."` for the current one).

use std::ffi::OsStr;
use std::io;
use std::path::Path;

use vcs_process::Exec;

mod parse;
pub use parse::{Issue, PullRequest, Repo};

/// Name of the underlying CLI binary this crate drives.
pub const BINARY: &str = "gh";

/// Run `gh <args>` and return trimmed stdout on success.
///
/// Fails if the process can't be spawned (e.g. `gh` not on `PATH`) or exits
/// with a non-zero status — stderr is surfaced in the error message.
pub fn run<I, S>(args: I) -> io::Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    vcs_process::run(BINARY, args)
}

/// Return the installed GitHub CLI version (`gh --version`).
pub fn version() -> io::Result<String> {
    run(["--version"])
}

/// A [`vcs_process::Exec`] builder preset to the `gh` binary — set a working
/// directory, env vars, or stdin before running.
pub fn exec() -> Exec {
    Exec::new(BINARY)
}

/// `gh` in `dir` (internal builder for the repo-scoped commands below).
fn at(dir: impl AsRef<Path>) -> Exec {
    Exec::new(BINARY).current_dir(dir)
}

/// Whether the user is authenticated (`gh auth status` exits zero).
pub fn auth_status() -> io::Result<bool> {
    Ok(Exec::new(BINARY)
        .args(["auth", "status"])
        .output()?
        .success())
}

/// The repository for `dir` (`gh repo view --json …`).
pub fn repo_view(dir: impl AsRef<Path>) -> io::Result<Repo> {
    let out = at(dir)
        .args(["repo", "view", "--json", "name,nameWithOwner,description"])
        .run()?;
    parse::from_json(&out)
}

/// Open/closed pull requests for `dir` (`gh pr list --json …`).
pub fn pr_list(dir: impl AsRef<Path>) -> io::Result<Vec<PullRequest>> {
    let out = at(dir)
        .args(["pr", "list", "--json", "number,title,state,headRefName"])
        .run()?;
    parse::from_json(&out)
}

/// A single pull request by number (`gh pr view <n> --json …`).
pub fn pr_view(dir: impl AsRef<Path>, number: u64) -> io::Result<PullRequest> {
    let out = at(dir)
        .args([
            "pr",
            "view",
            &number.to_string(),
            "--json",
            "number,title,state,headRefName",
        ])
        .run()?;
    parse::from_json(&out)
}

/// Issues for `dir` (`gh issue list --json …`).
pub fn issue_list(dir: impl AsRef<Path>) -> io::Result<Vec<Issue>> {
    let out = at(dir)
        .args(["issue", "list", "--json", "number,title,state"])
        .run()?;
    parse::from_json(&out)
}

/// Call the GitHub REST/GraphQL API and return the raw JSON body (`gh api`).
pub fn api(endpoint: &str) -> io::Result<String> {
    Exec::new(BINARY).args(["api", endpoint]).run()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_name_is_gh() {
        assert_eq!(BINARY, "gh");
    }

    // Requires the `gh` binary on PATH, so it's ignored by default and not
    // exercised in CI. Run locally with `cargo test -- --ignored`.
    #[test]
    #[ignore = "requires the gh binary to be installed"]
    fn version_mentions_gh() {
        let v = version().expect("gh should be installed");
        assert!(v.to_lowercase().contains("gh"), "unexpected output: {v}");
    }
}
