//! `vcs-forge` — a backend-agnostic facade over [`vcs-github`](vcs_github),
//! [`vcs-gitlab`](vcs_gitlab), and [`vcs-gitea`](vcs_gitea).
//!
//! [`Forge`] is a cwd-bound handle that dispatches the *common* forge operations
//! (auth, repo view, and the PR/MR lifecycle) to whichever CLI backs it,
//! returning forge-agnostic DTOs ([`ForgePr`], [`ForgeRepo`], …). It is the
//! `gh`/`glab`/`tea` analogue of how [`vcs-core`](https://crates.io/crates/vcs-core)'s
//! `Repo` sits over git and jj.
//!
//! Unlike a repository, a forge has **no filesystem marker** — it's identified by
//! the remote host — so a `Forge` is **constructed explicitly**
//! ([`Forge::github`] / [`Forge::gitlab`] / [`Forge::gitea`]), optionally guided
//! by [`ForgeKind::from_remote_url`] applied to a remote URL the caller already
//! holds. The CLIs differ in coverage: Gitea's `tea` has no current-repo view,
//! draft toggle, or checks command, so [`repo_view`](ForgeApi::repo_view),
//! [`pr_mark_ready`](ForgeApi::pr_mark_ready), and [`pr_checks`](ForgeApi::pr_checks)
//! return [`Error::Unsupported`] there.
//!
//! ```no_run
//! use vcs_forge::{Forge, ForgeApi};
//! # async fn run() -> vcs_forge::Result<()> {
//! let forge = Forge::github(".");
//! let prs = forge.pr_list().await?;
//! # let _ = prs;
//! # Ok(()) }
//! ```
//!
//! The handle is generic over the [`ProcessRunner`] so tests can inject a fake;
//! the [`Forge::github`]/`gitlab`/`gitea` constructors use the real job-backed
//! runner, while [`Forge::for_github`]/`for_gitlab`/`for_gitea` take an explicit
//! client (a test seam).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use processkit::{JobRunner, ProcessRunner};
use vcs_gitea::Gitea;
use vcs_github::GitHub;
use vcs_gitlab::GitLab;

mod dto;
mod error;
mod gitea_forge;
mod github_forge;
mod gitlab_forge;

pub use dto::{CiStatus, ForgeKind, ForgePr, ForgePrState, ForgeRepo, MergeStrategy};
pub use error::{Error, Result};

// Re-export the underlying wrappers so a consumer depending only on `vcs-forge`
// can construct the clients (`Forge::for_github(cwd, GitHub::new())`) and reach
// forge-specific operations off the common surface.
pub use vcs_gitea;
pub use vcs_github;
pub use vcs_gitlab;

/// The per-CLI client behind a [`Forge`]. Shared via `Arc` so [`Forge::at`] can
/// re-anchor the cwd cheaply without rebuilding the client.
enum Backend<R: ProcessRunner> {
    GitHub(Arc<GitHub<R>>),
    GitLab(Arc<GitLab<R>>),
    Gitea(Arc<Gitea<R>>),
}

impl<R: ProcessRunner> Backend<R> {
    fn shared(&self) -> Self {
        match self {
            Backend::GitHub(c) => Backend::GitHub(Arc::clone(c)),
            Backend::GitLab(c) => Backend::GitLab(Arc::clone(c)),
            Backend::Gitea(c) => Backend::Gitea(Arc::clone(c)),
        }
    }
}

/// A cwd-bound, forge-agnostic handle. Operations run against the bound directory
/// ([`cwd`](Forge::cwd)); the CLI infers the repository from that directory's git
/// remote. Use [`at`](Forge::at) for a sibling handle bound elsewhere.
pub struct Forge<R: ProcessRunner = JobRunner> {
    cwd: PathBuf,
    backend: Backend<R>,
}

impl Forge<JobRunner> {
    /// A GitHub-backed handle bound to `cwd`, using the real job-backed runner.
    pub fn github(cwd: impl Into<PathBuf>) -> Self {
        Forge {
            cwd: cwd.into(),
            backend: Backend::GitHub(Arc::new(GitHub::new())),
        }
    }

