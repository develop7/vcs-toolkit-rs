//! `vcs-watch` — filesystem-watch a git/jj repository and emit typed state-change
//! events.
//!
//! A [`RepoWatcher`] watches a repository's `.git`/`.jj` state directory (and,
//! optionally, the working tree), **debounces** the burst of writes a VCS
//! operation makes, **re-queries** the repo state through
//! [`vcs-core`](vcs_core)'s batched [`snapshot`](vcs_core::Repo::snapshot), and
//! **diffs** it against the previous state to yield typed [`RepoEvent`]s. Each
//! settled change arrives as a [`RepoChange`] carrying both the new
//! [`RepoSnapshot`] (to render a prompt/status line) and the deltas (to react).
//!
//! Re-query-and-diff — rather than interpreting raw filesystem events — is what
//! makes it robust: git's ref temp-file renames, `index.lock` churn, and reflog
//! noise all just coalesce into one "re-check the settled state" instead of being
//! (mis)read as events.
//!
//! ```no_run
//! use vcs_core::Repo;
//! use vcs_watch::RepoWatcher;
//! # async fn run() -> vcs_watch::Result<()> {
//! let repo = Repo::open(".")?;
//! let mut watcher = RepoWatcher::watch(repo).await?;
//! while let Some(change) = watcher.recv().await {
//!     for event in &change.events {
//!         println!("{event:?}");
//!     }
//!     // `change.snapshot` is the fresh full state.
//! }
//! # Ok(()) }
//! ```
//!
//! **Runtime:** unlike the rest of the toolkit (which hides tokio behind
//! `processkit`), `vcs-watch` uses **tokio at runtime** — the watch task and the
//! debounce timer run on the caller's tokio runtime, so build/await it from
//! within one.

use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::{RecursiveMode, Watcher};
use tokio::sync::mpsc;
use vcs_core::{BackendKind, VcsRepo};

mod error;
mod event;

pub use error::{Error, Result};
pub use event::{RepoChange, RepoEvent};
// Re-export the snapshot types a consumer reads off a `RepoChange`, so depending
// on `vcs-watch` alone suffices.
pub use vcs_core::{OperationState, RepoSnapshot};

/// Default quiet window: a re-query fires once the watched dir has been silent
/// for this long after the last event.
const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(250);
/// Default ceiling: even under a continuous stream of events, re-query at least
/// this often (so a long bulk operation still reports progress).
const DEFAULT_MAX_WAIT: Duration = Duration::from_secs(1);
/// Bounded output channel: a slow consumer applies backpressure (the loop pauses
/// re-querying), and pending filesystem signals coalesce into one catch-up query.
const OUTPUT_CAPACITY: usize = 64;

/// Builder for a [`RepoWatcher`] — set the watch scope and debounce timing, then
/// [`build`](Builder::build).
pub struct Builder {
    repo: Box<dyn VcsRepo>,
    working_tree: bool,
    debounce: Duration,
    max_wait: Duration,
}

impl Builder {
    /// Also watch the **working tree** recursively, so a bare unstaged edit
    /// (`vim file`) fires [`WorkingCopyChanged`](RepoEvent::WorkingCopyChanged)
    /// immediately. Off by default (only the `.git`/`.jj` state dir is watched,
    /// which catches an unstaged edit once it touches the index / a jj snapshot).
    ///
    /// Note: `notify` is `.gitignore`-unaware, so this also watches ignored and
    /// build directories — heavier on a large tree.
    pub fn working_tree(mut self, yes: bool) -> Self {
        self.working_tree = yes;
        self
    }

    /// The quiet window: re-query once the watched dir has been silent this long
    /// after the last event (default 250 ms). Coalesces an operation's write
    /// burst into one re-check.
    pub fn debounce(mut self, window: Duration) -> Self {
        self.debounce = window;
        self
    }

    /// The ceiling on how long a continuous event stream defers the re-query
    /// (default 1 s) — a long bulk operation still reports at this cadence.
    pub fn max_wait(mut self, ceiling: Duration) -> Self {
        self.max_wait = ceiling;
        self
    }

