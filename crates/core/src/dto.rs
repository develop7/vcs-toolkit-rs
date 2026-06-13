//! Backend-agnostic data types the facade returns, generalising the per-tool
//! shapes of `vcs-git` and `vcs-jj` into one set a consumer can use without
//! knowing which backend is in play.

use std::path::PathBuf;

/// Which version-control tool backs a [`Repo`](crate::Repo).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[non_exhaustive]
pub enum BackendKind {
    /// A plain Git repository.
    Git,
    /// A Jujutsu repository (possibly colocated with Git).
    Jj,
}

impl BackendKind {
    /// The tool's short name (`"git"` / `"jj"`).
    pub fn as_str(self) -> &'static str {
        match self {
            BackendKind::Git => "git",
            BackendKind::Jj => "jj",
        }
    }
}

/// How a file changed in the working copy — the shared [`vcs_diff::ChangeKind`]
/// (one type across the wrappers and the facade, no remapping). The status-code
/// mappers in the backends turn git's `XY` codes / jj's letters into it.
pub use vcs_diff::ChangeKind;

/// One changed path in the working copy, unified across `git status` /
/// `jj diff --summary`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[non_exhaustive]
pub struct FileChange {
    /// The path (the *new* path for a rename).
    pub path: String,
    /// The original path for a rename, populated by **both** backends (git's
    /// `R old -> new` status; jj's `{old => new}` diff-summary form); `None`
    /// for non-renames.
    pub old_path: Option<String>,
    /// How the file changed.
    pub kind: ChangeKind,
}

/// Aggregate insertion/deletion counts for the working copy — the shared
/// [`vcs_diff::DiffStat`], returned by the backends directly (no remapping).
pub use vcs_diff::DiffStat;

/// One attached worktree (git) / workspace (jj).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[non_exhaustive]
pub struct WorktreeInfo {
    /// Filesystem path of the worktree's working copy.
    pub path: PathBuf,
    /// The branch (git) or first bookmark (jj) on it; `None` when detached/none.
    pub branch: Option<String>,
    /// The checked-out commit; `None` when unavailable (e.g. a bare git entry).
    pub commit: Option<String>,
    /// A bare git worktree entry (always `false` for jj).
    pub is_bare: bool,
}

/// Whether the working copy is mid-operation, unified across the backends'
/// different models: git exposes an in-progress merge or rebase as on-disk state
/// (`MERGE_HEAD` / a `rebase-*` dir), while jj has no multi-step operations — it
/// records a conflict directly on the working-copy change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[non_exhaustive]
pub enum OperationState {
    /// No operation in progress and no conflict.
    Clear,
    /// A git merge is in progress (`MERGE_HEAD` present).
    Merge,
    /// A git rebase is in progress (a `rebase-merge`/`rebase-apply` dir present).
    Rebase,
    /// The working copy has an unresolved conflict (chiefly jj, which records
    /// conflicts on the change rather than pausing an operation).
    Conflict,
}

/// Upstream tracking for the current branch: the upstream ref and how far the
/// branch is ahead/behind it. Only meaningful as a whole — git reports the three
/// together or not at all — so [`RepoSnapshot`] carries it as one
/// `Option<UpstreamTracking>` rather than three coupled `Option`s.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[non_exhaustive]
pub struct UpstreamTracking {
    /// The upstream tracking branch, e.g. `"origin/main"`.
    pub branch: String,
    /// Commits the local branch is ahead of the upstream.
    pub ahead: usize,
    /// Commits the local branch is behind the upstream.
    pub behind: usize,
}

