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

/// An issue (`tea issues list --output json` / `tea issues <index> --output
/// json`), flattened from Gitea's REST `Issue` object.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Issue {
    /// Issue number (Gitea's `number`; tea's `index` is accepted as an alias).
    pub number: u64,
    /// Issue title.
    pub title: String,
    /// State, e.g. `"open"`, `"closed"` (Gitea's lower-case spelling).
    pub state: String,
    /// Issue body / description (Gitea's `body`).
    pub body: String,
    /// Web URL (Gitea's `html_url`).
    pub url: String,
}

#[derive(Deserialize)]
struct IssueJson {
    // No `default`: an issue entry always carries `number` (or tea's `index`
    // alias), so a missing id is a real parse failure, not a silent `0` that
    // `issue_view` could then "find".
    #[serde(alias = "index")]
    number: u64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    body: String,
    #[serde(rename = "html_url", default)]
    html_url: String,
}

impl From<IssueJson> for Issue {
    fn from(raw: IssueJson) -> Self {
        Issue {
            number: raw.number,
            title: raw.title,
            state: raw.state,
            body: raw.body,
            url: raw.html_url,
        }
    }
}

/// A release (`tea releases list --output json`), flattened from Gitea's REST
/// `Release` object.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Release {
    /// Git tag the release points at (Gitea's `tag_name`).
    pub tag: String,
    /// Release title (Gitea's `name`).
    pub title: String,
    /// Publish timestamp as Gitea renders it, e.g. `"2023-07-26T13:02:36Z"`
    /// (Gitea's `published_at`); empty for an unpublished draft.
    pub published_at: String,
    /// Whether the release is a draft (Gitea's `draft`).
    pub draft: bool,
    /// Whether the release is a pre-release (Gitea's `prerelease`).
    pub prerelease: bool,
    /// Web URL (Gitea's `html_url`).
    pub url: String,
}

#[derive(Deserialize)]
struct ReleaseJson {
    // No `default`: a release entry always carries `tag_name`, so a missing tag
    // is a real parse failure rather than a silent empty string.
    tag_name: String,
    // Gitea names the release title `name`.
    #[serde(rename = "name", default)]
    title: String,
    #[serde(default)]
    published_at: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(rename = "html_url", default)]
    html_url: String,
}

impl From<ReleaseJson> for Release {
    fn from(raw: ReleaseJson) -> Self {
        Release {
            tag: raw.tag_name,
            title: raw.title,
            published_at: raw.published_at,
            draft: raw.draft,
            prerelease: raw.prerelease,
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

/// Parse `tea issues list --output json` into the flattened [`Issue`]s.
pub(crate) fn parse_issue_list(json: &str) -> Result<Vec<Issue>> {
    let raw: Vec<IssueJson> = from_json(json)?;
    Ok(raw.into_iter().map(Issue::from).collect())
}

/// Parse `tea issues <index> --output json` into a single [`Issue`]. Unlike the
/// list, the single-issue view yields one object, not an array.
pub(crate) fn parse_issue(json: &str) -> Result<Issue> {
    let raw: IssueJson = from_json(json)?;
    Ok(Issue::from(raw))
}

/// Parse `tea releases list --output json` into the flattened [`Release`]s.
pub(crate) fn parse_release_list(json: &str) -> Result<Vec<Release>> {
    let raw: Vec<ReleaseJson> = from_json(json)?;
    Ok(raw.into_iter().map(Release::from).collect())
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

    // tea marshals Gitea's REST `Issue` for `tea issues list --output json`;
    // flatten `html_url` and tolerate the `index` alias + missing body.
    #[test]
    fn parses_issue_list_flattening_fields() {
        let json = r#"[
            {"number": 12, "title": "Bug", "state": "open", "body": "broken",
             "html_url": "https://gitea/issues/12"}
        ]"#;
        let issues = parse_issue_list(json).expect("parse issues");
        assert_eq!(issues.len(), 1);
        assert_eq!(
            issues[0],
            Issue {
                number: 12,
                title: "Bug".into(),
                state: "open".into(),
                body: "broken".into(),
                url: "https://gitea/issues/12".into(),
            }
        );
    }

    // tea's column projection may name the number `index` and omit body/url;
    // the alias + defaults must keep it parseable.
    #[test]
    fn issue_tolerates_index_alias_and_missing_fields() {
        let json = r#"[{"index": 4, "title": "wip", "state": "open"}]"#;
        let issues = parse_issue_list(json).expect("parse issues");
        assert_eq!(issues[0].number, 4);
        assert_eq!(issues[0].body, "");
        assert_eq!(issues[0].url, "");
    }

    // The single-issue view (`tea issues <index> --output json`) yields one
    // object, not an array.
    #[test]
    fn parses_single_issue_object() {
        let json = r#"{"number": 7, "title": "One", "state": "closed", "body": "b",
                       "html_url": "https://gitea/issues/7"}"#;
        let issue = parse_issue(json).expect("parse issue");
        assert_eq!(issue.number, 7);
        assert_eq!(issue.title, "One");
        assert_eq!(issue.state, "closed");
    }

    // tea marshals Gitea's REST `Release`: `name` is the title, `tag_name` the
    // tag, plus draft/prerelease/published_at/html_url.
    #[test]
    fn parses_release_list_flattening_fields() {
        let json = r#"[
            {"tag_name": "0.1", "name": "First", "draft": false,
             "prerelease": false, "published_at": "2023-07-26T13:02:36Z",
             "html_url": "https://gitea/releases/0.1"}
        ]"#;
        let releases = parse_release_list(json).expect("parse releases");
        assert_eq!(releases.len(), 1);
        assert_eq!(
            releases[0],
            Release {
                tag: "0.1".into(),
                title: "First".into(),
                published_at: "2023-07-26T13:02:36Z".into(),
                draft: false,
                prerelease: false,
                url: "https://gitea/releases/0.1".into(),
            }
        );
    }

    // A draft release has no publish timestamp and flips `draft`; defaults must
    // keep it parseable when those optional fields are absent.
    #[test]
    fn release_tolerates_draft_and_missing_fields() {
        let json = r#"[{"tag_name": "v2", "name": "Two", "draft": true}]"#;
        let releases = parse_release_list(json).expect("parse releases");
        assert_eq!(releases[0].tag, "v2");
        assert!(releases[0].draft);
        assert_eq!(releases[0].published_at, "");
        assert!(!releases[0].prerelease);
    }

    // A release entry without `tag_name` is a real parse failure, not a silent
    // empty tag.
    #[test]
    fn release_missing_tag_is_a_parse_error() {
        match parse_release_list(r#"[{"name": "no tag"}]"#).unwrap_err() {
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