    /// Start watching. Captures the baseline state, registers the filesystem
    /// watch, and spawns the background re-query task on the current tokio
    /// runtime.
    pub async fn build(self) -> Result<RepoWatcher> {
        let root = self.repo.root().to_path_buf();
        // The dirs whose writes mean "re-check": the `.git`/`.jj` state dir, plus
        // — for a linked git worktree — the *shared* git dir it points at via
        // `commondir` (where `refs/heads/*` and `packed-refs` actually live, so
        // branch create/delete is seen). See `state_dirs`.
        let state_dirs = state_dirs(self.repo.kind(), &root)?;

        // Bridge: notify's callback thread pushes a unit signal per event into an
        // unbounded channel (non-blocking, thread-safe); the debounce loop drains
        // it. Build the watcher and register paths *before* the baseline snapshot,
        // so a change racing the baseline is queued, not lost.
        let (raw_tx, raw_rx) = mpsc::unbounded_channel::<()>();
        let mut watcher = notify::recommended_watcher(move |_res| {
            // Content is irrelevant — we re-query state, so any event (or watch
            // error) just means "re-check". Send fails only after the loop ends.
            let _ = raw_tx.send(());
        })?;
        if self.working_tree {
            watcher.watch(&root, RecursiveMode::Recursive)?;
            // A worktree gitlink puts the real (private and shared) git dirs
            // outside `root`; cover any not already under the recursive root watch.
            for dir in &state_dirs {
                if !dir.starts_with(&root) {
                    watcher.watch(dir, RecursiveMode::Recursive)?;
                }
            }
        } else {
            for dir in &state_dirs {
                watcher.watch(dir, RecursiveMode::Recursive)?;
            }
        }

        let snapshot = self.repo.snapshot().await?;
        let branches = self.repo.local_branches().await?;
        let baseline = snapshot.clone();
        let prev = event::WatchState::from_snapshot(&snapshot, branches);

        let (out_tx, out_rx) = mpsc::channel::<RepoChange>(OUTPUT_CAPACITY);
        let task = tokio::spawn(watch_loop(
            self.repo,
            raw_rx,
            out_tx,
            prev,
            self.debounce,
            self.max_wait,
        ));

        Ok(RepoWatcher {
            rx: out_rx,
            current: baseline,
            _watcher: watcher,
            task,
        })
    }
}

/// A live watch over a repository, yielding [`RepoChange`]s as the repo's state
/// changes. Dropping it stops the filesystem watch and the background task.
pub struct RepoWatcher {
    rx: mpsc::Receiver<RepoChange>,
    current: RepoSnapshot,
    // Held to keep the OS watch alive; dropping it ends the watch (and the loop).
    _watcher: notify::RecommendedWatcher,
    task: tokio::task::JoinHandle<()>,
}

impl RepoWatcher {
    /// A builder over `repo` (any [`VcsRepo`] — e.g. a [`vcs_core::Repo`]).
    pub fn builder(repo: impl VcsRepo + 'static) -> Builder {
        Builder {
            repo: Box::new(repo),
            working_tree: false,
            debounce: DEFAULT_DEBOUNCE,
            max_wait: DEFAULT_MAX_WAIT,
        }
    }

    /// Start watching `repo` with the defaults (state dir only, 250 ms debounce).
    pub async fn watch(repo: impl VcsRepo + 'static) -> Result<RepoWatcher> {
        Self::builder(repo).build().await
    }

    /// Await the next settled change. Returns `None` once the watcher is dropped
    /// or its background task ends.
    pub async fn recv(&mut self) -> Option<RepoChange> {
        let change = self.rx.recv().await?;
        self.current = change.snapshot.clone();
        Some(change)
    }

    /// The most recent known snapshot — the baseline captured at
    /// [`build`](Builder::build), then the snapshot from each [`recv`](Self::recv).
    /// It advances **only when you call [`recv`](Self::recv)**, so it is as fresh
    /// as your last `recv`, not a live view.
    pub fn current(&self) -> &RepoSnapshot {
        &self.current
    }
}

impl Drop for RepoWatcher {
    fn drop(&mut self) {
        // The dropped `_watcher` already closes the signal channel (ending the
        // loop); abort is belt-and-braces for prompt teardown.
        self.task.abort();
    }
}

/// The background loop: coalesce a burst of filesystem signals, re-query the
/// settled state, diff against the previous, and emit a [`RepoChange`] when
/// anything changed.
async fn watch_loop(
    repo: Box<dyn VcsRepo>,
    mut raw_rx: mpsc::UnboundedReceiver<()>,
    out_tx: mpsc::Sender<RepoChange>,
    mut prev: event::WatchState,
    debounce: Duration,
    max_wait: Duration,
) {
    loop {
        // Block until the first signal (or exit when the watcher is dropped).
        if raw_rx.recv().await.is_none() {
            return;
        }
        // Coalesce the burst: reset a `debounce` quiet-timer on every new signal,
        // but never wait past `max_wait` total.
        let deadline = tokio::time::Instant::now() + max_wait;
        loop {
            tokio::select! {
                biased;
                sig = raw_rx.recv() => {
                    if sig.is_none() {
                        return; // watcher dropped mid-burst
                    }
                    if tokio::time::Instant::now() >= deadline {
                        break; // ceiling reached — re-query now
                    }
                    // else: another event — loop resets the quiet timer
                }
                _ = tokio::time::sleep(debounce) => break, // settled
            }
        }

        // Re-query the settled state. A transient failure (e.g. `index.lock` held
        // mid-operation) is skipped — the next event re-checks once it settles.
        let snapshot = match repo.snapshot().await {
            Ok(s) => s,
            Err(_e) => {
                #[cfg(feature = "tracing")]
                tracing::debug!(error = %_e, "vcs-watch: snapshot re-query failed; skipping");
                continue;
            }
        };
        let branches = match repo.local_branches().await {
            Ok(b) => b,
            Err(_e) => {
                #[cfg(feature = "tracing")]
                tracing::debug!(error = %_e, "vcs-watch: branch re-query failed; skipping");
                continue;
            }
        };

        let next = event::WatchState::from_snapshot(&snapshot, branches);
        let events = event::diff(&prev, &next);
        prev = next;
        if events.is_empty() {
            continue;
        }
        if out_tx.send(RepoChange { snapshot, events }).await.is_err() {
            return; // receiver dropped — stop
        }
    }
}

