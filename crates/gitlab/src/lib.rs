//! `vcs-gitlab` — automate GitLab from Rust through the `glab` CLI.
//!
//! Async, mockable, and structured-error: consumers depend on the [`GitLabApi`]
//! trait and substitute a mock for the real [`GitLab`] client in tests. Commands
//! run inside an OS job (via [`processkit`]) so a `glab` subprocess is never
//! orphaned, and honour an optional [timeout](GitLab::default_timeout).
//!
//! The surface is the **lean merge-request lifecycle** — auth, project view, and
//! MR list / view / create / merge / mark-ready / close, plus the pipeline
//! status. It deserializes `glab … --output json` (GitLab's REST JSON) into typed
//! structs. The sibling [`vcs-github`](https://crates.io/crates/vcs-github) and
//! [`vcs-gitea`](https://crates.io/crates/vcs-gitea) wrappers mirror this shape,
//! and the [`vcs-forge`](https://crates.io/crates/vcs-forge) facade unifies all
//! three.
//!
//! Two test seams: enable the `mock` feature for a `mockall`-generated
//! `MockGitLabApi`, or inject a fake runner with
//! `GitLab::with_runner(`[`ScriptedRunner`](processkit::ScriptedRunner)`)`.

use std::path::Path;

use processkit::ProcessRunner;
// Re-export the processkit types in this crate's public API (also brings
// `Error`/`Result`/`ProcessResult` into scope here).
pub use processkit::{Error, ProcessResult, Result};

mod parse;
pub use parse::{CiStatus, MergeRequest, Project};

/// Name of the underlying CLI binary this crate drives.
///
/// Note on injection safety: the lean surface has **no bare positional string
/// slot** for a caller value — MR ids are `u64` (never flag-like), the
/// title/body/branch arguments ride in flag-VALUE positions (`--title <t>`,
/// `--source-branch <b>`) where glab consumes the next token verbatim, and
/// `run`/`run_args` are the caller-owns-the-argv escape hatch. So unlike
/// `vcs-github` (whose `api`/`release_view` take bare positionals) there is
/// nothing here to guard with `vcs_cli_support::reject_flag_like`; add it back
/// the moment a bare positional is introduced.
pub const BINARY: &str = "glab";

/// How [`GitLabApi::mr_merge`] merges the MR. GitLab's default is a merge commit;
/// `Squash`/`Rebase` add the corresponding flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MergeStrategy {
    /// A merge commit (glab's default — no extra flag).
    Merge,
    /// Squash the commits into one (`--squash`).
    Squash,
    /// Rebase the source onto the target (`--rebase`).
    Rebase,
}

impl MergeStrategy {
    /// The glab flag for this strategy, or `None` for the default merge commit.
    fn flag(self) -> Option<&'static str> {
        match self {
            MergeStrategy::Merge => None,
            MergeStrategy::Squash => Some("--squash"),
            MergeStrategy::Rebase => Some("--rebase"),
        }
    }
}

