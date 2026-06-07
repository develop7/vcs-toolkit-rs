//! `vcs-mcp` — a [Model Context Protocol](https://modelcontextprotocol.io)
//! server that exposes the toolkit's typed repository operations as MCP **tools**,
//! so an agent harness drives a git/jj repo (and its forge) through structured,
//! validated calls instead of raw shell.
//!
//! The server wraps the two facades — [`vcs_core::Repo`] (git/jj) and
//! [`vcs_forge::Forge`] (GitHub/GitLab/Gitea) — and serializes their DTOs to JSON.
//! **Read tools are always available; mutating tools are gated**: they're
//! registered (and annotated `destructiveHint`) but reject calls unless the
//! server was started with writes allowed ([`VcsMcpServer::new`]'s `allow_write`).
//!
//! Build a [`VcsMcpServer`] and serve it over a transport (the `vcs-mcp` binary
//! uses stdio):
//!
//! ```no_run
//! # use vcs_core::Repo;
//! # use vcs_mcp::VcsMcpServer;
//! # use rmcp::{ServiceExt, transport::stdio};
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let repo = Repo::open(".")?;
//! let server = VcsMcpServer::new(repo, None, /* allow_write */ false);
//! server.serve(stdio()).await?.waiting().await?;
//! # Ok(()) }
//! ```

use std::path::Path;
use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, Implementation, ServerCapabilities, ServerInfo};
use rmcp::schemars;
use rmcp::{ErrorData, ServerHandler, tool, tool_handler, tool_router};
use serde::Deserialize;
use vcs_core::{Repo, VcsRepo};
use vcs_forge::{Forge, ForgeApi};

// --- Tool parameter structs (Deserialize + JsonSchema → the MCP input schema) --

/// Switch the working copy to a branch/bookmark/revision.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CheckoutParams {
    /// The branch, bookmark, or revision to switch to (git checkout / jj edit).
    pub reference: String,
}

/// Commit exactly these paths.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CommitParams {
    /// Repo-relative paths to commit (and nothing else).
    pub paths: Vec<String>,
    /// The commit message.
    pub message: String,
}

/// Probe a merge.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TryMergeParams {
    /// The branch/revision to probe merging into the current work.
    pub source: String,
}

/// Create a worktree/workspace.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateWorktreeParams {
    /// Filesystem path for the new worktree/workspace.
    pub path: String,
    /// The new branch/bookmark to create on it.
    pub branch: String,
    /// The base revision to start it from.
    pub base: String,
}

/// Remove a worktree/workspace.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RemoveWorktreeParams {
    /// Filesystem path of the worktree/workspace to remove.
    pub path: String,
    /// Force removal even with local changes (git only).
    #[serde(default)]
    pub force: bool,
}

/// A pull/merge request by number.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PrNumberParams {
    /// The PR/MR number (GitLab uses the project-scoped `iid`).
    pub number: u64,
}

/// Open a pull/merge request.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PrCreateParams {
    /// Title.
    pub title: String,
    /// Body / description.
    pub body: String,
    /// Source/head branch; omit for the current branch.
    #[serde(default)]
    pub source: Option<String>,
    /// Target/base branch; omit for the repo default.
    #[serde(default)]
    pub target: Option<String>,
}

/// Merge a pull/merge request.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PrMergeParams {
    /// The PR/MR number.
    pub number: u64,
    /// Merge strategy.
    pub strategy: MergeStrategyArg,
}

/// Close a pull/merge request.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PrCloseParams {
    /// The PR/MR number.
    pub number: u64,
    /// Also delete the source branch (GitHub only).
    #[serde(default)]
    pub delete_branch: bool,
}

/// How [`forge_pr_merge`](VcsMcpServer::forge_pr_merge) merges.
#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MergeStrategyArg {
    /// A merge commit.
    Merge,
    /// Squash into one commit.
    Squash,
    /// Rebase onto the target.
    Rebase,
}

impl From<MergeStrategyArg> for vcs_forge::MergeStrategy {
    fn from(s: MergeStrategyArg) -> Self {
        match s {
            MergeStrategyArg::Merge => vcs_forge::MergeStrategy::Merge,
            MergeStrategyArg::Squash => vcs_forge::MergeStrategy::Squash,
            MergeStrategyArg::Rebase => vcs_forge::MergeStrategy::Rebase,
        }
    }
}

// --- The server --------------------------------------------------------------

