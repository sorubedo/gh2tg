use std::collections::BTreeMap;

use thiserror::Error;

use crate::{
    actions::{self, ActionCheckOutcome, ActionCheckpoint},
    commit::{self, CommitCheckOutcome, CommitCheckpoint},
    config::{RepositoryConfig, Settings},
    github::{
        GitHubClient, GitHubError, RepositoryId,
        releases::{self as github_releases},
    },
    message,
    release::{self, ReleaseCheck},
    state::{
        Cursors, ProgramStateSession, RepositoryStateSnapshot, StateChange, StateError,
        StateFailure, StateResponseCode,
    },
    telegram::{MessageThreadId, TelegramClient, TelegramError, TelegramErrorCode},
    telegram_report::{
        AttachmentDownloadMethod, TelegramReportAttachment, TelegramReportPublishFailure,
        TelegramReportPublisher,
    },
};
use teloxide::types::{ChatFullInfoKind, ChatFullInfoPublicKind, ChatMemberKind};

#[derive(Debug, Default)]
pub struct RunSummary {
    pub repositories: Vec<RepositoryResult>,
}

#[derive(Debug)]
pub struct RepositoryResult {
    pub repository: String,
    pub published: usize,
    pub changed: bool,
    pub error: Option<String>,
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    Commit(#[from] crate::commit::CommitError),
    #[error(transparent)]
    GitHub(#[from] GitHubError),
    #[error(transparent)]
    Telegram(#[from] TelegramError),
    #[error(transparent)]
    State(#[from] StateFailure),
    #[error(transparent)]
    TelegramGroupPreparation(#[from] TelegramGroupPreparationError),
    #[error(transparent)]
    TelegramReport(#[from] TelegramReportPublishFailure),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TelegramGroupPreparationStatusCode {
    Ready,
    Prepared,
    StateGroupMismatch,
    StateReferencesGeneralTopic,
    InvalidStateTopicId,
    InvalidStateTopicName,
    InvalidConfiguredTopicName,
    DuplicateConfiguredRepositoryId,
    GroupNotFound,
    NotSupergroup,
    ForumTopicsDisabled,
    InvalidBotToken,
    BotNotMember,
    BotNotAdministrator,
    BotCannotManageTopics,
    TopicNotFound,
    PermissionDenied,
    RateLimited,
    NetworkError,
    TelegramApiError,
    InvalidTelegramResponse,
    LocalIoError,
    InvalidRepository,
    InvalidGitHubToken,
    GitHubRepositoryNotFound,
    GitHubPermissionDenied,
    GitHubRateLimited,
    GitHubNetworkError,
    GitHubApiError,
    StateFileReadFailed,
    StateJsonParseFailed,
    StateSerializationFailed,
    StateSchemaUnsupported,
    StateContentInvalid,
    StateMutationRejected,
    StateFileWriteFailed,
}

impl TelegramGroupPreparationStatusCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "READY",
            Self::Prepared => "PREPARED",
            Self::StateGroupMismatch => "STATE_GROUP_MISMATCH",
            Self::StateReferencesGeneralTopic => "STATE_REFERENCES_GENERAL_TOPIC",
            Self::InvalidStateTopicId => "INVALID_STATE_TOPIC_ID",
            Self::InvalidStateTopicName => "INVALID_STATE_TOPIC_NAME",
            Self::InvalidConfiguredTopicName => "INVALID_CONFIGURED_TOPIC_NAME",
            Self::DuplicateConfiguredRepositoryId => "DUPLICATE_CONFIGURED_REPOSITORY_ID",
            Self::GroupNotFound => "GROUP_NOT_FOUND",
            Self::NotSupergroup => "NOT_SUPERGROUP",
            Self::ForumTopicsDisabled => "FORUM_TOPICS_DISABLED",
            Self::InvalidBotToken => "INVALID_BOT_TOKEN",
            Self::BotNotMember => "BOT_NOT_MEMBER",
            Self::BotNotAdministrator => "BOT_NOT_ADMINISTRATOR",
            Self::BotCannotManageTopics => "BOT_CANNOT_MANAGE_TOPICS",
            Self::TopicNotFound => "TOPIC_NOT_FOUND",
            Self::PermissionDenied => "PERMISSION_DENIED",
            Self::RateLimited => "RATE_LIMITED",
            Self::NetworkError => "NETWORK_ERROR",
            Self::TelegramApiError => "TELEGRAM_API_ERROR",
            Self::InvalidTelegramResponse => "INVALID_TELEGRAM_RESPONSE",
            Self::LocalIoError => "LOCAL_IO_ERROR",
            Self::InvalidRepository => "INVALID_REPOSITORY",
            Self::InvalidGitHubToken => "INVALID_GITHUB_TOKEN",
            Self::GitHubRepositoryNotFound => "GITHUB_REPOSITORY_NOT_FOUND",
            Self::GitHubPermissionDenied => "GITHUB_PERMISSION_DENIED",
            Self::GitHubRateLimited => "GITHUB_RATE_LIMITED",
            Self::GitHubNetworkError => "GITHUB_NETWORK_ERROR",
            Self::GitHubApiError => "GITHUB_API_ERROR",
            Self::StateFileReadFailed => "STATE_FILE_READ_FAILED",
            Self::StateJsonParseFailed => "STATE_JSON_PARSE_FAILED",
            Self::StateSerializationFailed => "STATE_SERIALIZATION_FAILED",
            Self::StateSchemaUnsupported => "STATE_SCHEMA_UNSUPPORTED",
            Self::StateContentInvalid => "STATE_CONTENT_INVALID",
            Self::StateMutationRejected => "STATE_MUTATION_REJECTED",
            Self::StateFileWriteFailed => "STATE_FILE_WRITE_FAILED",
        }
    }
}

impl From<TelegramErrorCode> for TelegramGroupPreparationStatusCode {
    fn from(value: TelegramErrorCode) -> Self {
        match value {
            TelegramErrorCode::GroupNotFound => Self::GroupNotFound,
            TelegramErrorCode::InvalidBotToken => Self::InvalidBotToken,
            TelegramErrorCode::BotNotMember => Self::BotNotMember,
            TelegramErrorCode::PermissionDenied => Self::PermissionDenied,
            TelegramErrorCode::RateLimited => Self::RateLimited,
            TelegramErrorCode::NetworkError => Self::NetworkError,
            TelegramErrorCode::TelegramApiError => Self::TelegramApiError,
            TelegramErrorCode::InvalidTelegramResponse => Self::InvalidTelegramResponse,
            TelegramErrorCode::LocalIoError => Self::LocalIoError,
        }
    }
}

impl From<StateResponseCode> for TelegramGroupPreparationStatusCode {
    fn from(value: StateResponseCode) -> Self {
        match value {
            StateResponseCode::StateGroupMismatch => Self::StateGroupMismatch,
            StateResponseCode::StateFileReadFailed => Self::StateFileReadFailed,
            StateResponseCode::StateJsonParseFailed => Self::StateJsonParseFailed,
            StateResponseCode::StateSerializationFailed => Self::StateSerializationFailed,
            StateResponseCode::StateSchemaUnsupported => Self::StateSchemaUnsupported,
            StateResponseCode::StateContentInvalid => Self::StateContentInvalid,
            StateResponseCode::StateMutationRejected => Self::StateMutationRejected,
            StateResponseCode::StateFileWriteFailed => Self::StateFileWriteFailed,
            StateResponseCode::StateLoadedExisting
            | StateResponseCode::StateInitializedDefault
            | StateResponseCode::StateReadCompleted
            | StateResponseCode::RepositoryStateNotTracked
            | StateResponseCode::StateChanged
            | StateResponseCode::StateUnchanged
            | StateResponseCode::StateSaved
            | StateResponseCode::StateSaveSkippedNoChanges => Self::LocalIoError,
        }
    }
}

#[derive(Debug, Error)]
pub enum TelegramGroupPreparationError {
    #[error(transparent)]
    State(#[from] StateFailure),
    #[error("failed to inspect the Telegram group: {source}")]
    GroupLookupFailed {
        #[source]
        source: TelegramError,
    },
    #[error("the target Telegram chat is not a supergroup")]
    NotSupergroup,
    #[error("forum topics are not enabled in the target Telegram supergroup")]
    ForumTopicsDisabled,
    #[error(
        "state record {repository_id} references Telegram General topic ID=1; refusing to rename General"
    )]
    StateReferencesGeneralTopic { repository_id: u64 },
    #[error("state record {repository_id} has an invalid topic ID: {topic_id}")]
    InvalidStateTopicId { repository_id: u64, topic_id: i32 },
    #[error("state record {repository_id} has an empty topic name")]
    InvalidStateTopicName { repository_id: u64 },
    #[error("configured repository {repository} has an empty topic name")]
    InvalidConfiguredTopicName { repository: String },
    #[error(
        "configured repositories {repository} and {conflicting_repository} resolve to the same GitHub repository ID={repository_id}"
    )]
    DuplicateConfiguredRepositoryId {
        repository: String,
        conflicting_repository: String,
        repository_id: u64,
    },
    #[error(
        "state file group {state_group_id} does not match TelegramClient group {configured_group_id}"
    )]
    StateGroupMismatch {
        state_group_id: i64,
        configured_group_id: i64,
    },
    #[error("failed to verify the Telegram bot identity: {source}")]
    BotIdentityLookupFailed {
        #[source]
        source: TelegramError,
    },
    #[error("failed to inspect the Telegram bot membership: {source}")]
    BotMembershipLookupFailed {
        #[source]
        source: TelegramError,
    },
    #[error("the Telegram bot is not a member of the target group")]
    BotNotMember,
    #[error("the Telegram bot is not an administrator of the target group")]
    BotNotAdministrator,
    #[error("the Telegram bot does not have can_manage_topics permission")]
    BotCannotManageTopics,
    #[error("failed to resolve configured repository {repository}: {source}")]
    RepositoryIdLookupFailed {
        repository: String,
        #[source]
        source: GitHubError,
    },
    #[error("failed to prepare topic {topic_id:?} for repository {repository_id}: {source}")]
    TopicOperationFailed {
        repository_id: u64,
        topic_id: Option<i32>,
        #[source]
        source: TelegramError,
    },
}