/// A one-shot snapshot of the common repository state — branch, upstream
/// tracking, ahead/behind, dirtiness, and operation state — gathered in **one or
/// two** process spawns instead of a call per field. The data a prompt, status
/// line, or TUI refresh needs. See [`Repo::snapshot`](crate::Repo::snapshot).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[non_exhaustive]
pub struct RepoSnapshot {
    /// The working-copy commit's **full** object id (git `HEAD` oid / jj `@`
    /// commit id) on both backends; `None` on an unborn git repo. Truncate for
    /// display.
    pub head: Option<String>,
    /// Current branch (git) / bookmark (jj); `None` when detached or unset.
    pub branch: Option<String>,
    /// Upstream tracking and how far the branch is ahead/behind it, as one unit —
    /// `Some` only when an upstream is configured, `None` otherwise (and **always
    /// `None` on jj**, which has no git-style upstream tracking). Bundling the
    /// three together makes the "all-or-nothing" relationship unrepresentable as a
    /// half-populated state. See [`UpstreamTracking`].
    pub tracking: Option<UpstreamTracking>,
    /// Whether the working copy has any uncommitted change (tracked or untracked).
    pub dirty: bool,
    /// Number of changed paths (tracked + untracked on git; the `@` change's
    /// files on jj).
    pub change_count: usize,
    /// Whether the working copy has an unresolved conflict.
    pub conflicted: bool,
    /// In-progress operation / conflict state (see [`OperationState`]).
    pub operation: OperationState,
}

/// The outcome of a [`try_merge`](crate::Repo::try_merge) probe. The probe
/// itself is rolled back before it returns, whatever the outcome — this only
/// *reports* what a real merge would do.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
// Adjacently tagged so the JSON is a *type-stable object* for both outcomes —
// `{"outcome":"Clean"}` and `{"outcome":"Conflicts","files":[…]}` — rather than
// serde's default externally-tagged shape, which would emit a bare string
// `"Clean"` for one variant and an object for the other (a polymorphic result an
// agent consumer can't branch on uniformly).
#[cfg_attr(feature = "serde", serde(tag = "outcome", content = "files"))]
#[non_exhaustive]
pub enum MergeProbe {
    /// The merge would apply without conflicts.
    Clean,
    /// The merge would conflict in these paths (repo-relative, `/` separators —
    /// the same contract as [`conflicted_files`](crate::Repo::conflicted_files)).
    Conflicts(Vec<String>),
}

impl MergeProbe {
    /// Whether the probe found no conflicts.
    pub fn is_clean(&self) -> bool {
        matches!(self, MergeProbe::Clean)
    }
}

/// How a worktree was materialised. The facade always reports
/// [`Plain`](CreateOutcome::Plain); the [`CowCloned`](CreateOutcome::CowCloned)
/// variant exists so a consumer that layers a copy-on-write strategy on top can
/// reuse this type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[non_exhaustive]
pub enum CreateOutcome {
    /// The tool materialised the working copy itself.
    Plain,
    /// A copy-on-write clone populated the working copy (consumer-supplied).
    CowCloned,
}

/// One commit in a [`Repo::log`](crate::Repo::log) result — enriched over the
/// wrappers' sparse `Commit`/`Change` with parent hashes, author/committer
/// timestamps, an optional commit body, and the per-commit changed paths
/// (populated when [`LogSpec::with_files`] is `true`; empty otherwise so a
/// caller never has to branch on a flag at the consumption site).
///
/// Backend nuance: jj's identity is the **change id** (stable across amends);
/// the *commit id* is the working-copy revision. The `sha` field carries the
/// commit id (the value `git log` and `jj log` would both identify the commit
/// by in their respective range syntax). A `jj`-specific change id is **not**
/// surfaced here — it lives in the wrapper's `Change` and is intentionally
/// dropped on the way to the facade, because the facade's contract is "the
/// commit hash, regardless of backend." Reach for
/// [`Repo::jj`](crate::Repo::jj) if you need it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[non_exhaustive]
pub struct LogEntry {
    /// Full commit hash.
    pub sha: String,
    /// Parent commit hashes (`rev-parse %P`); empty for a root commit, two
    /// entries for an octopus merge.
    pub parents: Vec<String>,
    /// Author name (`%an` / jj `author.email()`-style string).
    pub author: String,
    /// Author timestamp, RFC 3339 / ISO 8601 (git `%aI` / jj `author.timestamp()`).
    pub authored_at: String,
    /// Committer name (git only — jj has no separate committer).
    pub committer: String,
    /// Committer timestamp, RFC 3339 (git only — empty on jj).
    pub committed_at: String,
    /// First line of the commit message.
    pub summary: String,
    /// The full commit message body, **excluding** the summary line and the
    /// trailing blank line; `None` when the message is single-line or
    /// unavailable (jj templates only emit the first line by default).
    pub body: Option<String>,
    /// Per-commit changed paths; empty unless
    /// [`LogSpec::with_files`](crate::LogSpec::with_files) was set.
    pub files: Vec<FileChange>,
}

