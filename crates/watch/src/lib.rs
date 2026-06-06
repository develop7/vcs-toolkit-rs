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
        let state_dir = state_dir(self.repo.kind(), &root)?;

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
            // A worktree gitlink puts the real git dir outside `root`; cover it.
            if !state_dir.starts_with(&root) {
                watcher.watch(&state_dir, RecursiveMode::Recursive)?;
            }
        } else {
            watcher.watch(&state_dir, RecursiveMode::Recursive)?;
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
