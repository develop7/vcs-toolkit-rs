//! `vcs-gitea` — automate Gitea (and Forgejo) from Rust through the `tea` CLI.
//!
//! Async, mockable, and structured-error: consumers depend on the [`GiteaApi`]
//! trait and substitute a mock for the real [`Gitea`] client in tests. Commands
//! run inside an OS job (via [`processkit`]) so a `tea` subprocess is never
//! orphaned, and honour an optional [timeout](Gitea::default_timeout).
//!
//! The surface is the **lean pull-request lifecycle** `tea` actually supports —
//! auth, and PR list / view / create / merge / close — deserializing
//! `tea … --output json` (the Gitea REST shape `tea` marshals). It is
//! deliberately narrower than [`vcs-github`](https://crates.io/crates/vcs-github)
//! / [`vcs-gitlab`](https://crates.io/crates/vcs-gitlab): `tea` has **no**
//! single-PR `view`, **no** current-repo view, **no** draft toggle, and **no** PR
//! checks command, so those operations are simply absent here (the
//! [`vcs-forge`](https://crates.io/crates/vcs-forge) facade reports them as
//! `Unsupported` for the Gitea backend). `pr_view` is synthesized by listing and
//! filtering.
//!
//! Two test seams: enable the `mock` feature for a `mockall`-generated
//! `MockGiteaApi`, or inject a fake runner with
//! `Gitea::with_runner(`[`ScriptedRunner`](processkit::ScriptedRunner)`)`.

use std::path::Path;

use processkit::ProcessRunner;
// Re-export the processkit types in this crate's public API (also brings
// `Error`/`Result`/`ProcessResult` into scope here).
pub use processkit::{Error, ProcessResult, Result};

mod parse;
pub use parse::PullRequest;

/// Name of the underlying CLI binary this crate drives.
///
/// Note on injection safety: like `vcs-gitlab`, the lean surface has **no bare
/// positional string slot** for a caller value — PR numbers are `u64`, the
/// title/body/branch arguments ride in flag-VALUE positions, and `run` is the
/// caller-owns-the-argv escape hatch. So there is nothing here to guard with
/// `vcs_cli_support::reject_flag_like`.
pub const BINARY: &str = "tea";

/// How [`GiteaApi::pr_merge`] merges the PR — maps to `tea pr merge --style`
/// (Gitea's default is a merge commit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MergeStrategy {
    /// A merge commit (`--style merge`).
    Merge,
    /// Squash the commits into one (`--style squash`).
    Squash,
    /// Rebase the source onto the target (`--style rebase`).
    Rebase,
}

impl MergeStrategy {
    /// The `tea pr merge --style` value for this strategy.
    fn style(self) -> &'static str {
        match self {
            MergeStrategy::Merge => "merge",
            MergeStrategy::Squash => "squash",
            MergeStrategy::Rebase => "rebase",
        }
    }
}