impl TelegramGroupPreparationError {
    pub fn status_code(&self) -> TelegramGroupPreparationStatusCode {
        match self {
            Self::State(source) => source.code.into(),
            Self::GroupLookupFailed { source }
            | Self::BotIdentityLookupFailed { source }
            | Self::BotMembershipLookupFailed { source } => source.error_code().into(),
            Self::NotSupergroup => TelegramGroupPreparationStatusCode::NotSupergroup,
            Self::ForumTopicsDisabled => TelegramGroupPreparationStatusCode::ForumTopicsDisabled,
            Self::StateReferencesGeneralTopic { .. } => {
                TelegramGroupPreparationStatusCode::StateReferencesGeneralTopic
            }
            Self::InvalidStateTopicId { .. } => {
                TelegramGroupPreparationStatusCode::InvalidStateTopicId
            }
            Self::InvalidStateTopicName { .. } => {
                TelegramGroupPreparationStatusCode::InvalidStateTopicName
            }
            Self::InvalidConfiguredTopicName { .. } => {
                TelegramGroupPreparationStatusCode::InvalidConfiguredTopicName
            }
            Self::DuplicateConfiguredRepositoryId { .. } => {
                TelegramGroupPreparationStatusCode::DuplicateConfiguredRepositoryId
            }
            Self::StateGroupMismatch { .. } => {
                TelegramGroupPreparationStatusCode::StateGroupMismatch
            }
            Self::BotNotMember => TelegramGroupPreparationStatusCode::BotNotMember,
            Self::BotNotAdministrator => TelegramGroupPreparationStatusCode::BotNotAdministrator,
            Self::BotCannotManageTopics => {
                TelegramGroupPreparationStatusCode::BotCannotManageTopics
            }
            Self::RepositoryIdLookupFailed { source, .. } => github_error_status_code(source),
            Self::TopicOperationFailed { source, .. } if source.is_topic_missing() => {
                TelegramGroupPreparationStatusCode::TopicNotFound
            }
            Self::TopicOperationFailed { source, .. } => source.error_code().into(),
        }
    }
}