/// The directories to watch for a backend, deduplicated. Normally one — the
/// `.git`/`.jj` state dir (see [`state_dir`]) — but a **linked git worktree** has
/// two: its private gitdir (HEAD/index/logs) *and* the shared git dir it points
/// at via `commondir` (`refs/heads/*` and `packed-refs`, where branch
/// create/delete actually lands). Watching only the private dir would miss every
/// `BranchCreated`/`BranchDeleted` on a worktree, since the shared dir is a
/// *sibling*, not nested under it (see [`common_dir`]).
///
/// Overlapping watches are harmless — the re-query+debounce coalesces duplicate
/// signals — but we drop a second dir whose normalized path equals the first, so
/// `notify` isn't asked to watch the same path twice.
fn state_dirs(kind: BackendKind, root: &Path) -> Result<Vec<PathBuf>> {
    let state_dir = state_dir(kind, root)?;
    let mut dirs = vec![state_dir.clone()];
    if let Some(shared) = common_dir(&state_dir)
        && normalize(&shared) != normalize(&state_dir)
    {
        dirs.push(shared);
    }
    Ok(dirs)
}

/// The directory to watch for a backend: `.jj` for jj, `.git` for git. A
/// worktree's `.git` is a gitlink *file* (`gitdir: <path>`); resolve it to the
/// real git directory. Best-effort — falls back to the `.git` path itself.
fn state_dir(kind: BackendKind, root: &Path) -> Result<PathBuf> {
    match kind {
        BackendKind::Jj => Ok(root.join(".jj")),
        BackendKind::Git => {
            let dot_git = root.join(".git");
            if dot_git.is_file() {
                let content = std::fs::read_to_string(&dot_git)?;
                if let Some(rest) = content.trim().strip_prefix("gitdir:") {
                    let p = PathBuf::from(rest.trim());
                    return Ok(if p.is_absolute() { p } else { root.join(p) });
                }
            }
            Ok(dot_git)
        }
        // `BackendKind` is `#[non_exhaustive]`; for an unknown future backend
        // watch the repo root itself — coarser, but it can't miss the state dir.
        _ => Ok(root.to_path_buf()),
    }
}

/// The **shared** git directory for a linked worktree, or `None` for a plain
/// repo. A linked worktree's resolved gitdir holds a `commondir` file whose
/// content is a path (typically relative, e.g. `../..`) to the shared `.git` —
/// where `refs/heads/*` and `packed-refs` live. We join it to the gitdir and
/// resolve `..` (lexically, matching the no-canonicalize style of [`state_dir`],
/// so the registered path stays plain rather than a Windows `\\?\` verbatim one).
/// A plain repo has no `commondir` file, so this is `None` and behaviour is
/// unchanged.
fn common_dir(state_dir: &Path) -> Option<PathBuf> {
    let commondir = state_dir.join("commondir");
    let content = std::fs::read_to_string(&commondir).ok()?;
    let rel = content.trim();
    if rel.is_empty() {
        return None;
    }
    let p = PathBuf::from(rel);
    let joined = if p.is_absolute() {
        p
    } else {
        state_dir.join(p)
    };
    Some(lexically_normalized(&joined))
}

