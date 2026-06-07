# Changelog — vcs-forge

All notable changes to the `vcs-forge` crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
This crate is versioned and published independently of the other workspace
crates; tag releases as `vcs-forge-v<version>`.

## [Unreleased]

### Added
- Initial release: a backend-agnostic facade over `vcs-github`, `vcs-gitlab`, and
  `vcs-gitea` — the forge analogue of `vcs-core`. `Forge<R>` is a cwd-bound handle
  dispatching the common forge operations to whichever CLI backs it; the
  object-safe `ForgeApi` trait mirrors the inherent methods for `&dyn ForgeApi`.
- Explicit construction (`Forge::github`/`gitlab`/`gitea` over the real runner;
  `Forge::for_github`/`for_gitlab`/`for_gitea` over an explicit client), plus a
  pure `ForgeKind::from_remote_url` host classifier (forges have no filesystem
  marker, so there is no auto-detection).
- Unified DTOs (`#[non_exhaustive]`): `ForgePr` + `ForgePrState`
  (`Open`/`Closed`/`Merged`, normalising the three forges' state spellings),
  `ForgeRepo`, `CiStatus` (`Passing`/`Failing`/`Pending`/`None`), `MergeStrategy`.
- The lean lifecycle: `auth_status`, `repo_view`, `pr_list`, `pr_view`,
  `pr_create`, `pr_merge`, `pr_mark_ready`, `pr_close`, `pr_checks`.
- An `Error::Unsupported { forge, operation }` variant: Gitea's `tea` has no
  current-repo view, draft toggle, or checks command, so `repo_view`,
  `pr_mark_ready`, and `pr_checks` return it for the Gitea backend (the call does
  not spawn). `Error::is_unsupported()` / `is_transient_fetch_error()` classifiers.
- Optional `serde` feature: derives `serde::Serialize` on the public DTOs
  (`ForgeKind`, `ForgePr`, `ForgePrState`, `ForgeRepo`, `CiStatus`,
  `MergeStrategy`) so a consumer (e.g. `vcs-mcp`) can emit them as JSON. **Off by
  default.**

### Changed
-

### Fixed
-

[Unreleased]: https://github.com/ZelAnton/vcs-toolkit-rs/commits/main/crates/forge
