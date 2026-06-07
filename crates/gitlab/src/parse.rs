//! Typed results from `glab … --output json` and the deserialization helpers.
//! Parsing is pure (over GitLab's REST JSON, which `glab` emits verbatim), so
//! these tests are hermetic and run on CI.

use processkit::{Error, Result};
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::BINARY;

/// A merge request (`glab mr list/view --output json`). The fields are GitLab's
/// REST `MergeRequest` object, which `glab` passes through unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[non_exhaustive]
pub struct MergeRequest {
    /// The **project-scoped** id (`iid`) — the `<id>` other `glab mr` commands
    /// take. (GitLab's global `id` is deliberately not surfaced.)
    pub iid: u64,
    /// MR title.
    pub title: String,
    /// State, e.g. `"opened"`, `"closed"`, `"merged"`, `"locked"` (GitLab's
    /// lower-case spelling — note it is `"opened"`, not `"open"`).
    pub state: String,
    /// Source (head) branch name.
    #[serde(default)]
    pub source_branch: String,
    /// Target (base) branch name.
    #[serde(default)]
    pub target_branch: String,
    /// Web URL.
    #[serde(default)]
    pub web_url: String,
    /// Whether the MR is a draft (GitLab's `draft`; the deprecated
    /// `work_in_progress` is not read).
    #[serde(default)]
    pub draft: bool,
}

/// A project (`glab repo view --output json`) — GitLab's REST `Project` object.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[non_exhaustive]
pub struct Project {
    /// Project name (the last path segment's display name).
    pub name: String,
    /// Full namespace path, e.g. `"group/subgroup/repo"`.
    #[serde(default)]
    pub path_with_namespace: String,
    /// Default branch name (empty for an empty project).
    #[serde(default)]
    pub default_branch: String,
    /// Web URL.
    #[serde(default)]
    pub web_url: String,
    /// Visibility, e.g. `"public"`, `"internal"`, `"private"`. `None` when glab
    /// omits the field — a consumer must treat an absent visibility as *unknown*,
    /// not as private (see [`ForgeRepo::private`](../../vcs_forge/struct.ForgeRepo.html)).
    #[serde(default)]
    pub visibility: Option<String>,
}

/// The coarse CI/pipeline outcome for an MR (`glab mr view … --output json`'s
/// `head_pipeline.status`), bucketed into the four states a caller acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CiStatus {
    /// The pipeline succeeded (`success`).
    Passing,
    /// The pipeline failed or was canceled (`failed`/`canceled`).
    Failing,
    /// The pipeline is still going (`running`/`pending`/`created`/…).
    Pending,
    /// No pipeline ran (none attached, or `skipped`).
    None,
}

impl CiStatus {
    /// Bucket a raw GitLab pipeline `status` string. Unknown values read as
    /// [`Pending`](CiStatus::Pending) (conservative — "not known to be done").
    pub(crate) fn from_gitlab(status: &str) -> Self {
        match status {
            "success" => CiStatus::Passing,
            "failed" | "canceled" | "cancelled" => CiStatus::Failing,
            "skipped" | "" => CiStatus::None,
            "running"
            | "pending"
            | "created"
            | "preparing"
            | "scheduled"
            | "waiting_for_resource"
            | "manual" => CiStatus::Pending,
            _ => CiStatus::Pending,
        }
    }
}

/// Deserialize `glab … --output json` output into `T`, mapping parse errors to
/// [`Error::Parse`].
pub(crate) fn from_json<T: DeserializeOwned>(json: &str) -> Result<T> {
    serde_json::from_str(json).map_err(|e| Error::Parse {
        program: BINARY.to_string(),
        message: e.to_string(),
    })
}

// The MR JSON carries the pipeline as a nested object; deserialize just the
// status off it. `head_pipeline` is the current one; `pipeline` is the older
// alias — accept either.
#[derive(Deserialize)]
struct MrPipelineJson {
    #[serde(default)]
    head_pipeline: Option<PipelineJson>,
    #[serde(default)]
    pipeline: Option<PipelineJson>,
}

#[derive(Deserialize)]
struct PipelineJson {
    #[serde(default)]
    status: String,
}

