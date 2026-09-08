pub mod actions;
pub mod app;
pub mod commit;
pub mod config;
pub mod github;
pub mod i18n;
pub mod message;
pub mod release;
pub mod state;
pub mod telegram;
pub mod telegram_report;

use std::{env, path::PathBuf, process::ExitCode};

use app::RunSummary;
use github::GitHubClient;
use state::ProgramStateSession;
use telegram::TelegramClient;
use thiserror::Error;

const DEFAULT_CONFIG_PATH: &str = "config.json";
const DEFAULT_STATE_PATH: &str = "state.json";

struct Cli {
    config_path: PathBuf,
    state_path: PathBuf,
}

enum CliCommand {
    Run(Cli),
    Help,
}

#[derive(Debug, Error)]
enum CliError {
    #[error("argument {0} is missing a file path")]
    MissingValue(&'static str),
    #[error("unknown argument: {0}")]
    UnknownArgument(String),
}

#[derive(Debug, Error)]
enum MainError {
    #[error(transparent)]
    Config(#[from] config::ConfigError),
    #[error(transparent)]
    State(#[from] state::StateFailure),
    #[error(transparent)]
    GitHub(#[from] github::GitHubError),
    #[error(transparent)]
    App(#[from] app::AppError),
}

#[tokio::main]
async fn main() -> ExitCode {
    let _ = dotenvy::dotenv();
    let translator = i18n::initialize();
    let cli = match parse_cli() {
        Ok(CliCommand::Run(cli)) => cli,
        Ok(CliCommand::Help) => {
            print_help(translator);
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            eprintln!("{}", translator.cli_failure(&error));
            eprintln!("{}", translator.help_hint());
            return ExitCode::FAILURE;
        }
    };

    match execute(&cli).await {
        Ok(summary) => {
            print_summary(translator, &summary);
            exit_code(&summary)
        }
        Err(error) => {
            eprintln!("{}", translator.main_failure(&error));
            ExitCode::FAILURE
        }
    }
}

async fn execute(cli: &Cli) -> Result<RunSummary, MainError> {
    let translator = i18n::current();
    println!("{}", translator.console(i18n::ConsoleMessage::RunStarted));
    println!(
        "{}",
        translator.console(i18n::ConsoleMessage::ConfigPath(&cli.config_path))
    );
    println!(
        "{}",
        translator.console(i18n::ConsoleMessage::StatePath(&cli.state_path))
    );

    let settings = config::load(&cli.config_path)?;
    println!(
        "{}",
        translator.console(i18n::ConsoleMessage::ConfigLoaded {
            schema_version: settings.schema_version,
            repository_count: settings.repositories.len(),
        })
    );

    let loaded = ProgramStateSession::load_state_file_or_initialize_default(
        &cli.state_path,
        settings.group_id,
    )?;
    println!(
        "{}",
        translator.console(i18n::ConsoleMessage::StateLoaded {
            code: loaded.code.as_str(),
        })
    );
    let mut session = loaded.value;
    let github = GitHubClient::new(settings.github_token.clone())?;
    let telegram = TelegramClient::new(settings.bot_token.clone(), settings.group_id);

    let telegram_preparation = app::prepare_telegram_supergroup_for_configured_repositories(
        &settings,
        &mut session,
        &github,
        &telegram,
    )
    .await
    .map_err(app::AppError::from)?;
    println!(
        "{}",
        translator.console(i18n::ConsoleMessage::TelegramPrepared {
            code: telegram_preparation.status_code.as_str(),
            checked: telegram_preparation.checked_topic_count,
            created: telegram_preparation.created_topic_count,
            recreated: telegram_preparation.recreated_topic_count,
            renamed: telegram_preparation.renamed_topic_count,
            locked: telegram_preparation.locked_topic_count,
        })
    );

    let summary = app::run(
        &settings,
        &mut session,
        &telegram_preparation,
        &github,
        &telegram,
    )
    .await;
    let saved = session.save_state_file_if_changed()?;
    println!(
        "{}",
        translator.console(i18n::ConsoleMessage::StateFileSaved {
            changed: saved.value.changed,
            code: saved.code.as_str(),
        })
    );
    Ok(summary)
}

fn parse_cli() -> Result<CliCommand, CliError> {
    let mut config_path = PathBuf::from(DEFAULT_CONFIG_PATH);
    let mut state_path = PathBuf::from(DEFAULT_STATE_PATH);
    let mut arguments = env::args_os().skip(1);

    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("-h" | "--help") => return Ok(CliCommand::Help),
            Some("-c" | "--config") => {
                config_path = arguments
                    .next()
                    .map(PathBuf::from)
                    .ok_or(CliError::MissingValue("--config"))?;
            }
            Some("-s" | "--state") => {
                state_path = arguments
                    .next()
                    .map(PathBuf::from)
                    .ok_or(CliError::MissingValue("--state"))?;
            }
            _ => {
                return Err(CliError::UnknownArgument(
                    argument.to_string_lossy().into_owned(),
                ));
            }
        }
    }

    Ok(CliCommand::Run(Cli {
        config_path,
        state_path,
    }))
}

fn print_help(translator: i18n::Translator) {
    println!("{}", translator.help());
}

fn exit_code(summary: &RunSummary) -> ExitCode {
    if summary.has_failures() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn print_summary(translator: i18n::Translator, summary: &RunSummary) {
    println!("{}", translator.console(i18n::ConsoleMessage::RunSummary));
    for result in &summary.repositories {
        match &result.error {
            Some(error) => eprintln!(
                "{}",
                translator.console(i18n::ConsoleMessage::RepositorySummaryFailed {
                    repository: &result.repository,
                    error,
                })
            ),
            None => println!(
                "{}",
                translator.console(i18n::ConsoleMessage::RepositorySummarySucceeded {
                    repository: &result.repository,
                    published: result.published,
                    changed: result.changed,
                })
            ),
        }
    }

    println!(
        "{}",
        translator.console(i18n::ConsoleMessage::RunSummaryTotal {
            published: summary.published_count(),
            changed: summary.has_changes(),
        })
    );
}
