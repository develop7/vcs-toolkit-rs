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

`tea` has no single-PR `view`, no current-repo view, no draft toggle, no
PR-checks command, and no single-release view (`tea releases` ignores any
positional and always lists). Consequences:

- **`pr_view` is synthesized** by listing with `--state all` and filtering by
  number — a missing number is an `Error::Parse`. (`issue_view`, by contrast, is
  a *first-class* `tea issues <index>` — see [Issues & releases](#issues--releases).)
- **`repo_view`, `pr_mark_ready`, `pr_checks`, and `release_view` are simply
  absent** from `GiteaApi`. Through the [`vcs-forge`](forge.md) facade they return
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
| `pr_create(dir, spec)` | `tea pr create --title … --description … [--head …] [--base …]` | `String` |
| `pr_merge(dir, number, strategy)` | `tea pr merge <number> --style merge\|rebase\|squash` | `()` |
| `pr_close(dir, number)` | `tea pr close <number>` | `()` |

`PullRequest` carries `number`, `title`, `state` (`"open"`/`"closed"`), `merged`
(a merged PR is also `state="closed"`), `head_branch`, `base_branch`, and `url`
(flattened from Gitea's nested `head.ref` / `base.ref` / `html_url`).

```rust
# use std::path::Path;
# use vcs_gitea::{Gitea, GiteaApi, MergeStrategy, PrCreate};
# async fn demo(tea: &Gitea, repo: &Path) -> Result<(), processkit::Error> {
for pr in tea.pr_list(repo).await? {
    println!("#{} [{}] {} — {}", pr.number, pr.state, pr.title, pr.url);
}
let out = tea
    .pr_create(repo, PrCreate::new("Add streaming", "Implements …")
        .head("feat/streaming").base("main"))
    .await?;
tea.pr_merge(repo, 7, MergeStrategy::Squash).await?;
# let _ = out; Ok(()) }
```

[`MergeStrategy`] is `Merge` / `Squash` / `Rebase`, mapped to `tea pr merge
--style`.

`pr_create` takes a [`PrCreate`] spec — build it through `PrCreate::new(title,
body)` and chain the optional `.head(b)` (`--head`; `None` = the current branch) /
`.base(b)` (`--base`; `None` = the repo default) setters. Public fields:
`title: String`, `body: String`, `head: Option<String>`, `base: Option<String>`.
Unlike `gh`/`glab`, `tea` prints a **textual summary** on success, not the new
PR's URL (it has no flag to shape create output), so do **not** parse the returned
`String` as a URL.

## Issues & releases

| Method | Runs | Returns |
|---|---|---|
| `issue_list(dir)` | `tea issues list --limit 100 --output json` | `Vec<Issue>` |
| `issue_view(dir, number)` | `tea issues <number> --output json` | [`Issue`] |
| `issue_create(dir, title, body)` | `tea issues create --title … --description …` | `String` |
| `release_list(dir)` | `tea releases list --limit 100 --output json` | `Vec<Release>` |

The list methods pin `--limit 100` so tea's default page size of 30 can't silently
truncate them; reach beyond 100 through `run`. Unlike `pr_view` (which lists and
filters), **`issue_view` is a first-class single-issue view** — `tea issues
<number>` (the bare-index form), deserializing one object. `issue_create`, like
`pr_create`, returns tea's textual summary verbatim — its final line is the new
issue's URL, but there is no flag to shape the output, so it is **not** a parsed
URL. There is intentionally **no `release_view`**: `tea releases` takes no
positional and always lists, so a single-release-by-tag view doesn't exist in
`tea` (the [`vcs-forge`](forge.md) facade reports it `Unsupported`).

`Issue` carries `number` (Gitea's `number`; tea's `index` is accepted as an
alias), `title`, `state` (`"open"`/`"closed"`), `body`, and `url` (Gitea's
`html_url`).

`Release` carries `tag` (Gitea's `tag_name`), `title` (Gitea's `name`),
`published_at` (e.g. `"2023-07-26T13:02:36Z"`, empty for an unpublished draft),
`draft`, `prerelease`, and `url` (Gitea's `html_url`).

```rust
# use std::path::Path;
# use vcs_gitea::{Gitea, GiteaApi};
# async fn demo(tea: &Gitea, repo: &Path) -> Result<(), processkit::Error> {
for issue in tea.issue_list(repo).await? {
    println!("#{} [{}] {}", issue.number, issue.state, issue.title);
}
let one = tea.issue_view(repo, 7).await?;        // first-class single-issue view
for rel in tea.release_list(repo).await? {
    println!("{} — {}", rel.tag, rel.title);
}
# let _ = one; Ok(()) }
```

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