/// The query spec for [`Repo::log`](crate::Repo::log). All fields are optional;
/// an empty `LogSpec { ..Default::default() }` returns the most recent commit.
///
/// `range` is the most useful field: a git range (`main..HEAD`,
/// `abc123..def456`) or a jj revset (`@ | main..@`, `ancestors(head(), 5)`).
/// `max_count` caps the result (git `-n` / jj `-l`); `since` is the free-form
/// "since" filter (git `--since` / jj `since(date)` revset clause).
///
/// Every free-form string is rejected by the wrapper's argv guard
/// (`reject_flag_like`) on the way through, so a leading-`-` value never
/// reaches the underlying command.
#[derive(Debug, Default, Clone)]
pub struct LogSpec<'a> {
    /// Revision range (git: `A..B`; jj: a revset). `None` = most recent commit
    /// only.
    pub range: Option<&'a str>,
    /// Cap on returned entries; `None` = wrapper default (typically 50).
    pub max_count: Option<usize>,
    /// Free-form "since" filter; rejected by the wrapper's argv guard.
    pub since: Option<&'a str>,
    /// Populate [`LogEntry::files`] (per-commit changed paths) — adds a second
    /// spawn on git (`--name-status`) and a wider template on jj.
    pub with_files: bool,
}

/// The output of [`Repo::diff`](crate::Repo::diff). Adjacently tagged so the
/// wire shape is **type-stable** — every variant is an object with a
/// `format` discriminant, never a bare string for one variant and an object
/// for another (same precedent as [`MergeProbe`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(tag = "format", content = "data"))]
#[non_exhaustive]
pub enum DiffOutput {
    /// Full unified diff text (the `text` may be truncated when
    /// [`DiffSpec::max_bytes`] was set; `truncated` is `true` in that case and
    /// `omitted_bytes` is the count of bytes dropped after the cap).
    Unified {
        /// The diff text (possibly truncated — see `truncated`).
        text: String,
        /// `true` when the wrapper truncated the output to honour
        /// [`DiffSpec::max_bytes`].
        truncated: bool,
        /// Bytes dropped after the truncation point; `0` when not truncated.
        omitted_bytes: u64,
    },
    /// Just the changed paths (git `--name-only` / jj `diff --name-only`).
    Names(Vec<String>),
    /// Aggregate insertion/deletion/file counts (reuses the existing
    /// [`DiffStat`] DTO so callers don't have to switch on the variant).
    Stat(DiffStat),
}

/// The query spec for [`Repo::diff`](crate::Repo::diff). A `range` selects the
/// committed history to diff; `paths` restricts it to a subset of the tree;
/// `format` chooses the wire shape; `max_bytes` caps the unified blob.
///
/// All free-form strings go through the wrapper's argv guard before reaching
/// the underlying command.
#[derive(Debug, Default, Clone)]
pub struct DiffSpec<'a> {
    /// Revision range (git: `A..B`; jj: a revset) or single revision. `None`
    /// = working-copy diff against the last commit (mirrors
    /// [`Repo::diff_stat`](crate::Repo::diff_stat)).
    pub range: Option<&'a str>,
    /// Restrict the diff to these repo-relative paths; `None` = the whole
    /// tree. Paths are joined after `--` in argv so they can't be smuggled
    /// as flags.
    pub paths: Option<&'a [String]>,
    /// Output format — `Unified` (default), `Names`, or `Stat`.
    pub format: DiffFormat,
    /// Cap on the unified-diff text length; `None` = no cap. The wrapper
    /// truncates with a marker and reports `omitted_bytes` in the result.
    pub max_bytes: Option<usize>,
}

