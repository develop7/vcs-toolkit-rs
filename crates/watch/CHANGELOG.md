# Changelog — vcs-watch

All notable changes to the `vcs-watch` crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
This crate is versioned and published independently of the other workspace
crates; tag releases as `vcs-watch-v<version>`.

## [Unreleased]

### Added
- Initial release: `RepoWatcher` filesystem-watches a git/jj repository and
  streams typed `RepoEvent`s. On each filesystem change it debounces the burst,
  re-queries `vcs-core`'s batched `Repo::snapshot()` (+ `local_branches()`), and
  diffs against the previous state — so raw-event noise (ref temp-renames,
  `index.lock`, reflog churn) coalesces into one re-check instead of spurious
  events.
- `RepoEvent` (`#[non_exhaustive]`): `HeadMoved`, `BranchSwitched`,
  `BranchCreated`/`BranchDeleted`, `WorkingCopyChanged`, `UpstreamChanged`,
  `AheadBehindChanged`, `OperationChanged`, `ConflictChanged`. Each settled change
  arrives as a `RepoChange { snapshot, events }` — the new full `RepoSnapshot`
  (re-exported from `vcs-core`) plus the deltas; `recv()` / `current()` consume it.
- Builder: `working_tree(bool)` (default off — state-dir-only watching; opt in to
  also watch the working tree for bare unstaged edits), `debounce(Duration)`
  (default 250 ms), `max_wait(Duration)` (default 1 s). Backend + watch dir come
  from `vcs-core`'s pure `detect` (`.jj` wins when colocated; worktree gitlinks
  resolved). Dropping the `RepoWatcher` stops the watch and the background task.
- The pure snapshot-`diff` is hermetically unit-tested; the notify → debounce →
  re-query → emit pipeline is covered by `#[ignore]` real-repo integration tests
  (git + jj).

### Notes
- This is the workspace's **first runtime tokio dependency** (everything else
  hides tokio behind `processkit`) and **first streaming API** — build/await the
  watcher inside a tokio runtime. Transient mid-operation re-query failures are
  skipped and retried on the next event (settled-state semantics).

### Changed
-

### Fixed
-

[Unreleased]: https://github.com/ZelAnton/vcs-toolkit-rs/commits/main/crates/watch