fn github_error_status_code(error: &GitHubError) -> TelegramGroupPreparationStatusCode {
    match error {
        GitHubError::InvalidRepository(_) => TelegramGroupPreparationStatusCode::InvalidRepository,
        GitHubError::InvalidToken(_) => TelegramGroupPreparationStatusCode::InvalidGitHubToken,
        GitHubError::Request { .. } => TelegramGroupPreparationStatusCode::GitHubNetworkError,
        GitHubError::RateLimited { .. } => TelegramGroupPreparationStatusCode::GitHubRateLimited,
        GitHubError::Http { status, .. } if status.as_u16() == 401 => {
            TelegramGroupPreparationStatusCode::InvalidGitHubToken
        }
        GitHubError::Http { status, .. } if status.as_u16() == 403 => {
            TelegramGroupPreparationStatusCode::GitHubPermissionDenied
        }
        GitHubError::Http { status, .. } if status.as_u16() == 404 => {
            TelegramGroupPreparationStatusCode::GitHubRepositoryNotFound
        }
        GitHubError::Http { status, .. } if status.as_u16() == 429 => {
            TelegramGroupPreparationStatusCode::GitHubRateLimited
        }
        GitHubError::Decode { .. } => TelegramGroupPreparationStatusCode::GitHubApiError,
        _ => TelegramGroupPreparationStatusCode::GitHubApiError,
    }
}

#[derive(Debug)]
pub struct PreparedRepository {
    pub repository_id: RepositoryId,
    pub thread_id: MessageThreadId,
    pub changed: bool,
}

#[derive(Debug)]
pub struct PreparedTelegramSupergroup {
    pub status_code: TelegramGroupPreparationStatusCode,
    pub repositories: BTreeMap<String, PreparedRepository>,
    pub checked_topic_count: usize,
    pub created_topic_count: usize,
    pub recreated_topic_count: usize,
    pub renamed_topic_count: usize,
    pub locked_topic_count: usize,
}

#[derive(Default)]
struct ProcessStats {
    published: usize,
    changed: bool,
}

