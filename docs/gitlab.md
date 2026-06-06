# vcs-gitlab — GitLab CLI guide

`vcs-gitlab` drives the GitLab CLI (`glab`) from Rust. Every operation is `async`,
runs inside an OS job (via [`processkit`]) so a `glab` subprocess is never
orphaned, and returns the structured `processkit::Error` instead of a stringly
exit. Commands ask for `--output json` and are deserialized into typed structs
(GitLab's REST JSON, which `glab` passes through); the crate never scrapes
human-readable output.

The surface is the **lean merge-request lifecycle** — it mirrors `vcs-github`'s
shape but covers only auth, project view, and the MR lifecycle. The
[`vcs-forge`](forge.md) facade unifies it with `vcs-github` and `vcs-gitea`.

Consumers code against the [`GitLabApi`] trait and substitute a fake in tests —
the real [`GitLab`] client only appears at the edges. See
[Testing & mocking](testing.md) for the two seams (the `mock` feature →
`MockGitLabApi`, or a `ScriptedRunner`).

Requires the `glab` binary on `PATH`, authenticated via `glab auth login`.

[`processkit`]: https://crates.io/crates/processkit

> ⚠️ **CLI surface tracks the installed `glab`, not a frozen contract.** The argv
> the code builds and the JSON shapes it parses are pinned by the hermetic tests
> (so a refactor can't silently change them). The `#[ignore]` integration smoke
> tests additionally check, against the real binary in CI, that `glab` integrates
> at all — `version` and `auth_status`. The create/merge/close **lifecycle** argv
> follows the documented `glab` CLI but is **not** exercised end-to-end in CI (that
> needs a live, authenticated GitLab); confirm it against your installed `glab` if
> a flag ever drifts.

## Construction

```rust
use vcs_gitlab::GitLab;
use std::time::Duration;

let glab = GitLab::new();                                  // real job-backed runner
let glab = GitLab::new().default_timeout(Duration::from_secs(30)); // kill long runs
```

`GitLab::with_runner(runner)` injects a fake `ProcessRunner` for tests.
`glab.at(dir)` returns a [`GitLabAt`] view whose project-scoped methods drop the
leading `dir` argument.

## Auth & version

```rust
# use vcs_gitlab::{GitLab, GitLabApi};
# async fn demo(glab: &GitLab) -> Result<(), processkit::Error> {
let v = glab.version().await?;          // String — "glab version 1.x …"
let authed = glab.auth_status().await?; // bool — true when `glab auth status` exits 0
# Ok(()) }
```

`auth_status` reads the exit code via `probe`: a timeout or unexpected failure
still surfaces as `processkit::Error`, never a silent `false`. **Caveat:** a
[known glab bug](https://gitlab.com/gitlab-org/cli/-/issues/911) can make
`glab auth status` exit `0` even when unauthenticated, so treat a `true` as
best-effort (the next API call is the real test); `false`/timeout are faithful.

## Project & merge requests

| Method | Runs | Returns |
|---|---|---|
| `repo_view(dir)` | `glab repo view --output json` | [`Project`] |
| `mr_list(dir)` | `glab mr list --output json` | `Vec<MergeRequest>` |
| `mr_view(dir, id)` | `glab mr view <id> --output json` | [`MergeRequest`] |
| `mr_create(dir, title, body, source, target)` | `glab mr create --title … --description … [--source-branch …] [--target-branch …] --yes` | `String` (the MR URL) |
| `mr_merge(dir, id, strategy)` | `glab mr merge <id> --yes --auto-merge=false [--squash\|--rebase]` | `()` |
| `mr_ready(dir, id)` | `glab mr update <id> --ready` | `()` |
| `mr_close(dir, id)` | `glab mr close <id>` | `()` |
| `mr_checks(dir, id)` | `glab mr view <id> --output json` (reads `head_pipeline.status`) | [`CiStatus`] |

`MergeRequest` carries `iid` (the project-scoped id the commands take), `title`,
`state` (GitLab's `"opened"`/`"closed"`/`"merged"`/`"locked"`), `source_branch`,
`target_branch`, `web_url`, and `draft`.

```rust
# use std::path::Path;
# use vcs_gitlab::{GitLab, GitLabApi, MergeStrategy};
# async fn demo(glab: &GitLab, repo: &Path) -> Result<(), processkit::Error> {
for mr in glab.mr_list(repo).await? {
    println!("!{} [{}] {} — {}", mr.iid, mr.state, mr.title, mr.web_url);
}
let url = glab
    .mr_create(repo, "Add streaming", "Implements …",
               Some("feat/streaming".into()), Some("main".into()))
    .await?;
glab.mr_merge(repo, 12, MergeStrategy::Squash).await?;
# let _ = url; Ok(()) }
```

[`MergeStrategy`] is `Merge` (glab's default merge commit), `Squash`, or `Rebase`.
`mr_merge` passes `--auto-merge=false` so it merges **immediately** rather than
enabling glab's default merge-when-pipeline-succeeds. [`CiStatus`] buckets the
pipeline into `Passing` / `Failing` / `Pending` / `None`.

## Escape hatch

`run(args)` / `run_raw(args)` (and the inherent `run_args(&[&str])` /
`run_raw_args`) drive any unmodelled `glab` command; `run` returns trimmed stdout
and errors on a non-zero exit, `run_raw` hands back the captured `ProcessResult`.

## See also

- [vcs-forge guide](forge.md) — the facade that unifies this with GitHub/Gitea.
- [vcs-github guide](github.md) — the broader-surfaced sibling this mirrors.
- [Testing & mocking](testing.md) — the `mock` feature and the `ScriptedRunner` seam.
- [Process model & errors](process-model.md) — OS-job containment, timeouts, and
  the `Error` / `ProcessResult` shapes.
- [crate README](../crates/gitlab/README.md) — quickstart and crate-level docs.
