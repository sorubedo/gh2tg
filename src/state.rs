use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    actions::WorkflowRunInfo,
    github::{
        GitHubClient, GitHubError, RepositoryId, parse_repository, resolve_github_repository_id,
    },
    release::ReleaseSelectorScope,
};

const SUPPORTED_SCHEMA_VERSION: u32 = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StateResponseCode {
    StateLoadedExisting,
    StateInitializedDefault,
    StateReadCompleted,
    RepositoryStateNotTracked,
    StateChanged,
    StateUnchanged,
    StateSaved,
    StateSaveSkippedNoChanges,
    StateFileReadFailed,
    StateJsonParseFailed,
    StateSerializationFailed,
    StateSchemaUnsupported,
    StateGroupMismatch,
    StateContentInvalid,
    StateMutationRejected,
    StateFileWriteFailed,
}

impl StateResponseCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StateLoadedExisting => "STATE_LOADED_EXISTING",
            Self::StateInitializedDefault => "STATE_INITIALIZED_DEFAULT",
            Self::StateReadCompleted => "STATE_READ_COMPLETED",
            Self::RepositoryStateNotTracked => "REPOSITORY_STATE_NOT_TRACKED",
            Self::StateChanged => "STATE_CHANGED",
            Self::StateUnchanged => "STATE_UNCHANGED",
            Self::StateSaved => "STATE_SAVED",
            Self::StateSaveSkippedNoChanges => "STATE_SAVE_SKIPPED_NO_CHANGES",
            Self::StateFileReadFailed => "STATE_FILE_READ_FAILED",
            Self::StateJsonParseFailed => "STATE_JSON_PARSE_FAILED",
            Self::StateSerializationFailed => "STATE_SERIALIZATION_FAILED",
            Self::StateSchemaUnsupported => "STATE_SCHEMA_UNSUPPORTED",
            Self::StateGroupMismatch => "STATE_GROUP_MISMATCH",
            Self::StateContentInvalid => "STATE_CONTENT_INVALID",
            Self::StateMutationRejected => "STATE_MUTATION_REJECTED",
            Self::StateFileWriteFailed => "STATE_FILE_WRITE_FAILED",
        }
    }
}

#[derive(Debug)]
pub struct StateSuccess<T> {
    pub code: StateResponseCode,
    pub value: T,
}

#[derive(Debug)]
pub struct StateFailure {
    pub code: StateResponseCode,
    pub error: StateError,
}

impl std::fmt::Display for StateFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "[{}] {}", self.code.as_str(), self.error)
    }
}

impl std::error::Error for StateFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

impl From<StateError> for StateFailure {
    fn from(error: StateError) -> Self {
        Self {
            code: error.code(),
            error,
        }
    }
}

pub type StateResult<T> = Result<StateSuccess<T>, StateFailure>;

#[derive(Debug)]
pub struct ProgramStateSession {
    path: PathBuf,
    original: StateDocument,
    state: StateDocument,
}

#[derive(Clone, Debug)]
pub struct RepositoryStateSnapshot {
    repository_id: u64,
    message_thread_id: i32,
    topic_name: String,
    cursors: Cursors,
}

impl RepositoryStateSnapshot {
    pub fn repository_id(&self) -> u64 {
        self.repository_id
    }

    pub fn message_thread_id(&self) -> i32 {
        self.message_thread_id
    }

    pub fn topic_name(&self) -> &str {
        &self.topic_name
    }

    pub fn cursors(&self) -> &Cursors {
        &self.cursors
    }
}

