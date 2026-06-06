//! Shared diff model for the [vcs-toolkit-rs](https://github.com/ZelAnton/vcs-toolkit-rs)
//! workspace.
//!
//! `git diff` and `jj diff --git` emit byte-identical **git-format unified
//! diffs**, so `vcs-git` and `vcs-jj` share one model and one parser instead of
//! each carrying a copy that could silently drift. This crate is the foundation
//! both depend on; it is **dependency-free** (std only) — pure data types and
//! pure functions, no process execution.
//!
//! - [`parse_diff`] turns git-format diff text into one [`FileDiff`] per file
//!   ([`Hunk`]s of [`DiffLine`]s, with the [`ChangeKind`]).
//! - [`DiffStat`] is the aggregate file/line count shape both `--shortstat`
//!   (git) and `--stat` (jj) parse into.
//! - [`Version`] + [`parse_dotted_version`] read a `<tool> --version` banner.
//!
//! The wrapper crates re-export these (e.g. `vcs_git::FileDiff`,
//! `vcs_git::parse_diff`, `vcs_git::GitVersion`), so consumers rarely name this
//! crate directly.

mod diff;
mod version;

pub use diff::{ChangeKind, DiffLine, DiffStat, FileDiff, Hunk, parse_diff};
pub use version::{Version, parse_dotted_version};
