# vcs-gitea — Gitea CLI guide

`vcs-gitea` drives the Gitea (and Forgejo) CLI (`tea`) from Rust. Every operation
is `async`, runs inside an OS job (via [`processkit`]) so a `tea` subprocess is
never orphaned, and returns the structured `processkit::Error`. Commands ask for
`--output json` and are deserialized into typed structs (the Gitea REST shape
`tea` marshals); the crate never scrapes human-readable output.

The surface is the **lean pull-request lifecycle** `tea` actually supports. It is
deliberately **narrower** than `vcs-github` / `vcs-gitlab` — see the capability
note below. The [`vcs-forge`](forge.md) facade unifies it with the other two.

Consumers code against the [`GiteaApi`] trait and substitute a fake in tests. See
[Testing & mocking](testing.md) for the two seams (the `mock` feature →
`MockGiteaApi`, or a `ScriptedRunner`).

Requires the `tea` binary on `PATH`, configured via `tea login add`.

[`processkit`]: https://crates.io/crates/processkit

> ⚠️ **CLI surface tracks the installed `tea`, not a frozen contract.** The argv
> the code builds and the JSON it parses are pinned by the hermetic tests; the
> `#[ignore]` integration smoke tests additionally check, against the real binary
> in CI, that `tea` integrates at all (`version` + `auth_status`). The PR
> **lifecycle** argv follows the documented `tea` CLI but is **not** exercised
> end-to-end in CI (that needs a live, authenticated Gitea); confirm it against
> your installed `tea` if a flag ever drifts.

## What `tea` does **not** do

`tea` has no single-PR `view`, no current-repo view, no draft toggle, and no
PR-checks command. Consequences:

- **`pr_view` is synthesized** by listing with `--state all` and filtering by
  number — a missing number is an `Error::Parse`.
- **`repo_view`, `pr_mark_ready`, and `pr_checks` are simply absent** from
  `GiteaApi`. Through the [`vcs-forge`](forge.md) facade they return
  `Error::Unsupported` for the Gitea backend (`err.is_unsupported()`).

## Construction

```rust
use vcs_gitea::Gitea;
let tea = Gitea::new();                 // real job-backed runner
```

`Gitea::with_runner(runner)` injects a fake `ProcessRunner` for tests;
`tea.at(dir)` returns a [`GiteaAt`] view whose repo-scoped methods drop `dir`.

## Auth & version

```rust
# use vcs_gitea::{Gitea, GiteaApi};
# async fn demo(tea: &Gitea) -> Result<(), processkit::Error> {
let v = tea.version().await?;          // String
let authed = tea.auth_status().await?; // bool — a non-empty `tea login list`
# Ok(()) }
```

`tea` has no per-instance `auth status`, so `auth_status` reads
`tea login list --output json` and reports whether at least one login is
configured.

## Pull requests

| Method | Runs | Returns |
|---|---|---|
| `pr_list(dir)` | `tea pr list --output json` | `Vec<PullRequest>` |
| `pr_view(dir, number)` | `tea pr list --state all --output json` + filter | [`PullRequest`] |
| `pr_create(dir, title, body, head, base)` | `tea pr create --title … --description … [--head …] [--base …]` | `String` |
| `pr_merge(dir, number, strategy)` | `tea pr merge <number> --style merge\|rebase\|squash` | `()` |
| `pr_close(dir, number)` | `tea pr close <number>` | `()` |

`PullRequest` carries `number`, `title`, `state` (`"open"`/`"closed"`), `merged`
(a merged PR is also `state="closed"`), `head_branch`, `base_branch`, and `url`
(flattened from Gitea's nested `head.ref` / `base.ref` / `html_url`).

```rust
# use std::path::Path;
# use vcs_gitea::{Gitea, GiteaApi, MergeStrategy};
# async fn demo(tea: &Gitea, repo: &Path) -> Result<(), processkit::Error> {
for pr in tea.pr_list(repo).await? {
    println!("#{} [{}] {} — {}", pr.number, pr.state, pr.title, pr.url);
}
tea.pr_merge(repo, 7, MergeStrategy::Squash).await?;
# Ok(()) }
```

[`MergeStrategy`] is `Merge` / `Squash` / `Rebase`, mapped to `tea pr merge
--style`.

## Escape hatch

`run`/`run_raw` (and the inherent `run_args`/`run_raw_args`) drive any unmodelled
`tea` command — e.g. flipping a Gitea draft (a `WIP:` title prefix) via
`tea pr edit`.

## See also

- [vcs-forge guide](forge.md) — the facade; note the Gitea `Unsupported` ops.
- [vcs-github guide](github.md) — the fuller-surfaced sibling this mirrors.
- [Testing & mocking](testing.md) — the `mock` feature and the `ScriptedRunner` seam.
- [Process model & errors](process-model.md) — OS-job containment, timeouts, and
  the `Error` / `ProcessResult` shapes.
- [crate README](../crates/gitea/README.md) — quickstart and crate-level docs.
