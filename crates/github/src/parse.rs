//! Typed results from `gh … --json` and the deserialization helper. Parsing is
//! pure, so these tests are hermetic and run on CI.

use std::io;

use serde::Deserialize;
use serde::de::DeserializeOwned;

/// A pull request (`gh pr list/view --json number,title,state,headRefName`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PullRequest {
    /// PR number.
    pub number: u64,
    /// PR title.
    pub title: String,
    /// State, e.g. `"OPEN"`, `"MERGED"`, `"CLOSED"`.
    pub state: String,
    /// Source branch name.
    #[serde(rename = "headRefName", default)]
    pub head_ref_name: String,
}

/// An issue (`gh issue list --json number,title,state`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Issue {
    /// Issue number.
    pub number: u64,
    /// Issue title.
    pub title: String,
    /// State, e.g. `"OPEN"`, `"CLOSED"`.
    pub state: String,
}

/// A repository (`gh repo view --json name,nameWithOwner,description`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Repo {
    /// Repository name.
    pub name: String,
    /// `owner/name`.
    #[serde(rename = "nameWithOwner", default)]
    pub name_with_owner: String,
    /// Description, `None` when GitHub returns `null`.
    #[serde(default)]
    pub description: Option<String>,
}

/// Deserialize `gh --json` output into `T`, mapping parse errors to `io::Error`.
pub(crate) fn from_json<T: DeserializeOwned>(json: &str) -> io::Result<T> {
    serde_json::from_str(json).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pr_list() {
        let json = r#"[
            {"number": 12, "title": "Add feature", "state": "OPEN", "headRefName": "feat/x"},
            {"number": 9, "title": "Fix bug", "state": "MERGED", "headRefName": "fix/y"}
        ]"#;
        let prs: Vec<PullRequest> = from_json(json).expect("parse prs");
        assert_eq!(prs.len(), 2);
        assert_eq!(
            prs[0],
            PullRequest {
                number: 12,
                title: "Add feature".into(),
                state: "OPEN".into(),
                head_ref_name: "feat/x".into(),
            }
        );
        assert_eq!(prs[1].state, "MERGED");
    }

    #[test]
    fn parses_issue_list() {
        let json = r#"[{"number": 3, "title": "Docs", "state": "OPEN"}]"#;
        let issues: Vec<Issue> = from_json(json).expect("parse issues");
        assert_eq!(issues[0].number, 3);
    }

    #[test]
    fn parses_repo_with_null_description() {
        let json = r#"{"name": "vcs-toolkit-rs", "nameWithOwner": "ZelAnton/vcs-toolkit-rs", "description": null}"#;
        let repo: Repo = from_json(json).expect("parse repo");
        assert_eq!(repo.name, "vcs-toolkit-rs");
        assert_eq!(repo.name_with_owner, "ZelAnton/vcs-toolkit-rs");
        assert_eq!(repo.description, None);
    }

    #[test]
    fn malformed_json_is_an_io_error() {
        let err = from_json::<Vec<Issue>>("not json").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }
}
