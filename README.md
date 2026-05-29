# vcs-toolkit-rs

A Rust toolkit for automating **Git**, **Jujutsu**, and **GitHub** through CLI
process execution. Rather than reimplementing each tool's protocol, these crates
shell out to the official binaries (`git`, `jj`, `gh`) and capture their output —
thin, predictable wrappers you can compose into automation.

## Crates

This is a Cargo workspace of three crates, each **versioned and published
independently**:

| Crate | Drives | crates.io name |
|---|---|---|
| [`crates/git`](crates/git) | the `git` binary | `vcs-git` |
| [`crates/jj`](crates/jj) | the `jj` (Jujutsu) binary | `vcs-jj` |
| [`crates/github`](crates/github) | the `gh` (GitHub CLI) binary | `vcs-github` |

Each crate is dependency-free at its core and exposes the same shape: a `run`
helper that executes the underlying binary with arbitrary arguments, plus
typed wrappers built on top.

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

## Conventions

See [AGENTS.md](AGENTS.md) for code style, dependency management (every
dependency gets a "why" comment; no fixed allow-list), the per-crate changelog
process, and the `jj` version-control workflow.
