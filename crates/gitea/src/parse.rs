//! Typed results from `tea … --output json` and the deserialization helpers.
//! `tea` marshals the Gitea SDK structs, so its JSON is the Gitea REST shape;
//! parsing is pure, so these tests are hermetic and run on CI.

use processkit::{Error, Result};
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::BINARY;

/// A pull request (`tea pr list --output json`), flattened from Gitea's REST
/// `PullRequest` object.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PullRequest {
    /// PR number (Gitea's `number`; tea's `index` is accepted as an alias).
    pub number: u64,
    /// PR title.
    pub title: String,
    /// State, e.g. `"open"`, `"closed"` (Gitea's lower-case spelling).
    pub state: String,
    /// Whether the PR has been merged (a merged PR also reports `state="closed"`).
    pub merged: bool,
    /// Source (head) branch name (Gitea's `head.ref`).
    pub head_branch: String,
    /// Target (base) branch name (Gitea's `base.ref`).
    pub base_branch: String,
    /// Web URL (Gitea's `html_url`).
    pub url: String,
}

// Gitea nests the head/base as objects carrying a `ref`; deserialize into these
// and flatten into the public `PullRequest`. `number`/`index` both map to the
// number so the DTO survives either tea's projection or the raw SDK struct.
#[derive(Deserialize)]
struct PrJson {
    // No `default`: a PR entry always carries `number` (or tea's `index` alias),
    // so a missing id is a real parse failure, not a silent `0` that `pr_view`
    // could then "find".
    #[serde(alias = "index")]
    number: u64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    merged: bool,
    #[serde(rename = "html_url", default)]
    html_url: String,
    #[serde(default)]
    head: Option<BranchJson>,
    #[serde(default)]
    base: Option<BranchJson>,
}

#[derive(Deserialize)]
struct BranchJson {
    #[serde(rename = "ref", default)]
    r#ref: String,
}

impl From<PrJson> for PullRequest {
    fn from(raw: PrJson) -> Self {
        PullRequest {
            number: raw.number,
            title: raw.title,
            state: raw.state,
            merged: raw.merged,
            head_branch: raw.head.map(|b| b.r#ref).unwrap_or_default(),
            base_branch: raw.base.map(|b| b.r#ref).unwrap_or_default(),
            url: raw.html_url,
        }
    }
}

/// Deserialize `tea … --output json` output into `T`, mapping parse errors to
/// [`Error::Parse`].
pub(crate) fn from_json<T: DeserializeOwned>(json: &str) -> Result<T> {
    serde_json::from_str(json).map_err(|e| Error::Parse {
        program: BINARY.to_string(),
        message: e.to_string(),
    })
}

/// Parse `tea pr list --output json` into the flattened [`PullRequest`]s.
pub(crate) fn parse_pr_list(json: &str) -> Result<Vec<PullRequest>> {
    let raw: Vec<PrJson> = from_json(json)?;
    Ok(raw.into_iter().map(PullRequest::from).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pr_list_flattening_branch_refs() {
        let json = r#"[
            {"number": 7, "title": "Add X", "state": "open", "merged": false,
             "html_url": "https://gitea/pr/7",
             "head": {"ref": "feat/x"}, "base": {"ref": "main"}}
        ]"#;
        let prs = parse_pr_list(json).expect("parse prs");
        assert_eq!(prs.len(), 1);
        assert_eq!(
            prs[0],
            PullRequest {
                number: 7,
                title: "Add X".into(),
                state: "open".into(),
                merged: false,
                head_branch: "feat/x".into(),
                base_branch: "main".into(),
                url: "https://gitea/pr/7".into(),
            }
        );
    }

    // tea's column projection may name the number `index` and omit head/base;
    // the alias + defaults must keep it parseable.
    #[test]
    fn pr_tolerates_index_alias_and_missing_branches() {
        let json = r#"[{"index": 3, "title": "wip", "state": "open"}]"#;
        let prs = parse_pr_list(json).expect("parse prs");
        assert_eq!(prs[0].number, 3);
        assert_eq!(prs[0].head_branch, "");
        assert!(!prs[0].merged);
    }

    #[test]
    fn malformed_json_is_a_parse_error() {
        match parse_pr_list("not json").unwrap_err() {
            Error::Parse { .. } => {}
            other => panic!("expected Parse, got {other:?}"),
        }
    }

    // auth_status counts the logins array; an empty array means "not logged in".
    #[test]
    fn login_array_counts() {
        let some: Vec<serde_json::Value> =
            from_json(r#"[{"name":"gitea"}]"#).expect("parse logins");
        assert!(!some.is_empty());
        let none: Vec<serde_json::Value> = from_json("[]").expect("parse empty");
        assert!(none.is_empty());
    }
}
