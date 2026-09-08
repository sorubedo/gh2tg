use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::{
    config::CommitConfig,
    github::{
        GitHubClient, GitHubError,
        commits::{self as github_commits, CommitComparisonStatus, FetchedCommitComparison},
    },
    state::CommitCursors,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitInfo {
    sha: String,
    message: String,
    author: String,
    committed_at: String,
    html_url: String,
}

impl CommitInfo {
    pub(crate) fn new(
        sha: String,
        message: String,
        author: String,
        committed_at: String,
        html_url: String,
    ) -> Self {
        Self {
            sha,
            message,
            author,
            committed_at,
            html_url,
        }
    }

    pub fn sha(&self) -> &str {
        &self.sha
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn author(&self) -> &str {
        &self.author
    }

    pub fn committed_at(&self) -> &str {
        &self.committed_at
    }

    pub fn html_url(&self) -> &str {
        &self.html_url
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CommitCandidate {
    commit: CommitInfo,
    parent_shas: Vec<String>,
}

impl CommitCandidate {
    pub(crate) fn new(commit: CommitInfo, parent_shas: Vec<String>) -> Self {
        Self {
            commit,
            parent_shas,
        }
    }

    fn sha(&self) -> &str {
        self.commit.sha()
    }

    fn parent_shas(&self) -> &[String] {
        &self.parent_shas
    }

    fn into_commit(self) -> CommitInfo {
        self.commit
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitCheckpoint {
    branch: String,
    sha: String,
}

impl CommitCheckpoint {
    fn new(branch: String, sha: String) -> Self {
        Self { branch, sha }
    }

    pub fn branch(&self) -> &str {
        &self.branch
    }

    pub fn sha(&self) -> &str {
        &self.sha
    }

    pub fn into_state_parts(self) -> (String, String) {
        (self.branch, self.sha)
    }
}

#[derive(Clone, Debug)]
pub struct CommitBatch {
    commits: Vec<CommitInfo>,
    checkpoint: CommitCheckpoint,
}

impl CommitBatch {
    fn new(commits: Vec<CommitInfo>, checkpoint: CommitCheckpoint) -> Self {
        Self {
            commits,
            checkpoint,
        }
    }

    pub fn branch(&self) -> &str {
        self.checkpoint.branch()
    }

    pub fn commits(&self) -> &[CommitInfo] {
        &self.commits
    }

    pub fn into_checkpoint(self) -> CommitCheckpoint {
        self.checkpoint
    }
}

#[derive(Clone, Debug)]
pub enum CommitCheckOutcome {
    BaselineEstablished { checkpoint: CommitCheckpoint },
    Unchanged,
    HistoryRewritten { checkpoint: CommitCheckpoint },
    UpdateDetected { batch: CommitBatch },
}

#[derive(Clone, Debug)]
pub struct CommitChecks {
    outcomes: Vec<CommitCheckOutcome>,
}

impl CommitChecks {
    pub fn baseline_count(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|outcome| matches!(outcome, CommitCheckOutcome::BaselineEstablished { .. }))
            .count()
    }

    pub fn rewritten_count(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|outcome| matches!(outcome, CommitCheckOutcome::HistoryRewritten { .. }))
            .count()
    }

    pub fn detected_commit_count(&self) -> usize {
        self.outcomes
            .iter()
            .map(|outcome| match outcome {
                CommitCheckOutcome::UpdateDetected { batch } => batch.commits().len(),
                _ => 0,
            })
            .sum()
    }

    pub fn into_outcomes(self) -> Vec<CommitCheckOutcome> {
        self.outcomes
    }
}

#[derive(Debug, Error)]
pub enum CommitError {
    #[error(transparent)]
    GitHub(#[from] GitHubError),
    #[error("branch {branch} commit graph contains duplicate SHA: {sha}")]
    DuplicateCommitSha { branch: String, sha: String },
    #[error("branch {branch} commit graph is not a valid directed acyclic graph")]
    InvalidCommitGraph { branch: String },
}

pub async fn check_for_commit_updates(
    github: &GitHubClient,
    repository: &str,
    config: &CommitConfig,
    cursors: &CommitCursors,
) -> Result<CommitChecks, CommitError> {
    let mut outcomes = Vec::with_capacity(config.branches.len());

    for branch in &config.branches {
        let head_sha = github_commits::fetch_branch_head(github, repository, branch).await?;
        let checkpoint = CommitCheckpoint::new(branch.clone(), head_sha.clone());
        let Some(previous_sha) = cursors.cursor_for(branch) else {
            outcomes.push(CommitCheckOutcome::BaselineEstablished { checkpoint });
            continue;
        };

        if previous_sha == head_sha {
            outcomes.push(CommitCheckOutcome::Unchanged);
            continue;
        }

        let fetched =
            github_commits::fetch_commit_comparison(github, repository, previous_sha, &head_sha)
                .await?;
        outcomes.push(classify_comparison(branch, checkpoint, fetched)?);
    }

    Ok(CommitChecks { outcomes })
}

fn classify_comparison(
    branch: &str,
    checkpoint: CommitCheckpoint,
    fetched: FetchedCommitComparison,
) -> Result<CommitCheckOutcome, CommitError> {
    let FetchedCommitComparison::Compared { status, commits } = fetched else {
        return Ok(CommitCheckOutcome::HistoryRewritten { checkpoint });
    };

    if status != CommitComparisonStatus::Ahead || commits.is_empty() {
        return Ok(CommitCheckOutcome::HistoryRewritten { checkpoint });
    }

    let commits = order_topologically(branch, commits)?;
    Ok(CommitCheckOutcome::UpdateDetected {
        batch: CommitBatch::new(commits, checkpoint),
    })
}

fn order_topologically(
    branch: &str,
    commits: Vec<CommitCandidate>,
) -> Result<Vec<CommitInfo>, CommitError> {
    let mut indices = BTreeMap::new();
    for (index, commit) in commits.iter().enumerate() {
        if indices.insert(commit.sha().to_owned(), index).is_some() {
            return Err(CommitError::DuplicateCommitSha {
                branch: branch.to_owned(),
                sha: commit.sha().to_owned(),
            });
        }
    }

    let mut indegrees = vec![0_usize; commits.len()];
    let mut children = vec![Vec::new(); commits.len()];
    for (child_index, commit) in commits.iter().enumerate() {
        for parent_sha in commit.parent_shas() {
            if let Some(&parent_index) = indices.get(parent_sha) {
                indegrees[child_index] += 1;
                children[parent_index].push(child_index);
            }
        }
    }

    let mut ready = indegrees
        .iter()
        .enumerate()
        .filter_map(|(index, &indegree)| (indegree == 0).then_some(index))
        .collect::<BTreeSet<_>>();
    let mut order = Vec::with_capacity(commits.len());
    while let Some(index) = ready.pop_first() {
        order.push(index);
        for &child_index in &children[index] {
            indegrees[child_index] -= 1;
            if indegrees[child_index] == 0 {
                ready.insert(child_index);
            }
        }
    }

    if order.len() != commits.len() {
        return Err(CommitError::InvalidCommitGraph {
            branch: branch.to_owned(),
        });
    }

    let mut commits = commits.into_iter().map(Some).collect::<Vec<_>>();
    Ok(order
        .into_iter()
        .map(|index| {
            commits[index]
                .take()
                .expect("commit index must be unique")
                .into_commit()
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_commits_are_ordered_after_both_parents() {
        let commits = vec![
            candidate("merge", &["left", "right"]),
            candidate("right", &["base"]),
            candidate("left", &["base"]),
        ];

        let ordered = order_topologically("main", commits)
            .expect("a valid merge graph must be ordered")
            .into_iter()
            .map(|commit| commit.sha().to_owned())
            .collect::<Vec<_>>();

        let merge_index = ordered
            .iter()
            .position(|sha| sha == "merge")
            .expect("merge commit must be present");
        let left_index = ordered
            .iter()
            .position(|sha| sha == "left")
            .expect("left parent must be present");
        let right_index = ordered
            .iter()
            .position(|sha| sha == "right")
            .expect("right parent must be present");
        assert!(left_index < merge_index);
        assert!(right_index < merge_index);
    }

    #[test]
    fn update_batch_checkpoints_the_pinned_head() {
        let checkpoint = CommitCheckpoint::new("main".to_owned(), "head".to_owned());
        let outcome = classify_comparison(
            "main",
            checkpoint,
            FetchedCommitComparison::Compared {
                status: CommitComparisonStatus::Ahead,
                commits: vec![candidate("left", &["base"]), candidate("head", &["left"])],
            },
        )
        .expect("an ahead comparison must produce an update");

        let CommitCheckOutcome::UpdateDetected { batch } = outcome else {
            panic!("an ahead comparison must produce a Commit batch");
        };
        assert_eq!(batch.commits().len(), 2);
        assert_eq!(batch.into_checkpoint().sha(), "head");
    }

    #[test]
    fn diverged_history_resets_to_the_pinned_head() {
        let checkpoint = CommitCheckpoint::new("main".to_owned(), "new-head".to_owned());
        let outcome = classify_comparison(
            "main",
            checkpoint,
            FetchedCommitComparison::Compared {
                status: CommitComparisonStatus::Diverged,
                commits: vec![candidate("new-head", &["other-base"])],
            },
        )
        .expect("diverged history must be classified");

        let CommitCheckOutcome::HistoryRewritten { checkpoint } = outcome else {
            panic!("diverged history must reset the Commit baseline");
        };
        assert_eq!(checkpoint.sha(), "new-head");
    }

    #[test]
    fn duplicate_commit_sha_is_rejected() {
        let error =
            order_topologically("main", vec![candidate("same", &[]), candidate("same", &[])])
                .expect_err("duplicate Commit SHAs must be rejected");

        assert!(matches!(
            error,
            CommitError::DuplicateCommitSha { branch, sha }
                if branch == "main" && sha == "same"
        ));
    }

    fn candidate(sha: &str, parent_shas: &[&str]) -> CommitCandidate {
        CommitCandidate::new(
            CommitInfo::new(
                sha.to_owned(),
                format!("Commit {sha}"),
                "octocat".to_owned(),
                "2026-09-08T00:00:00Z".to_owned(),
                format!("https://github.com/owner/repo/commit/{sha}"),
            ),
            parent_shas.iter().map(|sha| (*sha).to_owned()).collect(),
        )
    }
}
