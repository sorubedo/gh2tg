use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs, io,
    path::{Path, PathBuf},
};

use regex::Regex;
use serde::Deserialize;
use thiserror::Error;

use crate::release::{ReleaseSelector, ReleaseTag, ReleaseTagError};

const SUPPORTED_SCHEMA_VERSION: u32 = 1;

pub struct Settings {
    pub bot_token: String,
    pub group_id: i64,
    pub github_token: String,
    pub schema_version: u32,
    pub repositories: BTreeMap<String, RepositoryConfig>,
}

#[derive(Clone, Debug)]
pub struct RepositoryConfig {
    pub topic_name: String,
    pub commits: Option<CommitConfig>,
    pub releases: Option<ReleaseConfig>,
    pub actions: Option<ActionsConfig>,
}

#[derive(Clone, Debug)]
pub struct CommitConfig {
    pub branches: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct ReleaseConfig {
    pub selector: ReleaseSelector,
    pub asset_regex: Option<Regex>,
}

#[derive(Clone, Debug)]
pub struct ActionsConfig {
    pub workflows: Vec<WorkflowConfig>,
}

#[derive(Clone, Debug)]
pub struct WorkflowConfig {
    pub workflow_file: String,
    pub branches: Vec<String>,
    pub conclusions: Vec<String>,
    pub artifact_regex: Option<Regex>,
}

struct Environment {
    bot_token: String,
    group_id: i64,
    github_token: String,
}

struct ConfigFile {
    schema_version: u32,
    repositories: BTreeMap<String, RepositoryConfig>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfigFile {
    schema_version: u32,
    repositories: BTreeMap<String, RawRepositoryConfig>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRepositoryConfig {
    topic_name: Option<String>,
    commits: Option<RawCommitConfig>,
    releases: Option<RawReleaseConfig>,
    actions: Option<RawActionsConfig>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCommitConfig {
    branches: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawReleaseConfig {
    #[serde(default)]
    include_prereleases: bool,
    tag: Option<String>,
    asset_regex: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawActionsConfig {
    workflows: Vec<RawWorkflowConfig>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWorkflowConfig {
    workflow_file: String,
    branches: Vec<String>,
    conclusions: Vec<String>,
    artifact_regex: Option<String>,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to load .env: {0}")]
    Dotenv(#[source] dotenvy::Error),
    #[error("missing environment variable {0}")]
    MissingEnvironment(&'static str),
    #[error("environment variable {0} is not valid Unicode")]
    InvalidEnvironment(&'static str),
    #[error("GH2TG_GROUP_ID must be a valid integer starting with -100")]
    InvalidGroupId,
    #[error("failed to read configuration file {path}: {source}")]
    ReadConfig {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to parse configuration file {path}: {source}")]
    ParseConfig {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("unsupported schema_version: {0}")]
    UnsupportedSchemaVersion(u32),
    #[error("invalid repository name {0}; expected owner/repo format")]
    InvalidRepositoryName(String),
    #[error("repository {repository} has an invalid Release Tag: {source}")]
    InvalidReleaseTag {
        repository: String,
        #[source]
        source: ReleaseTagError,
    },
    #[error("repository {repository} field {field} must not be empty")]
    EmptyField { repository: String, field: String },
    #[error("repository {repository} has an invalid regex in {field}: {source}")]
    InvalidRegex {
        repository: String,
        field: String,
        #[source]
        source: regex::Error,
    },
    #[error("repository {repository} field {field} contains a duplicate value: {value}")]
    DuplicateValue {
        repository: String,
        field: String,
        value: String,
    },
    #[error(
        "repository {repository} contains a duplicate Actions target: {workflow_file} / {branch}"
    )]
    DuplicateActionTarget {
        repository: String,
        workflow_file: String,
        branch: String,
    },
}

pub fn load(path: &Path) -> Result<Settings, ConfigError> {
    match dotenvy::dotenv() {
        Ok(_) => {}
        Err(error) if error.not_found() => {}
        Err(error) => return Err(ConfigError::Dotenv(error)),
    }

    let environment = load_environment()?;
    let config = load_json(path)?;

    Ok(Settings {
        bot_token: environment.bot_token,
        group_id: environment.group_id,
        github_token: environment.github_token,
        schema_version: config.schema_version,
        repositories: config.repositories,
    })
}

fn load_environment() -> Result<Environment, ConfigError> {
    Ok(Environment {
        bot_token: required_environment("GH2TG_BOT_TOKEN")?,
        group_id: parse_group_id(&required_environment("GH2TG_GROUP_ID")?)?,
        github_token: required_environment("GH2TG_GITHUB_TOKEN")?,
    })
}

fn required_environment(name: &'static str) -> Result<String, ConfigError> {
    match env::var(name) {
        Ok(value) if value.trim().is_empty() => Err(ConfigError::MissingEnvironment(name)),
        Ok(value) => Ok(value),
        Err(env::VarError::NotPresent) => Err(ConfigError::MissingEnvironment(name)),
        Err(env::VarError::NotUnicode(_)) => Err(ConfigError::InvalidEnvironment(name)),
    }
}

fn parse_group_id(value: &str) -> Result<i64, ConfigError> {
    if !value.starts_with("-100") {
        return Err(ConfigError::InvalidGroupId);
    }

    value.parse().map_err(|_| ConfigError::InvalidGroupId)
}

fn load_json(path: &Path) -> Result<ConfigFile, ConfigError> {
    let contents = fs::read_to_string(path).map_err(|source| ConfigError::ReadConfig {
        path: path.to_path_buf(),
        source,
    })?;

    parse_json(path, &contents)
}

fn parse_json(path: &Path, contents: &str) -> Result<ConfigFile, ConfigError> {
    let raw: RawConfigFile =
        serde_json::from_str(contents).map_err(|source| ConfigError::ParseConfig {
            path: path.to_path_buf(),
            source,
        })?;

    validate_schema_version(raw.schema_version)?;

    let repositories = raw
        .repositories
        .into_iter()
        .map(|(name, raw)| {
            validate_repository_name(&name)?;
            compile_repository_config(&name, raw).map(|config| (name, config))
        })
        .collect::<Result<_, _>>()?;

    Ok(ConfigFile {
        schema_version: raw.schema_version,
        repositories,
    })
}

fn validate_schema_version(version: u32) -> Result<(), ConfigError> {
    if version != SUPPORTED_SCHEMA_VERSION {
        return Err(ConfigError::UnsupportedSchemaVersion(version));
    }

    Ok(())
}

fn validate_repository_name(name: &str) -> Result<(), ConfigError> {
    let Some((owner, repository)) = name.split_once('/') else {
        return Err(ConfigError::InvalidRepositoryName(name.to_owned()));
    };

    if owner.is_empty()
        || repository.is_empty()
        || repository.contains('/')
        || owner.trim() != owner
        || repository.trim() != repository
    {
        return Err(ConfigError::InvalidRepositoryName(name.to_owned()));
    }

    Ok(())
}

fn compile_repository_config(
    repository: &str,
    raw: RawRepositoryConfig,
) -> Result<RepositoryConfig, ConfigError> {
    let topic_name = raw.topic_name.unwrap_or_else(|| repository.to_owned());
    validate_nonempty(repository, "topic_name", &topic_name)?;

    let commits = raw
        .commits
        .map(|config| {
            validate_list(repository, "commits.branches", &config.branches)?;
            validate_unique_values(repository, "commits.branches", &config.branches)?;
            Ok(CommitConfig {
                branches: config.branches,
            })
        })
        .transpose()?;

    let releases = raw
        .releases
        .map(|config| {
            let tag = config
                .tag
                .map(ReleaseTag::new)
                .transpose()
                .map_err(|source| ConfigError::InvalidReleaseTag {
                    repository: repository.to_owned(),
                    source,
                })?;

            let selector = match tag {
                Some(tag) => ReleaseSelector::Tag(tag),
                None if config.include_prereleases => ReleaseSelector::LatestIncludingPrereleases,
                None => ReleaseSelector::LatestStable,
            };

            Ok(ReleaseConfig {
                selector,
                asset_regex: compile_regex(repository, "releases.asset_regex", config.asset_regex)?,
            })
        })
        .transpose()?;

    let actions = raw
        .actions
        .map(|config| {
            if config.workflows.is_empty() {
                return Err(ConfigError::EmptyField {
                    repository: repository.to_owned(),
                    field: "actions.workflows".to_owned(),
                });
            }

            let workflows = config
                .workflows
                .into_iter()
                .enumerate()
                .map(|(index, workflow)| compile_workflow(repository, index, workflow))
                .collect::<Result<Vec<_>, _>>()?;
            validate_unique_action_targets(repository, &workflows)?;

            Ok(ActionsConfig { workflows })
        })
        .transpose()?;

    Ok(RepositoryConfig {
        topic_name,
        commits,
        releases,
        actions,
    })
}

fn compile_workflow(
    repository: &str,
    index: usize,
    raw: RawWorkflowConfig,
) -> Result<WorkflowConfig, ConfigError> {
    let prefix = format!("actions.workflows[{index}]");
    validate_nonempty(
        repository,
        &format!("{prefix}.workflow_file"),
        &raw.workflow_file,
    )?;
    validate_list(repository, &format!("{prefix}.branches"), &raw.branches)?;
    validate_unique_values(repository, &format!("{prefix}.branches"), &raw.branches)?;
    validate_list(
        repository,
        &format!("{prefix}.conclusions"),
        &raw.conclusions,
    )?;
    validate_unique_values(
        repository,
        &format!("{prefix}.conclusions"),
        &raw.conclusions,
    )?;

    Ok(WorkflowConfig {
        workflow_file: raw.workflow_file,
        branches: raw.branches,
        conclusions: raw.conclusions,
        artifact_regex: compile_regex(
            repository,
            &format!("{prefix}.artifact_regex"),
            raw.artifact_regex,
        )?,
    })
}

fn compile_regex(
    repository: &str,
    field: &str,
    pattern: Option<String>,
) -> Result<Option<Regex>, ConfigError> {
    pattern
        .map(|pattern| {
            Regex::new(&pattern).map_err(|source| ConfigError::InvalidRegex {
                repository: repository.to_owned(),
                field: field.to_owned(),
                source,
            })
        })
        .transpose()
}

fn validate_list(repository: &str, field: &str, values: &[String]) -> Result<(), ConfigError> {
    if values.is_empty() {
        return Err(ConfigError::EmptyField {
            repository: repository.to_owned(),
            field: field.to_owned(),
        });
    }

    for value in values {
        validate_nonempty(repository, field, value)?;
    }

    Ok(())
}

fn validate_unique_values(
    repository: &str,
    field: &str,
    values: &[String],
) -> Result<(), ConfigError> {
    let mut unique_values = BTreeSet::new();
    for value in values {
        if !unique_values.insert(value) {
            return Err(ConfigError::DuplicateValue {
                repository: repository.to_owned(),
                field: field.to_owned(),
                value: value.clone(),
            });
        }
    }

    Ok(())
}

fn validate_unique_action_targets(
    repository: &str,
    workflows: &[WorkflowConfig],
) -> Result<(), ConfigError> {
    let mut targets = BTreeSet::new();
    for workflow in workflows {
        for branch in &workflow.branches {
            if !targets.insert((&workflow.workflow_file, branch)) {
                return Err(ConfigError::DuplicateActionTarget {
                    repository: repository.to_owned(),
                    workflow_file: workflow.workflow_file.clone(),
                    branch: branch.clone(),
                });
            }
        }
    }

    Ok(())
}

fn validate_nonempty(repository: &str, field: &str, value: &str) -> Result<(), ConfigError> {
    if value.trim().is_empty() {
        return Err(ConfigError::EmptyField {
            repository: repository.to_owned(),
            field: field.to_owned(),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{ConfigError, parse_json};

    #[test]
    fn rejects_duplicate_commit_branches() {
        let result = parse_json(
            Path::new("config.json"),
            r#"{
                "schema_version": 1,
                "repositories": {
                    "owner/repo": {
                        "commits": {
                            "branches": ["main", "main"]
                        }
                    }
                }
            }"#,
        );
        let error = match result {
            Ok(_) => panic!("duplicate Commit branches must be rejected"),
            Err(error) => error,
        };

        assert!(matches!(
            error,
            ConfigError::DuplicateValue { field, value, .. }
                if field == "commits.branches" && value == "main"
        ));
    }

    #[test]
    fn rejects_duplicate_actions_targets() {
        let result = parse_json(
            Path::new("config.json"),
            r#"{
                "schema_version": 1,
                "repositories": {
                    "owner/repo": {
                        "actions": {
                            "workflows": [
                                {
                                    "workflow_file": ".github/workflows/build.yml",
                                    "branches": ["main"],
                                    "conclusions": ["success"]
                                },
                                {
                                    "workflow_file": ".github/workflows/build.yml",
                                    "branches": ["main"],
                                    "conclusions": ["failure"]
                                }
                            ]
                        }
                    }
                }
            }"#,
        );
        let error = match result {
            Ok(_) => panic!("duplicate Actions targets must be rejected"),
            Err(error) => error,
        };

        assert!(matches!(
            error,
            ConfigError::DuplicateActionTarget {
                workflow_file,
                branch,
                ..
            } if workflow_file == ".github/workflows/build.yml" && branch == "main"
        ));
    }

    #[test]
    fn rejects_duplicate_actions_conclusions() {
        let result = parse_json(
            Path::new("config.json"),
            r#"{
                "schema_version": 1,
                "repositories": {
                    "owner/repo": {
                        "actions": {
                            "workflows": [
                                {
                                    "workflow_file": ".github/workflows/build.yml",
                                    "branches": ["main"],
                                    "conclusions": ["success", "success"]
                                }
                            ]
                        }
                    }
                }
            }"#,
        );
        let error = match result {
            Ok(_) => panic!("duplicate Actions conclusions must be rejected"),
            Err(error) => error,
        };

        assert!(matches!(
            error,
            ConfigError::DuplicateValue { field, value, .. }
                if field == "actions.workflows[0].conclusions" && value == "success"
        ));
    }
}