#[derive(Clone, Debug)]
pub enum StateChange {
    EnsureRepositoryTopicState {
        repository_id: u64,
        message_thread_id: i32,
        topic_name: String,
    },
    ReplaceCommitBranchCursor {
        repository_id: u64,
        branch: String,
        sha: String,
    },
    ReplaceReleaseCursor {
        repository_id: u64,
        cursor: ReleaseCursor,
    },
    ReplaceActionBranchCursor {
        repository_id: u64,
        workflow_file: String,
        branch: String,
        cursor: Box<ActionCursor>,
    },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StateChangeReport {
    pub changed: bool,
    pub repository_created: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StateSaveReport {
    pub changed: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Cursors {
    commits: CommitCursors,
    releases: ReleaseCursor,
    actions: ActionsCursors,
}

impl Cursors {
    pub fn commits(&self) -> &CommitCursors {
        &self.commits
    }

    pub fn releases(&self) -> &ReleaseCursor {
        &self.releases
    }

    pub fn actions(&self) -> &ActionsCursors {
        &self.actions
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct CommitCursors {
    branches: BTreeMap<String, String>,
}

impl CommitCursors {
    pub fn cursor_for(&self, branch: &str) -> Option<&str> {
        self.branches.get(branch).map(String::as_str)
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReleaseCursor {
    selector_scope: Option<ReleaseSelectorScope>,
    release_id: Option<u64>,
    updated_at: Option<String>,
    #[serde(default)]
    etag: Option<String>,
}

impl ReleaseCursor {
    pub fn new(
        selector_scope: ReleaseSelectorScope,
        release_id: Option<u64>,
        updated_at: Option<String>,
        etag: Option<String>,
    ) -> Self {
        Self {
            selector_scope: Some(selector_scope),
            release_id,
            updated_at,
            etag,
        }
    }

    pub fn selector_scope(&self) -> Option<&ReleaseSelectorScope> {
        self.selector_scope.as_ref()
    }

    pub fn release_id(&self) -> Option<u64> {
        self.release_id
    }

    pub fn updated_at(&self) -> Option<&str> {
        self.updated_at.as_deref()
    }

    pub fn etag(&self) -> Option<&str> {
        self.etag.as_deref()
    }

    pub fn etag_for(&self, scope: &ReleaseSelectorScope) -> Option<&str> {
        (self.selector_scope.as_ref() == Some(scope))
            .then(|| self.etag())
            .flatten()
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ActionsCursors {
    workflows: BTreeMap<String, WorkflowCursors>,
}

impl ActionsCursors {
    pub fn cursor_for(&self, workflow_file: &str, branch: &str) -> Option<&ActionCursor> {
        self.workflows
            .get(workflow_file)
            .and_then(|workflow| workflow.branches.get(branch))
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
struct WorkflowCursors {
    branches: BTreeMap<String, ActionCursor>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ActionCursor {
    #[serde(default)]
    conclusion_scope: BTreeSet<String>,
    run: Option<WorkflowRunInfo>,
    artifact_ids: BTreeSet<u64>,
    #[serde(default)]
    artifact_scope: Option<String>,
    #[serde(default)]
    workflow_run_etags: BTreeMap<String, String>,
    #[serde(default)]
    artifact_etag: Option<String>,
    #[serde(default)]
    artifact_page_count: Option<usize>,
}

impl ActionCursor {
    pub fn new(
        conclusion_scope: BTreeSet<String>,
        run: Option<WorkflowRunInfo>,
        artifact_ids: BTreeSet<u64>,
        artifact_scope: Option<String>,
        workflow_run_etags: BTreeMap<String, String>,
        artifact_etag: Option<String>,
        artifact_page_count: Option<usize>,
    ) -> Self {
        Self {
            conclusion_scope,
            run,
            artifact_ids,
            artifact_scope,
            workflow_run_etags,
            artifact_etag,
            artifact_page_count,
        }
    }

    pub fn conclusion_scope(&self) -> &BTreeSet<String> {
        &self.conclusion_scope
    }

    pub fn run(&self) -> Option<&WorkflowRunInfo> {
        self.run.as_ref()
    }

    pub fn run_id(&self) -> Option<u64> {
        self.run.as_ref().map(WorkflowRunInfo::id)
    }

    pub fn artifact_ids(&self) -> &BTreeSet<u64> {
        &self.artifact_ids
    }

    pub fn contains_artifact(&self, artifact_id: u64) -> bool {
        self.artifact_ids.contains(&artifact_id)
    }

    pub fn artifact_scope(&self) -> Option<&str> {
        self.artifact_scope.as_deref()
    }

    pub fn workflow_run_etags(&self) -> &BTreeMap<String, String> {
        &self.workflow_run_etags
    }

    pub fn workflow_run_etag_for(&self, conclusion: &str) -> Option<&str> {
        self.workflow_run_etags.get(conclusion).map(String::as_str)
    }

    pub fn artifact_etag(&self) -> Option<&str> {
        self.artifact_etag.as_deref()
    }

    pub fn artifact_page_count(&self) -> Option<usize> {
        self.artifact_page_count
    }
}

#[derive(Debug, Error)]
pub enum StateError {
    #[error("failed to read state file {path}: {source}")]
    ReadIo {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to parse state file {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to serialize state file: {0}")]
    Serialize(#[source] serde_json::Error),
    #[error("failed to write state file {path}: {source}")]
    WriteIo {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("unsupported state schema_version: {0}")]
    UnsupportedSchemaVersion(u32),
    #[error(
        "state file belongs to group {state_group_id}, but configured BETTER_CI_GROUP_ID is {configured_group_id}"
    )]
    GroupMismatch {
        state_group_id: i64,
        configured_group_id: i64,
    },
    #[error("state record {repository_id} has an invalid topic ID: {topic_id}")]
    InvalidTopicId { repository_id: u64, topic_id: i32 },
    #[error("state record {repository_id} has an empty topic name")]
    InvalidTopicName { repository_id: u64 },
    #[error("repository {repository_id} is not present in state")]
    RepositoryNotTracked { repository_id: u64 },
    #[error("repository {repository_id} has an empty branch name")]
    EmptyBranch { repository_id: u64 },
    #[error("repository {repository_id} has an empty commit SHA")]
    EmptyCommitSha { repository_id: u64 },
    #[error("repository {repository_id} has an empty workflow file name")]
    EmptyWorkflowFile { repository_id: u64 },
}

impl StateError {
    pub const fn code(&self) -> StateResponseCode {
        match self {
            Self::ReadIo { .. } => StateResponseCode::StateFileReadFailed,
            Self::Parse { .. } => StateResponseCode::StateJsonParseFailed,
            Self::Serialize(_) => StateResponseCode::StateSerializationFailed,
            Self::WriteIo { .. } => StateResponseCode::StateFileWriteFailed,
            Self::UnsupportedSchemaVersion(_) => StateResponseCode::StateSchemaUnsupported,
            Self::GroupMismatch { .. } => StateResponseCode::StateGroupMismatch,
            Self::InvalidTopicId { .. } | Self::InvalidTopicName { .. } => {
                StateResponseCode::StateContentInvalid
            }
            Self::RepositoryNotTracked { .. }
            | Self::EmptyBranch { .. }
            | Self::EmptyCommitSha { .. }
            | Self::EmptyWorkflowFile { .. } => StateResponseCode::StateMutationRejected,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct StateDocument {
    schema_version: u32,
    group_id: i64,
    #[serde(default)]
    repository_ids: BTreeMap<String, u64>,
    #[serde(default)]
    repositories: BTreeMap<u64, RepositoryDocument>,
}

#[derive(Deserialize)]
struct StateHeader {
    schema_version: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct RepositoryDocument {
    message_thread_id: i32,
    topic_name: String,
    #[serde(default)]
    cursors: Cursors,
}

impl StateDocument {
    fn new(group_id: i64) -> Self {
        Self {
            schema_version: SUPPORTED_SCHEMA_VERSION,
            group_id,
            repository_ids: BTreeMap::new(),
            repositories: BTreeMap::new(),
        }
    }
}

impl ProgramStateSession {
    pub fn load_state_file_or_initialize_default(
        path: &Path,
        expected_group_id: i64,
    ) -> StateResult<Self> {
        match load_state_document(path, expected_group_id)? {
            LoadedState::Existing(state) => Ok(StateSuccess {
                code: StateResponseCode::StateLoadedExisting,
                value: Self {
                    path: path.to_path_buf(),
                    original: state.clone(),
                    state,
                },
            }),
            LoadedState::Default(state) => Ok(StateSuccess {
                code: StateResponseCode::StateInitializedDefault,
                value: Self {
                    path: path.to_path_buf(),
                    original: state.clone(),
                    state,
                },
            }),
        }
    }

    pub fn read_all_tracked_repository_states(&self) -> StateResult<Vec<RepositoryStateSnapshot>> {
        let snapshots = self
            .state
            .repositories
            .iter()
            .map(|(&repository_id, repository)| self.snapshot(repository_id, repository))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(StateSuccess {
            code: StateResponseCode::StateReadCompleted,
            value: snapshots,
        })
    }

    pub fn read_repository_state(
        &self,
        repository_id: u64,
    ) -> StateResult<Option<RepositoryStateSnapshot>> {
        let snapshot = self
            .state
            .repositories
            .get(&repository_id)
            .map(|repository| self.snapshot(repository_id, repository))
            .transpose()?;

        Ok(StateSuccess {
            code: if snapshot.is_some() {
                StateResponseCode::StateReadCompleted
            } else {
                StateResponseCode::RepositoryStateNotTracked
            },
            value: snapshot,
        })
    }

    pub async fn resolve_repository_id(
        &mut self,
        github: &GitHubClient,
        repository: &str,
    ) -> Result<RepositoryId, GitHubError> {
        let repository_name = parse_repository(repository)?;
        let repository_key = format!("{}/{}", repository_name.owner, repository_name.name);

        if let Some(&database_id) = self.state.repository_ids.get(&repository_key) {
            return RepositoryId::from_database_id(database_id, repository_name);
        }

        let resolved = resolve_github_repository_id(github, repository_name).await?;
        self.state
            .repository_ids
            .insert(repository_key, resolved.database_id());
        Ok(resolved)
    }

    pub fn apply_state_change(&mut self, change: StateChange) -> StateResult<StateChangeReport> {
        let (report, code) = match change {
            StateChange::EnsureRepositoryTopicState {
                repository_id,
                message_thread_id,
                topic_name,
            } => {
                self.ensure_repository_topic_state(repository_id, message_thread_id, topic_name)?
            }
            StateChange::ReplaceCommitBranchCursor {
                repository_id,
                branch,
                sha,
            } => self.replace_commit_branch_cursor(repository_id, branch, sha)?,
            StateChange::ReplaceReleaseCursor {
                repository_id,
                cursor,
            } => self.replace_release_cursor(repository_id, cursor)?,
            StateChange::ReplaceActionBranchCursor {
                repository_id,
                workflow_file,
                branch,
                cursor,
            } => {
                self.replace_action_branch_cursor(repository_id, workflow_file, branch, *cursor)?
            }
        };

        Ok(StateSuccess {
            code,
            value: report,
        })
    }

    pub fn save_state_file_if_changed(self) -> StateResult<StateSaveReport> {
        if self.state == self.original {
            return Ok(StateSuccess {
                code: StateResponseCode::StateSaveSkippedNoChanges,
                value: StateSaveReport { changed: false },
            });
        }

        let mut bytes = serde_json::to_vec_pretty(&self.state).map_err(StateError::Serialize)?;
        bytes.push(b'\n');
        fs::write(&self.path, &bytes).map_err(|source| StateError::WriteIo {
            path: self.path,
            source,
        })?;

        Ok(StateSuccess {
            code: StateResponseCode::StateSaved,
            value: StateSaveReport { changed: true },
        })
    }

    fn snapshot(
        &self,
        repository_id: u64,
        repository: &RepositoryDocument,
    ) -> Result<RepositoryStateSnapshot, StateError> {
        validate_repository_state(repository_id, repository)?;
        Ok(RepositoryStateSnapshot {
            repository_id,
            message_thread_id: repository.message_thread_id,
            topic_name: repository.topic_name.clone(),
            cursors: repository.cursors.clone(),
        })
    }

    fn ensure_repository_topic_state(
        &mut self,
        repository_id: u64,
        message_thread_id: i32,
        topic_name: String,
    ) -> Result<(StateChangeReport, StateResponseCode), StateError> {
        validate_topic(repository_id, message_thread_id, &topic_name)?;

        let repository_created = !self.state.repositories.contains_key(&repository_id);
        let repository = self
            .state
            .repositories
            .entry(repository_id)
            .or_insert_with(|| RepositoryDocument {
                message_thread_id,
                topic_name: topic_name.clone(),
                cursors: Cursors::default(),
            });
        let changed = repository_created
            || repository.message_thread_id != message_thread_id
            || repository.topic_name != topic_name;

        repository.message_thread_id = message_thread_id;
        repository.topic_name = topic_name;

        Ok((
            StateChangeReport {
                changed,
                repository_created,
            },
            if changed {
                StateResponseCode::StateChanged
            } else {
                StateResponseCode::StateUnchanged
            },
        ))
    }

    fn replace_commit_branch_cursor(
        &mut self,
        repository_id: u64,
        branch: String,
        sha: String,
    ) -> Result<(StateChangeReport, StateResponseCode), StateError> {
        if branch.trim().is_empty() {
            return Err(StateError::EmptyBranch { repository_id });
        }
        if sha.trim().is_empty() {
            return Err(StateError::EmptyCommitSha { repository_id });
        }

        let repository = self.repository_mut(repository_id)?;
        let changed = repository.cursors.commits.branches.get(&branch) != Some(&sha);
        repository.cursors.commits.branches.insert(branch, sha);
        Ok((
            StateChangeReport {
                changed,
                repository_created: false,
            },
            if changed {
                StateResponseCode::StateChanged
            } else {
                StateResponseCode::StateUnchanged
            },
        ))
    }

    fn replace_release_cursor(
        &mut self,
        repository_id: u64,
        cursor: ReleaseCursor,
    ) -> Result<(StateChangeReport, StateResponseCode), StateError> {
        let repository = self.repository_mut(repository_id)?;
        let changed = repository.cursors.releases != cursor;
        repository.cursors.releases = cursor;
        Ok((
            StateChangeReport {
                changed,
                repository_created: false,
            },
            if changed {
                StateResponseCode::StateChanged
            } else {
                StateResponseCode::StateUnchanged
            },
        ))
    }

    fn replace_action_branch_cursor(
        &mut self,
        repository_id: u64,
        workflow_file: String,
        branch: String,
        cursor: ActionCursor,
    ) -> Result<(StateChangeReport, StateResponseCode), StateError> {
        if workflow_file.trim().is_empty() {
            return Err(StateError::EmptyWorkflowFile { repository_id });
        }
        if branch.trim().is_empty() {
            return Err(StateError::EmptyBranch { repository_id });
        }

        let repository = self.repository_mut(repository_id)?;
        let stored_cursor = repository
            .cursors
            .actions
            .workflows
            .entry(workflow_file)
            .or_default()
            .branches
            .insert(branch, cursor.clone());
        let changed = stored_cursor.as_ref() != Some(&cursor);
        Ok((
            StateChangeReport {
                changed,
                repository_created: false,
            },
            if changed {
                StateResponseCode::StateChanged
            } else {
                StateResponseCode::StateUnchanged
            },
        ))
    }

    fn repository_mut(
        &mut self,
        repository_id: u64,
    ) -> Result<&mut RepositoryDocument, StateError> {
        self.state
            .repositories
            .get_mut(&repository_id)
            .ok_or(StateError::RepositoryNotTracked { repository_id })
    }
}

enum LoadedState {
    Existing(StateDocument),
    Default(StateDocument),
}

fn load_state_document(path: &Path, expected_group_id: i64) -> Result<LoadedState, StateError> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            return Ok(LoadedState::Default(StateDocument::new(expected_group_id)));
        }
        Err(source) => {
            return Err(StateError::ReadIo {
                path: path.to_path_buf(),
                source,
            });
        }
    };

    let header: StateHeader =
        serde_json::from_str(&contents).map_err(|source| StateError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
    if header.schema_version != SUPPORTED_SCHEMA_VERSION {
        return Err(StateError::UnsupportedSchemaVersion(header.schema_version));
    }

    let state = serde_json::from_str(&contents).map_err(|source| StateError::Parse {
        path: path.to_path_buf(),
        source,
    })?;
    validate_document(&state, expected_group_id)?;
    Ok(LoadedState::Existing(state))
}

fn validate_document(state: &StateDocument, expected_group_id: i64) -> Result<(), StateError> {
    if state.schema_version != SUPPORTED_SCHEMA_VERSION {
        return Err(StateError::UnsupportedSchemaVersion(state.schema_version));
    }
    if state.group_id != expected_group_id {
        return Err(StateError::GroupMismatch {
            state_group_id: state.group_id,
            configured_group_id: expected_group_id,
        });
    }
    for (&repository_id, repository) in &state.repositories {
        validate_repository_state(repository_id, repository)?;
    }
    Ok(())
}

fn validate_repository_state(
    repository_id: u64,
    repository: &RepositoryDocument,
) -> Result<(), StateError> {
    validate_topic(
        repository_id,
        repository.message_thread_id,
        &repository.topic_name,
    )
}

fn validate_topic(
    repository_id: u64,
    message_thread_id: i32,
    topic_name: &str,
) -> Result<(), StateError> {
    if message_thread_id <= 0 || message_thread_id == 1 {
        return Err(StateError::InvalidTopicId {
            repository_id,
            topic_id: message_thread_id,
        });
    }
    if topic_name.trim().is_empty() {
        return Err(StateError::InvalidTopicName { repository_id });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, fs};

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn loads_reads_modifies_and_saves_state_with_codes() {
        let directory = tempdir().expect("temporary directory must be created");
        let path = directory.path().join("state.json");

        let loaded = ProgramStateSession::load_state_file_or_initialize_default(&path, 42)
            .expect("missing state file must initialize a default state");
        assert_eq!(loaded.code, StateResponseCode::StateInitializedDefault);
        let mut session = loaded.value;

        let created = session
            .apply_state_change(StateChange::EnsureRepositoryTopicState {
                repository_id: 7,
                message_thread_id: 8,
                topic_name: "repo".to_owned(),
            })
            .expect("topic state must be accepted");
        assert_eq!(created.code, StateResponseCode::StateChanged);
        assert!(created.value.changed);
        assert!(created.value.repository_created);

        let unchanged = session
            .apply_state_change(StateChange::EnsureRepositoryTopicState {
                repository_id: 7,
                message_thread_id: 8,
                topic_name: "repo".to_owned(),
            })
            .expect("reapplying the same topic state must be accepted");
        assert_eq!(unchanged.code, StateResponseCode::StateUnchanged);
        assert!(!unchanged.value.changed);

        let commit = session
            .apply_state_change(StateChange::ReplaceCommitBranchCursor {
                repository_id: 7,
                branch: "main".to_owned(),
                sha: "abc123".to_owned(),
            })
            .expect("commit cursor must be accepted");
        assert_eq!(commit.code, StateResponseCode::StateChanged);

        let release = session
            .apply_state_change(StateChange::ReplaceReleaseCursor {
                repository_id: 7,
                cursor: ReleaseCursor::new(
                    ReleaseSelectorScope::LatestStable,
                    Some(11),
                    Some("2026-09-01T00:00:00Z".to_owned()),
                    Some("\"release-11\"".to_owned()),
                ),
            })
            .expect("release cursor must be accepted");
        assert_eq!(release.code, StateResponseCode::StateChanged);

        let action = session
            .apply_state_change(StateChange::ReplaceActionBranchCursor {
                repository_id: 7,
                workflow_file: ".github/workflows/build.yml".to_owned(),
                branch: "main".to_owned(),
                cursor: Box::new(ActionCursor::new(
                    BTreeSet::from(["success".to_owned()]),
                    Some(WorkflowRunInfo::new(
                        13,
                        "Build".to_owned(),
                        "Build release".to_owned(),
                        "success".to_owned(),
                        "2026-09-08T00:00:00Z".to_owned(),
                        "https://github.com/owner/repo/actions/runs/13".to_owned(),
                    )),
                    BTreeSet::from([17]),
                    Some("^linux-.*$".to_owned()),
                    BTreeMap::from([("success".to_owned(), "\"run-13\"".to_owned())]),
                    Some("\"artifacts-13\"".to_owned()),
                    Some(1),
                )),
            })
            .expect("actions cursor must be accepted");
        assert_eq!(action.code, StateResponseCode::StateChanged);

        let saved = session
            .save_state_file_if_changed()
            .expect("changed state must be saved");
        assert_eq!(saved.code, StateResponseCode::StateSaved);
        assert!(saved.value.changed);

        let loaded = ProgramStateSession::load_state_file_or_initialize_default(&path, 42)
            .expect("saved state must be loadable");
        assert_eq!(loaded.code, StateResponseCode::StateLoadedExisting);
        let read = loaded
            .value
            .read_repository_state(7)
            .expect("tracked repository must be readable");
        assert_eq!(read.code, StateResponseCode::StateReadCompleted);
        let snapshot = read.value.expect("repository must exist");
        assert_eq!(snapshot.topic_name(), "repo");
        assert_eq!(
            snapshot.cursors().commits().cursor_for("main"),
            Some("abc123")
        );
        assert_eq!(snapshot.cursors().releases().release_id(), Some(11));
        assert_eq!(snapshot.cursors().releases().etag(), Some("\"release-11\""));
        assert_eq!(
            snapshot
                .cursors()
                .releases()
                .etag_for(&ReleaseSelectorScope::LatestStable),
            Some("\"release-11\"")
        );
        assert_eq!(
            snapshot
                .cursors()
                .releases()
                .etag_for(&ReleaseSelectorScope::LatestIncludingPrereleases),
            None
        );
        assert_eq!(
            snapshot
                .cursors()
                .actions()
                .cursor_for(".github/workflows/build.yml", "main")
                .and_then(ActionCursor::run_id),
            Some(13)
        );
        let action_cursor = snapshot
            .cursors()
            .actions()
            .cursor_for(".github/workflows/build.yml", "main")
            .expect("Actions cursor must exist");
        assert_eq!(
            action_cursor.conclusion_scope(),
            &BTreeSet::from(["success".to_owned()])
        );
        assert_eq!(action_cursor.artifact_scope(), Some("^linux-.*$"));
        assert_eq!(
            action_cursor.workflow_run_etag_for("success"),
            Some("\"run-13\"")
        );
        assert_eq!(action_cursor.artifact_etag(), Some("\"artifacts-13\""));
        assert_eq!(action_cursor.artifact_page_count(), Some(1));
        let contents = fs::read_to_string(path).expect("state file must exist");
        assert!(contents.ends_with('\n'));
        assert!(!contents.contains("etag_scope"));
        assert!(!contents.contains("initialized"));
    }

    #[tokio::test]
    async fn resolves_repository_id_from_persisted_mapping_without_api_request() {
        let directory = tempdir().expect("temporary directory must be created");
        let path = directory.path().join("state.json");
        fs::write(
            &path,
            concat!(
                "{\"schema_version\":4,\"group_id\":42,",
                "\"repository_ids\":{\"owner/repo\":7},",
                "\"repositories\":{}}\n"
            ),
        )
        .expect("fixture must be written");

        let mut session = ProgramStateSession::load_state_file_or_initialize_default(&path, 42)
            .expect("state file must load")
            .value;
        let github =
            GitHubClient::new("test-token".to_owned()).expect("GitHub client must be created");

        let resolved = session
            .resolve_repository_id(&github, "owner/repo")
            .await
            .expect("cached repository ID must be resolved locally");

        assert_eq!(resolved.database_id(), 7);
        assert_eq!(resolved.full_name(), "owner/repo");
    }

    #[test]
    fn persists_repository_id_mapping() {
        let directory = tempdir().expect("temporary directory must be created");
        let path = directory.path().join("state.json");
        let mut session = ProgramStateSession::load_state_file_or_initialize_default(&path, 42)
            .expect("missing state file must initialize a default state")
            .value;

        session
            .state
            .repository_ids
            .insert("owner/repo".to_owned(), 7);
        session
            .save_state_file_if_changed()
            .expect("state with repository ID mapping must be saved");

        let contents = fs::read_to_string(path).expect("state file must exist");
        assert!(contents.contains("\"repository_ids\": {"));
        assert!(contents.contains("\"owner/repo\": 7"));
    }

    #[test]
    fn skips_saving_when_state_is_unchanged() {
        let directory = tempdir().expect("temporary directory must be created");
        let path = directory.path().join("state.json");
        fs::write(
            &path,
            "{\"schema_version\":4,\"group_id\":42,\"repositories\":{}}\n",
        )
        .expect("fixture must be written");

        let session = ProgramStateSession::load_state_file_or_initialize_default(&path, 42)
            .expect("state file must load")
            .value;
        let saved = session
            .save_state_file_if_changed()
            .expect("unchanged state must be accepted");
        assert_eq!(saved.code, StateResponseCode::StateSaveSkippedNoChanges);
        assert!(!saved.value.changed);
    }

    #[test]
    fn loads_release_cursor_without_etag() {
        let directory = tempdir().expect("temporary directory must be created");
        let path = directory.path().join("state.json");
        fs::write(
            &path,
            concat!(
                "{\"schema_version\":4,\"group_id\":42,\"repositories\":{\"7\":{",
                "\"message_thread_id\":8,\"topic_name\":\"repo\",\"cursors\":{",
                "\"commits\":{\"branches\":{}},",
                "\"releases\":{\"selector_scope\":{\"kind\":\"latest_stable\"},",
                "\"release_id\":11,\"updated_at\":\"2026-09-01T00:00:00Z\"},",
                "\"actions\":{\"workflows\":{}}",
                "}}}}\n"
            ),
        )
        .expect("fixture must be written");

        let session = ProgramStateSession::load_state_file_or_initialize_default(&path, 42)
            .expect("state without release ETag must load")
            .value;
        let snapshot = session
            .read_repository_state(7)
            .expect("repository state must be readable")
            .value
            .expect("repository must exist");

        assert_eq!(snapshot.cursors().releases().release_id(), Some(11));
        assert_eq!(snapshot.cursors().releases().etag(), None);
        assert_eq!(
            snapshot.cursors().releases().updated_at(),
            Some("2026-09-01T00:00:00Z")
        );
    }

    #[test]
    fn rejects_invalid_group_with_a_specific_code() {
        let directory = tempdir().expect("temporary directory must be created");
        let path = directory.path().join("state.json");
        fs::write(
            &path,
            "{\"schema_version\":4,\"group_id\":99,\"repositories\":{}}\n",
        )
        .expect("fixture must be written");

        let error = ProgramStateSession::load_state_file_or_initialize_default(&path, 42)
            .expect_err("group mismatch must be rejected");
        assert_eq!(error.code, StateResponseCode::StateGroupMismatch);
    }

    #[test]
    fn rejects_schema_one_before_decoding_its_release_cursor() {
        let directory = tempdir().expect("temporary directory must be created");
        let path = directory.path().join("state.json");
        fs::write(
            &path,
            concat!(
                "{\"schema_version\":1,\"group_id\":42,\"repositories\":{\"7\":{",
                "\"message_thread_id\":8,\"topic_name\":\"repo\",\"cursors\":{",
                "\"releases\":{\"initialized\":true,\"release_id\":11,",
                "\"assets\":{\"20\":\"sha256:old\"},\"etag_scope\":\"old\"}",
                "}}}}\n"
            ),
        )
        .expect("fixture must be written");

        let error = ProgramStateSession::load_state_file_or_initialize_default(&path, 42)
            .expect_err("state schema one must be rejected explicitly");
        assert_eq!(error.code, StateResponseCode::StateSchemaUnsupported);
    }

    #[test]
    fn rejects_schema_three_before_decoding_its_action_cursor() {
        let directory = tempdir().expect("temporary directory must be created");
        let path = directory.path().join("state.json");
        fs::write(
            &path,
            r#"{
                "schema_version": 3,
                "group_id": 42,
                "repositories": {
                    "7": {
                        "message_thread_id": 8,
                        "topic_name": "repo",
                        "cursors": {
                            "actions": {
                                "workflows": {
                                    ".github/workflows/build.yml": {
                                        "branches": {
                                            "main": {
                                                "run_id": 13,
                                                "artifact_ids": [17]
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            "#,
        )
        .expect("fixture must be written");

        let error = ProgramStateSession::load_state_file_or_initialize_default(&path, 42)
            .expect_err("state schema three must be rejected explicitly");
        assert_eq!(error.code, StateResponseCode::StateSchemaUnsupported);
    }
}
