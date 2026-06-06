//! Integration tests for `vcs-gitea`. Ignored by default (require the `tea`
//! binary). The PR commands need network + a configured login and are not
//! exercised here — their JSON parsing is covered by the hermetic unit tests in
//! `src/parse.rs` and the scripted-runner tests in `src/lib.rs`. Run with
//! `cargo test -p vcs-gitea -- --ignored`.
//!
//! `tea` is rarely pre-installed, so each test **skips gracefully** (prints and
//! returns) when the binary is absent, rather than failing — CI installs it
//! best-effort.

use vcs_gitea::{Gitea, GiteaApi};

/// Whether `tea` is on PATH (a successful `--version` spawn).
async fn tea_present() -> bool {
    Gitea::new().version().await.is_ok()
}

#[tokio::test]
#[ignore = "requires the tea binary"]
async fn version_runs() {
    if !tea_present().await {
        eprintln!("skipping: tea not installed");
        return;
    }
    let v = Gitea::new().version().await.expect("tea version");
    assert!(!v.trim().is_empty(), "expected a version string");
}

#[tokio::test]
#[ignore = "requires the tea binary"]
async fn auth_status_does_not_error() {
    if !tea_present().await {
        eprintln!("skipping: tea not installed");
        return;
    }
    // Reports the bool whether or not a login is configured; must not error.
    let _authed = Gitea::new()
        .auth_status()
        .await
        .expect("auth_status should not error");
}