    /// A GitLab-backed handle bound to `cwd`, using the real job-backed runner.
    pub fn gitlab(cwd: impl Into<PathBuf>) -> Self {
        Forge {
            cwd: cwd.into(),
            backend: Backend::GitLab(Arc::new(GitLab::new())),
        }
    }

    /// A Gitea-backed handle bound to `cwd`, using the real job-backed runner.
    pub fn gitea(cwd: impl Into<PathBuf>) -> Self {
        Forge {
            cwd: cwd.into(),
            backend: Backend::Gitea(Arc::new(Gitea::new())),
        }
    }
}

impl<R: ProcessRunner> Forge<R> {
    /// Build a GitHub-backed handle from an explicit client — for a custom runner
    /// (e.g. a test seam) or a pre-configured [`GitHub`].
    pub fn for_github(cwd: impl Into<PathBuf>, client: GitHub<R>) -> Self {
        Forge {
            cwd: cwd.into(),
            backend: Backend::GitHub(Arc::new(client)),
        }
    }

    /// Build a GitLab-backed handle from an explicit [`GitLab`] client.
    pub fn for_gitlab(cwd: impl Into<PathBuf>, client: GitLab<R>) -> Self {
        Forge {
            cwd: cwd.into(),
            backend: Backend::GitLab(Arc::new(client)),
        }
    }

    /// Build a Gitea-backed handle from an explicit [`Gitea`] client.
    pub fn for_gitea(cwd: impl Into<PathBuf>, client: Gitea<R>) -> Self {
        Forge {
            cwd: cwd.into(),
            backend: Backend::Gitea(Arc::new(client)),
        }
    }

    /// Which forge drives this handle.
    pub fn kind(&self) -> ForgeKind {
        match &self.backend {
            Backend::GitHub(_) => ForgeKind::GitHub,
            Backend::GitLab(_) => ForgeKind::GitLab,
            Backend::Gitea(_) => ForgeKind::Gitea,
        }
    }

    /// The directory operations run against.
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// A sibling handle bound to `dir`, sharing this handle's client.
    pub fn at(&self, dir: impl Into<PathBuf>) -> Self {
        Forge {
            cwd: dir.into(),
            backend: self.backend.shared(),
        }
    }

    /// Whether the user is authenticated (GitHub/GitLab: a zero-exit `auth
    /// status`; Gitea: at least one configured login).
    pub async fn auth_status(&self) -> Result<bool> {
        match &self.backend {
            Backend::GitHub(c) => github_forge::auth_status(c).await,
            Backend::GitLab(c) => gitlab_forge::auth_status(c).await,
            Backend::Gitea(c) => gitea_forge::auth_status(c).await,
        }
    }

    /// The repository/project for the bound directory. **[`Unsupported`](Error::Unsupported)
    /// on Gitea** (`tea` has no current-repo view).
    pub async fn repo_view(&self) -> Result<ForgeRepo> {
        match &self.backend {
            Backend::GitHub(c) => github_forge::repo_view(c, &self.cwd).await,
            Backend::GitLab(c) => gitlab_forge::repo_view(c, &self.cwd).await,
            Backend::Gitea(_) => Err(unsupported(ForgeKind::Gitea, "repo_view")),
        }
    }

    /// Open pull/merge requests for the bound directory.
    pub async fn pr_list(&self) -> Result<Vec<ForgePr>> {
        match &self.backend {
            Backend::GitHub(c) => github_forge::pr_list(c, &self.cwd).await,
            Backend::GitLab(c) => gitlab_forge::pr_list(c, &self.cwd).await,
            Backend::Gitea(c) => gitea_forge::pr_list(c, &self.cwd).await,
        }
    }

