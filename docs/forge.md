# vcs-forge — the forge facade

`vcs-forge` is a **forge-agnostic facade** over [`vcs-github`](github.md),
[`vcs-gitlab`](gitlab.md), and [`vcs-gitea`](gitea.md) — the `gh`/`glab`/`tea`
analogue of how [`vcs-core`](core.md) sits over git and jj. A [`Forge`] handle
dispatches the common forge operations to whichever CLI backs it and returns
**unified DTOs**, so a tool can target "the forge" instead of one specifically.

Consumers can hold a `&dyn ForgeApi` to stay generic over the runner; build a
`Forge` over a fake runner for hermetic tests.

## No auto-detection — construct explicitly

A repository has a filesystem marker (`.git`/`.jj`) that [`vcs-core`](core.md)
detects; a **forge does not** — it's identified by the remote *host*. So a
`Forge` is built explicitly:

```rust
use vcs_forge::{Forge, ForgeApi};

let forge = Forge::github(".");   // or ::gitlab(".") / ::gitea(".")
```

[`ForgeKind::from_remote_url`] is a pure, best-effort helper for picking the kind
from a remote URL you already hold (e.g. from a `vcs_core::Repo`):

```rust
use vcs_forge::{Forge, ForgeKind};

# fn pick(url: &str) -> Forge {
let forge = match ForgeKind::from_remote_url(url) {
    Some(ForgeKind::GitLab) => Forge::gitlab("."),
    Some(ForgeKind::Gitea)  => Forge::gitea("."),
    _                       => Forge::github("."), // github.com or unknown
};
# forge }
```

It recognises the **public SaaS** hosts — `github.com`, `gitlab.com`,
`gitea.com`, `codeberg.org`, and their proper subdomains — with an anchored
match, so a lookalike like `gitlab.com.attacker.net` returns `None`, not GitLab. A
**self-hosted** instance on an arbitrary domain also returns `None`
(indistinguishable by host alone — pick the kind yourself).

`Forge::for_github(cwd, client)` / `for_gitlab` / `for_gitea` take an explicit
client (the test seam); `forge.at(dir)` re-binds the cwd, sharing the client.

## Operations

```rust
pub async fn auth_status(&self)  -> Result<bool>;
pub async fn repo_view(&self)    -> Result<ForgeRepo>;
pub async fn pr_list(&self)      -> Result<Vec<ForgePr>>;
pub async fn pr_view(&self, number: u64) -> Result<ForgePr>;
pub async fn pr_create(&self, title: &str, body: &str,
                       source: Option<String>, target: Option<String>) -> Result<String>;
pub async fn pr_merge(&self, number: u64, strategy: MergeStrategy) -> Result<()>;
pub async fn pr_mark_ready(&self, number: u64) -> Result<()>;
pub async fn pr_close(&self, number: u64, delete_branch: bool) -> Result<()>;
pub async fn pr_checks(&self, number: u64) -> Result<CiStatus>;
```

Every method mirrors an inherent method on [`Forge`]; the object-safe `ForgeApi`
trait adds nothing but the `&dyn` boundary.

## Unified DTOs

[`ForgePr`] generalises GitHub's PR, GitLab's MR, and Gitea's PR: `number` (the id
each CLI takes — GitLab's `iid`), `title`, `state` ([`ForgePrState`]),
`source_branch`, `target_branch`, `url`, `draft`.

**State normalisation** ([`ForgePrState`]):

| Forge | "open" | "closed" | "merged" |
|---|---|---|---|
| GitHub | `OPEN` | `CLOSED` | `MERGED` |
| GitLab | `opened` | `closed` / `locked` | `merged` |
| Gitea | `state="open"` | `state="closed"` | `merged=true` |

[`ForgeRepo`] is `name` / `owner` / `default_branch` / `url` / `private` (GitLab's
owner is the namespace path). [`CiStatus`] is `Passing` / `Failing` / `Pending` /
`None` — GitHub aggregates its per-check buckets into it, GitLab maps its pipeline
status. [`MergeStrategy`] (`Merge` / `Squash` / `Rebase`) maps to each CLI's flag.

`draft` is **best-effort**: only GitLab reports it on the lean surface; GitHub and
Gitea report `false` (their lean JSON doesn't carry the flag).

## Capability matrix

The CLIs differ in coverage. Gitea's `tea` lacks three operations, which return
[`Error::Unsupported { forge, operation }`] (the call does **not** spawn);
`delete_branch` on `pr_close` is GitHub-only.

| Operation | GitHub | GitLab | Gitea |
|---|:---:|:---:|:---:|
| `auth_status` / `pr_list` / `pr_view` / `pr_create` / `pr_merge` / `pr_close` | ✅ | ✅ | ✅ |
| `repo_view` | ✅ | ✅ | ❌ Unsupported |
| `pr_mark_ready` | ✅ | ✅ | ❌ Unsupported |
| `pr_checks` | ✅ | ✅ | ❌ Unsupported |
| `pr_close` honours `delete_branch` | ✅ | ignored | ignored |

```rust
# use vcs_forge::{Forge, ForgeApi, Error};
# async fn demo(forge: &Forge) {
match forge.pr_checks(7).await {
    Ok(status) => println!("CI: {status:?}"),
    Err(e) if e.is_unsupported() => println!("this forge has no checks command"),
    Err(e) => eprintln!("{e}"),
}
# }
```

`Error` is `Forge(processkit::Error)` or `Unsupported { forge, operation }`, with
`is_unsupported()` and `is_transient_fetch_error()` classifiers.

## See also

- [vcs-github](github.md) / [vcs-gitlab](gitlab.md) / [vcs-gitea](gitea.md) — the
  wrapped clients and their per-CLI surfaces.
- [vcs-core guide](core.md) — the sibling facade over git/jj.
- [Cookbook](cookbook.md) — the open-a-PR recipe.
- [Process model & errors](process-model.md) — OS-job containment and the `Error`
  shapes underneath.
- [crate README](../crates/forge/README.md) — quickstart and crate-level docs.