/// The GitLab operations this crate exposes — the interface consumers code
/// against and mock in tests. The **lean MR lifecycle**; reach unmodelled `glab`
/// commands through [`run`](GitLabApi::run).
#[cfg_attr(feature = "mock", mockall::automock)]
#[async_trait::async_trait]
pub trait GitLabApi: Send + Sync {
    /// Run `glab <args>`, returning trimmed stdout (throws on a non-zero exit).
    async fn run(&self, args: &[String]) -> Result<String>;
    /// Like [`GitLabApi::run`] but never errors on a non-zero exit — returns the
    /// captured [`ProcessResult`].
    async fn run_raw(&self, args: &[String]) -> Result<ProcessResult<String>>;
    /// Installed GitLab CLI version (`glab --version`).
    async fn version(&self) -> Result<String>;
    /// Whether the user is authenticated (`glab auth status` exits zero).
    ///
    /// **Caveat:** this reflects glab's exit code, and a long-standing glab bug
    /// ([gitlab-org/cli#911]) can make `glab auth status` exit `0` even when *not*
    /// authenticated, so a `true` here is a best-effort signal, not a guarantee —
    /// a subsequent API call is the real test. A `false`, a spawn failure, or a
    /// timeout are still reported faithfully.
    ///
    /// [gitlab-org/cli#911]: https://gitlab.com/gitlab-org/cli/-/issues/911
    async fn auth_status(&self) -> Result<bool>;
    /// The project for `dir` (`glab repo view --output json`).
    async fn repo_view(&self, dir: &Path) -> Result<Project>;
    /// Open merge requests for `dir` (`glab mr list --output json`).
    async fn mr_list(&self, dir: &Path) -> Result<Vec<MergeRequest>>;
    /// A single merge request by its project-scoped id
    /// (`glab mr view <id> --output json`).
    async fn mr_view(&self, dir: &Path, id: u64) -> Result<MergeRequest>;
    /// Open a merge request, returning the command's output (the MR URL on
    /// success) (`glab mr create`). `source` (the source branch; `None` = the
    /// current branch) and `target` (the target; `None` = the project default)
    /// are owned `Option<String>`s to keep the trait `mockall`-friendly.
    async fn mr_create(
        &self,
        dir: &Path,
        title: &str,
        body: &str,
        source: Option<String>,
        target: Option<String>,
    ) -> Result<String>;
    /// Merge a merge request **immediately** (`glab mr merge <id> --yes
    /// --auto-merge=false [--squash|--rebase]`) — `--auto-merge=false` overrides
    /// glab's default of enabling merge-when-pipeline-succeeds. See
    /// [`MergeStrategy`].
    async fn mr_merge(&self, dir: &Path, id: u64, strategy: MergeStrategy) -> Result<()>;
    /// Mark a draft merge request as ready (`glab mr update <id> --ready`).
    async fn mr_ready(&self, dir: &Path, id: u64) -> Result<()>;
    /// Close a merge request without merging (`glab mr close <id>`).
    async fn mr_close(&self, dir: &Path, id: u64) -> Result<()>;
    /// The MR's pipeline status, bucketed (`glab mr view <id> --output json`,
    /// reading `head_pipeline.status`). [`CiStatus::None`] when no pipeline ran.
    async fn mr_checks(&self, dir: &Path, id: u64) -> Result<CiStatus>;
}

processkit::cli_client!(
    /// The real GitLab client. Generic over the [`ProcessRunner`] so tests can
    /// inject a fake process executor; `GitLab::new()` uses the real job-backed
    /// runner.
    pub struct GitLab => BINARY
);

#[async_trait::async_trait]
impl<R: ProcessRunner> GitLabApi for GitLab<R> {
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
        // `glab auth status` exits 0 when authenticated, non-zero when not — an
        // exit-code answer. `probe` reads it as a bool but still errors on a spawn
        // failure, timeout, or any unexpected outcome, rather than silently
        // reporting "not authenticated".
        self.core.probe(self.core.command(["auth", "status"])).await
    }

    async fn repo_view(&self, dir: &Path) -> Result<Project> {
        self.core
            .try_parse(
                self.core
                    .command_in(dir, ["repo", "view", "--output", "json"]),
                parse::from_json,
            )
            .await
    }

    async fn mr_list(&self, dir: &Path) -> Result<Vec<MergeRequest>> {
        self.core
            .try_parse(
                self.core
                    .command_in(dir, ["mr", "list", "--output", "json"]),
                parse::from_json,
            )
            .await
    }

    async fn mr_view(&self, dir: &Path, id: u64) -> Result<MergeRequest> {
        let id = id.to_string();
        self.core
            .try_parse(
                self.core
                    .command_in(dir, ["mr", "view", id.as_str(), "--output", "json"]),
                parse::from_json,
            )
            .await
    }

    async fn mr_create(
        &self,
        dir: &Path,
        title: &str,
        body: &str,
        source: Option<String>,
        target: Option<String>,
    ) -> Result<String> {
        // `--yes` skips glab's interactive submission confirmation (a headless run
        // would otherwise hang waiting on the prompt).
        let mut args = vec![
            "mr",
            "create",
            "--title",
            title,
            "--description",
            body,
            "--yes",
        ];
        if let Some(source) = source.as_deref() {
            args.push("--source-branch");
            args.push(source);
        }
        if let Some(target) = target.as_deref() {
            args.push("--target-branch");
            args.push(target);
        }
        self.core.text(self.core.command_in(dir, args)).await
    }

    async fn mr_merge(&self, dir: &Path, id: u64, strategy: MergeStrategy) -> Result<()> {
        let id = id.to_string();
        // `--yes` skips the confirmation prompt. `--auto-merge=false` forces an
        // *immediate* merge: glab's `--auto-merge` defaults to `true`, which —
        // with a running pipeline — would enable merge-when-pipeline-succeeds
        // instead of merging now, so a method named `mr_merge` wouldn't actually
        // merge. The strategy flag is added only for squash/rebase (a plain merge
        // commit is glab's default).
        let mut args = vec!["mr", "merge", id.as_str(), "--yes", "--auto-merge=false"];
        if let Some(flag) = strategy.flag() {
            args.push(flag);
        }
        self.core.unit(self.core.command_in(dir, args)).await
    }

    async fn mr_ready(&self, dir: &Path, id: u64) -> Result<()> {
        let id = id.to_string();
        self.core
            .unit(
                self.core
                    .command_in(dir, ["mr", "update", id.as_str(), "--ready"]),
            )
            .await
    }

    async fn mr_close(&self, dir: &Path, id: u64) -> Result<()> {
        let id = id.to_string();
        self.core
            .unit(self.core.command_in(dir, ["mr", "close", id.as_str()]))
            .await
    }

    async fn mr_checks(&self, dir: &Path, id: u64) -> Result<CiStatus> {
        let id = id.to_string();
        self.core
            .try_parse(
                self.core
                    .command_in(dir, ["mr", "view", id.as_str(), "--output", "json"]),
                parse::parse_ci_status,
            )
            .await
    }
}