    /// A single PR/MR by number (GitLab `iid`). On Gitea this lists and filters
    /// (`tea` has no single-PR view).
    pub async fn pr_view(&self, number: u64) -> Result<ForgePr> {
        match &self.backend {
            Backend::GitHub(c) => github_forge::pr_view(c, &self.cwd, number).await,
            Backend::GitLab(c) => gitlab_forge::pr_view(c, &self.cwd, number).await,
            Backend::Gitea(c) => gitea_forge::pr_view(c, &self.cwd, number).await,
        }
    }

    /// Open a PR/MR, returning the CLI's success output — a URL on GitHub/GitLab;
    /// `tea` prints a textual summary (no URL). `source` (the source branch;
    /// `None` = the current branch) and `target` (the target; `None` = the repo
    /// default) are owned `Option<String>`s.
    pub async fn pr_create(
        &self,
        title: &str,
        body: &str,
        source: Option<String>,
        target: Option<String>,
    ) -> Result<String> {
        match &self.backend {
            Backend::GitHub(c) => {
                github_forge::pr_create(c, &self.cwd, title, body, source, target).await
            }
            Backend::GitLab(c) => {
                gitlab_forge::pr_create(c, &self.cwd, title, body, source, target).await
            }
            Backend::Gitea(c) => {
                gitea_forge::pr_create(c, &self.cwd, title, body, source, target).await
            }
        }
    }

    /// Merge a PR/MR with the given [`MergeStrategy`].
    pub async fn pr_merge(&self, number: u64, strategy: MergeStrategy) -> Result<()> {
        match &self.backend {
            Backend::GitHub(c) => github_forge::pr_merge(c, &self.cwd, number, strategy).await,
            Backend::GitLab(c) => gitlab_forge::pr_merge(c, &self.cwd, number, strategy).await,
            Backend::Gitea(c) => gitea_forge::pr_merge(c, &self.cwd, number, strategy).await,
        }
    }

    /// Mark a draft PR/MR as ready for review. **[`Unsupported`](Error::Unsupported)
    /// on Gitea** (`tea` has no draft toggle — a Gitea draft is a `WIP:` title
    /// prefix, edited via the raw client).
    pub async fn pr_mark_ready(&self, number: u64) -> Result<()> {
        match &self.backend {
            Backend::GitHub(c) => github_forge::pr_mark_ready(c, &self.cwd, number).await,
            Backend::GitLab(c) => gitlab_forge::pr_mark_ready(c, &self.cwd, number).await,
            Backend::Gitea(_) => Err(unsupported(ForgeKind::Gitea, "pr_mark_ready")),
        }
    }

    /// Close a PR/MR without merging. `delete_branch` applies to GitHub only
    /// (`gh pr close --delete-branch`); GitLab and Gitea ignore it.
    pub async fn pr_close(&self, number: u64, delete_branch: bool) -> Result<()> {
        match &self.backend {
            Backend::GitHub(c) => github_forge::pr_close(c, &self.cwd, number, delete_branch).await,
            Backend::GitLab(c) => gitlab_forge::pr_close(c, &self.cwd, number).await,
            Backend::Gitea(c) => gitea_forge::pr_close(c, &self.cwd, number).await,
        }
    }

    /// The PR/MR's coarse CI status (see [`CiStatus`]). **[`Unsupported`](Error::Unsupported)
    /// on Gitea** (`tea` has no checks command).
    pub async fn pr_checks(&self, number: u64) -> Result<CiStatus> {
        match &self.backend {
            Backend::GitHub(c) => github_forge::pr_checks(c, &self.cwd, number).await,
            Backend::GitLab(c) => gitlab_forge::pr_checks(c, &self.cwd, number).await,
            Backend::Gitea(_) => Err(unsupported(ForgeKind::Gitea, "pr_checks")),
        }
    }
}

fn unsupported(forge: ForgeKind, operation: &'static str) -> Error {
    Error::Unsupported { forge, operation }
}