impl RunSummary {
    pub fn has_failures(&self) -> bool {
        self.repositories
            .iter()
            .any(|result| result.error.is_some())
    }

    pub fn has_changes(&self) -> bool {
        self.repositories.iter().any(|result| result.changed)
    }

    pub fn published_count(&self) -> usize {
        self.repositories
            .iter()
            .map(|result| result.published)
            .sum()
    }
}

struct TopicPreparationPlan {
    repository_id: u64,
    desired_topic_name: String,
    existing_thread_id: Option<i32>,
    configured_repository_name: Option<String>,
    configured_repository_id: Option<RepositoryId>,
}

struct TopicPreparationOutcome {
    thread_id: MessageThreadId,
    created: bool,
    recreated: bool,
    renamed: bool,
    locked: bool,
}

pub async fn prepare_telegram_supergroup_for_configured_repositories(
    settings: &Settings,
    state: &mut ProgramStateSession,
    github: &GitHubClient,
    telegram: &TelegramClient,
) -> Result<PreparedTelegramSupergroup, TelegramGroupPreparationError> {
    let chat_info = telegram
        .get_chat_info()
        .await
        .map_err(|source| TelegramGroupPreparationError::GroupLookupFailed { source })?;
    let (is_supergroup, has_forum_topics) = match &chat_info.kind {
        ChatFullInfoKind::Public(public_chat) => match &public_chat.kind {
            ChatFullInfoPublicKind::Supergroup(supergroup) => (true, supergroup.is_forum),
            ChatFullInfoPublicKind::Group(_) | ChatFullInfoPublicKind::Channel(_) => (false, false),
        },
        ChatFullInfoKind::Private(_) => (false, false),
    };

    if !is_supergroup {
        return Err(TelegramGroupPreparationError::NotSupergroup);
    }
    if !has_forum_topics {
        return Err(TelegramGroupPreparationError::ForumTopicsDisabled);
    }

    let bot_identity = telegram
        .get_bot_identity()
        .await
        .map_err(|source| TelegramGroupPreparationError::BotIdentityLookupFailed { source })?;
    let bot_membership = telegram
        .get_chat_member(bot_identity.id)
        .await
        .map_err(|source| TelegramGroupPreparationError::BotMembershipLookupFailed { source })?;

    match bot_membership.kind {
        ChatMemberKind::Owner(_) => {}
        ChatMemberKind::Administrator(administrator) if administrator.can_manage_topics => {}
        ChatMemberKind::Administrator(_) => {
            return Err(TelegramGroupPreparationError::BotCannotManageTopics);
        }
        ChatMemberKind::Member(_) | ChatMemberKind::Restricted(_) => {
            return Err(TelegramGroupPreparationError::BotNotAdministrator);
        }
        ChatMemberKind::Left | ChatMemberKind::Banned(_) => {
            return Err(TelegramGroupPreparationError::BotNotMember);
        }
    }

    let tracked_repositories = state.read_all_tracked_repository_states()?.value;
    let mut topic_plans = BTreeMap::new();
    for repository_state in &tracked_repositories {
        let repository_id = repository_state.repository_id();
        validate_state_topic_id(repository_state)?;
        topic_plans.insert(
            repository_id,
            TopicPreparationPlan {
                repository_id,
                desired_topic_name: repository_state.topic_name().to_owned(),
                existing_thread_id: Some(repository_state.message_thread_id()),
                configured_repository_name: None,
                configured_repository_id: None,
            },
        );
    }

    let configured_repository_count = settings.repositories.len();
    for (repository, repository_config) in &settings.repositories {
        if repository_config.topic_name.trim().is_empty() {
            return Err(TelegramGroupPreparationError::InvalidConfiguredTopicName {
                repository: repository.clone(),
            });
        }

        let repository_id = state
            .resolve_repository_id(github, repository)
            .await
            .map_err(
                |source| TelegramGroupPreparationError::RepositoryIdLookupFailed {
                    repository: repository.clone(),
                    source,
                },
            )?;
        let database_id = repository_id.database_id();

        if let Some(existing_plan) = topic_plans.get(&database_id)
            && let Some(conflicting_repository) = &existing_plan.configured_repository_name
        {
            return Err(
                TelegramGroupPreparationError::DuplicateConfiguredRepositoryId {
                    repository: repository.clone(),
                    conflicting_repository: conflicting_repository.clone(),
                    repository_id: database_id,
                },
            );
        }

        topic_plans
            .entry(database_id)
            .and_modify(|plan| {
                plan.desired_topic_name = repository_config.topic_name.clone();
                plan.configured_repository_name = Some(repository.clone());
                plan.configured_repository_id = Some(repository_id.clone());
            })
            .or_insert_with(|| TopicPreparationPlan {
                repository_id: database_id,
                desired_topic_name: repository_config.topic_name.clone(),
                existing_thread_id: None,
                configured_repository_name: Some(repository.clone()),
                configured_repository_id: Some(repository_id.clone()),
            });
    }

    for topic_plan in topic_plans.values() {
        if topic_plan.desired_topic_name.trim().is_empty() {
            if let Some(repository) = &topic_plan.configured_repository_name {
                return Err(TelegramGroupPreparationError::InvalidConfiguredTopicName {
                    repository: repository.clone(),
                });
            }
            return Err(TelegramGroupPreparationError::InvalidStateTopicName {
                repository_id: topic_plan.repository_id,
            });
        }
    }

    let checked_topic_count = topic_plans.len();
    let mut prepared_repositories = BTreeMap::new();
    let mut created_topic_count = 0;
    let mut recreated_topic_count = 0;
    let mut renamed_topic_count = 0;
    let mut locked_topic_count = 0;
    let mut any_change = false;

    for topic_plan in topic_plans.values() {
        let topic_outcome = prepare_topic(topic_plan, telegram).await?;
        let state_change = state.apply_state_change(StateChange::EnsureRepositoryTopicState {
            repository_id: topic_plan.repository_id,
            message_thread_id: topic_outcome.thread_id.0,
            topic_name: topic_plan.desired_topic_name.clone(),
        })?;
        let state_changed = state_change.value.changed;

        created_topic_count += usize::from(topic_outcome.created);
        recreated_topic_count += usize::from(topic_outcome.recreated);
        renamed_topic_count += usize::from(topic_outcome.renamed);
        locked_topic_count += usize::from(topic_outcome.locked);
        let repository_changed = state_changed
            || topic_outcome.created
            || topic_outcome.recreated
            || topic_outcome.renamed;
        any_change |= repository_changed;

        if let (Some(repository), Some(repository_id)) = (
            &topic_plan.configured_repository_name,
            &topic_plan.configured_repository_id,
        ) {
            prepared_repositories.insert(
                repository.clone(),
                PreparedRepository {
                    repository_id: repository_id.clone(),
                    thread_id: topic_outcome.thread_id,
                    changed: repository_changed,
                },
            );
        }
    }

    debug_assert_eq!(prepared_repositories.len(), configured_repository_count);

    Ok(PreparedTelegramSupergroup {
        status_code: if any_change {
            TelegramGroupPreparationStatusCode::Prepared
        } else {
            TelegramGroupPreparationStatusCode::Ready
        },
        repositories: prepared_repositories,
        checked_topic_count,
        created_topic_count,
        recreated_topic_count,
        renamed_topic_count,
        locked_topic_count,
    })
}