/// Resolve `.`/`..` components without touching the filesystem, keeping the path
/// in its original (non-verbatim) form — `commondir`'s `../..` plus a Windows
/// gitdir would otherwise leave literal `..` segments in the watched path.
fn lexically_normalized(p: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            Component::ParentDir => {
                // Pop a real segment; keep a leading `..` that can't be resolved.
                if !out.pop() {
                    out.push(comp);
                }
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// Canonicalize for comparison and strip the Windows verbatim prefix (`\\?\…`,
/// which `canonicalize` adds), so two spellings of the same dir dedup. Mirrors
/// `vcs-core`'s path-compare normalization; falls back to the input when the path
/// can't be canonicalized (then equal paths still compare equal byte-for-byte).
fn normalize(p: &Path) -> PathBuf {
    let canonical = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    #[cfg(windows)]
    {
        let s = canonical.to_string_lossy();
        if let Some(rest) = s.strip_prefix(r"\\?\")
            && !rest.starts_with("UNC\\")
        {
            return PathBuf::from(rest.to_string());
        }
    }
    canonical
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// A unique, self-cleaning temp dir (no temp-dir crate needed for these
    /// hermetic helper tests — pid + counter keeps parallel tests from colliding).
    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!(
                "vcs-watch-commondir-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&p).expect("create scratch dir");
            Scratch(p)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    // A plain (non-worktree) git dir has no `commondir` file → no shared dir, so
    // behaviour is exactly today's single-dir watch.
    #[test]
    fn no_commondir_file_yields_none() {
        let scratch = Scratch::new();
        let git_dir = scratch.0.join(".git");
        std::fs::create_dir_all(&git_dir).expect("mkdir .git");
        assert_eq!(common_dir(&git_dir), None);
    }

    // A linked-worktree layout: the private gitdir holds `commondir` = `../..`
    // (git's actual content), which must resolve to the sibling shared `.git`.
    #[test]
    fn relative_commondir_resolves_to_shared_git_dir() {
        let scratch = Scratch::new();
        let shared = scratch.0.join(".git");
        let private = shared.join("worktrees").join("wt");
        std::fs::create_dir_all(&private).expect("mkdir private gitdir");
        // git writes `../..` (relative to the private dir) here.
        std::fs::write(private.join("commondir"), "../..\n").expect("write commondir");

        let resolved = common_dir(&private).expect("Some(shared dir)");
        // `<shared>/worktrees/wt` + `../..` == `<shared>` (lexically, no `..` left).
        assert_eq!(resolved, lexically_normalized(&shared));
        assert!(
            !resolved.to_string_lossy().contains(".."),
            "the `..` segments must be resolved, got {}",
            resolved.display()
        );
    }

    // An absolute `commondir` (git permits it) is taken as-is.
    #[test]
    fn absolute_commondir_is_used_verbatim() {
        let scratch = Scratch::new();
        let shared = scratch.0.join("shared-git");
        let private = scratch.0.join("private");
        std::fs::create_dir_all(&private).expect("mkdir private");
        std::fs::write(private.join("commondir"), format!("{}\n", shared.display()))
            .expect("write commondir");

        assert_eq!(common_dir(&private), Some(lexically_normalized(&shared)));
    }

    // `state_dirs` returns both the private and shared dirs for a worktree, and
    // the shared dir is not the private one (so two distinct watches register).
    #[test]
    fn state_dirs_includes_private_and_shared_for_worktree() {
        let scratch = Scratch::new();
        let root = scratch.0.join("wt-worktree");
        let shared = scratch.0.join(".git");
        let private = shared.join("worktrees").join("wt");
        std::fs::create_dir_all(&private).expect("mkdir private gitdir");
        std::fs::create_dir_all(&root).expect("mkdir worktree root");
        std::fs::write(private.join("commondir"), "../..\n").expect("write commondir");
        // The worktree's `.git` gitlink file points at the private dir.
        std::fs::write(
            root.join(".git"),
            format!("gitdir: {}\n", private.display()),
        )
        .expect("write gitlink");

        let dirs = state_dirs(BackendKind::Git, &root).expect("state_dirs");
        assert_eq!(dirs.len(), 2, "private + shared, got {dirs:?}");
        assert_eq!(normalize(&dirs[0]), normalize(&private));
        assert_eq!(normalize(&dirs[1]), normalize(&shared));
    }

    // When `commondir` resolves back to the state dir itself (degenerate), the
    // duplicate is dropped — we never register the same path twice.
    #[test]
    fn self_referential_commondir_is_deduped() {
        let scratch = Scratch::new();
        let git_dir = scratch.0.join(".git");
        std::fs::create_dir_all(&git_dir).expect("mkdir .git");
        // `.` resolves to the dir itself.
        std::fs::write(git_dir.join("commondir"), ".\n").expect("write commondir");
        // The gitlink points the worktree root at this very dir.
        let root = scratch.0.join("root");
        std::fs::create_dir_all(&root).expect("mkdir root");
        std::fs::write(
            root.join(".git"),
            format!("gitdir: {}\n", git_dir.display()),
        )
        .expect("write gitlink");

        let dirs = state_dirs(BackendKind::Git, &root).expect("state_dirs");
        assert_eq!(dirs.len(), 1, "self-reference deduped, got {dirs:?}");
    }
}
