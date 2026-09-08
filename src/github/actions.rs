use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use serde::Deserialize;

use crate::actions::{ActionArtifact, WorkflowRunInfo};

use super::{ConditionalJson, GitHubClient, GitHubError};

const ARTIFACT_PAGE_SIZE: usize = 100;

pub(crate) struct WorkflowRunCandidatesFetch {
    pub(crate) candidates: Vec<WorkflowRunInfo>,
    pub(crate) not_modified_conclusions: BTreeSet<String>,
    pub(crate) etags: BTreeMap<String, String>,
}

pub(crate) enum FetchedWorkflowRunArtifacts {
    NotModified,
    Modified {
        artifacts: Vec<ActionArtifact>,
        etag: Option<String>,
        page_count: usize,
    },
}

#[derive(Clone, Deserialize)]
struct ApiWorkflowRun {
    id: u64,
    name: Option<String>,
    display_title: String,
    head_branch: Option<String>,
    conclusion: Option<String>,
    created_at: String,
    html_url: String,
}

#[derive(Deserialize)]
struct WorkflowRunsResponse {
    workflow_runs: Vec<ApiWorkflowRun>,
}

#[derive(Deserialize)]
struct ApiArtifact {
    id: u64,
    name: String,
    archive_download_url: String,
    expired: bool,
}

#[derive(Deserialize)]
struct ArtifactsResponse {
    artifacts: Vec<ApiArtifact>,
}

pub(crate) async fn fetch_latest_workflow_run_candidates(
    github: &GitHubClient,
    repository: &str,
    workflow_file: &str,
    branch: &str,
    conclusions: &[String],
    current_etags: &BTreeMap<String, String>,
) -> Result<WorkflowRunCandidatesFetch, GitHubError> {
    let workflow_id = workflow_file_name(workflow_file)?;
    let base_url =
        github.repository_url(repository, &["actions", "workflows", workflow_id, "runs"])?;
    let mut candidates = Vec::new();
    let mut not_modified_conclusions = BTreeSet::new();
    let mut etags = BTreeMap::new();

    for conclusion in conclusions {
        let mut url = base_url.clone();
        url.query_pairs_mut()
            .append_pair("branch", branch)
            .append_pair("status", conclusion)
            .append_pair("per_page", "1")
            .append_pair("page", "1");
        match github
            .get_json_conditionally::<WorkflowRunsResponse>(
                url,
                current_etags.get(conclusion).map(String::as_str),
            )
            .await?
        {
            ConditionalJson::NotModified => {
                not_modified_conclusions.insert(conclusion.clone());
                if let Some(etag) = current_etags.get(conclusion) {
                    etags.insert(conclusion.clone(), etag.clone());
                }
            }
            ConditionalJson::NotFound { .. } => {
                return Err(GitHubError::WorkflowNotFound {
                    workflow_file: workflow_file.to_owned(),
                });
            }
            ConditionalJson::Modified {
                value: response,
                etag,
            } => {
                if let Some(etag) = etag {
                    etags.insert(conclusion.clone(), etag);
                }
                if let Some(run) = response
                    .workflow_runs
                    .into_iter()
                    .find(|run| workflow_run_matches(run, branch, conclusion))
                {
                    candidates.push(workflow_run_info(workflow_file, run));
                }
            }
        }
    }

    Ok(WorkflowRunCandidatesFetch {
        candidates,
        not_modified_conclusions,
        etags,
    })
}

pub(crate) async fn fetch_workflow_run_artifacts(
    github: &GitHubClient,
    repository: &str,
    run_id: u64,
    etag: Option<&str>,
) -> Result<FetchedWorkflowRunArtifacts, GitHubError> {
    let run_id = run_id.to_string();
    let base_url = github.repository_url(repository, &["actions", "runs", &run_id, "artifacts"])?;
    let mut first_page_url = base_url.clone();
    first_page_url
        .query_pairs_mut()
        .append_pair("per_page", &ARTIFACT_PAGE_SIZE.to_string())
        .append_pair("page", "1");
    let (first_page, response_etag) = match github
        .get_json_conditionally::<ArtifactsResponse>(first_page_url, etag)
        .await?
    {
        ConditionalJson::NotModified => return Ok(FetchedWorkflowRunArtifacts::NotModified),
        ConditionalJson::NotFound { .. } => {
            return Err(GitHubError::WorkflowRunNotFound {
                run_id: run_id.parse().expect("run ID must be numeric"),
            });
        }
        ConditionalJson::Modified { value, etag } => (value, etag),
    };

    let first_page_size = first_page.artifacts.len();
    let mut artifacts = available_artifacts(first_page.artifacts);
    let mut page_count = 1;
    if first_page_size < ARTIFACT_PAGE_SIZE {
        return Ok(FetchedWorkflowRunArtifacts::Modified {
            artifacts,
            etag: response_etag,
            page_count,
        });
    }

    for page in 2.. {
        let mut url = base_url.clone();
        url.query_pairs_mut()
            .append_pair("per_page", &ARTIFACT_PAGE_SIZE.to_string())
            .append_pair("page", &page.to_string());
        let response: ArtifactsResponse = github.get_json(url).await?;
        let response_size = response.artifacts.len();
        artifacts.extend(available_artifacts(response.artifacts));
        page_count = page;
        if response_size < ARTIFACT_PAGE_SIZE {
            break;
        }
    }

    Ok(FetchedWorkflowRunArtifacts::Modified {
        artifacts,
        etag: response_etag,
        page_count,
    })
}

fn workflow_run_matches(run: &ApiWorkflowRun, branch: &str, conclusion: &str) -> bool {
    run.head_branch.as_deref() == Some(branch) && run.conclusion.as_deref() == Some(conclusion)
}

fn workflow_run_info(workflow_file: &str, run: ApiWorkflowRun) -> WorkflowRunInfo {
    WorkflowRunInfo::new(
        run.id,
        run.name.unwrap_or_else(|| workflow_file.to_owned()),
        run.display_title,
        run.conclusion.unwrap_or_else(|| "unknown".to_owned()),
        run.created_at,
        run.html_url,
    )
}

fn available_artifacts(artifacts: Vec<ApiArtifact>) -> Vec<ActionArtifact> {
    artifacts
        .into_iter()
        .filter(|artifact| !artifact.expired)
        .map(|artifact| {
            ActionArtifact::new(artifact.id, artifact.name, artifact.archive_download_url)
        })
        .collect()
}

fn workflow_file_name(workflow_file: &str) -> Result<&str, GitHubError> {
    Path::new(workflow_file)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| GitHubError::InvalidFileName(workflow_file.to_owned()))
}