/// The forge-agnostic common surface of [`Forge`], as a trait — so a consumer can
/// hold a `Box<dyn ForgeApi>` / `&dyn ForgeApi` and code against the operations
/// without naming the [`ProcessRunner`] generic.
///
/// Every method mirrors the like-named inherent method on [`Forge`].
#[async_trait::async_trait]
pub trait ForgeApi: Send + Sync {
    /// Which forge drives this handle.
    fn kind(&self) -> ForgeKind;
    /// The directory operations run against.
    fn cwd(&self) -> &Path;
    /// See [`Forge::auth_status`].
    async fn auth_status(&self) -> Result<bool>;
    /// See [`Forge::repo_view`].
    async fn repo_view(&self) -> Result<ForgeRepo>;
    /// See [`Forge::pr_list`].
    async fn pr_list(&self) -> Result<Vec<ForgePr>>;
    /// See [`Forge::pr_view`].
    async fn pr_view(&self, number: u64) -> Result<ForgePr>;
    /// See [`Forge::pr_create`].
    async fn pr_create(
        &self,
        title: &str,
        body: &str,
        source: Option<String>,
        target: Option<String>,
    ) -> Result<String>;
    /// See [`Forge::pr_merge`].
    async fn pr_merge(&self, number: u64, strategy: MergeStrategy) -> Result<()>;
    /// See [`Forge::pr_mark_ready`].
    async fn pr_mark_ready(&self, number: u64) -> Result<()>;
    /// See [`Forge::pr_close`].
    async fn pr_close(&self, number: u64, delete_branch: bool) -> Result<()>;
    /// See [`Forge::pr_checks`].
    async fn pr_checks(&self, number: u64) -> Result<CiStatus>;
}

