//! Gitea-backed implementations of the facade operations: thin calls to the
//! `vcs-gitea` client plus pure mappers from its types into the unified DTOs.
//!
//! `tea` has no current-repo view, draft toggle, or PR-checks command, so
//! `repo_view` / `pr_mark_ready` / `pr_checks` have no function here — the
//! [`Forge`](crate::Forge) dispatch returns [`Unsupported`](crate::Error::Unsupported)
//! for the Gitea backend instead.

use std::path::Path;

use processkit::ProcessRunner;
use vcs_gitea::{Gitea, GiteaApi, MergeStrategy as GtMs, PullRequest};

use crate::dto::{ForgePr, ForgePrState, MergeStrategy};
use crate::error::Result;

pub(crate) async fn auth_status<R: ProcessRunner>(tea: &Gitea<R>) -> Result<bool> {
    Ok(tea.auth_status().await?)
}

pub(crate) async fn pr_list<R: ProcessRunner>(tea: &Gitea<R>, dir: &Path) -> Result<Vec<ForgePr>> {
    Ok(tea.pr_list(dir).await?.into_iter().map(map_pr).collect())
}

pub(crate) async fn pr_view<R: ProcessRunner>(
    tea: &Gitea<R>,
    dir: &Path,
    number: u64,
) -> Result<ForgePr> {
    Ok(map_pr(tea.pr_view(dir, number).await?))
}

pub(crate) async fn pr_create<R: ProcessRunner>(
    tea: &Gitea<R>,
    dir: &Path,
    title: &str,
    body: &str,
    source: Option<String>,
    target: Option<String>,
) -> Result<String> {
    Ok(tea.pr_create(dir, title, body, source, target).await?)
}

pub(crate) async fn pr_merge<R: ProcessRunner>(
    tea: &Gitea<R>,
    dir: &Path,
    number: u64,
    strategy: MergeStrategy,
) -> Result<()> {
    let ms = match strategy {
        MergeStrategy::Merge => GtMs::Merge,
        MergeStrategy::Squash => GtMs::Squash,
        MergeStrategy::Rebase => GtMs::Rebase,
    };
    tea.pr_merge(dir, number, ms).await?;
    Ok(())
}

// `tea pr close` takes no branch-deletion flag, so `delete_branch` is ignored.
pub(crate) async fn pr_close<R: ProcessRunner>(
    tea: &Gitea<R>,
    dir: &Path,
    number: u64,
) -> Result<()> {
    tea.pr_close(dir, number).await?;
    Ok(())
}

fn map_pr(pr: PullRequest) -> ForgePr {
    ForgePr {
        number: pr.number,
        // Gitea reports `merged` separately; a merged PR is also `state="closed"`.
        state: if pr.merged {
            ForgePrState::Merged
        } else if pr.state.eq_ignore_ascii_case("closed") {
            ForgePrState::Closed
        } else {
            ForgePrState::Open
        },
        title: pr.title,
        source_branch: pr.head_branch,
        target_branch: pr.base_branch,
        url: pr.url,
        draft: false,
    }
}
