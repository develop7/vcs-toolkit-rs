//! End-to-end tests for the typed `vcs-jj` client against a real temporary
//! repository. Ignored by default (require the `jj` binary); run with
//! `cargo test -p vcs-jj -- --ignored`.

mod common;

use std::path::Path;
use std::process::Command;

use common::TempDir;
use vcs_jj::{Jj, JjApi};

/// Create a fresh jj repo in `dir` with a deterministic identity.
fn init_repo(dir: &Path) {
    let jj = |args: &[&str]| {
        Command::new(vcs_jj::BINARY)
            .current_dir(dir)
            .args(args)
            .status()
            .expect("jj command");
    };
    jj(&["git", "init"]);
    jj(&["config", "set", "--repo", "user.name", "Test"]);
    jj(&["config", "set", "--repo", "user.email", "test@example.com"]);
}

#[tokio::test]
#[ignore = "requires the jj binary"]
async fn describe_new_and_log_cycle() {
    let tmp = TempDir::new("cycle");
    let dir = tmp.path();
    init_repo(dir);
    let jj = Jj::new();

    // Fresh working copy: an empty change with no description.
    let head = jj.current_change(dir).await.expect("current_change");
    assert!(!head.change_id.is_empty());
    assert!(head.empty, "fresh working copy should be empty");
    assert_eq!(head.description, "");

    // Describe it, then read it back.
    jj.describe(dir, "hello jj").await.expect("describe");
    assert_eq!(
        jj.current_change(dir)
            .await
            .expect("current_change")
            .description,
        "hello jj"
    );

    // Start a new change; it becomes the working copy.
    jj.new_change(dir, "second change").await.expect("new");
    assert_eq!(
        jj.current_change(dir)
            .await
            .expect("current_change")
            .description,
        "second change"
    );

    // Both changes are reachable from @.
    let log = jj.log(dir, "::@", 10).await.expect("log");
    assert!(
        log.len() >= 2,
        "expected at least two changes, got {}",
        log.len()
    );
    assert!(log.iter().any(|c| c.description == "hello jj"));

    // status returns something without erroring.
    jj.status(dir).await.expect("status");
}

#[tokio::test]
#[ignore = "requires the jj binary"]
async fn bookmark_create_set_and_list() {
    let tmp = TempDir::new("bookmarks");
    let dir = tmp.path();
    init_repo(dir);
    let jj = Jj::new();

    jj.describe(dir, "rooted").await.expect("describe");
    Command::new(vcs_jj::BINARY)
        .current_dir(dir)
        .args(["bookmark", "create", "mark", "-r", "@"])
        .status()
        .expect("bookmark create");
    // Move it via the typed API.
    jj.bookmark_set(dir, "mark", "@")
        .await
        .expect("bookmark_set");

    let bookmarks = jj.bookmarks(dir).await.expect("bookmarks");
    assert!(
        bookmarks.iter().any(|b| b.name == "mark"),
        "expected bookmark 'mark', got {bookmarks:?}"
    );
}