fn validate_state_topic_id(
    repository_state: &RepositoryStateSnapshot,
) -> Result<(), TelegramGroupPreparationError> {
    let repository_id = repository_state.repository_id();
    if repository_state.message_thread_id() == 1 {
        return Err(TelegramGroupPreparationError::StateReferencesGeneralTopic { repository_id });
    }
    if repository_state.message_thread_id() <= 0 {
        return Err(TelegramGroupPreparationError::InvalidStateTopicId {
            repository_id: repository_state.repository_id(),
            topic_id: repository_state.message_thread_id(),
        });
    }
    Ok(())
}

async fn prepare_topic(
    topic_plan: &TopicPreparationPlan,
    telegram: &TelegramClient,
) -> Result<TopicPreparationOutcome, TelegramGroupPreparationError> {
    let translator = crate::i18n::current();
    let topic_owner = topic_plan
        .configured_repository_name
        .as_deref()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("state repository {}", topic_plan.repository_id));

    let (thread_id, created, recreated, renamed) = match topic_plan.existing_thread_id {
        None => {
            println!(
                "{}",
                translator.console(crate::i18n::ConsoleMessage::TopicCreating {
                    owner: &topic_owner,
                    topic_name: &topic_plan.desired_topic_name,
                })
            );
            let thread_id = telegram
                .create_topic(&topic_plan.desired_topic_name)
                .await
                .map_err(
                    |source| TelegramGroupPreparationError::TopicOperationFailed {
                        repository_id: topic_plan.repository_id,
                        topic_id: None,
                        source,
                    },
                )?;
            (thread_id, true, false, false)
        }
        Some(existing_thread_id) => {
            let existing_thread_id = MessageThreadId(existing_thread_id);
            match telegram
                .rename_topic(existing_thread_id, &topic_plan.desired_topic_name)
                .await
            {
                Ok(()) => (existing_thread_id, false, false, true),
                Err(error) if error.is_topic_not_modified() => {
                    (existing_thread_id, false, false, false)
                }
                Err(error) if error.is_topic_missing() => {
                    println!(
                        "{}",
                        translator.console(crate::i18n::ConsoleMessage::TopicRecreating {
                            owner: &topic_owner,
                            thread_id: existing_thread_id.0,
                            topic_name: &topic_plan.desired_topic_name,
                        })
                    );
                    let thread_id = telegram
                        .create_topic(&topic_plan.desired_topic_name)
                        .await
                        .map_err(
                            |source| TelegramGroupPreparationError::TopicOperationFailed {
                                repository_id: topic_plan.repository_id,
                                topic_id: Some(existing_thread_id.0),
                                source,
                            },
                        )?;
                    (thread_id, false, true, false)
                }
                Err(source) => {
                    return Err(TelegramGroupPreparationError::TopicOperationFailed {
                        repository_id: topic_plan.repository_id,
                        topic_id: Some(existing_thread_id.0),
                        source,
                    });
                }
            }
        }
    };

    let locked = match telegram.close_topic(thread_id).await {
        Ok(()) => true,
        Err(error) if error.is_topic_not_modified() => true,
        Err(source) => {
            return Err(TelegramGroupPreparationError::TopicOperationFailed {
                repository_id: topic_plan.repository_id,
                topic_id: Some(thread_id.0),
                source,
            });
        }
    };

    Ok(TopicPreparationOutcome {
        thread_id,
        created,
        recreated,
        renamed,
        locked,
    })
}