/// The Gitea operations this crate exposes — the interface consumers code
/// against and mock in tests. The **lean PR lifecycle** `tea` supports; reach
/// unmodelled `tea` commands through [`run`](GiteaApi::run).
#[cfg_attr(feature = "mock", mockall::automock)]
#[async_trait::async_trait]
pub trait GiteaApi: Send + Sync {
    /// Run `tea <args>`, returning trimmed stdout (throws on a non-zero exit).
    async fn run(&self, args: &[String]) -> Result<String>;
    /// Like [`GiteaApi::run`] but never errors on a non-zero exit — returns the
    /// captured [`ProcessResult`].
    async fn run_raw(&self, args: &[String]) -> Result<ProcessResult<String>>;
    /// Installed Gitea CLI version (`tea --version`).
    async fn version(&self) -> Result<String>;
    /// Whether at least one login is configured (`tea login list --output json`
    /// is a non-empty array). `tea` has no per-instance `auth status`, so this is
    /// the closest "are we logged in" signal. Must not error on an unusual
    /// outcome: a non-zero exit (e.g. no config file yet) reads as `false`, the
    /// same as an empty array; only a spawn failure or timeout errors.
    async fn auth_status(&self) -> Result<bool>;
    /// Open pull requests for `dir` (`tea pr list --limit 100 --output json`).
    /// Returns up to 100 open PRs; use [`run`](GiteaApi::run) for more.
    async fn pr_list(&self, dir: &Path) -> Result<Vec<PullRequest>>;
    /// A single pull request by number. `tea` has no single-PR view, so this
    /// **lists** (`tea pr list --state all --limit 999 --output json`) and filters
    /// by number; a missing number is an [`Error::Parse`]. The high `--limit`
    /// guards against a false "not found", but PRs beyond the first 999 are still
    /// not found.
    async fn pr_view(&self, dir: &Path, number: u64) -> Result<PullRequest>;
    /// Open a pull request, returning the command's output (`tea pr create`).
    /// Unlike `gh`/`glab`, `tea` prints a textual summary on success, **not** the
    /// new PR's URL (it has no `--output`/`--fields` flag to shape create output),
    /// so do not parse this as a URL. `head` (the source branch; `None` = the
    /// current branch) and `base` (the target; `None` = the repo default) are
    /// owned `Option<String>`s to keep the trait `mockall`-friendly.
    async fn pr_create(
        &self,
        dir: &Path,
        title: &str,
        body: &str,
        head: Option<String>,
        base: Option<String>,
    ) -> Result<String>;
    /// Merge a pull request (`tea pr merge <number> --style merge|rebase|squash`)
    /// — see [`MergeStrategy`].
    async fn pr_merge(&self, dir: &Path, number: u64, strategy: MergeStrategy) -> Result<()>;
    /// Close a pull request without merging (`tea pr close <number>`).
    async fn pr_close(&self, dir: &Path, number: u64) -> Result<()>;
}

processkit::cli_client!(
    /// The real Gitea client. Generic over the [`ProcessRunner`] so tests can
    /// inject a fake process executor; `Gitea::new()` uses the real job-backed
    /// runner.
    pub struct Gitea => BINARY
);

#[async_trait::async_trait]
impl<R: ProcessRunner> GiteaApi for Gitea<R> {
    async fn run(&self, args: &[String]) -> Result<String> {
        self.core.text(self.core.command(args)).await
    }

    async fn run_raw(&self, args: &[String]) -> Result<ProcessResult<String>> {
        self.core.capture(self.core.command(args)).await
    }

    async fn version(&self) -> Result<String> {
        self.core.text(self.core.command(["--version"])).await
    }

    async fn auth_status(&self) -> Result<bool> {
        // `tea login list --output json` is a global (non-repo) command that
        // yields the configured logins as a JSON array; non-empty ⇒ logged in.
        // `capture` (not `text`) so a NON-ZERO exit — e.g. tea erroring because no
        // config file exists yet — reads as "not logged in" rather than surfacing
        // as an error; a spawn failure or timeout still errors via `ensure_success`.
        let res = self
            .core
            .capture(self.core.command(["login", "list", "--output", "json"]))
            .await?;
        if res.code() != Some(0) {
            // A timeout / signal-kill (no exit code) is a genuine failure;
            // `ensure_success` surfaces it as `Error::Timeout`/IO. A plain
            // non-zero exit, however, just means "no logins" → false.
            if res.code().is_none() {
                res.ensure_success()?;
            }
            return Ok(false);
        }
        let json = res.stdout().trim();
        // Treat empty output as "no logins" rather than a parse error — some tea
        // builds print nothing (not `[]`) when none are configured.
        if json.is_empty() {
            return Ok(false);
        }
        let logins: Vec<serde_json::Value> = parse::from_json(json)?;
        Ok(!logins.is_empty())
    }

    async fn pr_list(&self, dir: &Path) -> Result<Vec<PullRequest>> {
        // `--limit 100` overrides tea's default page size (30), which would
        // otherwise silently truncate the list.
        self.core
            .try_parse(
                self.core
                    .command_in(dir, ["pr", "list", "--limit", "100", "--output", "json"]),
                parse::parse_pr_list,
            )
            .await
    }

    async fn pr_view(&self, dir: &Path, number: u64) -> Result<PullRequest> {
        // `tea` has no single-PR view; list all states and filter by number. A
        // high `--limit` is essential here: without it, tea's default page size
        // (30) would make any PR past the first page a false "not found".
        let prs = self
            .core
            .try_parse(
                self.core.command_in(
                    dir,
                    [
                        "pr", "list", "--state", "all", "--limit", "999", "--output", "json",
                    ],
                ),
                parse::parse_pr_list,
            )
            .await?;
        prs.into_iter()
            .find(|pr| pr.number == number)
            .ok_or_else(|| Error::Parse {
                program: BINARY.to_string(),
                message: format!("no pull request #{number} in `tea pr list`"),
            })
    }

