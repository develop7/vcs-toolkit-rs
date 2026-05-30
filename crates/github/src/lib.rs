//! `vcs-github` — automate GitHub from Rust through the `gh` CLI.
//!
//! The API is built for **mockability**: consumers depend on the [`GitHubApi`]
//! trait and substitute a mock for the real [`GitHub`] client in their tests.
//! Commands run inside an OS job (via [`vcs_process`]) so a `gh` subprocess is
//! never orphaned.
//!
//! Two test seams: mock the interface (`mock` feature → `MockGitHubApi`), or
//! inject a [`ScriptedRunner`](vcs_process::ScriptedRunner) via
//! [`GitHub::with_runner`] to drive the real argument-building and JSON parsing
//! against canned output.

use std::io;
use std::path::Path;

use vcs_process::{Exec, JobRunner, Output, Runner};

mod parse;
pub use parse::{Issue, PullRequest, Repo};

/// Name of the underlying CLI binary this crate drives.
pub const BINARY: &str = "gh";

/// The GitHub operations this crate exposes — the interface consumers code
/// against and mock in tests.
#[cfg_attr(feature = "mock", mockall::automock)]
pub trait GitHubApi {
    /// Installed GitHub CLI version (`gh --version`).
    fn version(&self) -> io::Result<String>;
    /// Whether the user is authenticated (`gh auth status` exits zero).
    fn auth_status(&self) -> io::Result<bool>;
    /// The repository for `dir` (`gh repo view --json …`).
    fn repo_view(&self, dir: &Path) -> io::Result<Repo>;
    /// Pull requests for `dir` (`gh pr list --json …`).
    fn pr_list(&self, dir: &Path) -> io::Result<Vec<PullRequest>>;
    /// A single pull request by number (`gh pr view <n> --json …`).
    fn pr_view(&self, dir: &Path, number: u64) -> io::Result<PullRequest>;
    /// Issues for `dir` (`gh issue list --json …`).
    fn issue_list(&self, dir: &Path) -> io::Result<Vec<Issue>>;
    /// Raw GitHub REST/GraphQL response body (`gh api <endpoint>`).
    fn api(&self, endpoint: &str) -> io::Result<String>;
}

/// The real GitHub client. Generic over the [`Runner`] so tests can inject a
/// fake process executor; `GitHub::new()` uses the real job-backed runner.
pub struct GitHub<R: Runner = JobRunner> {
    runner: R,
}

impl GitHub<JobRunner> {
    /// A client backed by the real `gh` binary.
    pub fn new() -> Self {
        GitHub { runner: JobRunner }
    }
}

impl Default for GitHub<JobRunner> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R: Runner> GitHub<R> {
    /// A client that runs commands through `runner` — pass a fake in tests.
    pub fn with_runner(runner: R) -> Self {
        GitHub { runner }
    }

    /// Build and run `gh <args>` (in `dir` if given), returning raw [`Output`].
    fn out(&self, dir: Option<&Path>, args: &[&str]) -> io::Result<Output> {
        let mut exec = Exec::new(BINARY);
        if let Some(dir) = dir {
            exec = exec.current_dir(dir);
        }
        exec = exec.args(args);
        self.runner.run(&exec)
    }

    /// Run and return raw stdout on success, else an error carrying stderr.
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

impl<R: Runner> GitHubApi for GitHub<R> {
    fn version(&self) -> io::Result<String> {
        Ok(self.stdout(None, &["--version"])?.trim().to_string())
    }

    fn auth_status(&self) -> io::Result<bool> {
        Ok(self.out(None, &["auth", "status"])?.success())
    }

    fn repo_view(&self, dir: &Path) -> io::Result<Repo> {
        let out = self.stdout(
            Some(dir),
            &["repo", "view", "--json", "name,nameWithOwner,description"],
        )?;
        parse::from_json(&out)
    }

    fn pr_list(&self, dir: &Path) -> io::Result<Vec<PullRequest>> {
        let out = self.stdout(
            Some(dir),
            &["pr", "list", "--json", "number,title,state,headRefName"],
        )?;
        parse::from_json(&out)
    }

    fn pr_view(&self, dir: &Path, number: u64) -> io::Result<PullRequest> {
        let n = number.to_string();
        let out = self.stdout(
            Some(dir),
            &["pr", "view", &n, "--json", "number,title,state,headRefName"],
        )?;
        parse::from_json(&out)
    }

    fn issue_list(&self, dir: &Path) -> io::Result<Vec<Issue>> {
        let out = self.stdout(
            Some(dir),
            &["issue", "list", "--json", "number,title,state"],
        )?;
        parse::from_json(&out)
    }

    fn api(&self, endpoint: &str) -> io::Result<String> {
        Ok(self.stdout(None, &["api", endpoint])?.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vcs_process::{Output, ScriptedRunner};

    #[test]
    fn binary_name_is_gh() {
        assert_eq!(BINARY, "gh");
    }

    // Hermetic: real pr_list() arg-building + JSON deserialization against canned
    // output — no `gh` binary or network needed, so this runs on CI.
    #[test]
    fn pr_list_parses_scripted_json() {
        let json = r#"[{"number":7,"title":"Add X","state":"OPEN","headRefName":"feat/x"}]"#;
        let gh = GitHub::with_runner(ScriptedRunner::new().on(["pr", "list"], Output::ok(json)));
        let prs = gh.pr_list(Path::new(".")).expect("pr_list");
        assert_eq!(prs.len(), 1);
        assert_eq!(prs[0].number, 7);
        assert_eq!(prs[0].head_ref_name, "feat/x");
    }

    // Hermetic: auth_status reflects the exit code without erroring.
    #[test]
    fn auth_status_reads_exit_code() {
        let yes = GitHub::with_runner(ScriptedRunner::new().on(["auth"], Output::ok("")));
        assert!(yes.auth_status().unwrap());
        let no = GitHub::with_runner(
            ScriptedRunner::new().on(["auth"], Output::fail(1, "not logged in")),
        );
        assert!(!no.auth_status().unwrap());
    }

    #[cfg(feature = "mock")]
    #[test]
    fn consumer_mocks_the_interface() {
        let mut mock = MockGitHubApi::new();
        mock.expect_auth_status().returning(|| Ok(true));
        assert!(mock.auth_status().unwrap());
    }
}
