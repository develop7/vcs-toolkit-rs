# vcs-watch — repo-event stream

`vcs-watch` filesystem-watches a git or jj repository and streams **typed
state-change events** — the foundation for prompts, status bars, TUIs, and
daemons. It's built on [`vcs-core`](core.md): on each filesystem change it
**re-queries** the repo's batched [`snapshot`](core.md#status--files), **diffs** it
against the previous state, and emits the deltas.

```rust
use vcs_core::Repo;
use vcs_watch::{RepoWatcher, RepoEvent};

# async fn demo() -> vcs_watch::Result<()> {
let repo = Repo::open(".")?;
let mut watcher = RepoWatcher::watch(repo).await?;
while let Some(change) = watcher.recv().await {
    for event in &change.events {
        match event {
            RepoEvent::HeadMoved { to, .. }      => println!("head → {to:?}"),
            RepoEvent::BranchCreated { name }    => println!("+branch {name}"),
            RepoEvent::WorkingCopyChanged { dirty, .. } => println!("dirty={dirty}"),
            other => println!("{other:?}"),
        }
    }
    // `change.snapshot` is the fresh full state — render a status line from it.
}
# Ok(()) }
```

## Why re-query + diff (not raw events)

Interpreting raw filesystem events is a trap: git writes refs through a temp-file
rename, churns `index.lock`, and appends to `.git/logs/` constantly. `vcs-watch`
treats **any** event as "something changed — re-check", coalesces the burst, takes
one fresh [`RepoSnapshot`](core.md#reposnapshot) (+ the branch list), and diffs.
Noise that doesn't change observable state produces **no** event. This also means
a stray event can't desync the consumer — every emission carries the true current
state.

## Events

[`RepoEvent`] (`#[non_exhaustive]`), derived by diffing two snapshots:

| Event | Fires when |
|---|---|
| `HeadMoved { from, to }` | the working-copy commit id changed (commit, checkout, reset, jj op) |
| `BranchSwitched { from, to }` | the *current* branch/bookmark changed (or detached → `None`) |
| `BranchCreated { name }` / `BranchDeleted { name }` | a local branch/bookmark appeared / was removed |
| `WorkingCopyChanged { dirty, change_count }` | dirtiness or the changed-path *count* moved |
| `UpstreamChanged { upstream }` | the upstream tracking branch changed (git only) |
| `AheadBehindChanged { ahead, behind }` | ahead/behind vs upstream changed (git only) |
| `OperationChanged { from, to }` | a git merge/rebase started or finished (**git only**) |
| `ConflictChanged { conflicted }` | the unresolved-conflict flag toggled (both backends) |

Two semantics worth knowing:

- **Conflicts → `ConflictChanged`, on both backends.** `OperationChanged` covers
  only git's merge/rebase lifecycle (`Clear`/`Merge`/`Rebase`); it never fires on
  jj. `vcs-core` derives jj's `operation` and `conflicted` from the same bit, so a
  jj conflict appearing would otherwise double-signal — the redundant
  `OperationChanged` is suppressed, and `ConflictChanged` is the one true conflict
  event everywhere. (A git merge that *has* conflicts is two distinct facts and
  fires both `OperationChanged` and `ConflictChanged`.)
- **`WorkingCopyChanged` is dirty-flag + path *count*, not file identity.**
  Swapping *which* file is edited while the count stays the same emits **nothing**
  (the status-line count is unchanged anyway). A consumer that needs the file set
  reads `change.snapshot` / calls `Repo::changed_files()`.

Each settled change arrives as a [`RepoChange`] `{ snapshot: RepoSnapshot, events:
Vec<RepoEvent> }` — `events` is never empty, and the events come in a stable order
(head, branch switch, created, deleted, working copy, upstream, ahead/behind,
operation, conflict; created/deleted names sorted).

## Building the watcher

```rust
# use std::time::Duration;
# use vcs_core::Repo;
# use vcs_watch::RepoWatcher;
# async fn demo(repo: Repo) -> vcs_watch::Result<()> {
let watcher = RepoWatcher::builder(repo)
    .working_tree(true)                       // also watch the working tree
    .debounce(Duration::from_millis(150))     // quiet window (default 250 ms)
    .max_wait(Duration::from_secs(2))         // re-query ceiling (default 1 s)
    .build()
    .await?;
# let _ = watcher; Ok(()) }
```

- **`recv().await -> Option<RepoChange>`** — the next settled change; `None` once
  the watcher is dropped. `current() -> &RepoSnapshot` is the last known state —
  the build-time baseline, advanced **only when you call `recv`** (it is as fresh
  as your last `recv`, not a live view). There is no `Stream` impl yet (the
  receiver is internal and `recv` maintains `current()`); drive it with the
  `recv()` loop — a `futures::Stream` adapter is a possible future addition.
- **Drop stops everything** — dropping the `RepoWatcher` ends the OS watch and the
  background task.

### Watch scope — state dir vs working tree

By default the watcher monitors only the **state directory** (`.git`/`.jj`):
HEAD, refs, the index, packed-refs, merge/rebase markers, the jj op log. This is
cheap and robust, and catches structural changes plus anything that touches the
index (staging, commit) or a jj snapshot. A **bare unstaged edit** (`vim file`
with no `git add`) doesn't touch the state dir, so it's seen only once it's staged
— unless you opt into **`working_tree(true)`**, which also watches the working
tree recursively and fires `WorkingCopyChanged` immediately. The trade-off:
working-tree watching is `.gitignore`-unaware (it also watches `target/` etc.) and
heavier on a large repo.

## Backends, colocation, worktrees

The backend (and which dir to watch) comes from `vcs-core`'s pure `detect`: `.jj`
for jj, `.git` for git, and **jj wins when colocated** — so a colocated repo is
watched via `.jj` (jj drives; its op-log change is the signal). A linked
worktree's `.git` is a gitlink *file*; the watcher resolves it to that worktree's
git directory (best-effort). One limitation there: a linked worktree's git dir
holds the per-worktree HEAD/index, but **branch refs are shared** under the main
`.git/refs` — so `BranchCreated`/`BranchDeleted` made from another checkout may not
be observed from a linked worktree (HEAD moves and working-copy changes still
are). Watch the main checkout if you need branch events.

## Semantics & limits

- **Transient re-query failures are skipped, not surfaced.** A snapshot taken
  while an operation holds `index.lock` may fail; the watcher skips that re-check
  and the next event re-queries the settled state. Setup failures (the watch can't
  start) surface from `build()`. Enable the `tracing` feature for a debug line on
  each skip.
- **Runtime.** Unlike the rest of the toolkit, `vcs-watch` uses **tokio at
  runtime** (the watch task + debounce timer). Build/await it inside a tokio
  runtime.

## See also

- [vcs-core guide](core.md) — the `Repo`/`RepoSnapshot` it re-queries.
- [Cookbook](cookbook.md) — a live status-line recipe.
- [crate README](../crates/watch/README.md) — quickstart.