impl<R: ProcessRunner> GitLab<R> {
    /// Run `glab <args>` over string slices — `glab.run_args(&["mr", "list"])`
    /// without allocating a `Vec<String>`. Inherent (not on the object-safe
    /// trait), so it can take `&[&str]`; forwards to the same path as
    /// [`GitLabApi::run`].
    pub async fn run_args(&self, args: &[&str]) -> Result<String> {
        self.core.text(self.core.command(args)).await
    }

    /// Like [`run_args`](GitLab::run_args) but never errors on a non-zero exit
    /// (mirrors [`GitLabApi::run_raw`]).
    pub async fn run_raw_args(&self, args: &[&str]) -> Result<ProcessResult<String>> {
        self.core.capture(self.core.command(args)).await
    }

    /// Bind a working directory, so the project-scoped methods omit that argument:
    /// `glab.at(dir).mr_list()` runs [`mr_list`](GitLabApi::mr_list) against `dir`.
    pub fn at<'a>(&'a self, dir: &'a Path) -> GitLabAt<'a, R> {
        GitLabAt { glab: self, dir }
    }
}

/// A [`GitLab`] client with a working directory bound, so its project-scoped
/// methods drop the leading `dir` argument (`glab.at(dir).mr_list()`). Construct
/// one with [`GitLab::at`].
pub struct GitLabAt<'a, R: ProcessRunner = processkit::JobRunner> {
    glab: &'a GitLab<R>,
    dir: &'a Path,
}

// Hand-written rather than derived: holding only references, the view is `Copy`
// for *every* runner. `#[derive(Copy)]` would add a spurious `R: Copy` bound the
// default `JobRunner` doesn't satisfy, silently dropping `Copy` on the handle.
impl<R: ProcessRunner> Clone for GitLabAt<'_, R> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<R: ProcessRunner> Copy for GitLabAt<'_, R> {}

/// Generate [`GitLabAt`] forwarders: `bare` methods forward verbatim, `dir`
/// methods inject `self.dir` as the first argument.
macro_rules! gitlab_at_forwarders {
    (
        bare { $( fn $bn:ident( $($ba:ident: $bt:ty),* $(,)? ) -> $br:ty; )* }
        dir  { $( fn $dn:ident( $($da:ident: $dt:ty),* $(,)? ) -> $dr:ty; )* }
    ) => {
        impl<'a, R: ProcessRunner> GitLabAt<'a, R> {
            $(
                #[doc = concat!("Bound form of [`GitLab`]'s `", stringify!($bn), "`.")]
                pub async fn $bn(&self, $($ba: $bt),*) -> $br {
                    self.glab.$bn($($ba),*).await
                }
            )*
            $(
                #[doc = concat!("Bound form of [`GitLab`]'s `", stringify!($dn), "` (with `dir` pre-bound).")]
                pub async fn $dn(&self, $($da: $dt),*) -> $dr {
                    self.glab.$dn(self.dir, $($da),*).await
                }
            )*
        }
    };
}