/// The output shape for [`Repo::diff`](crate::Repo::diff). Stringly-typed at
/// the MCP boundary (matches the `forge_pr_merge` `strategy` precedent);
/// parsed into this enum by the mcp layer.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[non_exhaustive]
pub enum DiffFormat {
    /// Full unified diff text (the default).
    #[default]
    Unified,
    /// Changed paths only.
    Names,
    /// Aggregate insertion/deletion/file counts.
    Stat,
}

impl DiffFormat {
    /// Parse the wire-side string. Accepts the lowercase / mixed-case forms
    /// most MCP callers will pass; an unknown value is `None` — the mcp layer
    /// is expected to surface that as an `invalid_params` MCP error (the
    /// `vcs-mcp` `core_err` mapping prefers `io::ErrorKind::InvalidInput`).
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "unified" | "Unified" => Some(DiffFormat::Unified),
            "names" | "Names" => Some(DiffFormat::Names),
            "stat" | "Stat" => Some(DiffFormat::Stat),
            _ => None,
        }
    }
}

/// The ref-state bundle returned by [`Repo::refs`](crate::Repo::refs): HEAD,
/// the current branch, the trunk / default branch, and the configured remotes.
/// The fields the workflow's review passes need in one call (the wishlist's
/// `refs` is exactly this shape — distinct from [`RepoSnapshot`], which also
/// carries working-copy / operation state).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[non_exhaustive]
pub struct RefsSnapshot {
    /// The working-copy commit's **full** object id (git `HEAD` / jj `@`).
    pub head_sha: String,
    /// Current branch (git) / bookmark (jj); `None` when detached or unset.
    pub current_branch: Option<String>,
    /// Trunk / default branch (git `origin/HEAD` short name; jj `trunk()`
    /// revset). `None` when undetectable (no `origin`, no local `main` /
    /// `master`).
    pub default_branch: Option<String>,
    /// Configured remotes; empty on a pure-jj repo with no git remote.
    pub remotes: Vec<RemoteInfo>,
}

/// A single configured remote (git `remote.<name>.url` / jj `jj git remote`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[non_exhaustive]
pub struct RemoteInfo {
    /// Remote name (`origin` is the common case).
    pub name: String,
    /// The remote's fetch URL.
    pub url: String,
}

// The optional `serde` feature derives `Serialize` on the facade DTOs.
#[cfg(all(test, feature = "serde"))]
mod serde_tests {
    use super::*;

    #[test]
    fn snapshot_and_file_change_serialize_to_clean_json() {
        let snap = RepoSnapshot {
            head: Some("abc".into()),
            branch: Some("main".into()),
            tracking: Some(UpstreamTracking {
                branch: "origin/main".into(),
                ahead: 1,
                behind: 0,
            }),
            dirty: true,
            change_count: 2,
            conflicted: false,
            operation: OperationState::Merge,
        };
        let v = serde_json::to_value(&snap).unwrap();
        assert_eq!(v["branch"], "main");
        assert_eq!(v["operation"], "Merge"); // enum → variant name
        assert_eq!(v["change_count"], 2);
        // Tracking serialises as one nested object (or null), not three fields.
        assert_eq!(v["tracking"]["branch"], "origin/main");
        assert_eq!(v["tracking"]["ahead"], 1);

        let fc = FileChange {
            path: "a.rs".into(),
            old_path: None,
            kind: ChangeKind::Added, // re-exported vcs_diff type, Serialize via vcs-diff/serde
        };
        let v = serde_json::to_value(fc).unwrap();
        assert_eq!(v["path"], "a.rs");
        assert_eq!(v["kind"], "Added");
    }