/// Parse the CI/pipeline status out of `glab mr view <id> --output json` —
/// `head_pipeline.status` (falling back to the deprecated `pipeline.status`);
/// no pipeline at all is [`CiStatus::None`].
pub(crate) fn parse_ci_status(json: &str) -> Result<CiStatus> {
    let raw: MrPipelineJson = from_json(json)?;
    let status = raw
        .head_pipeline
        .or(raw.pipeline)
        .map(|p| p.status)
        .unwrap_or_default();
    Ok(CiStatus::from_gitlab(&status))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mr_list() {
        let json = r#"[
            {"iid": 12, "title": "Add feature", "state": "opened",
             "source_branch": "feat/x", "target_branch": "main",
             "web_url": "https://gl/mr/12", "draft": false}
        ]"#;
        let mrs: Vec<MergeRequest> = from_json(json).expect("parse mrs");
        assert_eq!(mrs.len(), 1);
        assert_eq!(
            mrs[0],
            MergeRequest {
                iid: 12,
                title: "Add feature".into(),
                state: "opened".into(),
                source_branch: "feat/x".into(),
                target_branch: "main".into(),
                web_url: "https://gl/mr/12".into(),
                draft: false,
            }
        );
    }

    // glab/GitLab omit fields that don't apply; the DTO must tolerate a minimal
    // object (only the required `iid`/`title`/`state`).
    #[test]
    fn mr_tolerates_missing_optional_fields() {
        let json = r#"{"iid": 5, "title": "wip", "state": "opened", "draft": true}"#;
        let mr: MergeRequest = from_json(json).expect("parse mr");
        assert_eq!(mr.source_branch, "");
        assert_eq!(mr.web_url, "");
        assert!(mr.draft);
    }

    #[test]
    fn parses_project_view() {
        let json = r#"{
            "name": "cli", "path_with_namespace": "gitlab-org/cli",
            "default_branch": "main", "web_url": "https://gl/p",
            "visibility": "public"
        }"#;
        let p: Project = from_json(json).expect("parse project");
        assert_eq!(p.name, "cli");
        assert_eq!(p.path_with_namespace, "gitlab-org/cli");
        assert_eq!(p.default_branch, "main");
        assert_eq!(p.visibility.as_deref(), Some("public"));
    }

    // glab omits `visibility` for some responses; it must deserialize to `None`
    // (unknown), never a default that a consumer could mistake for private.
    #[test]
    fn project_tolerates_missing_visibility() {
        let json = r#"{"name":"cli","path_with_namespace":"o/cli","default_branch":"main"}"#;
        let p: Project = from_json(json).expect("parse project");
        assert_eq!(p.visibility, None);
    }

    #[test]
    fn malformed_json_is_a_parse_error() {
        match from_json::<Vec<MergeRequest>>("not json").unwrap_err() {
            Error::Parse { .. } => {}
            other => panic!("expected Parse, got {other:?}"),
        }
    }

    #[test]
    fn ci_status_buckets_pipeline_states() {
        assert_eq!(CiStatus::from_gitlab("success"), CiStatus::Passing);
        assert_eq!(CiStatus::from_gitlab("failed"), CiStatus::Failing);
        assert_eq!(CiStatus::from_gitlab("canceled"), CiStatus::Failing);
        assert_eq!(CiStatus::from_gitlab("running"), CiStatus::Pending);
        assert_eq!(CiStatus::from_gitlab("manual"), CiStatus::Pending);
        assert_eq!(CiStatus::from_gitlab("skipped"), CiStatus::None);
        assert_eq!(CiStatus::from_gitlab(""), CiStatus::None);
        // Unknown future states read as Pending, not a panic.
        assert_eq!(CiStatus::from_gitlab("brand_new"), CiStatus::Pending);
    }

    #[test]
    fn parse_ci_status_reads_head_pipeline_then_falls_back() {
        // head_pipeline wins.
        let json =
            r#"{"iid":1,"head_pipeline":{"status":"success"},"pipeline":{"status":"failed"}}"#;
        assert_eq!(parse_ci_status(json).unwrap(), CiStatus::Passing);
        // Falls back to the deprecated `pipeline` when there's no head_pipeline.
        let json = r#"{"iid":1,"pipeline":{"status":"failed"}}"#;
        assert_eq!(parse_ci_status(json).unwrap(), CiStatus::Failing);
        // No pipeline at all → None.
        let json = r#"{"iid":1}"#;
        assert_eq!(parse_ci_status(json).unwrap(), CiStatus::None);
    }
}
