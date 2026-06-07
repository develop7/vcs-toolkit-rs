# Changelog — vcs-mcp

All notable changes to the `vcs-mcp` crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
This crate is versioned and published independently of the other workspace
crates; tag releases as `vcs-mcp-v<version>`.

## [Unreleased]

### Added
- Initial release: `vcs-mcp`, a Model Context Protocol (MCP) server exposing the
  `vcs-core` (`Repo`) and `vcs-forge` (`Forge`) operations as agent-callable
  tools. A lib (`VcsMcpServer`, hermetically testable) plus the `vcs-mcp` binary,
  which serves MCP over **stdio** for an `mcpServers` config entry. The workspace's
  **first binary crate** and **second runtime-tokio** crate (after `vcs-watch`).
- **Read tools** (always available, annotated `readOnlyHint`): `repo_snapshot`,
  `repo_info`, `repo_status`, `repo_diff_stat`, `repo_branches`,
  `repo_current_branch`, `repo_conflicts`, `repo_worktrees`, `repo_try_merge`
  (a rollback merge probe); forge: `forge_auth_status`, `forge_repo_view`,
  `forge_pr_list`, `forge_pr_view`, `forge_pr_checks`. Each returns the facade DTO
  as JSON (via the facades' optional `serde` feature).
- **Mutating tools** (gated behind `--allow-write`, annotated `destructiveHint`):
  `repo_commit`, `repo_checkout`, `repo_fetch`, `repo_create_worktree`,
  `repo_remove_worktree`; forge: `forge_pr_create`, `forge_pr_merge`,
  `forge_pr_close`. With writes disabled they reject up front
  ("write tools are disabled; restart the server with --allow-write") before
  spawning anything.
- **CLI:** `--repo <path>` (default cwd), `--forge github|gitlab|gitea` (override),
  `--allow-write` (off by default → read-only), `--timeout <seconds>` (per-command
  deadline, default 120; `0` disables), `--help`. The forge is auto-detected from
  the `origin` remote (`ForgeKind::from_remote_url`) — works on a colocated jj
  repo; a pure-jj repo with no git remote has no forge, and the `forge_*` tools
  then return a clear "no forge configured" error.
- **Hardened by default:** the binary opens the repo with a hardened git client
  (`Git::hardened()` — repo hooks and `core.fsmonitor` disabled, repo-redirecting
  `GIT_*` scrubbed, system config skipped), so serving a repository you didn't
  create can't execute its hooks even on a read tool. jj has no repo-local hooks.
  Every git/forge command also runs under the `--timeout` deadline so a stalled
  network call can't hang a request. The server advertises its identity as
  `vcs-mcp` (with the crate version) over the MCP wire.
- The tool logic, write-gating, serialization, and the `#[tool_router]`/
  `#[tool_handler]` wiring are covered hermetically (a `ScriptedRunner`-backed
  `Repo`, plus an in-process rmcp client round-trip over an in-memory duplex
  transport); `#[ignore]` tests drive the read tools and a gated mutation against a
  real temporary git repo.

### Notes
- Built on [`rmcp`](https://crates.io/crates/rmcp) (the official MCP Rust SDK).
  Read-only by default; one coarse `--allow-write` flag enables all mutations
  (a per-tool allowlist and more tools — issues/releases — are future, additive
  work). The wrappers' argv injection guards apply under every tool.

### Changed
-

### Fixed
-

[Unreleased]: https://github.com/ZelAnton/vcs-toolkit-rs/commits/main/crates/mcp
