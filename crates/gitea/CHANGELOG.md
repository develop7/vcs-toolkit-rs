# Changelog — vcs-gitea

All notable changes to the `vcs-gitea` crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
This crate is versioned and published independently of the other workspace
crates; tag releases as `vcs-gitea-v<version>`.

## [Unreleased]

### Added
- Initial release: `GiteaApi` trait + `Gitea` client wrapping the `tea` CLI,
  mirroring `vcs-github`'s shape (async, `#[non_exhaustive]` DTOs, the structured
  `processkit::Error`, the `mock` feature → `MockGiteaApi`, and the
  `Gitea::with_runner` scripted-runner seam).
- The **lean pull-request lifecycle** `tea` supports, deserializing
  `tea … --output json` (the Gitea REST shape): `auth_status` (a non-empty
  `login list`), `pr_list` (`PullRequest`), `pr_view` (synthesized by listing
  with `--state all` and filtering by number — `tea` has no single-PR view),
  `pr_create(title, body, head, base)`, `pr_merge(number, MergeStrategy)`
  (`--style merge|rebase|squash`), and `pr_close`.
- Raw escape hatches `run`/`run_raw` (+ inherent `run_args`/`run_raw_args`), and
  a `Gitea::at(dir)` → `GiteaAt` bound view mirroring every repo-scoped method.

### Notes
- Deliberately narrower than `vcs-github`/`vcs-gitlab`: `tea` exposes no
  current-repo view, no draft toggle, and no PR-checks command, so `repo_view`,
  `pr_mark_ready`, and `pr_checks` are absent (the `vcs-forge` facade reports them
  as `Unsupported` for the Gitea backend).

### Changed
- Bumped `processkit` to **0.7** — the re-exported `Error` is now
  `#[non_exhaustive]` and gains variants (`NotReady`, `Unsupported`;
  `Cancelled`/`ResourceLimit` behind features), `Command` is `#[must_use]`,
  and `ProcessResult` gains `program()`. Breaking for consumers that match
  the re-exported types exhaustively.
- `auth_status` tolerates a non-zero `tea login list` exit (e.g. no config file
  yet) and reports `false` instead of erroring, matching its "reports the bool,
  must not error" contract.
- `pr_create` doc: tea prints a textual summary (no URL) and has no flag to
  shape the create output — documented instead of implied parity with gh/glab.

### Fixed
- `pr_list` passes `--limit 100` (tea's default page of 30 silently truncated
  larger sets), and `pr_view` — which lists and filters by number — uses
  `--limit 999`, so a PR beyond the first page is no longer a false "not found"
  (PRs beyond 999 still are; documented).

[Unreleased]: https://github.com/ZelAnton/vcs-toolkit-rs/commits/main/crates/gitea
