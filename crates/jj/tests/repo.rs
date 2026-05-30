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

#[test]
#[ignore = "requires the jj binary"]
fn describe_new_and_log_cycle() {
    let tmp = TempDir::new("cycle");
    let dir = tmp.path();
    init_repo(dir);
    let jj = Jj::new();

    // Fresh working copy: a change with no description.
    let head = jj.current_change(dir).expect("current_change");
    assert!(!head.change_id.is_empty());
    assert!(!head.commit_id.is_empty());
    assert_eq!(head.description, "");

    // Describe it, then read it back.
    jj.describe(dir, "hello jj").expect("describe");
    assert_eq!(
        jj.current_change(dir).expect("current_change").description,
        "hello jj"
    );

    // Start a new change; it becomes the working copy.
    jj.new_change(dir, "second change").expect("new");
    assert_eq!(
        jj.current_change(dir).expect("current_change").description,
        "second change"
    );

    // Both changes are reachable from @.
    let log = jj.log(dir, "::@", 10).expect("log");
    assert!(
        log.len() >= 2,
        "expected at least two changes, got {}",
        log.len()
    );
    assert!(log.iter().any(|c| c.description == "hello jj"));

    // status returns something without erroring.
    jj.status(dir).expect("status");
}

#[test]
#[ignore = "requires the jj binary"]
fn bookmarks_are_listed() {
    let tmp = TempDir::new("bookmarks");
    let dir = tmp.path();
    init_repo(dir);
    let jj = Jj::new();

    jj.describe(dir, "rooted").expect("describe");
    Command::new(vcs_jj::BINARY)
        .current_dir(dir)
        .args(["bookmark", "create", "mark", "-r", "@"])
        .status()
        .expect("bookmark create");

    let bookmarks = jj.bookmarks(dir).expect("bookmarks");
    assert!(
        bookmarks.iter().any(|b| b.name == "mark"),
        "expected bookmark 'mark', got {bookmarks:?}"
    );
}
