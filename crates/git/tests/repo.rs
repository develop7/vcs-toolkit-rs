//! End-to-end tests for the typed `vcs-git` client against a real temporary
//! repository. Ignored by default (require the `git` binary); run with
//! `cargo test -p vcs-git -- --ignored`.

mod common;

use std::path::{Path, PathBuf};

use common::TempDir;
use vcs_git::{Git, GitApi};

/// Give the repo a deterministic identity so commits don't depend on global config.
fn configure(dir: &Path) {
    for (key, val) in [
        ("user.email", "test@example.com"),
        ("user.name", "Test"),
        ("commit.gpgsign", "false"),
    ] {
        std::process::Command::new(vcs_git::BINARY)
            .current_dir(dir)
            .args(["config", key, val])
            .status()
            .expect("git config");
    }
}

#[test]
#[ignore = "requires the git binary"]
fn init_status_add_commit_log_cycle() {
    let tmp = TempDir::new("cycle");
    let dir = tmp.path();
    let git = Git::new();

    git.init(dir).expect("init");
    configure(dir);

    // Untracked file shows up in status.
    std::fs::write(dir.join("file.txt"), "hello\n").expect("write file");
    let status = git.status(dir).expect("status");
    assert_eq!(status.len(), 1);
    assert_eq!(status[0].code, "??");
    assert_eq!(status[0].path, "file.txt");

    // Stage + commit, then status is clean.
    git.add(dir, &[PathBuf::from("file.txt")]).expect("add");
    git.commit(dir, "initial commit").expect("commit");
    assert!(git.status(dir).expect("status").is_empty());

    // Log reflects the commit.
    let log = git.log(dir, 10).expect("log");
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].subject, "initial commit");
    assert_eq!(log[0].author, "Test");
    assert_eq!(log[0].hash.len(), 40, "full sha expected");

    // Branch introspection.
    let branch = git.current_branch(dir).expect("current_branch");
    assert!(!branch.is_empty());
    let branches = git.branches(dir).expect("branches");
    assert!(branches.iter().any(|b| b.current && b.name == branch));

    // rev_parse resolves HEAD to the commit hash.
    assert_eq!(git.rev_parse(dir, "HEAD").expect("rev-parse"), log[0].hash);
}

#[test]
#[ignore = "requires the git binary"]
fn diff_is_empty_tracks_worktree_changes() {
    let tmp = TempDir::new("diff");
    let dir = tmp.path();
    let git = Git::new();

    git.init(dir).expect("init");
    configure(dir);
    std::fs::write(dir.join("a.txt"), "one\n").expect("write");
    git.add(dir, &[PathBuf::from("a.txt")]).expect("add");
    git.commit(dir, "add a").expect("commit");

    assert!(git.diff_is_empty(dir).expect("clean"), "no changes yet");

    std::fs::write(dir.join("a.txt"), "two\n").expect("modify");
    assert!(
        !git.diff_is_empty(dir).expect("dirty"),
        "unstaged change should be visible"
    );
}