    // `MergeProbe` is adjacently tagged: BOTH outcomes are objects with an
    // `outcome` discriminant — a stable shape a tool consumer can branch on,
    // never a bare string for one case and an object for the other.
    #[test]
    fn merge_probe_serializes_to_a_type_stable_object() {
        let clean = serde_json::to_value(MergeProbe::Clean).unwrap();
        assert_eq!(clean["outcome"], "Clean");
        assert!(clean.get("files").is_none(), "{clean}");

        let conflicts =
            serde_json::to_value(MergeProbe::Conflicts(vec!["a.rs".into(), "b.rs".into()]))
                .unwrap();
        assert_eq!(conflicts["outcome"], "Conflicts");
        assert_eq!(conflicts["files"][0], "a.rs");
        assert_eq!(conflicts["files"][1], "b.rs");
    }

    // `DiffOutput` is adjacently tagged so the wire shape is type-stable:
    // every variant serialises as `{"format": <discriminant>, "data": ...}`,
    // never a bare string for one variant and an object for another.
    #[test]
    fn diff_output_serializes_to_a_type_stable_object() {
        let unified = serde_json::to_value(DiffOutput::Unified {
            text: "@@ -1 +1 @@\n-a\n+b\n".into(),
            truncated: false,
            omitted_bytes: 0,
        })
        .unwrap();
        assert_eq!(unified["format"], "Unified");
        assert!(unified["data"]["text"].as_str().unwrap().contains("-a"));
        assert_eq!(unified["data"]["truncated"], false);
        assert_eq!(unified["data"]["omitted_bytes"], 0);

        let names = serde_json::to_value(DiffOutput::Names(vec!["a.rs".into(), "b.rs".into()]))
            .unwrap();
        assert_eq!(names["format"], "Names");
        assert_eq!(names["data"][0], "a.rs");
        assert_eq!(names["data"][1], "b.rs");

        let stat = serde_json::to_value(DiffOutput::Stat(DiffStat::new(2, 3, 1))).unwrap();
        assert_eq!(stat["format"], "Stat");
        assert_eq!(stat["data"]["files_changed"], 2);
        assert_eq!(stat["data"]["insertions"], 3);
        assert_eq!(stat["data"]["deletions"], 1);
    }

    // `RefsSnapshot` round-trips the bundle (head + branch + default + remotes).
    #[test]
    fn refs_snapshot_serializes_with_branch_and_remotes() {
        let snap = RefsSnapshot {
            head_sha: "abc123".into(),
            current_branch: Some("feat/x".into()),
            default_branch: Some("main".into()),
            remotes: vec![RemoteInfo {
                name: "origin".into(),
                url: "git@github.com:foo/bar.git".into(),
            }],
        };
        let v = serde_json::to_value(&snap).unwrap();
        assert_eq!(v["head_sha"], "abc123");
        assert_eq!(v["current_branch"], "feat/x");
        assert_eq!(v["default_branch"], "main");
        assert_eq!(v["remotes"][0]["name"], "origin");
        assert_eq!(v["remotes"][0]["url"], "git@github.com:foo/bar.git");
    }

    // `LogEntry` carries the per-commit file list when populated; empty vec
    // when not (no `Option<Vec<…>>` shape — the caller doesn't have to branch
    // on a flag at the consumption site).
    #[test]
    fn log_entry_with_files_round_trips() {
        let entry = LogEntry {
            sha: "deadbeef".into(),
            parents: vec!["cafebabe".into()],
            author: "Alice".into(),
            authored_at: "2026-06-13T10:00:00+00:00".into(),
            committer: "Alice".into(),
            committed_at: "2026-06-13T10:00:00+00:00".into(),
            summary: "fix: thing".into(),
            body: None,
            files: vec![FileChange {
                path: "a.rs".into(),
                old_path: None,
                kind: ChangeKind::Modified,
            }],
        };
        let v = serde_json::to_value(&entry).unwrap();
        assert_eq!(v["sha"], "deadbeef");
        assert_eq!(v["parents"][0], "cafebabe");
        assert_eq!(v["files"][0]["path"], "a.rs");
        assert_eq!(v["files"][0]["kind"], "Modified");
    }
}
