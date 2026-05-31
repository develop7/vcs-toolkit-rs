# vcs-toolkit-rs

A Rust toolkit for automating **Git**, **Jujutsu**, and **GitHub** through CLI
process execution. Rather than reimplementing each tool's protocol, these crates
shell out to the official binaries (`git`, `jj`, `gh`) and capture their output —
thin, predictable wrappers you can compose into automation.

Every command is **async** (tokio) and runs inside an OS **job** (a Windows Job
Object or a Linux cgroup v2) so the whole process tree dies with the parent — no
orphaned subprocesses. That shared mechanism lives in `vcs-process`, which also
provides timeouts and the structured `CommandError`.

## Crates

This is a Cargo workspace of four crates, each **versioned and published
independently**:

| Crate | Drives | crates.io name |
|---|---|---|
| [`crates/process`](crates/process) | the job-backed process launcher (shared) | `vcs-process` |
| [`crates/git`](crates/git) | the `git` binary | `vcs-git` |
| [`crates/jj`](crates/jj) | the `jj` (Jujutsu) binary | `vcs-jj` |
| [`crates/github`](crates/github) | the `gh` (GitHub CLI) binary | `vcs-github` |

Each wrapper exposes an **interface trait** (`GitApi`/`JjApi`/`GitHubApi`) and a
real client (`Git`/`Jj`/`GitHub`) with typed, repo-scoped async commands that
return parsed structs and fail with the structured `CommandError`. They delegate
process launching to `vcs-process` and depend on `async-trait`; `vcs-github`
additionally adds `serde`/`serde_json` to deserialize `gh … --json` output.

### Built for testing

Consumers code against the trait and substitute a fake in their tests — two seams:

```rust
use vcs_git::{Git, GitApi};
use std::path::Path;

// Production code depends on the interface, not the concrete client:
async fn current(git: &dyn GitApi) -> Result<String, vcs_process::CommandError> {
    git.current_branch(Path::new(".")).await
}

let git = Git::new();              // real, job-backed git
// current(&git).await ...
```

- **Mock the interface** — enable the `mock` feature; `mockall` generates
  `MockGitApi` for stubbing whole methods (`expect_current_branch().returning(…)`).
- **Inject a runner** — `Git::with_runner(vcs_process::ScriptedRunner::new()…)`
  feeds canned binary output through the *real* argument-building and parsing.

## Build, test

```bash
cargo build                         # build all crates
cargo test                          # unit + integration tests (whole workspace)
cargo test -p vcs-git               # one crate
cargo test -- --ignored             # tests that require the real binaries installed
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
```

Tests that shell out to the real `git` / `jj` / `gh` binaries are marked
`#[ignore]` so CI stays hermetic; run them locally with `--ignored`.

## Publishing

Each crate releases on its own cadence. Bump the `version` in that crate's
`Cargo.toml` (the single source of truth), update its `CHANGELOG.md`, tag as
`<crate>-v<version>` (e.g. `vcs-git-v0.2.0`), then `cargo publish -p <crate>`.
The `Release` GitHub Action (`workflow_dispatch`) automates the bump, changelog
promotion, tag, and publish for a chosen crate.

**Publish order:** `vcs-process` must be on crates.io *before* the wrappers,
since `vcs-git`/`vcs-jj`/`vcs-github` depend on it by version. Release
`vcs-process` first whenever its version changed.

## Conventions

See [AGENTS.md](AGENTS.md) for code style, dependency management (every
dependency gets a "why" comment; no fixed allow-list), the per-crate changelog
process, and the `jj` version-control workflow.