// Delegates to the inherent methods, which method resolution prefers — so these
// bodies dispatch through `Forge`'s real implementations, not back into the trait.
#[async_trait::async_trait]
impl<R: ProcessRunner> ForgeApi for Forge<R> {
    fn kind(&self) -> ForgeKind {
        self.kind()
    }
    fn cwd(&self) -> &Path {
        self.cwd()
    }
    async fn auth_status(&self) -> Result<bool> {
        self.auth_status().await
    }
    async fn repo_view(&self) -> Result<ForgeRepo> {
        self.repo_view().await
    }
    async fn pr_list(&self) -> Result<Vec<ForgePr>> {
        self.pr_list().await
    }
    async fn pr_view(&self, number: u64) -> Result<ForgePr> {
        self.pr_view(number).await
    }
    async fn pr_create(
        &self,
        title: &str,
        body: &str,
        source: Option<String>,
        target: Option<String>,
    ) -> Result<String> {
        self.pr_create(title, body, source, target).await
    }
    async fn pr_merge(&self, number: u64, strategy: MergeStrategy) -> Result<()> {
        self.pr_merge(number, strategy).await
    }
    async fn pr_mark_ready(&self, number: u64) -> Result<()> {
        self.pr_mark_ready(number).await
    }
    async fn pr_close(&self, number: u64, delete_branch: bool) -> Result<()> {
        self.pr_close(number, delete_branch).await
    }
    async fn pr_checks(&self, number: u64) -> Result<CiStatus> {
        self.pr_checks(number).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use processkit::{RecordingRunner, Reply, ScriptedRunner};

    fn github(runner: ScriptedRunner) -> Forge<ScriptedRunner> {
        Forge::for_github("/repo", GitHub::with_runner(runner))
    }
    fn gitlab(runner: ScriptedRunner) -> Forge<ScriptedRunner> {
        Forge::for_gitlab("/repo", GitLab::with_runner(runner))
    }
    fn gitea(runner: ScriptedRunner) -> Forge<ScriptedRunner> {
        Forge::for_gitea("/repo", Gitea::with_runner(runner))
    }

    #[tokio::test]
    async fn kind_reflects_backend() {
        assert_eq!(github(ScriptedRunner::new()).kind(), ForgeKind::GitHub);
        assert_eq!(gitlab(ScriptedRunner::new()).kind(), ForgeKind::GitLab);
        assert_eq!(gitea(ScriptedRunner::new()).kind(), ForgeKind::Gitea);
    }

    // GitHub's "OPEN"/"MERGED" states map onto the unified ForgePrState.
    #[tokio::test]
    async fn github_pr_list_maps_to_unified() {
        let json = r#"[{"number":7,"title":"X","state":"MERGED","headRefName":"feat","baseRefName":"main","url":"u"}]"#;
        let forge = github(ScriptedRunner::new().on(["pr", "list"], Reply::ok(json)));
        let prs = forge.pr_list().await.unwrap();
        assert_eq!(prs[0].number, 7);
        assert_eq!(prs[0].state, ForgePrState::Merged);
        assert_eq!(prs[0].source_branch, "feat");
    }

    // GitLab `repo_view` maps a known "public" visibility to private == false.
    #[tokio::test]
    async fn gitlab_repo_view_maps_public_visibility() {
        let json = r#"{"name":"cli","path_with_namespace":"gitlab-org/cli","default_branch":"main","web_url":"u","visibility":"public"}"#;
        let forge = gitlab(ScriptedRunner::new().on(["repo", "view"], Reply::ok(json)));
        let repo = forge.repo_view().await.unwrap();
        assert_eq!(repo.owner, "gitlab-org");
        assert_eq!(repo.name, "cli");
        assert!(!repo.private);
    }

    // When glab omits `visibility`, the facade must NOT report the repo as private
    // — an unknown visibility is the conservative `false`, never a false privacy.
    #[tokio::test]
    async fn gitlab_repo_view_absent_visibility_is_not_private() {
        let json =
            r#"{"name":"cli","path_with_namespace":"o/cli","default_branch":"main","web_url":"u"}"#;
        let forge = gitlab(ScriptedRunner::new().on(["repo", "view"], Reply::ok(json)));
        let repo = forge.repo_view().await.unwrap();
        assert!(!repo.private, "absent visibility must not be private");
    }

    // GitLab's `iid` becomes the number and "opened" maps to Open.
    #[tokio::test]
    async fn gitlab_pr_list_maps_iid_and_state() {
        let json = r#"[{"iid":12,"title":"X","state":"opened","source_branch":"feat","target_branch":"main","web_url":"u","draft":true}]"#;
        let forge = gitlab(ScriptedRunner::new().on(["mr", "list"], Reply::ok(json)));
        let prs = forge.pr_list().await.unwrap();
        assert_eq!(prs[0].number, 12);
        assert_eq!(prs[0].state, ForgePrState::Open);
        assert!(prs[0].draft);
    }

    // Gitea's `merged` flag drives Merged even though `state` is "closed".
    #[tokio::test]
    async fn gitea_pr_view_filters_and_maps_merged() {
        let json = r#"[{"number":9,"title":"Nine","state":"closed","merged":true,"head":{"ref":"f"},"base":{"ref":"main"},"html_url":"u"}]"#;
        let forge = gitea(ScriptedRunner::new().on(["pr", "list"], Reply::ok(json)));
        let pr = forge.pr_view(9).await.unwrap();
        assert_eq!(pr.state, ForgePrState::Merged);
        assert_eq!(pr.target_branch, "main");
    }

    // The Gitea backend reports the three unmodelled ops as Unsupported, naming
    // the operation — and without spawning anything.
    #[tokio::test]
    async fn gitea_unsupported_ops_error_without_spawning() {
        let rec = RecordingRunner::replying(Reply::ok(""));
        let forge = Forge::for_gitea("/repo", Gitea::with_runner(&rec));
        for err in [
            forge.repo_view().await.unwrap_err(),
            forge.pr_mark_ready(1).await.unwrap_err(),
            forge.pr_checks(1).await.unwrap_err(),
        ] {
            assert!(err.is_unsupported(), "{err:?}");
        }
        assert!(rec.calls().is_empty(), "unsupported ops must not spawn");
    }

    // The unified MergeStrategy maps to each CLI's own flag.
    #[tokio::test]
    async fn pr_merge_maps_strategy_per_backend() {
        let rec = RecordingRunner::replying(Reply::ok(""));
        Forge::for_github("/repo", GitHub::with_runner(&rec))
            .pr_merge(5, MergeStrategy::Squash)
            .await
            .unwrap();
        assert_eq!(rec.only_call().args_str(), ["pr", "merge", "5", "--squash"]);

        let rec = RecordingRunner::replying(Reply::ok(""));
        Forge::for_gitlab("/repo", GitLab::with_runner(&rec))
            .pr_merge(5, MergeStrategy::Rebase)
            .await
            .unwrap();
        assert_eq!(
            rec.only_call().args_str(),
            [
                "mr",
                "merge",
                "5",
                "--yes",
                "--auto-merge=false",
                "--rebase"
            ]
        );

        let rec = RecordingRunner::replying(Reply::ok(""));
        Forge::for_gitea("/repo", Gitea::with_runner(&rec))
            .pr_merge(5, MergeStrategy::Merge)
            .await
            .unwrap();
        assert_eq!(
            rec.only_call().args_str(),
            ["pr", "merge", "5", "--style", "merge"]
        );
    }

    // GitHub's per-check buckets aggregate into one coarse CiStatus.
    #[tokio::test]
    async fn github_pr_checks_aggregates_buckets() {
        let json = r#"[{"name":"a","bucket":"pass"},{"name":"b","bucket":"fail"}]"#;
        let forge = github(ScriptedRunner::new().on(["pr", "checks"], Reply::ok(json)));
        assert_eq!(forge.pr_checks(1).await.unwrap(), CiStatus::Failing);

        let json = r#"[{"name":"a","bucket":"pass"},{"name":"b","bucket":"pending"}]"#;
        let forge = github(ScriptedRunner::new().on(["pr", "checks"], Reply::ok(json)));
        assert_eq!(forge.pr_checks(1).await.unwrap(), CiStatus::Pending);

        // A cancelled check is a failure (short-circuits like `fail`).
        let json = r#"[{"name":"a","bucket":"pass"},{"name":"b","bucket":"cancel"}]"#;
        let forge = github(ScriptedRunner::new().on(["pr", "checks"], Reply::ok(json)));
        assert_eq!(forge.pr_checks(1).await.unwrap(), CiStatus::Failing);

        // All-skipped (no pass/fail/pending) and an empty list both read as None.
        let json = r#"[{"name":"a","bucket":"skipping"}]"#;
        let forge = github(ScriptedRunner::new().on(["pr", "checks"], Reply::ok(json)));
        assert_eq!(forge.pr_checks(1).await.unwrap(), CiStatus::None);
        let forge = github(ScriptedRunner::new().on(["pr", "checks"], Reply::ok("[]")));
        assert_eq!(forge.pr_checks(1).await.unwrap(), CiStatus::None);
    }

    // `at` re-binds the cwd while sharing the backend.
    #[tokio::test]
    async fn at_rebinds_cwd_and_shares_backend() {
        let forge = github(ScriptedRunner::new());
        let moved = forge.at("/repo/sub");
        assert_eq!(moved.cwd(), Path::new("/repo/sub"));
        assert_eq!(moved.kind(), ForgeKind::GitHub);
    }

    // `&dyn ForgeApi` must dispatch through the real inherent methods (a delegating
    // body that recursed would stack-overflow here instead of returning).
    #[tokio::test]
    async fn forge_api_trait_object_dispatches() {
        let json = r#"[{"iid":1,"title":"X","state":"opened","source_branch":"f","target_branch":"main","web_url":"u"}]"#;
        let forge = gitlab(ScriptedRunner::new().on(["mr", "list"], Reply::ok(json)));
        let dynamic: &dyn ForgeApi = &forge;
        assert_eq!(dynamic.kind(), ForgeKind::GitLab);
        assert_eq!(dynamic.pr_list().await.unwrap()[0].number, 1);
    }
}
