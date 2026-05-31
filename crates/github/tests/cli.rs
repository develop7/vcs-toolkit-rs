//! Integration tests for `vcs-github`. Ignored by default (require the `gh`
//! binary). The repo/pr/issue commands need network + authentication and are
//! not exercised here — their JSON parsing is covered by the hermetic unit
//! tests in `src/parse.rs` and the scripted-runner tests in `src/lib.rs`. Run
//! with `cargo test -p vcs-github -- --ignored`.

use vcs_github::{GitHub, GitHubApi};

#[tokio::test]
#[ignore = "requires the gh binary"]
async fn version_mentions_gh() {
    let v = GitHub::new()
        .version()
        .await
        .expect("gh should be installed");
    assert!(v.to_lowercase().contains("gh"), "unexpected: {v}");
}

#[tokio::test]
#[ignore = "requires the gh binary"]
async fn auth_status_does_not_error() {
    // Reports the bool whether or not the user is logged in; must not error.
    let _authed = GitHub::new()
        .auth_status()
        .await
        .expect("auth_status should not error");
}