gitlab_at_forwarders! {
    bare {
        fn run(args: &[String]) -> Result<String>;
        fn run_raw(args: &[String]) -> Result<ProcessResult<String>>;
        fn run_args(args: &[&str]) -> Result<String>;
        fn run_raw_args(args: &[&str]) -> Result<ProcessResult<String>>;
        fn version() -> Result<String>;
        fn auth_status() -> Result<bool>;
    }
    dir {
        fn repo_view() -> Result<Project>;
        fn mr_list() -> Result<Vec<MergeRequest>>;
        fn mr_view(id: u64) -> Result<MergeRequest>;
        fn mr_create(title: &str, body: &str, source: Option<String>, target: Option<String>) -> Result<String>;
        fn mr_merge(id: u64, strategy: MergeStrategy) -> Result<()>;
        fn mr_ready(id: u64) -> Result<()>;
        fn mr_close(id: u64) -> Result<()>;
        fn mr_checks(id: u64) -> Result<CiStatus>;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use processkit::{RecordingRunner, Reply, ScriptedRunner};

    #[test]
    fn binary_name_is_glab() {
        assert_eq!(BINARY, "glab");
    }

    // Compile-time guard: the bound view stays `Copy` for the default `JobRunner`.
    #[allow(dead_code)]
    fn bound_view_is_copy_for_default_runner() {
        fn assert_copy<T: Copy>() {}
        assert_copy::<GitLabAt<'static, processkit::JobRunner>>();
    }

    // The bound view (`glab.at(dir)`) must produce byte-identical argv to the
    // dir-taking call.
    #[tokio::test]
    async fn bound_view_matches_dir_taking_calls() {
        let dir = Path::new("/repo");
        let rec = RecordingRunner::replying(Reply::ok("[]"));
        let glab = GitLab::with_runner(&rec);

        glab.mr_list(dir).await.unwrap();
        glab.at(dir).mr_list().await.unwrap();
        glab.mr_ready(dir, 7).await.unwrap();
        glab.at(dir).mr_ready(7).await.unwrap();

        let calls = rec.calls();
        assert_eq!(calls[0].args_str(), calls[1].args_str());
        assert_eq!(calls[2].args_str(), calls[3].args_str());
        assert_eq!(calls[1].cwd.as_deref(), Some(dir.as_os_str()));
    }

    #[tokio::test]
    async fn run_args_forwards_str_slices() {
        let glab =
            GitLab::with_runner(ScriptedRunner::new().on(["api", "/version"], Reply::ok("ok\n")));
        assert_eq!(glab.run_args(&["api", "/version"]).await.unwrap(), "ok");
    }

    // Hermetic: real mr_list() arg-building + JSON deserialization against canned
    // output — no `glab` binary or network needed, so this runs on CI.
    #[tokio::test]
    async fn mr_list_parses_scripted_json() {
        let json = r#"[{"iid":7,"title":"Add X","state":"opened","source_branch":"feat/x","target_branch":"main","web_url":"u","draft":false}]"#;
        let glab = GitLab::with_runner(ScriptedRunner::new().on(["mr", "list"], Reply::ok(json)));
        let mrs = glab.mr_list(Path::new(".")).await.expect("mr_list");
        assert_eq!(mrs.len(), 1);
        assert_eq!(mrs[0].iid, 7);
        assert_eq!(mrs[0].target_branch, "main");
    }

    // mr_list builds the `--output json` argv.
    #[tokio::test]
    async fn mr_list_builds_output_json_argv() {
        let rec = RecordingRunner::replying(Reply::ok("[]"));
        let glab = GitLab::with_runner(&rec);
        glab.mr_list(Path::new("/repo")).await.expect("mr_list");
        assert_eq!(
            rec.only_call().args_str(),
            ["mr", "list", "--output", "json"]
        );
    }

    // Hermetic: auth_status reflects the exit code without erroring.
    #[tokio::test]
    async fn auth_status_reads_exit_code() {
        let yes = GitLab::with_runner(ScriptedRunner::new().on(["auth"], Reply::ok("")));
        assert!(yes.auth_status().await.unwrap());
        let no = GitLab::with_runner(
            ScriptedRunner::new().on(["auth"], Reply::fail(1, "not logged in")),
        );
        assert!(!no.auth_status().await.unwrap());
    }

    // A timed-out auth check must error, not silently report "not authenticated".
    #[tokio::test]
    async fn auth_status_errors_on_timeout() {
        let glab = GitLab::with_runner(ScriptedRunner::new().on(["auth"], Reply::timeout()));
        assert!(matches!(
            glab.auth_status().await.unwrap_err(),
            Error::Timeout { .. }
        ));
    }

    // mr_create assembles title/description/--yes, then the optional source/target
    // branch flags, and returns the trimmed output (the MR URL).
    #[tokio::test]
    async fn mr_create_appends_source_and_target() {
        let rec = RecordingRunner::replying(Reply::ok("https://gl/mr/9\n"));
        let glab = GitLab::with_runner(&rec);
        let url = glab
            .mr_create(
                Path::new("/repo"),
                "T",
                "B",
                Some("feat/x".to_string()),
                Some("main".to_string()),
            )
            .await
            .expect("mr_create");
        assert_eq!(url, "https://gl/mr/9");
        assert_eq!(
            rec.only_call().args_str(),
            [
                "mr",
                "create",
                "--title",
                "T",
                "--description",
                "B",
                "--yes",
                "--source-branch",
                "feat/x",
                "--target-branch",
                "main"
            ]
        );
    }

    // With no source/target, mr_create omits both branch flags.
    #[tokio::test]
    async fn mr_create_omits_branch_flags_when_none() {
        let rec = RecordingRunner::replying(Reply::ok("https://gl/mr/1\n"));
        let glab = GitLab::with_runner(&rec);
        glab.mr_create(Path::new("/repo"), "T", "B", None, None)
            .await
            .expect("mr_create");
        assert_eq!(
            rec.only_call().args_str(),
            [
                "mr",
                "create",
                "--title",
                "T",
                "--description",
                "B",
                "--yes"
            ]
        );
    }

    // mr_merge adds `--yes`, and the strategy flag only for squash/rebase.
    #[tokio::test]
    async fn mr_merge_builds_strategy_argv() {
        for (strategy, expected) in [
            (
                MergeStrategy::Merge,
                vec!["mr", "merge", "5", "--yes", "--auto-merge=false"],
            ),
            (
                MergeStrategy::Squash,
                vec![
                    "mr",
                    "merge",
                    "5",
                    "--yes",
                    "--auto-merge=false",
                    "--squash",
                ],
            ),
            (
                MergeStrategy::Rebase,
                vec![
                    "mr",
                    "merge",
                    "5",
                    "--yes",
                    "--auto-merge=false",
                    "--rebase",
                ],
            ),
        ] {
            let rec = RecordingRunner::replying(Reply::ok(""));
            let glab = GitLab::with_runner(&rec);
            glab.mr_merge(Path::new("/repo"), 5, strategy)
                .await
                .expect("mr_merge");
            assert_eq!(rec.only_call().args_str(), expected);
        }
    }

    // mr_ready maps to `mr update <id> --ready`; mr_close to `mr close <id>`.
    #[tokio::test]
    async fn mr_ready_and_close_build_expected_argv() {
        let rec = RecordingRunner::replying(Reply::ok(""));
        let glab = GitLab::with_runner(&rec);
        glab.mr_ready(Path::new("/repo"), 3).await.expect("ready");
        assert_eq!(rec.only_call().args_str(), ["mr", "update", "3", "--ready"]);

        let rec = RecordingRunner::replying(Reply::ok(""));
        let glab = GitLab::with_runner(&rec);
        glab.mr_close(Path::new("/repo"), 3).await.expect("close");
        assert_eq!(rec.only_call().args_str(), ["mr", "close", "3"]);
    }

    // mr_checks reads the MR's head_pipeline status and buckets it.
    #[tokio::test]
    async fn mr_checks_buckets_pipeline_status() {
        let json = r#"{"iid":4,"head_pipeline":{"status":"failed"}}"#;
        let glab = GitLab::with_runner(ScriptedRunner::new().on(["mr", "view"], Reply::ok(json)));
        assert_eq!(
            glab.mr_checks(Path::new("."), 4).await.unwrap(),
            CiStatus::Failing
        );
    }

    // repo_view parses the GitLab Project JSON.
    #[tokio::test]
    async fn repo_view_parses_project() {
        let json = r#"{"name":"cli","path_with_namespace":"gitlab-org/cli","default_branch":"main","web_url":"u","visibility":"public"}"#;
        let glab = GitLab::with_runner(ScriptedRunner::new().on(["repo", "view"], Reply::ok(json)));
        let p = glab.repo_view(Path::new(".")).await.expect("repo_view");
        assert_eq!(p.path_with_namespace, "gitlab-org/cli");
        assert_eq!(p.default_branch, "main");
    }

    #[cfg(feature = "mock")]
    #[tokio::test]
    async fn consumer_mocks_the_interface() {
        let mut mock = MockGitLabApi::new();
        mock.expect_auth_status().returning(|| Ok(true));
        assert!(mock.auth_status().await.unwrap());
    }
}
