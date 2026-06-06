//! Integration tests for `vcs-gitlab`. Ignored by default (require the `glab`
//! binary). The repo/MR commands need network + authentication and are not
//! exercised here — their JSON parsing is covered by the hermetic unit tests in
//! `src/parse.rs` and the scripted-runner tests in `src/lib.rs`. Run with
//! `cargo test -p vcs-gitlab -- --ignored`.
//!
//! `glab` is less universally installed than `git`/`gh`, so each test **skips
//! gracefully** (prints and returns) when the binary is absent, rather than
//! failing — CI installs it best-effort.

use vcs_gitlab::{GitLab, GitLabApi};

/// Whether `glab` is on PATH (a successful `--version` spawn).
async fn glab_present() -> bool {
    GitLab::new().version().await.is_ok()
}

#[tokio::test]
#[ignore = "requires the glab binary"]
async fn version_mentions_glab() {
    if !glab_present().await {
        eprintln!("skipping: glab not installed");
        return;
    }
    let v = GitLab::new().version().await.expect("glab version");
    // glab prints "glab version x.y.z …".
    assert!(v.to_lowercase().contains("glab"), "unexpected: {v}");
}

#[tokio::test]
#[ignore = "requires the glab binary"]
async fn auth_status_does_not_error() {
    if !glab_present().await {
        eprintln!("skipping: glab not installed");
        return;
    }
    // Reports the bool whether or not the user is logged in; must not error.
    let _authed = GitLab::new()
        .auth_status()
        .await
        .expect("auth_status should not error");
}
