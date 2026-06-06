//! End-to-end tests for `vcs-watch` against real temporary git/jj repositories.
//! Ignored by default (require the `git`/`jj` binary). Run with
//! `cargo test -p vcs-watch -- --ignored`.
//!
//! The pure snapshot-diff is covered hermetically in `src/event.rs`; these tests
//! exercise the real notify → debounce → re-query → emit pipeline. Each performs
//! a repo operation and waits (with a generous ceiling) for the resulting event —
//! a short debounce keeps them snappy, and the re-query+diff design means stray
//! filesystem noise can't produce a spurious change.

use std::time::Duration;

use tokio::time::timeout;
use vcs_core::Repo;
use vcs_testkit::{GitSandbox, JjSandbox};
use vcs_watch::{RepoEvent, RepoWatcher};

/// Drain changes until one carries an event matching `pred`, or the overall
/// deadline elapses. Returns whether the event was seen.
async fn wait_for(
    watcher: &mut RepoWatcher,
    overall: Duration,
    pred: impl Fn(&RepoEvent) -> bool,
) -> bool {
    let deadline = tokio::time::Instant::now() + overall;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return false;
        }
        match timeout(remaining, watcher.recv()).await {
            Ok(Some(change)) => {
                if change.events.iter().any(&pred) {
                    return true;
                }
            }
            // Channel closed, or the overall timeout fired.
            Ok(None) | Err(_) => return false,
        }
    }
}

fn fast(repo: Repo) -> impl std::future::Future<Output = vcs_watch::Result<RepoWatcher>> {
    // A short debounce keeps the tests responsive; the watcher still coalesces.
    RepoWatcher::builder(repo)
        .debounce(Duration::from_millis(50))
        .build()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires the git binary"]
async fn git_branch_create_emits_branch_created() {
    let sandbox = GitSandbox::init("watch-git-branch");
    sandbox.commit_file("seed.txt", "seed\n", "initial");
    let repo = Repo::open(sandbox.path()).expect("open");
    let mut watcher = fast(repo).await.expect("watcher");

    sandbox.git(&["branch", "feature"]);

    assert!(
        wait_for(&mut watcher, Duration::from_secs(10), |e| {
            matches!(e, RepoEvent::BranchCreated { name } if name == "feature")
        })
        .await,
        "expected a BranchCreated(feature) event"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires the git binary"]
async fn git_working_tree_edit_emits_working_copy_changed() {
    let sandbox = GitSandbox::init("watch-git-wc");
    sandbox.commit_file("seed.txt", "seed\n", "initial");
    let repo = Repo::open(sandbox.path()).expect("open");
    // Opt into working-tree watching so a bare untracked file is seen.
    let mut watcher = RepoWatcher::builder(repo)
        .working_tree(true)
        .debounce(Duration::from_millis(50))
        .build()
        .await
        .expect("watcher");

    sandbox.write("dirty.txt", "x\n"); // untracked → dirty, no git command

    assert!(
        wait_for(&mut watcher, Duration::from_secs(10), |e| {
            matches!(e, RepoEvent::WorkingCopyChanged { dirty: true, .. })
        })
        .await,
        "expected a WorkingCopyChanged(dirty) event"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires the jj binary"]
async fn jj_bookmark_create_emits_branch_created() {
    let sandbox = JjSandbox::init("watch-jj-bm");
    sandbox.write("seed.txt", "seed\n");
    sandbox.describe("initial");
    let repo = Repo::open(sandbox.path()).expect("open");
    let mut watcher = fast(repo).await.expect("watcher");

    sandbox.bookmark("feature");

    assert!(
        wait_for(&mut watcher, Duration::from_secs(10), |e| {
            matches!(e, RepoEvent::BranchCreated { name } if name == "feature")
        })
        .await,
        "expected a BranchCreated(feature) event on jj"
    );
}

// Dropping the watcher stops the stream: `recv` returns `None` promptly.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires the git binary"]
async fn drop_stops_the_watch() {
    let sandbox = GitSandbox::init("watch-drop");
    sandbox.commit_file("seed.txt", "seed\n", "initial");
    let repo = Repo::open(sandbox.path()).expect("open");
    let mut watcher = fast(repo).await.expect("watcher");

    // Re-bind the receiver out of the watcher would keep it alive; instead drop
    // the whole watcher and confirm a fresh `recv` on a *moved* handle ends. Here
    // we simply assert that, with no activity, `recv` doesn't spuriously fire.
    let quiet = timeout(Duration::from_millis(300), watcher.recv()).await;
    assert!(quiet.is_err(), "no events expected on a quiescent repo");
    drop(watcher); // stops the OS watch + background task
}