/// An MCP server over a single repository (and, optionally, its forge). Held as
/// object-safe trait handles, so it's runner-agnostic; clone is cheap (`Arc`).
/// Construct with [`new`](Self::new).
#[derive(Clone)]
pub struct VcsMcpServer {
    repo: Arc<dyn VcsRepo>,
    forge: Option<Arc<dyn ForgeApi>>,
    allow_write: bool,
    tool_router: ToolRouter<Self>,
}

impl VcsMcpServer {
    /// Build a server bound to `repo`, with an optional `forge` (PR/MR tools), and
    /// `allow_write` controlling whether the mutating tools are callable.
    pub fn new(repo: Repo, forge: Option<Forge>, allow_write: bool) -> Self {
        Self::from_handles(
            Arc::new(repo),
            forge.map(|f| Arc::new(f) as Arc<dyn ForgeApi>),
            allow_write,
        )
    }

    /// Build from already-erased handles — the seam tests use to inject a `Repo`
    /// over a fake `ProcessRunner`.
    fn from_handles(
        repo: Arc<dyn VcsRepo>,
        forge: Option<Arc<dyn ForgeApi>>,
        allow_write: bool,
    ) -> Self {
        Self {
            repo,
            forge,
            allow_write,
            tool_router: Self::tool_router(),
        }
    }

    /// Reject a mutating tool call when writes are disabled.
    fn require_write(&self) -> Result<(), ErrorData> {
        if self.allow_write {
            Ok(())
        } else {
            Err(ErrorData::invalid_params(
                "write tools are disabled; restart the server with --allow-write".to_string(),
                None,
            ))
        }
    }

    /// The configured forge, or a clear error when none was resolved.
    fn forge(&self) -> Result<&dyn ForgeApi, ErrorData> {
        self.forge.as_deref().ok_or_else(|| {
            ErrorData::invalid_params(
                "no forge is configured for this repository (pass --forge github|gitlab|gitea)"
                    .to_string(),
                None,
            )
        })
    }
}

/// Encode a serializable value as a JSON text result.
fn ok_json<T: serde::Serialize>(value: &T) -> Result<CallToolResult, ErrorData> {
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}

/// Map a `vcs-core` error into an MCP error.
fn core_err(e: vcs_core::Error) -> ErrorData {
    ErrorData::internal_error(e.to_string(), None)
}

/// Map a `vcs-forge` error into an MCP error — an `Unsupported` op is a
/// client-facing invalid-request; a forge/network failure is internal.
fn forge_err(e: vcs_forge::Error) -> ErrorData {
    if e.is_unsupported() {
        ErrorData::invalid_params(e.to_string(), None)
    } else {
        ErrorData::internal_error(e.to_string(), None)
    }
}

#[tool_router]
impl VcsMcpServer {
    // --- repo: read --------------------------------------------------------

