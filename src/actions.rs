use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
};

use serde::{Deserialize, Serialize};

use crate::{
    config::{ActionsConfig, WorkflowConfig},
    github::{
        GitHubClient, GitHubError,
        actions::{
            self as github_actions, FetchedWorkflowRunArtifacts, WorkflowRunCandidatesFetch,
        },
    },
    state::{ActionCursor, ActionsCursors},
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkflowRunInfo {
    id: u64,
    name: String,
    title: String,
    conclusion: String,
    created_at: String,
    html_url: String,
}

impl WorkflowRunInfo {
    pub(crate) fn new(
        id: u64,
        name: String,
        title: String,
        conclusion: String,
        created_at: String,
        html_url: String,
    ) -> Self {
        Self {
            id,
            name,
            title,
            conclusion,
            created_at,
            html_url,
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn conclusion(&self) -> &str {
        &self.conclusion
    }

    pub fn created_at(&self) -> &str {
        &self.created_at
    }

    pub fn html_url(&self) -> &str {
        &self.html_url
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionArtifact {
    id: u64,
    name: String,
    archive_download_url: String,
}

impl ActionArtifact {
    pub(crate) fn new(id: u64, name: String, archive_download_url: String) -> Self {
        Self {
            id,
            name,
            archive_download_url,
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn archive_download_url(&self) -> &str {
        &self.archive_download_url
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionTarget {
    workflow_file: String,
    branch: String,
}

impl ActionTarget {
    fn new(workflow_file: String, branch: String) -> Self {
        Self {
            workflow_file,
            branch,
        }
    }

    pub fn workflow_file(&self) -> &str {
        &self.workflow_file
    }

    pub fn branch(&self) -> &str {
        &self.branch
    }
}

#[derive(Clone, Debug)]
pub struct ActionCheckpoint {
    target: ActionTarget,
    cursor: ActionCursor,
}

impl ActionCheckpoint {
    fn new(target: ActionTarget, cursor: ActionCursor) -> Self {
        Self { target, cursor }
    }

    pub fn target(&self) -> &ActionTarget {
        &self.target
    }

    pub fn into_state_parts(self) -> (String, String, ActionCursor) {
        (self.target.workflow_file, self.target.branch, self.cursor)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkflowRunUpdateKind {
    NewWorkflowRun,
    NewArtifacts,
}

#[derive(Clone, Debug)]
pub struct WorkflowRunUpdate {
    kind: WorkflowRunUpdateKind,
    run: WorkflowRunInfo,
    artifacts: Vec<ActionArtifact>,
    checkpoint: ActionCheckpoint,
}

impl WorkflowRunUpdate {
    pub fn kind(&self) -> WorkflowRunUpdateKind {
        self.kind
    }

    pub fn target(&self) -> &ActionTarget {
        self.checkpoint.target()
    }

    pub fn run(&self) -> &WorkflowRunInfo {
        &self.run
    }

    pub fn artifacts(&self) -> &[ActionArtifact] {
        &self.artifacts
    }

    pub fn into_checkpoint(self) -> ActionCheckpoint {
        self.checkpoint
    }
}

#[derive(Clone, Debug)]
pub enum ActionCheckOutcome {
    BaselineEstablished { checkpoint: ActionCheckpoint },
    Unchanged { checkpoint: ActionCheckpoint },
    UpdateDetected { update: WorkflowRunUpdate },
}

#[derive(Clone, Debug)]
pub struct ActionsCheck {
    outcomes: Vec<ActionCheckOutcome>,
}

impl ActionsCheck {
    pub fn baseline_count(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|outcome| matches!(outcome, ActionCheckOutcome::BaselineEstablished { .. }))
            .count()
    }

    pub fn detected_update_count(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|outcome| matches!(outcome, ActionCheckOutcome::UpdateDetected { .. }))
            .count()
    }

    pub fn into_outcomes(self) -> Vec<ActionCheckOutcome> {
        self.outcomes
    }
}

pub async fn check_for_action_updates(
    github: &GitHubClient,
    repository: &str,
    config: &ActionsConfig,
    cursors: &ActionsCursors,
) -> Result<ActionsCheck, GitHubError> {
    let mut passive_outcomes = Vec::new();
    let mut detected_updates = Vec::new();

    for workflow in &config.workflows {
        for branch in &workflow.branches {
            let current = cursors.cursor_for(&workflow.workflow_file, branch);
            let outcome =
                check_workflow_branch_for_updates(github, repository, workflow, branch, current)
                    .await?;

            match outcome {
                ActionCheckOutcome::UpdateDetected { update } => detected_updates.push(update),
                outcome => passive_outcomes.push(outcome),
            }
        }
    }

    detected_updates.sort_by(|left, right| {
        compare_workflow_runs(left.run(), right.run())
            .then_with(|| {
                left.target()
                    .workflow_file()
                    .cmp(right.target().workflow_file())
            })
            .then_with(|| left.target().branch().cmp(right.target().branch()))
    });
    passive_outcomes.extend(
        detected_updates
            .into_iter()
            .map(|update| ActionCheckOutcome::UpdateDetected { update }),
    );

    Ok(ActionsCheck {
        outcomes: passive_outcomes,
    })
}

async fn check_workflow_branch_for_updates(
    github: &GitHubClient,
    repository: &str,
    workflow: &WorkflowConfig,
    branch: &str,
    current: Option<&ActionCursor>,
) -> Result<ActionCheckOutcome, GitHubError> {
    let target = ActionTarget::new(workflow.workflow_file.clone(), branch.to_owned());
    let conclusion_scope = workflow
        .conclusions
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let artifact_scope = workflow
        .artifact_regex
        .as_ref()
        .map(|pattern| pattern.as_str().to_owned());
    let conclusion_scope_unchanged =
        current.is_some_and(|cursor| cursor.conclusion_scope() == &conclusion_scope);
    let empty_etags = BTreeMap::new();
    let current_workflow_run_etags = current
        .filter(|_| conclusion_scope_unchanged)
        .map(ActionCursor::workflow_run_etags)
        .unwrap_or(&empty_etags);
    let fetched = github_actions::fetch_latest_workflow_run_candidates(
        github,
        repository,
        &workflow.workflow_file,
        branch,
        &workflow.conclusions,
        current_workflow_run_etags,
    )
    .await?;
    let (selected_run, workflow_run_etags) =
        select_latest_workflow_run(current, conclusion_scope_unchanged, fetched);
    let baseline_required = current.is_none()
        || !conclusion_scope_unchanged
        || current.and_then(ActionCursor::artifact_scope) != artifact_scope.as_deref();

    if baseline_required {
        let artifact_selection = fetch_matching_workflow_run_artifacts(
            github,
            repository,
            selected_run.as_ref(),
            workflow,
            None,
        )
        .await?;
        let checkpoint = build_action_checkpoint(
            target,
            conclusion_scope,
            selected_run,
            artifact_scope,
            workflow_run_etags,
            artifact_selection,
        );
        return Ok(ActionCheckOutcome::BaselineEstablished { checkpoint });
    }

    let current = current.expect("a matching Actions scope must have a current cursor");
    let Some(selected_run) = selected_run else {
        return Ok(ActionCheckOutcome::Unchanged {
            checkpoint: preserve_action_checkpoint(target, current, workflow_run_etags),
        });
    };
    if current
        .run_id()
        .is_some_and(|current_run_id| selected_run.id() < current_run_id)
    {
        return Ok(ActionCheckOutcome::Unchanged {
            checkpoint: preserve_action_checkpoint(target, current, workflow_run_etags),
        });
    }

    let is_new_workflow_run = current.run_id() != Some(selected_run.id());
    let conditional_artifact_etag = (!is_new_workflow_run
        && current.artifact_page_count() == Some(1))
    .then(|| current.artifact_etag())
    .flatten();
    let artifact_selection = fetch_matching_workflow_run_artifacts(
        github,
        repository,
        Some(&selected_run),
        workflow,
        conditional_artifact_etag,
    )
    .await?;

    match artifact_selection {
        MatchingArtifactsFetch::NotModified => Ok(ActionCheckOutcome::Unchanged {
            checkpoint: ActionCheckpoint::new(
                target,
                ActionCursor::new(
                    conclusion_scope,
                    Some(selected_run),
                    current.artifact_ids().clone(),
                    artifact_scope,
                    workflow_run_etags,
                    current.artifact_etag().map(str::to_owned),
                    current.artifact_page_count(),
                ),
            ),
        }),
        MatchingArtifactsFetch::Fetched {
            artifacts,
            artifact_ids,
            etag,
            page_count,
        } => {
            let new_artifacts = if is_new_workflow_run {
                artifacts.clone()
            } else {
                artifacts
                    .iter()
                    .filter(|artifact| !current.contains_artifact(artifact.id()))
                    .cloned()
                    .collect()
            };
            let checkpoint = ActionCheckpoint::new(
                target,
                ActionCursor::new(
                    conclusion_scope,
                    Some(selected_run.clone()),
                    artifact_ids,
                    artifact_scope,
                    workflow_run_etags,
                    etag,
                    Some(page_count),
                ),
            );

            if is_new_workflow_run {
                return Ok(ActionCheckOutcome::UpdateDetected {
                    update: WorkflowRunUpdate {
                        kind: WorkflowRunUpdateKind::NewWorkflowRun,
                        run: selected_run,
                        artifacts: new_artifacts,
                        checkpoint,
                    },
                });
            }
            if !new_artifacts.is_empty() {
                return Ok(ActionCheckOutcome::UpdateDetected {
                    update: WorkflowRunUpdate {
                        kind: WorkflowRunUpdateKind::NewArtifacts,
                        run: selected_run,
                        artifacts: new_artifacts,
                        checkpoint,
                    },
                });
            }

            Ok(ActionCheckOutcome::Unchanged { checkpoint })
        }
    }
}

fn select_latest_workflow_run(
    current: Option<&ActionCursor>,
    conclusion_scope_unchanged: bool,
    fetched: WorkflowRunCandidatesFetch,
) -> (Option<WorkflowRunInfo>, BTreeMap<String, String>) {
    let mut candidates = fetched.candidates;
    if conclusion_scope_unchanged
        && let Some(current_run) = current.and_then(ActionCursor::run)
        && fetched
            .not_modified_conclusions
            .contains(current_run.conclusion())
    {
        candidates.push(current_run.clone());
    }

    let latest = candidates.into_iter().max_by(compare_workflow_runs);
    (latest, fetched.etags)
}

fn compare_workflow_runs(left: &WorkflowRunInfo, right: &WorkflowRunInfo) -> Ordering {
    left.created_at()
        .cmp(right.created_at())
        .then_with(|| left.id().cmp(&right.id()))
}

async fn fetch_matching_workflow_run_artifacts(
    github: &GitHubClient,
    repository: &str,
    run: Option<&WorkflowRunInfo>,
    workflow: &WorkflowConfig,
    etag: Option<&str>,
) -> Result<MatchingArtifactsFetch, GitHubError> {
    let (Some(run), Some(pattern)) = (run, workflow.artifact_regex.as_ref()) else {
        return Ok(MatchingArtifactsFetch::Fetched {
            artifacts: Vec::new(),
            artifact_ids: BTreeSet::new(),
            etag: None,
            page_count: 0,
        });
    };

    match github_actions::fetch_workflow_run_artifacts(github, repository, run.id(), etag).await? {
        FetchedWorkflowRunArtifacts::NotModified => Ok(MatchingArtifactsFetch::NotModified),
        FetchedWorkflowRunArtifacts::Modified {
            artifacts,
            etag,
            page_count,
        } => {
            let artifacts = artifacts
                .into_iter()
                .filter(|artifact| pattern.is_match(artifact.name()))
                .collect::<Vec<_>>();
            let artifact_ids = artifacts
                .iter()
                .map(ActionArtifact::id)
                .collect::<BTreeSet<_>>();
            Ok(MatchingArtifactsFetch::Fetched {
                artifacts,
                artifact_ids,
                etag,
                page_count,
            })
        }
    }
}

fn build_action_checkpoint(
    target: ActionTarget,
    conclusion_scope: BTreeSet<String>,
    run: Option<WorkflowRunInfo>,
    artifact_scope: Option<String>,
    workflow_run_etags: BTreeMap<String, String>,
    artifact_selection: MatchingArtifactsFetch,
) -> ActionCheckpoint {
    let (artifact_ids, artifact_etag, artifact_page_count) = match artifact_selection {
        MatchingArtifactsFetch::NotModified => {
            unreachable!("a baseline artifact request cannot be conditional")
        }
        MatchingArtifactsFetch::Fetched {
            artifact_ids,
            etag,
            page_count,
            ..
        } => (artifact_ids, etag, Some(page_count)),
    };

    ActionCheckpoint::new(
        target,
        ActionCursor::new(
            conclusion_scope,
            run,
            artifact_ids,
            artifact_scope,
            workflow_run_etags,
            artifact_etag,
            artifact_page_count,
        ),
    )
}

fn preserve_action_checkpoint(
    target: ActionTarget,
    current: &ActionCursor,
    workflow_run_etags: BTreeMap<String, String>,
) -> ActionCheckpoint {
    ActionCheckpoint::new(
        target,
        ActionCursor::new(
            current.conclusion_scope().clone(),
            current.run().cloned(),
            current.artifact_ids().clone(),
            current.artifact_scope().map(str::to_owned),
            workflow_run_etags,
            current.artifact_etag().map(str::to_owned),
            current.artifact_page_count(),
        ),
    )
}

enum MatchingArtifactsFetch {
    NotModified,
    Fetched {
        artifacts: Vec<ActionArtifact>,
        artifact_ids: BTreeSet<u64>,
        etag: Option<String>,
        page_count: usize,
    },
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::mpsc,
        thread,
        time::Duration,
    };

    use regex::Regex;
    use url::Url;

    use crate::{config::WorkflowConfig, github::GitHubClient, state::ActionCursor};

    use super::{
        ActionCheckOutcome, WorkflowRunInfo, WorkflowRunUpdateKind,
        check_workflow_branch_for_updates,
    };

    #[tokio::test]
    async fn detects_new_artifacts_when_the_workflow_run_response_is_not_modified() {
        let responses = vec![
            json_response(
                "200 OK",
                Some("\"runs-100\""),
                workflow_runs_body(100, "success"),
            ),
            json_response(
                "200 OK",
                Some("\"artifacts-100-a\""),
                artifacts_body(&[(10, "linux-old")]),
            ),
            json_response("304 Not Modified", None, String::new()),
            json_response(
                "200 OK",
                Some("\"artifacts-100-b\""),
                artifacts_body(&[(10, "linux-old"), (11, "linux-new")]),
            ),
        ];
        let (api_root, requests, server) = start_test_server(responses);
        let github = GitHubClient::with_api_root(Some("test-token".to_owned()), api_root)
            .expect("GitHub client must be created");
        let workflow = workflow_config("^linux-");

        let baseline =
            check_workflow_branch_for_updates(&github, "owner/repo", &workflow, "main", None)
                .await
                .expect("initial Actions check must succeed");
        let ActionCheckOutcome::BaselineEstablished { checkpoint } = baseline else {
            panic!("the initial Actions check must establish a baseline");
        };
        let (_, _, cursor) = checkpoint.into_state_parts();
        assert_eq!(cursor.run_id(), Some(100));
        assert_eq!(
            cursor.workflow_run_etag_for("success"),
            Some("\"runs-100\"")
        );
        assert_eq!(cursor.artifact_etag(), Some("\"artifacts-100-a\""));
        assert!(cursor.contains_artifact(10));

        let updated = check_workflow_branch_for_updates(
            &github,
            "owner/repo",
            &workflow,
            "main",
            Some(&cursor),
        )
        .await
        .expect("subsequent Actions check must succeed");
        let ActionCheckOutcome::UpdateDetected { update } = updated else {
            panic!("a new Artifact on the current Run must be detected");
        };
        assert_eq!(update.kind(), WorkflowRunUpdateKind::NewArtifacts);
        assert_eq!(update.run().id(), 100);
        assert_eq!(update.artifacts().len(), 1);
        assert_eq!(update.artifacts()[0].id(), 11);
        let (_, _, updated_cursor) = update.into_checkpoint().into_state_parts();
        assert_eq!(updated_cursor.artifact_etag(), Some("\"artifacts-100-b\""));
        assert!(updated_cursor.contains_artifact(10));
        assert!(updated_cursor.contains_artifact(11));

        let captured = receive_requests(&requests, 4);
        assert!(!captured[0].contains("if-none-match"));
        assert!(!captured[1].contains("if-none-match"));
        assert!(captured[2].contains("if-none-match: \"runs-100\""));
        assert!(captured[3].contains("if-none-match: \"artifacts-100-a\""));
        server.join().expect("test server must stop");
    }

    #[tokio::test]
    async fn changing_the_artifact_filter_establishes_a_new_baseline() {
        let responses = vec![
            json_response("304 Not Modified", None, String::new()),
            json_response(
                "200 OK",
                Some("\"artifacts-100-new-scope\""),
                artifacts_body(&[(10, "linux-old"), (11, "windows-new")]),
            ),
        ];
        let (api_root, requests, server) = start_test_server(responses);
        let github = GitHubClient::with_api_root(Some("test-token".to_owned()), api_root)
            .expect("GitHub client must be created");
        let workflow = workflow_config("^windows-");
        let current = ActionCursor::new(
            ["success".to_owned()].into_iter().collect(),
            Some(workflow_run(100, "success")),
            [10].into_iter().collect(),
            Some("^linux-".to_owned()),
            [("success".to_owned(), "\"runs-100\"".to_owned())]
                .into_iter()
                .collect(),
            Some("\"artifacts-100-old-scope\"".to_owned()),
            Some(1),
        );

        let outcome = check_workflow_branch_for_updates(
            &github,
            "owner/repo",
            &workflow,
            "main",
            Some(&current),
        )
        .await
        .expect("Actions check after a filter change must succeed");
        let ActionCheckOutcome::BaselineEstablished { checkpoint } = outcome else {
            panic!("a changed Artifact filter must establish a baseline");
        };
        let (_, _, cursor) = checkpoint.into_state_parts();
        assert_eq!(cursor.artifact_scope(), Some("^windows-"));
        assert!(!cursor.contains_artifact(10));
        assert!(cursor.contains_artifact(11));

        let captured = receive_requests(&requests, 2);
        assert!(captured[0].contains("if-none-match: \"runs-100\""));
        assert!(!captured[1].contains("if-none-match"));
        server.join().expect("test server must stop");
    }

    fn workflow_config(artifact_regex: &str) -> WorkflowConfig {
        WorkflowConfig {
            workflow_file: ".github/workflows/build.yml".to_owned(),
            branches: vec!["main".to_owned()],
            conclusions: vec!["success".to_owned()],
            artifact_regex: Some(Regex::new(artifact_regex).expect("test regex must compile")),
        }
    }

    fn workflow_run(id: u64, conclusion: &str) -> WorkflowRunInfo {
        WorkflowRunInfo::new(
            id,
            "Build".to_owned(),
            "Build commit".to_owned(),
            conclusion.to_owned(),
            "2026-09-08T01:00:00Z".to_owned(),
            format!("https://github.com/owner/repo/actions/runs/{id}"),
        )
    }

    fn workflow_runs_body(id: u64, conclusion: &str) -> String {
        format!(
            concat!(
                "{{\"workflow_runs\":[{{",
                "\"id\":{id},\"name\":\"Build\",",
                "\"display_title\":\"Build commit\",",
                "\"head_branch\":\"main\",\"conclusion\":\"{conclusion}\",",
                "\"created_at\":\"2026-09-08T01:00:00Z\",",
                "\"html_url\":\"https://github.com/owner/repo/actions/runs/{id}\"",
                "}}]}}"
            ),
            id = id,
            conclusion = conclusion,
        )
    }

    fn artifacts_body(artifacts: &[(u64, &str)]) -> String {
        let artifacts = artifacts
            .iter()
            .map(|(id, name)| {
                format!(
                    concat!(
                        "{{\"id\":{id},\"name\":\"{name}\",",
                        "\"archive_download_url\":\"https://api.github.com/artifacts/{id}\",",
                        "\"expired\":false}}"
                    ),
                    id = id,
                    name = name,
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("{{\"artifacts\":[{artifacts}]}}")
    }

    fn json_response(status: &str, etag: Option<&str>, body: String) -> String {
        let etag = etag.map_or_else(String::new, |etag| format!("ETag: {etag}\r\n"));
        format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{etag}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    fn start_test_server(
        responses: Vec<String>,
    ) -> (Url, mpsc::Receiver<String>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("test server must bind");
        let address = listener
            .local_addr()
            .expect("test server must have an address");
        let (request_sender, request_receiver) = mpsc::channel();
        let server = thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().expect("test request must connect");
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .expect("read timeout must be set");
                let mut request = Vec::new();
                let mut buffer = [0_u8; 1024];
                while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                    let read = stream.read(&mut buffer).expect("request must be readable");
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..read]);
                }
                request_sender
                    .send(
                        String::from_utf8(request)
                            .expect("test request must be UTF-8")
                            .to_ascii_lowercase(),
                    )
                    .expect("test request must be reported");
                stream
                    .write_all(response.as_bytes())
                    .expect("test response must be written");
            }
        });
        let api_root = Url::parse(&format!("http://{address}/")).expect("test URL must parse");
        (api_root, request_receiver, server)
    }

    fn receive_requests(receiver: &mpsc::Receiver<String>, count: usize) -> Vec<String> {
        (0..count)
            .map(|_| {
                receiver
                    .recv_timeout(Duration::from_secs(5))
                    .expect("test request must be captured")
            })
            .collect()
    }
}
