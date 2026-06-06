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
-

### Fixed
-

[Unreleased]: https://github.com/ZelAnton/vcs-toolkit-rs/commits/main/crates/gitea
