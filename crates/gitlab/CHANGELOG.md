# Changelog — vcs-gitlab

All notable changes to the `vcs-gitlab` crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
This crate is versioned and published independently of the other workspace
crates; tag releases as `vcs-gitlab-v<version>`.

## [Unreleased]

### Added
- Initial release: `GitLabApi` trait + `GitLab` client wrapping the `glab` CLI,
  mirroring `vcs-github`'s shape (async, `#[non_exhaustive]` DTOs, the structured
  `processkit::Error`, the `mock` feature → `MockGitLabApi`, and the
  `GitLab::with_runner` scripted-runner seam).
- The **lean merge-request lifecycle**, deserializing `glab … --output json`
  (GitLab's REST JSON): `auth_status`, `repo_view` (`Project`),
  `mr_list`/`mr_view` (`MergeRequest`), `mr_create(title, body, source, target)`
  → URL, `mr_merge(id, MergeStrategy)` (merges **immediately** via
  `--auto-merge=false`, overriding glab's default merge-when-pipeline-succeeds;
  `--squash`/`--rebase`/default merge), `mr_ready`, `mr_close`, and `mr_checks`
  → `CiStatus` (the MR's bucketed `head_pipeline.status`).
- `auth_status` documents the glab exit-code caveat ([gitlab-org/cli#911]): a
  `true` is best-effort (glab can exit 0 while unauthenticated); `false`/timeout
  are faithful.

[gitlab-org/cli#911]: https://gitlab.com/gitlab-org/cli/-/issues/911
- Raw escape hatches `run`/`run_raw` (+ inherent `run_args`/`run_raw_args`), and
  a `GitLab::at(dir)` → `GitLabAt` bound view mirroring every project-scoped
  method.

### Changed
- `Project.visibility` is now `Option<String>` (absent in the JSON → `None`
  instead of a misleading empty string).
- `auth_status` reports `false` on **any** non-zero exit (was: errored on exits
  other than 0/1), matching its "reports the bool, must not error" contract.

### Fixed
- `mr_list` passes `--per-page 100` — glab's default of 30 silently truncated
  larger result sets. The cap is now explicit and documented.

[Unreleased]: https://github.com/ZelAnton/vcs-toolkit-rs/commits/main/crates/gitlab
