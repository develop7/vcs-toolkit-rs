//! End-to-end tests for the typed `vcs-jj` commands against a real temporary
//! repository. Ignored by default (require the `jj` binary); run with
//! `cargo test -p vcs-jj -- --ignored`.

mod common;

use std::path::Path;

use common::TempDir;

/// Create a fresh jj repo in `dir` with a deterministic identity.
fn init_repo(dir: &Path) {
    vcs_jj::exec()
        .current_dir(dir)
        .args(["git", "init"])
        .run()
        .expect("jj git init");
    for (key, val) in [("user.name", "Test"), ("user.email", "test@example.com")] {
        vcs_jj::exec()
            .current_dir(dir)
            .args(["config", "set", "--repo", key, val])
            .run()
            .expect("jj config set");
    }
}

#[test]
#[ignore = "requires the jj binary"]
fn describe_new_and_log_cycle() {
    let tmp = TempDir::new("cycle");
    let dir = tmp.path();
    init_repo(dir);

    // Fresh working copy: a change with no description.
    let head = vcs_jj::current_change(dir).expect("current_change");
    assert!(!head.change_id.is_empty());
    assert!(!head.commit_id.is_empty());
    assert_eq!(head.description, "");

    // Describe it, then read it back.
    vcs_jj::describe(dir, "hello jj").expect("describe");
    assert_eq!(
        vcs_jj::current_change(dir)
            .expect("current_change")
            .description,
        "hello jj"
    );

    // Start a new change; it becomes the working copy.
    vcs_jj::new_change(dir, "second change").expect("new");
    assert_eq!(
        vcs_jj::current_change(dir)
            .expect("current_change")
            .description,
        "second change"
    );

    // Both changes are reachable from @.
    let log = vcs_jj::log(dir, "::@", 10).expect("log");
    assert!(
        log.len() >= 2,
        "expected at least two changes, got {}",
        log.len()
    );
    assert!(log.iter().any(|c| c.description == "hello jj"));

    // status returns something without erroring.
    vcs_jj::status(dir).expect("status");
}

#[test]
#[ignore = "requires the jj binary"]
fn bookmarks_are_listed() {
    let tmp = TempDir::new("bookmarks");
    let dir = tmp.path();
    init_repo(dir);

    vcs_jj::describe(dir, "rooted").expect("describe");
    vcs_jj::exec()
        .current_dir(dir)
        .args(["bookmark", "create", "mark", "-r", "@"])
        .run()
        .expect("bookmark create");

    let bookmarks = vcs_jj::bookmarks(dir).expect("bookmarks");
    assert!(
        bookmarks.iter().any(|b| b.name == "mark"),
        "expected bookmark 'mark', got {bookmarks:?}"
    );
}