pub async fn run(
    settings: &Settings,
    state: &mut ProgramStateSession,
    preparation: &PreparedTelegramSupergroup,
    github: &GitHubClient,
    telegram: &TelegramClient,
) -> RunSummary {
    let translator = crate::i18n::current();
    let mut summary = RunSummary::default();

    for (repository, config) in &settings.repositories {
        println!(
            "{}",
            translator.console(crate::i18n::ConsoleMessage::RepositoryStarted { repository })
        );
        let mut stats = ProcessStats::default();
        let prepared_repository = preparation
            .repositories
            .get(repository)
            .expect("every configured repository must be prepared");
        stats.changed = prepared_repository.changed;
        let error = process_repository(
            repository,
            prepared_repository,
            config,
            state,
            github,
            telegram,
            &mut stats,
        )
        .await
        .err()
        .map(|error| translator.app_error(&error));
        match &error {
            Some(error) => eprintln!(
                "{}",
                translator
                    .console(crate::i18n::ConsoleMessage::RepositoryFailed { repository, error })
            ),
            None => println!(
                "{}",
                translator.console(crate::i18n::ConsoleMessage::RepositoryCompleted {
                    repository,
                    published: stats.published,
                    changed: stats.changed,
                })
            ),
        }
        let result = RepositoryResult {
            repository: repository.clone(),
            published: stats.published,
            changed: stats.changed,
            error,
        };
        summary.repositories.push(result);
    }

    summary
}

async fn process_repository(
    repository: &str,
    prepared_repository: &PreparedRepository,
    config: &RepositoryConfig,
    state: &mut ProgramStateSession,
    github: &GitHubClient,
    telegram: &TelegramClient,
    stats: &mut ProcessStats,
) -> Result<(), AppError> {
    let translator = crate::i18n::current();
    let repository_id = &prepared_repository.repository_id;
    let thread_id = prepared_repository.thread_id;
    let repository_key = repository_id.database_id();
    if let Some(commit_config) = &config.commits {
        process_commits(
            repository,
            repository_key,
            commit_config,
            state,
            thread_id,
            github,
            telegram,
            stats,
        )
        .await?;
    } else {
        println!(
            "{}",
            translator.console(crate::i18n::ConsoleMessage::FeatureDisabled {
                repository,
                feature: crate::i18n::Feature::Commit,
            })
        );
    }

    if let Some(release_config) = &config.releases {
        process_releases(
            repository,
            repository_id,
            repository_key,
            release_config,
            state,
            thread_id,
            github,
            telegram,
            stats,
        )
        .await?;
    } else {
        println!(
            "{}",
            translator.console(crate::i18n::ConsoleMessage::FeatureDisabled {
                repository,
                feature: crate::i18n::Feature::Release,
            })
        );
    }

    if let Some(actions_config) = &config.actions {
        process_actions(
            repository,
            repository_key,
            actions_config,
            state,
            thread_id,
            github,
            telegram,
            stats,
        )
        .await?;
    } else {
        println!(
            "{}",
            translator.console(crate::i18n::ConsoleMessage::FeatureDisabled {
                repository,
                feature: crate::i18n::Feature::Actions,
            })
        );
    }

    Ok(())
}

fn read_repository_cursors(
    state: &ProgramStateSession,
    repository_id: u64,
) -> Result<Cursors, AppError> {
    let result = state.read_repository_state(repository_id)?;
    result
        .value
        .map(|snapshot| snapshot.cursors().clone())
        .ok_or_else(|| {
            AppError::State(StateFailure::from(StateError::RepositoryNotTracked {
                repository_id,
            }))
        })
}

