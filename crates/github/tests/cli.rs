//! Integration tests for `vcs-github`. Ignored by default (require the `gh`
//! binary). The repo/pr/issue commands need network + authentication and are
//! not exercised here — their JSON parsing is covered by the hermetic unit
//! tests in `src/parse.rs`. Run with `cargo test -p vcs-github -- --ignored`.

#[test]
#[ignore = "requires the gh binary"]
fn version_mentions_gh() {
    let v = vcs_github::version().expect("gh should be installed");
    assert!(v.to_lowercase().contains("gh"), "unexpected: {v}");
}

#[test]
#[ignore = "requires the gh binary"]
fn auth_status_does_not_error() {
    // `auth_status` reports the bool whether or not the user is logged in; it
    // must not surface a non-zero exit as an error.
    let _authed = vcs_github::auth_status().expect("auth_status should not error");
}