    async fn pr_create(
        &self,
        dir: &Path,
        title: &str,
        body: &str,
        head: Option<String>,
        base: Option<String>,
    ) -> Result<String> {
        let mut args = vec!["pr", "create", "--title", title, "--description", body];
        if let Some(head) = head.as_deref() {
            args.push("--head");
            args.push(head);
        }
        if let Some(base) = base.as_deref() {
            args.push("--base");
            args.push(base);
        }
        self.core.text(self.core.command_in(dir, args)).await
    }

    async fn pr_merge(&self, dir: &Path, number: u64, strategy: MergeStrategy) -> Result<()> {
        let n = number.to_string();
        self.core
            .unit(self.core.command_in(
                dir,
                ["pr", "merge", n.as_str(), "--style", strategy.style()],
            ))
            .await
    }

    async fn pr_close(&self, dir: &Path, number: u64) -> Result<()> {
        let n = number.to_string();
        self.core
            .unit(self.core.command_in(dir, ["pr", "close", n.as_str()]))
            .await
    }
}

impl<R: ProcessRunner> Gitea<R> {
    /// Run `tea <args>` over string slices — `tea.run_args(&["pr", "list"])`
    /// without allocating a `Vec<String>`. Inherent (not on the object-safe
    /// trait), so it can take `&[&str]`; forwards to the same path as
    /// [`GiteaApi::run`].
    pub async fn run_args(&self, args: &[&str]) -> Result<String> {
        self.core.text(self.core.command(args)).await
    }

    /// Like [`run_args`](Gitea::run_args) but never errors on a non-zero exit
    /// (mirrors [`GiteaApi::run_raw`]).
    pub async fn run_raw_args(&self, args: &[&str]) -> Result<ProcessResult<String>> {
        self.core.capture(self.core.command(args)).await
    }

    /// Bind a working directory, so the repo-scoped methods omit that argument:
    /// `tea.at(dir).pr_list()` runs [`pr_list`](GiteaApi::pr_list) against `dir`.
    pub fn at<'a>(&'a self, dir: &'a Path) -> GiteaAt<'a, R> {
        GiteaAt { tea: self, dir }
    }
}

/// A [`Gitea`] client with a working directory bound, so its repo-scoped methods
/// drop the leading `dir` argument (`tea.at(dir).pr_list()`). Construct one with
/// [`Gitea::at`].
pub struct GiteaAt<'a, R: ProcessRunner = processkit::JobRunner> {
    tea: &'a Gitea<R>,
    dir: &'a Path,
}

// Hand-written rather than derived: holding only references, the view is `Copy`
// for *every* runner. `#[derive(Copy)]` would add a spurious `R: Copy` bound the
// default `JobRunner` doesn't satisfy, silently dropping `Copy` on the handle.
impl<R: ProcessRunner> Clone for GiteaAt<'_, R> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<R: ProcessRunner> Copy for GiteaAt<'_, R> {}

/// Generate [`GiteaAt`] forwarders: `bare` methods forward verbatim, `dir`
/// methods inject `self.dir` as the first argument.
macro_rules! gitea_at_forwarders {
    (
        bare { $( fn $bn:ident( $($ba:ident: $bt:ty),* $(,)? ) -> $br:ty; )* }
        dir  { $( fn $dn:ident( $($da:ident: $dt:ty),* $(,)? ) -> $dr:ty; )* }
    ) => {
        impl<'a, R: ProcessRunner> GiteaAt<'a, R> {
            $(
                #[doc = concat!("Bound form of [`Gitea`]'s `", stringify!($bn), "`.")]
                pub async fn $bn(&self, $($ba: $bt),*) -> $br {
                    self.tea.$bn($($ba),*).await
                }
            )*
            $(
                #[doc = concat!("Bound form of [`Gitea`]'s `", stringify!($dn), "` (with `dir` pre-bound).")]
                pub async fn $dn(&self, $($da: $dt),*) -> $dr {
                    self.tea.$dn(self.dir, $($da),*).await
                }
            )*
        }
    };
}