    #[tool(
        description = "A batched snapshot of the repo state: branch, upstream, ahead/behind, HEAD, dirtiness, change count, conflict, and operation state.",
        annotations(read_only_hint = true)
    )]
    pub async fn repo_snapshot(&self) -> Result<CallToolResult, ErrorData> {
        ok_json(&self.repo.snapshot().await.map_err(core_err)?)
    }

    #[tool(
        description = "Which backend (git/jj), the repository root, the working directory, and the configured forge (if any).",
        annotations(read_only_hint = true)
    )]
    pub async fn repo_info(&self) -> Result<CallToolResult, ErrorData> {
        ok_json(&serde_json::json!({
            "backend": self.repo.kind().as_str(),
            "root": self.repo.root().to_string_lossy(),
            "cwd": self.repo.cwd().to_string_lossy(),
            "forge": self.forge.as_ref().map(|f| f.kind().as_str()),
        }))
    }

    #[tool(
        description = "The working-copy changes (added/modified/deleted/renamed paths).",
        annotations(read_only_hint = true)
    )]
    pub async fn repo_status(&self) -> Result<CallToolResult, ErrorData> {
        ok_json(&self.repo.changed_files().await.map_err(core_err)?)
    }

    #[tool(
        description = "Aggregate insertion/deletion/file counts for the working copy.",
        annotations(read_only_hint = true)
    )]
    pub async fn repo_diff_stat(&self) -> Result<CallToolResult, ErrorData> {
        ok_json(&self.repo.diff_stat().await.map_err(core_err)?)
    }

    #[tool(
        description = "Local branch (git) / bookmark (jj) names.",
        annotations(read_only_hint = true)
    )]
    pub async fn repo_branches(&self) -> Result<CallToolResult, ErrorData> {
        ok_json(&self.repo.local_branches().await.map_err(core_err)?)
    }

    #[tool(
        description = "The current branch/bookmark (null when detached/unset).",
        annotations(read_only_hint = true)
    )]
    pub async fn repo_current_branch(&self) -> Result<CallToolResult, ErrorData> {
        ok_json(&self.repo.current_branch().await.map_err(core_err)?)
    }

    #[tool(
        description = "Paths with unresolved merge conflicts (repo-relative, '/'-separated).",
        annotations(read_only_hint = true)
    )]
    pub async fn repo_conflicts(&self) -> Result<CallToolResult, ErrorData> {
        ok_json(&self.repo.conflicted_files().await.map_err(core_err)?)
    }

    #[tool(
        description = "Attached worktrees (git) / workspaces (jj).",
        annotations(read_only_hint = true)
    )]
    pub async fn repo_worktrees(&self) -> Result<CallToolResult, ErrorData> {
        ok_json(&self.repo.list_worktrees().await.map_err(core_err)?)
    }

    #[tool(
        description = "Probe whether merging `source` into the current work would conflict, WITHOUT leaving a trace (the probe is always rolled back). Read-only, but it spawns a real trial merge.",
        annotations(read_only_hint = true)
    )]
    pub async fn repo_try_merge(
        &self,
        Parameters(p): Parameters<TryMergeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        ok_json(&self.repo.try_merge(&p.source).await.map_err(core_err)?)
    }

    // --- repo: mutations (gated) ------------------------------------------

    #[tool(
        description = "Commit exactly the given paths with a message (git commit --only / jj commit <filesets>). Requires --allow-write.",
        annotations(destructive_hint = true)
    )]
    pub async fn repo_commit(
        &self,
        Parameters(p): Parameters<CommitParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.require_write()?;
        self.repo
            .commit_paths(&p.paths, &p.message)
            .await
            .map_err(core_err)?;
        ok_json(&serde_json::json!({ "committed_paths": p.paths.len() }))
    }

    #[tool(
        description = "Switch the working copy to a branch/bookmark/revision (git checkout / jj edit). Requires --allow-write.",
        annotations(destructive_hint = true)
    )]
    pub async fn repo_checkout(
        &self,
        Parameters(p): Parameters<CheckoutParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.require_write()?;
        self.repo.checkout(&p.reference).await.map_err(core_err)?;
        ok_json(&serde_json::json!({ "checked_out": p.reference }))
    }

    #[tool(
        description = "Fetch from the default remote (git fetch / jj git fetch). Requires --allow-write.",
        annotations(destructive_hint = true)
    )]
    pub async fn repo_fetch(&self) -> Result<CallToolResult, ErrorData> {
        self.require_write()?;
        self.repo.fetch().await.map_err(core_err)?;
        ok_json(&serde_json::json!({ "fetched": true }))
    }

    #[tool(
        description = "Create a worktree/workspace at `path` on a new `branch` from `base`. Requires --allow-write.",
        annotations(destructive_hint = true)
    )]
    pub async fn repo_create_worktree(
        &self,
        Parameters(p): Parameters<CreateWorktreeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.require_write()?;
        let outcome = self
            .repo
            .create_worktree(Path::new(&p.path), &p.branch, &p.base)
            .await
            .map_err(core_err)?;
        ok_json(&outcome)
    }

    #[tool(
        description = "Remove the worktree/workspace at `path`. Requires --allow-write.",
        annotations(destructive_hint = true)
    )]
    pub async fn repo_remove_worktree(
        &self,
        Parameters(p): Parameters<RemoveWorktreeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.require_write()?;
        self.repo
            .remove_worktree(Path::new(&p.path), p.force)
            .await
            .map_err(core_err)?;
        ok_json(&serde_json::json!({ "removed": p.path }))
    }

    // --- forge: read -------------------------------------------------------

    #[tool(
        description = "Whether the forge CLI reports an authenticated session.",
        annotations(read_only_hint = true)
    )]
    pub async fn forge_auth_status(&self) -> Result<CallToolResult, ErrorData> {
        ok_json(&self.forge()?.auth_status().await.map_err(forge_err)?)
    }

    #[tool(
        description = "The repository/project on the configured forge (Unsupported on Gitea).",
        annotations(read_only_hint = true)
    )]
    pub async fn forge_repo_view(&self) -> Result<CallToolResult, ErrorData> {
        ok_json(&self.forge()?.repo_view().await.map_err(forge_err)?)
    }

    #[tool(
        description = "Open pull/merge requests on the configured forge.",
        annotations(read_only_hint = true)
    )]
    pub async fn forge_pr_list(&self) -> Result<CallToolResult, ErrorData> {
        ok_json(&self.forge()?.pr_list().await.map_err(forge_err)?)
    }

    #[tool(
        description = "A single pull/merge request by number.",
        annotations(read_only_hint = true)
    )]
    pub async fn forge_pr_view(
        &self,
        Parameters(p): Parameters<PrNumberParams>,
    ) -> Result<CallToolResult, ErrorData> {
        ok_json(&self.forge()?.pr_view(p.number).await.map_err(forge_err)?)
    }

    #[tool(
        description = "The PR/MR's coarse CI status (Unsupported on Gitea).",
        annotations(read_only_hint = true)
    )]
    pub async fn forge_pr_checks(
        &self,
        Parameters(p): Parameters<PrNumberParams>,
    ) -> Result<CallToolResult, ErrorData> {
        ok_json(&self.forge()?.pr_checks(p.number).await.map_err(forge_err)?)
    }

    // --- forge: mutations (gated) -----------------------------------------

    #[tool(
        description = "Open a pull/merge request, returning the CLI's output (the URL on success). Requires --allow-write.",
        annotations(destructive_hint = true)
    )]
    pub async fn forge_pr_create(
        &self,
        Parameters(p): Parameters<PrCreateParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.require_write()?;
        let out = self
            .forge()?
            .pr_create(&p.title, &p.body, p.source, p.target)
            .await
            .map_err(forge_err)?;
        ok_json(&serde_json::json!({ "output": out }))
    }

    #[tool(
        description = "Merge a pull/merge request with a strategy (merge|squash|rebase). Requires --allow-write.",
        annotations(destructive_hint = true)
    )]
    pub async fn forge_pr_merge(
        &self,
        Parameters(p): Parameters<PrMergeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.require_write()?;
        self.forge()?
            .pr_merge(p.number, p.strategy.into())
            .await
            .map_err(forge_err)?;
        ok_json(&serde_json::json!({ "merged": p.number }))
    }

    #[tool(
        description = "Close a pull/merge request without merging. Requires --allow-write.",
        annotations(destructive_hint = true)
    )]
    pub async fn forge_pr_close(
        &self,
        Parameters(p): Parameters<PrCloseParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.require_write()?;
        self.forge()?
            .pr_close(p.number, p.delete_branch)
            .await
            .map_err(forge_err)?;
        ok_json(&serde_json::json!({ "closed": p.number }))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for VcsMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            // Identify as vcs-mcp on the wire. `ServerInfo::new` defaults the
            // server_info to `Implementation::from_build_env()`, whose `env!` is
            // expanded in *rmcp's* crate — so without this it advertises "rmcp".
            .with_server_info(Implementation::new("vcs-mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Drive a git/jj repository (and its forge) through typed tools. Read tools \
                 (repo_*/forge_* queries) are always available; mutating tools require the server \
                 to have been started with --allow-write, and reject calls otherwise.",
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use processkit::{Reply, ScriptedRunner};
    use vcs_core::vcs_git::Git;

    /// A git-backed server over a scripted runner — no real binary, no forge.
    fn git_server(runner: ScriptedRunner, allow_write: bool) -> VcsMcpServer {
        let repo: Arc<dyn VcsRepo> =
            Arc::new(Repo::from_git("/repo", "/repo", Git::with_runner(runner)));
        VcsMcpServer::from_handles(repo, None, allow_write)
    }

    /// The JSON of a successful tool result (serialised wire form).
    fn result_json(r: &CallToolResult) -> String {
        serde_json::to_string(r).expect("CallToolResult serialises")
    }

    // A read tool calls the facade and returns its DTO as JSON.
    #[tokio::test]
    async fn read_tool_returns_dto_json() {
        let server = git_server(
            ScriptedRunner::new().on(["rev-parse"], Reply::ok("main\n")),
            false,
        );
        let out = server.repo_current_branch().await.expect("tool ok");
        assert!(result_json(&out).contains("main"), "{}", result_json(&out));
    }

    // Read tools work even when writes are disabled (the default).
    #[tokio::test]
    async fn read_tool_works_in_readonly_mode() {
        let server = git_server(
            ScriptedRunner::new().on(["status"], Reply::ok(" M a.rs\0")),
            false,
        );
        let out = server.repo_status().await.expect("status ok");
        assert!(result_json(&out).contains("a.rs"));
    }

    // A mutation tool is gated when writes are disabled — it errors WITHOUT
    // reaching the runner. The scripted runner has NO `checkout` rule, so if the
    // gate failed and the tool spawned, the call would error differently than the
    // gate's `--allow-write` message.
    #[tokio::test]
    async fn mutation_is_gated_without_allow_write() {
        let server = git_server(ScriptedRunner::new(), /* allow_write */ false);
        let err = server
            .repo_checkout(Parameters(CheckoutParams {
                reference: "feat".into(),
            }))
            .await
            .expect_err("gated");
        assert!(
            format!("{err:?}").contains("allow-write"),
            "error should mention --allow-write: {err:?}"
        );
    }

    // With writes enabled, the same tool reaches the runner and returns success.
    #[tokio::test]
    async fn mutation_reaches_runner_with_allow_write() {
        let server = git_server(ScriptedRunner::new().on(["checkout"], Reply::ok("")), true);
        let out = server
            .repo_checkout(Parameters(CheckoutParams {
                reference: "feat".into(),
            }))
            .await
            .expect("checkout ok");
        assert!(result_json(&out).contains("feat"));
    }

    // Forge tools report a clear error when no forge was configured.
    #[tokio::test]
    async fn forge_tools_error_without_a_forge() {
        let server = git_server(ScriptedRunner::new(), true);
        let err = server.forge_pr_list().await.expect_err("no forge");
        assert!(
            format!("{err:?}").contains("no forge"),
            "should mention no forge: {err:?}"
        );
    }

    // The macro-generated tool definitions carry the right MCP annotations: read
    // tools are read-only, mutation tools are destructive.
    #[test]
    fn tool_annotations_mark_read_vs_destructive() {
        let read = VcsMcpServer::repo_snapshot_tool_attr();
        assert_eq!(read.annotations.unwrap().read_only_hint, Some(true));
        let write = VcsMcpServer::repo_commit_tool_attr();
        assert_eq!(write.annotations.unwrap().destructive_hint, Some(true));
    }

    // The server identifies itself as `vcs-mcp` on the wire, not rmcp's default
    // build-env identity (which would say "rmcp").
    #[test]
    fn server_info_identifies_as_vcs_mcp() {
        let server = git_server(ScriptedRunner::new(), false);
        let info = server.get_info();
        assert_eq!(info.server_info.name, "vcs-mcp");
        assert_eq!(info.server_info.version, env!("CARGO_PKG_VERSION"));
    }

    /// A no-op MCP client handler for the in-process round-trip.
    #[derive(Clone, Default)]
    struct TestClient;
    impl rmcp::ClientHandler for TestClient {
        fn get_info(&self) -> rmcp::model::ClientInfo {
            rmcp::model::ClientInfo::default()
        }
    }

    // End-to-end through rmcp: an in-process client lists the tools and calls a
    // read tool over an in-memory transport — proving the #[tool_router]/
    // #[tool_handler] wiring routes calls, not just that the methods compile.
    #[tokio::test]
    async fn in_process_client_lists_and_calls_tools() {
        use rmcp::ServiceExt;
        use rmcp::model::CallToolRequestParams;

        let server = git_server(
            ScriptedRunner::new().on(["rev-parse"], Reply::ok("main\n")),
            false,
        );
        let (server_t, client_t) = tokio::io::duplex(4096);
        let server_handle = tokio::spawn(async move {
            if let Ok(running) = server.serve(server_t).await {
                let _ = running.waiting().await;
            }
        });

        let client = TestClient.serve(client_t).await.expect("client connects");

        let tools = client.list_all_tools().await.expect("list_tools");
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
        assert!(names.contains(&"repo_snapshot"), "{names:?}");
        assert!(names.contains(&"repo_commit"), "{names:?}");
        assert!(names.contains(&"forge_pr_list"), "{names:?}");

        let result = client
            .call_tool(CallToolRequestParams::new("repo_current_branch"))
            .await
            .expect("call repo_current_branch");
        let text = result
            .content
            .first()
            .and_then(|c| c.raw.as_text())
            .map(|t| t.text.as_str())
            .expect("text content");
        assert!(text.contains("main"), "{text}");

        let _ = client.cancel().await;
        server_handle.abort();
    }
}