#[allow(clippy::too_many_arguments)]
async fn process_commits(
    repository: &str,
    repository_id: u64,
    config: &crate::config::CommitConfig,
    state: &mut ProgramStateSession,
    thread_id: MessageThreadId,
    github: &GitHubClient,
    telegram: &TelegramClient,
    stats: &mut ProcessStats,
) -> Result<(), AppError> {
    let translator = crate::i18n::current();
    println!(
        "{}",
        translator.console(crate::i18n::ConsoleMessage::CommitQuery {
            repository,
            branches: config.branches.len(),
        })
    );
    let cursors = read_repository_cursors(state, repository_id)?;
    let check =
        commit::check_for_commit_updates(github, repository, config, cursors.commits()).await?;
    println!(
        "{}",
        translator.console(crate::i18n::ConsoleMessage::CommitCheck {
            repository,
            baselines: check.baseline_count(),
            rewritten: check.rewritten_count(),
            updates: check.detected_commit_count(),
        })
    );

    for outcome in check.into_outcomes() {
        match outcome {
            CommitCheckOutcome::BaselineEstablished { checkpoint } => {
                stats.changed |= apply_commit_checkpoint(state, repository_id, checkpoint)?;
            }
            CommitCheckOutcome::Unchanged => {}
            CommitCheckOutcome::HistoryRewritten { checkpoint } => {
                println!(
                    "{}",
                    translator.console(crate::i18n::ConsoleMessage::CommitHistoryRewritten {
                        repository,
                        branch: checkpoint.branch(),
                        sha: checkpoint.sha().get(..7).unwrap_or(checkpoint.sha()),
                    })
                );
                stats.changed |= apply_commit_checkpoint(state, repository_id, checkpoint)?;
            }
            CommitCheckOutcome::UpdateDetected { batch } => {
                let publisher = TelegramReportPublisher::new(github, telegram, thread_id);
                for commit in batch.commits() {
                    println!(
                        "{}",
                        translator.console(crate::i18n::ConsoleMessage::CommitPublishing {
                            repository,
                            branch: batch.branch(),
                            sha: commit.sha().get(..7).unwrap_or(commit.sha()),
                        })
                    );
                    publisher
                        .publish_telegram_report(
                            message::format_commit(translator, repository, batch.branch(), commit),
                            Vec::new(),
                        )
                        .await?;
                    stats.published += 1;
                }
                stats.changed |=
                    apply_commit_checkpoint(state, repository_id, batch.into_checkpoint())?;
            }
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn process_releases(
    repository: &str,
    repository_id: &RepositoryId,
    state_repository_id: u64,
    config: &crate::config::ReleaseConfig,
    state: &mut ProgramStateSession,
    thread_id: MessageThreadId,
    github: &GitHubClient,
    telegram: &TelegramClient,
    stats: &mut ProcessStats,
) -> Result<(), AppError> {
    let translator = crate::i18n::current();
    println!(
        "{}",
        translator.console(crate::i18n::ConsoleMessage::ReleaseQuery {
            repository,
            specific_tag: config.selector.tag().is_some(),
        })
    );
    let current_cursor = read_repository_cursors(state, state_repository_id)?
        .releases()
        .clone();
    let check =
        release::check_for_new_release(github, repository_id, config, &current_cursor).await?;

    match check {
        ReleaseCheck::NotFound { checkpoint, .. } => {
            stats.changed |= apply_release_checkpoint(state, state_repository_id, checkpoint)?;
            println!(
                "{}",
                translator.console(crate::i18n::ConsoleMessage::ReleaseNotFound { repository })
            );
        }
        ReleaseCheck::BaselineEstablished {
            release_id,
            checkpoint,
            ..
        } => {
            stats.changed |= apply_release_checkpoint(state, state_repository_id, checkpoint)?;
            println!(
                "{}",
                translator.console(crate::i18n::ConsoleMessage::ReleaseBaselineEstablished {
                    repository,
                    release_id: release_id.value(),
                },)
            );
        }
        ReleaseCheck::Unchanged { checkpoint, .. } => {
            stats.changed |= apply_release_checkpoint(state, state_repository_id, checkpoint)?;
            println!(
                "{}",
                translator.console(crate::i18n::ConsoleMessage::ReleaseUnchanged { repository })
            );
        }
        check @ (ReleaseCheck::NewRelease { .. } | ReleaseCheck::UpdatedRelease { .. }) => {
            let (release, checkpoint, updated) = match check {
                ReleaseCheck::NewRelease {
                    release,
                    checkpoint,
                    ..
                } => (release, checkpoint, false),
                ReleaseCheck::UpdatedRelease {
                    release,
                    checkpoint,
                    ..
                } => (release, checkpoint, true),
                _ => unreachable!("release check variant was matched above"),
            };
            let release_id = release.id();
            let assets = release
                .assets()
                .iter()
                .filter(|asset| {
                    config
                        .asset_regex
                        .as_ref()
                        .is_some_and(|pattern| pattern.is_match(asset.file_name().as_str()))
                })
                .cloned()
                .collect::<Vec<_>>();
            println!(
                "{}",
                translator.console(crate::i18n::ConsoleMessage::ReleasePublishing {
                    repository,
                    updated,
                    release_id: release_id.value(),
                    attachments: release.assets().len(),
                    uploads: assets.len(),
                })
            );
            let attachments = assets
                .iter()
                .map(|asset| {
                    github_releases::release_asset_download_url(github, repository_id, asset).map(
                        |download_url| {
                            TelegramReportAttachment::new(
                                asset.file_name().as_str(),
                                download_url.to_string(),
                                AttachmentDownloadMethod::GitHubReleaseAsset,
                                asset.digest().map(|digest| digest.as_str().to_owned()),
                            )
                        },
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            let publisher = TelegramReportPublisher::new(github, telegram, thread_id);
            publisher
                .publish_telegram_report(
                    message::format_release(translator, repository, &release, updated),
                    attachments,
                )
                .await?;
            stats.changed |= apply_release_checkpoint(state, state_repository_id, checkpoint)?;
            stats.published += 1;
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn process_actions(
    repository: &str,
    repository_id: u64,
    config: &crate::config::ActionsConfig,
    state: &mut ProgramStateSession,
    thread_id: MessageThreadId,
    github: &GitHubClient,
    telegram: &TelegramClient,
    stats: &mut ProcessStats,
) -> Result<(), AppError> {
    let translator = crate::i18n::current();
    let tracked_pairs = config
        .workflows
        .iter()
        .map(|workflow| workflow.branches.len())
        .sum::<usize>();
    println!(
        "{}",
        translator.console(crate::i18n::ConsoleMessage::ActionsQuery {
            repository,
            targets: tracked_pairs,
        })
    );
    let cursors = read_repository_cursors(state, repository_id)?;
    let check =
        actions::check_for_action_updates(github, repository, config, cursors.actions()).await?;
    println!(
        "{}",
        translator.console(crate::i18n::ConsoleMessage::ActionsCheck {
            repository,
            baselines: check.baseline_count(),
            updates: check.detected_update_count(),
        })
    );

    for outcome in check.into_outcomes() {
        match outcome {
            ActionCheckOutcome::BaselineEstablished { checkpoint }
            | ActionCheckOutcome::Unchanged { checkpoint } => {
                stats.changed |= apply_action_checkpoint(state, repository_id, checkpoint)?;
            }
            ActionCheckOutcome::UpdateDetected { update } => {
                println!(
                    "{}",
                    translator.console(crate::i18n::ConsoleMessage::ActionsPublishing {
                        repository,
                        workflow: update.target().workflow_file(),
                        branch: update.target().branch(),
                        run_id: update.run().id(),
                        kind: update.kind(),
                        artifacts: update.artifacts().len(),
                    })
                );
                let attachments = update
                    .artifacts()
                    .iter()
                    .map(|artifact| {
                        TelegramReportAttachment::new(
                            format!("{}.zip", artifact.name()),
                            artifact.archive_download_url(),
                            AttachmentDownloadMethod::GitHubActionsArtifact,
                            None,
                        )
                    })
                    .collect();
                let publisher = TelegramReportPublisher::new(github, telegram, thread_id);
                publisher
                    .publish_telegram_report(
                        message::format_workflow_run(translator, repository, &update),
                        attachments,
                    )
                    .await?;
                stats.changed |=
                    apply_action_checkpoint(state, repository_id, update.into_checkpoint())?;
                stats.published += 1;
            }
        }
    }

    Ok(())
}

fn apply_release_checkpoint(
    state: &mut ProgramStateSession,
    repository_id: u64,
    checkpoint: crate::state::ReleaseCursor,
) -> Result<bool, AppError> {
    let change = state.apply_state_change(StateChange::ReplaceReleaseCursor {
        repository_id,
        cursor: checkpoint,
    })?;
    Ok(change.value.changed)
}

fn apply_commit_checkpoint(
    state: &mut ProgramStateSession,
    repository_id: u64,
    checkpoint: CommitCheckpoint,
) -> Result<bool, AppError> {
    let (branch, sha) = checkpoint.into_state_parts();
    let change = state.apply_state_change(StateChange::ReplaceCommitBranchCursor {
        repository_id,
        branch,
        sha,
    })?;
    Ok(change.value.changed)
}

fn apply_action_checkpoint(
    state: &mut ProgramStateSession,
    repository_id: u64,
    checkpoint: ActionCheckpoint,
) -> Result<bool, AppError> {
    let (workflow_file, branch, cursor) = checkpoint.into_state_parts();
    let change = state.apply_state_change(StateChange::ReplaceActionBranchCursor {
        repository_id,
        workflow_file,
        branch,
        cursor: Box::new(cursor),
    })?;
    Ok(change.value.changed)
}