gitea_at_forwarders! {
    bare {
        fn run(args: &[String]) -> Result<String>;
        fn run_raw(args: &[String]) -> Result<ProcessResult<String>>;
        fn run_args(args: &[&str]) -> Result<String>;
        fn run_raw_args(args: &[&str]) -> Result<ProcessResult<String>>;
        fn version() -> Result<String>;
        fn auth_status() -> Result<bool>;
    }
    dir {
        fn pr_list() -> Result<Vec<PullRequest>>;
        fn pr_view(number: u64) -> Result<PullRequest>;
        fn pr_create(title: &str, body: &str, head: Option<String>, base: Option<String>) -> Result<String>;
        fn pr_merge(number: u64, strategy: MergeStrategy) -> Result<()>;
        fn pr_close(number: u64) -> Result<()>;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use processkit::{RecordingRunner, Reply, ScriptedRunner};

    #[test]
    fn binary_name_is_tea() {
        assert_eq!(BINARY, "tea");
    }

    // Compile-time guard: the bound view stays `Copy` for the default `JobRunner`.
    #[allow(dead_code)]
    fn bound_view_is_copy_for_default_runner() {
        fn assert_copy<T: Copy>() {}
        assert_copy::<GiteaAt<'static, processkit::JobRunner>>();
    }

    // The bound view (`tea.at(dir)`) must produce byte-identical argv to the
    // dir-taking call.
    #[tokio::test]
    async fn bound_view_matches_dir_taking_calls() {
        let dir = Path::new("/repo");
        let rec = RecordingRunner::replying(Reply::ok("[]"));
        let tea = Gitea::with_runner(&rec);

        tea.pr_list(dir).await.unwrap();
        tea.at(dir).pr_list().await.unwrap();
        tea.pr_close(dir, 7).await.unwrap();
        tea.at(dir).pr_close(7).await.unwrap();

        let calls = rec.calls();
        assert_eq!(calls[0].args_str(), calls[1].args_str());
        assert_eq!(calls[2].args_str(), calls[3].args_str());
        assert_eq!(calls[1].cwd.as_deref(), Some(dir.as_os_str()));
    }

    #[tokio::test]
    async fn run_args_forwards_str_slices() {
        let tea = Gitea::with_runner(ScriptedRunner::new().on(["whoami"], Reply::ok("me\n")));
        assert_eq!(tea.run_args(&["whoami"]).await.unwrap(), "me");
    }

    // Hermetic: real pr_list() arg-building + JSON deserialization against canned
    // output — no `tea` binary or network needed, so this runs on CI.
    #[tokio::test]
    async fn pr_list_parses_scripted_json() {
        let json = r#"[{"number":7,"title":"Add X","state":"open","merged":false,"html_url":"u","head":{"ref":"feat/x"},"base":{"ref":"main"}}]"#;
        let tea = Gitea::with_runner(ScriptedRunner::new().on(["pr", "list"], Reply::ok(json)));
        let prs = tea.pr_list(Path::new(".")).await.expect("pr_list");
        assert_eq!(prs.len(), 1);
        assert_eq!(prs[0].number, 7);
        assert_eq!(prs[0].head_branch, "feat/x");
    }

    // pr_view lists all states and filters by number.
    #[tokio::test]
    async fn pr_view_filters_listing_by_number() {
        let json = r#"[
            {"number":7,"title":"Seven","state":"open"},
            {"number":9,"title":"Nine","state":"closed","merged":true}
        ]"#;
        let tea = Gitea::with_runner(ScriptedRunner::new().on(["pr", "list"], Reply::ok(json)));
        let pr = tea.pr_view(Path::new("."), 9).await.expect("pr_view");
        assert_eq!(pr.title, "Nine");
        assert!(pr.merged);
    }

    // pr_view passes `--state all` so a closed/merged PR is found, and a missing
    // number is a parse error rather than a panic.
    #[tokio::test]
    async fn pr_view_requests_all_states_and_errors_when_missing() {
        let rec = RecordingRunner::replying(Reply::ok("[]"));
        let tea = Gitea::with_runner(&rec);
        let err = tea.pr_view(Path::new("/repo"), 5).await.unwrap_err();
        assert!(matches!(err, Error::Parse { .. }));
        assert_eq!(
            rec.only_call().args_str(),
            [
                "pr", "list", "--state", "all", "--limit", "999", "--output", "json"
            ]
        );
    }

    // pr_list pins an explicit `--limit 100` so tea's default page size (30) does
    // not silently truncate the list.
    #[tokio::test]
    async fn pr_list_pins_limit_100() {
        let rec = RecordingRunner::replying(Reply::ok("[]"));
        let tea = Gitea::with_runner(&rec);
        tea.pr_list(Path::new("/repo")).await.expect("pr_list");
        assert_eq!(
            rec.only_call().args_str(),
            ["pr", "list", "--limit", "100", "--output", "json"]
        );
    }

    // auth_status reads the logins array: non-empty ⇒ true, empty ⇒ false.
    #[tokio::test]
    async fn auth_status_counts_logins() {
        let yes = Gitea::with_runner(
            ScriptedRunner::new().on(["login", "list"], Reply::ok(r#"[{"name":"gitea"}]"#)),
        );
        assert!(yes.auth_status().await.unwrap());
        let no = Gitea::with_runner(ScriptedRunner::new().on(["login", "list"], Reply::ok("[]")));
        assert!(!no.auth_status().await.unwrap());
        // Some tea builds print nothing (not `[]`) when no login is configured;
        // that must read as `false`, not a parse error.
        let empty = Gitea::with_runner(ScriptedRunner::new().on(["login", "list"], Reply::ok("")));
        assert!(!empty.auth_status().await.unwrap());
        // A non-zero exit (e.g. tea erroring because no config file exists) must
        // read as "not logged in" — never an error.
        let failed = Gitea::with_runner(
            ScriptedRunner::new().on(["login", "list"], Reply::fail(1, "no config")),
        );
        assert!(!failed.auth_status().await.unwrap());
        let weird =
            Gitea::with_runner(ScriptedRunner::new().on(["login", "list"], Reply::fail(2, "boom")));
        assert!(!weird.auth_status().await.unwrap());
    }

    // A timed-out login check must error, not silently report "not logged in".
    #[tokio::test]
    async fn auth_status_errors_on_timeout() {
        let tea = Gitea::with_runner(ScriptedRunner::new().on(["login", "list"], Reply::timeout()));
        assert!(matches!(
            tea.auth_status().await.unwrap_err(),
            Error::Timeout { .. }
        ));
    }

    // pr_create assembles title/description then optional head/base.
    #[tokio::test]
    async fn pr_create_appends_head_and_base() {
        let rec = RecordingRunner::replying(Reply::ok("#9\n"));
        let tea = Gitea::with_runner(&rec);
        tea.pr_create(
            Path::new("/repo"),
            "T",
            "B",
            Some("feat/x".to_string()),
            Some("main".to_string()),
        )
        .await
        .expect("pr_create");
        assert_eq!(
            rec.only_call().args_str(),
            [
                "pr",
                "create",
                "--title",
                "T",
                "--description",
                "B",
                "--head",
                "feat/x",
                "--base",
                "main"
            ]
        );
    }

    // pr_merge maps the strategy to `--style`; pr_close to `pr close <n>`.
    #[tokio::test]
    async fn pr_merge_and_close_build_expected_argv() {
        let rec = RecordingRunner::replying(Reply::ok(""));
        let tea = Gitea::with_runner(&rec);
        tea.pr_merge(Path::new("/repo"), 5, MergeStrategy::Squash)
            .await
            .expect("merge");
        assert_eq!(
            rec.only_call().args_str(),
            ["pr", "merge", "5", "--style", "squash"]
        );

        let rec = RecordingRunner::replying(Reply::ok(""));
        let tea = Gitea::with_runner(&rec);
        tea.pr_close(Path::new("/repo"), 5).await.expect("close");
        assert_eq!(rec.only_call().args_str(), ["pr", "close", "5"]);
    }

    #[cfg(feature = "mock")]
    #[tokio::test]
    async fn consumer_mocks_the_interface() {
        let mut mock = MockGiteaApi::new();
        mock.expect_auth_status().returning(|| Ok(true));
        assert!(mock.auth_status().await.unwrap());
    }
}
