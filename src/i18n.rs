use std::{env, path::Path, sync::OnceLock};

use crate::{
    CliError, MainError,
    actions::WorkflowRunUpdateKind,
    app::{AppError, TelegramGroupPreparationError},
    commit::CommitError,
    config::ConfigError,
    github::GitHubError,
    release::ReleaseTagError,
    state::{StateError, StateFailure},
    telegram::TelegramError,
    telegram_report::{TelegramReportPublishError, TelegramReportPublishFailure},
};

static LANGUAGE: OnceLock<Language> = OnceLock::new();

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Language {
    #[default]
    English,
    Chinese,
    Japanese,
}

impl Language {
    fn from_environment() -> Self {
        let lc_all = env::var("LC_ALL").ok();
        let lc_messages = env::var("LC_MESSAGES").ok();
        let lang = env::var("LANG").ok();
        Self::from_locale_values(lc_all.as_deref(), lc_messages.as_deref(), lang.as_deref())
    }

    pub(crate) fn from_locale_values(
        lc_all: Option<&str>,
        lc_messages: Option<&str>,
        lang: Option<&str>,
    ) -> Self {
        [lc_all, lc_messages, lang]
            .into_iter()
            .flatten()
            .find(|value| !value.trim().is_empty())
            .and_then(parse_locale)
            .unwrap_or_default()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Translator {
    language: Language,
}

impl Translator {
    pub const fn new(language: Language) -> Self {
        Self { language }
    }

    pub const fn language(self) -> Language {
        self.language
    }

    pub fn text(self, text: Text) -> &'static str {
        match (self.language, text) {
            (Language::English, Text::Unknown) => "Unknown",
            (Language::English, Text::NotPublished) => "Not published",
            (Language::English, Text::CommitTitle) => "[Commit]",
            (Language::English, Text::ReleaseTitle) => "[Release]",
            (Language::English, Text::ReleaseUpdatedTitle) => "[Release Updated]",
            (Language::English, Text::ActionsTitle) => "[Actions]",
            (Language::English, Text::Branch) => "Branch",
            (Language::English, Text::Commit) => "Commit",
            (Language::English, Text::Author) => "Author",
            (Language::English, Text::Time) => "Time",
            (Language::English, Text::Tag) => "Tag",
            (Language::English, Text::Name) => "Name",
            (Language::English, Text::ReleaseId) => "Release ID",
            (Language::English, Text::Status) => "Status",
            (Language::English, Text::CreatedAt) => "Created at",
            (Language::English, Text::PublishedAt) => "Published at",
            (Language::English, Text::UpdatedAt) => "Updated at",
            (Language::English, Text::Target) => "Target",
            (Language::English, Text::Attachments) => "Attachments",
            (Language::English, Text::AttachmentList) => "Attachment list",
            (Language::English, Text::Workflow) => "Workflow",
            (Language::English, Text::Result) => "Result",
            (Language::English, Text::Run) => "Run",
            (Language::English, Text::Draft) => "Draft",
            (Language::English, Text::Prerelease) => "Prerelease",
            (Language::English, Text::Stable) => "Stable",
            (Language::English, Text::Immutable) => "Immutable",
            (Language::English, Text::Attachment) => "attachment",
            (Language::English, Text::AttachmentCountSuffix) => "",

            (Language::Chinese, Text::Unknown) => "未知",
            (Language::Chinese, Text::NotPublished) => "未发布",
            (Language::Chinese, Text::CommitTitle) => "[Commit]",
            (Language::Chinese, Text::ReleaseTitle) => "[Release]",
            (Language::Chinese, Text::ReleaseUpdatedTitle) => "[Release 更新]",
            (Language::Chinese, Text::ActionsTitle) => "[Actions]",
            (Language::Chinese, Text::Branch) => "分支",
            (Language::Chinese, Text::Commit) => "提交",
            (Language::Chinese, Text::Author) => "作者",
            (Language::Chinese, Text::Time) => "时间",
            (Language::Chinese, Text::Tag) => "Tag",
            (Language::Chinese, Text::Name) => "名称",
            (Language::Chinese, Text::ReleaseId) => "Release ID",
            (Language::Chinese, Text::Status) => "状态",
            (Language::Chinese, Text::CreatedAt) => "创建时间",
            (Language::Chinese, Text::PublishedAt) => "发布时间",
            (Language::Chinese, Text::UpdatedAt) => "更新时间",
            (Language::Chinese, Text::Target) => "目标",
            (Language::Chinese, Text::Attachments) => "附件",
            (Language::Chinese, Text::AttachmentList) => "附件列表",
            (Language::Chinese, Text::Workflow) => "工作流",
            (Language::Chinese, Text::Result) => "结果",
            (Language::Chinese, Text::Run) => "运行",
            (Language::Chinese, Text::Draft) => "草稿",
            (Language::Chinese, Text::Prerelease) => "预发布",
            (Language::Chinese, Text::Stable) => "稳定版",
            (Language::Chinese, Text::Immutable) => "不可变",
            (Language::Chinese, Text::Attachment) => "附件",
            (Language::Chinese, Text::AttachmentCountSuffix) => " 个",

            (Language::Japanese, Text::Unknown) => "不明",
            (Language::Japanese, Text::NotPublished) => "未公開",
            (Language::Japanese, Text::CommitTitle) => "[Commit]",
            (Language::Japanese, Text::ReleaseTitle) => "[Release]",
            (Language::Japanese, Text::ReleaseUpdatedTitle) => "[Release 更新]",
            (Language::Japanese, Text::ActionsTitle) => "[Actions]",
            (Language::Japanese, Text::Branch) => "ブランチ",
            (Language::Japanese, Text::Commit) => "コミット",
            (Language::Japanese, Text::Author) => "作成者",
            (Language::Japanese, Text::Time) => "時刻",
            (Language::Japanese, Text::Tag) => "タグ",
            (Language::Japanese, Text::Name) => "名前",
            (Language::Japanese, Text::ReleaseId) => "Release ID",
            (Language::Japanese, Text::Status) => "状態",
            (Language::Japanese, Text::CreatedAt) => "作成日時",
            (Language::Japanese, Text::PublishedAt) => "公開日時",
            (Language::Japanese, Text::UpdatedAt) => "更新日時",
            (Language::Japanese, Text::Target) => "対象",
            (Language::Japanese, Text::Attachments) => "添付ファイル",
            (Language::Japanese, Text::AttachmentList) => "添付ファイル一覧",
            (Language::Japanese, Text::Workflow) => "ワークフロー",
            (Language::Japanese, Text::Result) => "結果",
            (Language::Japanese, Text::Run) => "実行",
            (Language::Japanese, Text::Draft) => "下書き",
            (Language::Japanese, Text::Prerelease) => "プレリリース",
            (Language::Japanese, Text::Stable) => "安定版",
            (Language::Japanese, Text::Immutable) => "変更不可",
            (Language::Japanese, Text::Attachment) => "添付ファイル",
            (Language::Japanese, Text::AttachmentCountSuffix) => " 件",
        }
    }

    pub fn help(self) -> &'static str {
        match self.language {
            Language::English => {
                "GH2TG - one-shot GitHub to Telegram poller\n\n\
Usage:\n  gh2tg [options]\n\n\
Options:\n  -c, --config <path>  Configuration file path (default: config.json)\n  -s, --state <path>   State file path (default: state.json)\n  -h, --help           Show help\n\n\
Environment:\n  LC_ALL / LC_MESSAGES / LANG  Output language: en, zh, or ja (default: en)"
            }
            Language::Chinese => {
                "GH2TG - 单次 GitHub 到 Telegram 轮询器\n\n\
用法:\n  gh2tg [选项]\n\n\
选项:\n  -c, --config <路径>  配置文件路径，默认 config.json\n  -s, --state <路径>   状态文件路径，默认 state.json\n  -h, --help           显示帮助\n\n\
环境变量:\n  LC_ALL / LC_MESSAGES / LANG  输出语言：en、zh 或 ja，默认 en"
            }
            Language::Japanese => {
                "GH2TG - GitHub から Telegram への単発ポーラー\n\n\
使用方法:\n  gh2tg [オプション]\n\n\
オプション:\n  -c, --config <パス>  設定ファイルのパス（既定: config.json）\n  -s, --state <パス>   状態ファイルのパス（既定: state.json）\n  -h, --help           ヘルプを表示\n\n\
環境変数:\n  LC_ALL / LC_MESSAGES / LANG  出力言語: en、zh、ja（既定: en）"
            }
        }
    }

    pub fn console(self, message: ConsoleMessage<'_>) -> String {
        match self.language {
            Language::English => console_english(message),
            Language::Chinese => console_chinese(message),
            Language::Japanese => console_japanese(message),
        }
    }

    pub(crate) fn cli_failure(self, error: &CliError) -> String {
        let detail = self.cli_error(error);
        match self.language {
            Language::English => format!("GH2TG argument error: {detail}"),
            Language::Chinese => format!("GH2TG 参数错误: {detail}"),
            Language::Japanese => format!("GH2TG 引数エラー: {detail}"),
        }
    }

    pub fn help_hint(self) -> &'static str {
        match self.language {
            Language::English => "Use --help to view available options",
            Language::Chinese => "使用 --help 查看可用参数",
            Language::Japanese => "利用可能なオプションは --help で確認できます",
        }
    }

    pub(crate) fn main_failure(self, error: &MainError) -> String {
        let (code, detail) = match error {
            MainError::App(AppError::TelegramGroupPreparation(error)) => (
                Some(error.status_code().as_str()),
                self.telegram_group_preparation_error(error),
            ),
            MainError::State(failure) => (
                Some(failure.code.as_str()),
                self.state_error(&failure.error),
            ),
            _ => (None, self.main_error(error)),
        };
        let code = code.map(|code| format!(" [{code}]")).unwrap_or_default();
        match self.language {
            Language::English => format!("GH2TG execution failed{code}: {detail}"),
            Language::Chinese => format!("GH2TG 执行失败{code}: {detail}"),
            Language::Japanese => format!("GH2TG の実行に失敗しました{code}: {detail}"),
        }
    }

    pub fn app_error(self, error: &AppError) -> String {
        match error {
            AppError::Commit(error) => self.commit_error(error),
            AppError::GitHub(error) => self.github_error(error),
            AppError::Telegram(error) => self.telegram_error(error),
            AppError::State(failure) => self.state_failure(failure),
            AppError::TelegramGroupPreparation(error) => {
                self.telegram_group_preparation_error(error)
            }
            AppError::TelegramReport(failure) => self.telegram_report_failure(failure),
        }
    }

    fn main_error(self, error: &MainError) -> String {
        match error {
            MainError::Config(error) => self.config_error(error),
            MainError::State(failure) => self.state_failure(failure),
            MainError::GitHub(error) => self.github_error(error),
            MainError::App(error) => self.app_error(error),
        }
    }

    fn cli_error(self, error: &CliError) -> String {
        if self.language == Language::English {
            return error.to_string();
        }
        match (self.language, error) {
            (Language::Chinese, CliError::MissingValue(argument)) => {
                format!("参数 {argument} 缺少文件路径")
            }
            (Language::Chinese, CliError::UnknownArgument(argument)) => {
                format!("未知参数: {argument}")
            }
            (Language::Japanese, CliError::MissingValue(argument)) => {
                format!("引数 {argument} にファイルパスがありません")
            }
            (Language::Japanese, CliError::UnknownArgument(argument)) => {
                format!("不明な引数: {argument}")
            }
            (Language::English, _) => unreachable!(),
        }
    }

    fn config_error(self, error: &ConfigError) -> String {
        if self.language == Language::English {
            return error.to_string();
        }
        match (self.language, error) {
            (Language::Chinese, ConfigError::Dotenv(source)) => {
                format!("无法加载 .env: {source}")
            }
            (Language::Chinese, ConfigError::MissingEnvironment(name)) => {
                format!("缺少环境变量 {name}")
            }
            (Language::Chinese, ConfigError::InvalidEnvironment(name)) => {
                format!("环境变量 {name} 不是有效的 Unicode")
            }
            (Language::Chinese, ConfigError::InvalidGroupId) => {
                "GH2TG_GROUP_ID 必须是以 -100 开头的有效整数".to_owned()
            }
            (Language::Chinese, ConfigError::ReadConfig { path, source }) => {
                format!("无法读取配置文件 {}: {source}", path.display())
            }
            (Language::Chinese, ConfigError::ParseConfig { path, source }) => {
                format!("无法解析配置文件 {}: {source}", path.display())
            }
            (Language::Chinese, ConfigError::UnsupportedSchemaVersion(version)) => {
                format!("不支持的 schema_version: {version}")
            }
            (Language::Chinese, ConfigError::InvalidRepositoryName(repository)) => {
                format!("无效的仓库名 {repository}，必须使用 owner/repo 格式")
            }
            (Language::Chinese, ConfigError::InvalidReleaseTag { repository, source }) => format!(
                "仓库 {repository} 的 Release Tag 无效: {}",
                self.release_tag_error(source)
            ),
            (Language::Chinese, ConfigError::EmptyField { repository, field }) => {
                format!("仓库 {repository} 的 {field} 不能为空")
            }
            (
                Language::Chinese,
                ConfigError::InvalidRegex {
                    repository,
                    field,
                    source,
                },
            ) => format!("仓库 {repository} 的正则 {field} 无效: {source}"),
            (
                Language::Chinese,
                ConfigError::DuplicateValue {
                    repository,
                    field,
                    value,
                },
            ) => format!("仓库 {repository} 的 {field} 包含重复值: {value}"),
            (
                Language::Chinese,
                ConfigError::DuplicateActionTarget {
                    repository,
                    workflow_file,
                    branch,
                },
            ) => format!("仓库 {repository} 重复配置 Actions 目标 {workflow_file} / {branch}"),

            (Language::Japanese, ConfigError::Dotenv(source)) => {
                format!(".env を読み込めません: {source}")
            }
            (Language::Japanese, ConfigError::MissingEnvironment(name)) => {
                format!("環境変数 {name} が設定されていません")
            }
            (Language::Japanese, ConfigError::InvalidEnvironment(name)) => {
                format!("環境変数 {name} は有効な Unicode ではありません")
            }
            (Language::Japanese, ConfigError::InvalidGroupId) => {
                "GH2TG_GROUP_ID は -100 で始まる有効な整数である必要があります".to_owned()
            }
            (Language::Japanese, ConfigError::ReadConfig { path, source }) => {
                format!("設定ファイル {} を読み込めません: {source}", path.display())
            }
            (Language::Japanese, ConfigError::ParseConfig { path, source }) => {
                format!("設定ファイル {} を解析できません: {source}", path.display())
            }
            (Language::Japanese, ConfigError::UnsupportedSchemaVersion(version)) => {
                format!("未対応の schema_version: {version}")
            }
            (Language::Japanese, ConfigError::InvalidRepositoryName(repository)) => {
                format!("無効なリポジトリ名 {repository}: owner/repo 形式で指定してください")
            }
            (Language::Japanese, ConfigError::InvalidReleaseTag { repository, source }) => format!(
                "リポジトリ {repository} の Release Tag が無効です: {}",
                self.release_tag_error(source)
            ),
            (Language::Japanese, ConfigError::EmptyField { repository, field }) => {
                format!("リポジトリ {repository} の {field} は空にできません")
            }
            (
                Language::Japanese,
                ConfigError::InvalidRegex {
                    repository,
                    field,
                    source,
                },
            ) => format!("リポジトリ {repository} の正規表現 {field} が無効です: {source}"),
            (
                Language::Japanese,
                ConfigError::DuplicateValue {
                    repository,
                    field,
                    value,
                },
            ) => format!("リポジトリ {repository} の {field} に重複した値があります: {value}"),
            (
                Language::Japanese,
                ConfigError::DuplicateActionTarget {
                    repository,
                    workflow_file,
                    branch,
                },
            ) => format!(
                "リポジトリ {repository} の Actions 対象が重複しています: {workflow_file} / {branch}"
            ),
            (Language::English, _) => unreachable!(),
        }
    }

    fn release_tag_error(self, error: &ReleaseTagError) -> String {
        match self.language {
            Language::English => error.to_string(),
            Language::Chinese => "Release Tag 不能为空".to_owned(),
            Language::Japanese => "Release Tag は空にできません".to_owned(),
        }
    }

    fn github_error(self, error: &GitHubError) -> String {
        if self.language == Language::English {
            return error.to_string();
        }
        match (self.language, error) {
            (Language::Chinese, GitHubError::InvalidToken(source)) => {
                format!("GitHub Token 无法作为 HTTP Header 使用: {source}")
            }
            (Language::Chinese, GitHubError::InvalidEtag(source)) => {
                format!("状态中的 GitHub ETag 无法作为 HTTP Header 使用: {source}")
            }
            (Language::Chinese, GitHubError::BuildClient(source)) => {
                format!("无法创建 GitHub HTTP 客户端: {source}")
            }
            (Language::Chinese, GitHubError::InvalidUrl(source)) => {
                format!("无效的 GitHub API 地址: {source}")
            }
            (Language::Chinese, GitHubError::InvalidRepository(repository)) => {
                format!("无效的仓库名 {repository}，必须使用 owner/repo 格式")
            }
            (Language::Chinese, GitHubError::InvalidRepositoryId) => {
                "GitHub 返回了无效的 Repository ID".to_owned()
            }
            (Language::Chinese, GitHubError::CannotBuildUrl) => {
                "无法构造 GitHub API 地址".to_owned()
            }
            (Language::Chinese, GitHubError::Request { url, source }) => {
                format!("GitHub 请求 {url} 失败: {source}")
            }
            (Language::Chinese, GitHubError::Decode { url, source }) => {
                format!("无法解析 GitHub 响应 {url}: {source}")
            }
            (Language::Chinese, GitHubError::Http { status, message }) => {
                format!("GitHub 返回 HTTP {status}: {message}")
            }
            (Language::Chinese, GitHubError::CreateDownload { path, source }) => {
                format!("无法创建下载文件 {}: {source}", path.display())
            }
            (Language::Chinese, GitHubError::WriteDownload { path, source }) => {
                format!("无法写入下载文件 {}: {source}", path.display())
            }
            (Language::Chinese, GitHubError::InvalidFileName(file_name)) => {
                format!("远端文件名不是安全的单个文件名: {file_name}")
            }
            (Language::Chinese, GitHubError::WorkflowNotFound { workflow_file }) => {
                format!("Workflow {workflow_file} 不存在")
            }
            (Language::Chinese, GitHubError::WorkflowRunNotFound { run_id }) => {
                format!("Workflow Run {run_id} 不存在")
            }

            (Language::Japanese, GitHubError::InvalidToken(source)) => {
                format!("GitHub Token を HTTP Header として使用できません: {source}")
            }
            (Language::Japanese, GitHubError::InvalidEtag(source)) => {
                format!("状態内の GitHub ETag を HTTP Header として使用できません: {source}")
            }
            (Language::Japanese, GitHubError::BuildClient(source)) => {
                format!("GitHub HTTP クライアントを作成できません: {source}")
            }
            (Language::Japanese, GitHubError::InvalidUrl(source)) => {
                format!("GitHub API の URL が無効です: {source}")
            }
            (Language::Japanese, GitHubError::InvalidRepository(repository)) => {
                format!("無効なリポジトリ名 {repository}: owner/repo 形式で指定してください")
            }
            (Language::Japanese, GitHubError::InvalidRepositoryId) => {
                "GitHub が無効な Repository ID を返しました".to_owned()
            }
            (Language::Japanese, GitHubError::CannotBuildUrl) => {
                "GitHub API の URL を構築できません".to_owned()
            }
            (Language::Japanese, GitHubError::Request { url, source }) => {
                format!("GitHub リクエスト {url} に失敗しました: {source}")
            }
            (Language::Japanese, GitHubError::Decode { url, source }) => {
                format!("GitHub レスポンス {url} を解析できません: {source}")
            }
            (Language::Japanese, GitHubError::Http { status, message }) => {
                format!("GitHub が HTTP {status} を返しました: {message}")
            }
            (Language::Japanese, GitHubError::CreateDownload { path, source }) => {
                format!(
                    "ダウンロードファイル {} を作成できません: {source}",
                    path.display()
                )
            }
            (Language::Japanese, GitHubError::WriteDownload { path, source }) => {
                format!(
                    "ダウンロードファイル {} に書き込めません: {source}",
                    path.display()
                )
            }
            (Language::Japanese, GitHubError::InvalidFileName(file_name)) => {
                format!("リモートファイル名は安全な単一ファイル名ではありません: {file_name}")
            }
            (Language::Japanese, GitHubError::WorkflowNotFound { workflow_file }) => {
                format!("Workflow {workflow_file} が存在しません")
            }
            (Language::Japanese, GitHubError::WorkflowRunNotFound { run_id }) => {
                format!("Workflow Run {run_id} が存在しません")
            }
            (Language::English, _) => unreachable!(),
        }
    }

    fn telegram_error(self, error: &TelegramError) -> String {
        match (self.language, error) {
            (Language::English, _) => error.to_string(),
            (Language::Chinese, TelegramError::Request(source)) => {
                format!("Telegram 请求失败: {source}")
            }
            (Language::Japanese, TelegramError::Request(source)) => {
                format!("Telegram リクエストに失敗しました: {source}")
            }
        }
    }

    fn state_failure(self, failure: &StateFailure) -> String {
        format!(
            "[{}] {}",
            failure.code.as_str(),
            self.state_error(&failure.error)
        )
    }

    fn state_error(self, error: &StateError) -> String {
        if self.language == Language::English {
            return error.to_string();
        }
        match (self.language, error) {
            (Language::Chinese, StateError::ReadIo { path, source }) => {
                format!("无法读取状态文件 {}: {source}", path.display())
            }
            (Language::Chinese, StateError::Parse { path, source }) => {
                format!("无法解析状态文件 {}: {source}", path.display())
            }
            (Language::Chinese, StateError::Serialize(source)) => {
                format!("无法序列化状态文件: {source}")
            }
            (Language::Chinese, StateError::WriteIo { path, source }) => {
                format!("无法写入状态文件 {}: {source}", path.display())
            }
            (Language::Chinese, StateError::UnsupportedSchemaVersion(version)) => {
                format!("不支持的状态 schema_version: {version}")
            }
            (
                Language::Chinese,
                StateError::GroupMismatch {
                    state_group_id,
                    configured_group_id,
                },
            ) => {
                format!(
                    "状态文件属于群组 {state_group_id}，当前 GH2TG_GROUP_ID 为 {configured_group_id}"
                )
            }
            (
                Language::Chinese,
                StateError::InvalidTopicId {
                    repository_id,
                    topic_id,
                },
            ) => format!("状态记录 {repository_id} 的话题 ID 无效: {topic_id}"),
            (Language::Chinese, StateError::InvalidTopicName { repository_id }) => {
                format!("状态记录 {repository_id} 的话题名称为空")
            }
            (Language::Chinese, StateError::RepositoryNotTracked { repository_id }) => {
                format!("状态记录中不存在仓库 {repository_id}")
            }
            (Language::Chinese, StateError::EmptyBranch { repository_id }) => {
                format!("仓库 {repository_id} 的分支名称为空")
            }
            (Language::Chinese, StateError::EmptyCommitSha { repository_id }) => {
                format!("仓库 {repository_id} 的 commit SHA 为空")
            }
            (Language::Chinese, StateError::EmptyWorkflowFile { repository_id }) => {
                format!("仓库 {repository_id} 的工作流文件名称为空")
            }

            (Language::Japanese, StateError::ReadIo { path, source }) => {
                format!("状態ファイル {} を読み込めません: {source}", path.display())
            }
            (Language::Japanese, StateError::Parse { path, source }) => {
                format!("状態ファイル {} を解析できません: {source}", path.display())
            }
            (Language::Japanese, StateError::Serialize(source)) => {
                format!("状態ファイルをシリアライズできません: {source}")
            }
            (Language::Japanese, StateError::WriteIo { path, source }) => {
                format!("状態ファイル {} に書き込めません: {source}", path.display())
            }
            (Language::Japanese, StateError::UnsupportedSchemaVersion(version)) => {
                format!("未対応の状態 schema_version: {version}")
            }
            (
                Language::Japanese,
                StateError::GroupMismatch {
                    state_group_id,
                    configured_group_id,
                },
            ) => format!(
                "状態ファイルのグループは {state_group_id} ですが、現在の GH2TG_GROUP_ID は {configured_group_id} です"
            ),
            (
                Language::Japanese,
                StateError::InvalidTopicId {
                    repository_id,
                    topic_id,
                },
            ) => format!("状態記録 {repository_id} のトピック ID が無効です: {topic_id}"),
            (Language::Japanese, StateError::InvalidTopicName { repository_id }) => {
                format!("状態記録 {repository_id} のトピック名が空です")
            }
            (Language::Japanese, StateError::RepositoryNotTracked { repository_id }) => {
                format!("リポジトリ {repository_id} は状態記録に存在しません")
            }
            (Language::Japanese, StateError::EmptyBranch { repository_id }) => {
                format!("リポジトリ {repository_id} のブランチ名が空です")
            }
            (Language::Japanese, StateError::EmptyCommitSha { repository_id }) => {
                format!("リポジトリ {repository_id} の commit SHA が空です")
            }
            (Language::Japanese, StateError::EmptyWorkflowFile { repository_id }) => {
                format!("リポジトリ {repository_id} のワークフローファイル名が空です")
            }
            (Language::English, _) => unreachable!(),
        }
    }

    fn commit_error(self, error: &CommitError) -> String {
        match (self.language, error) {
            (_, CommitError::GitHub(error)) => self.github_error(error),
            (Language::English, _) => error.to_string(),
            (Language::Chinese, CommitError::DuplicateCommitSha { branch, sha }) => {
                format!("分支 {branch} 的 Commit 图包含重复 SHA: {sha}")
            }
            (Language::Chinese, CommitError::InvalidCommitGraph { branch }) => {
                format!("分支 {branch} 的 Commit 图不是有效的有向无环图")
            }
            (Language::Japanese, CommitError::DuplicateCommitSha { branch, sha }) => {
                format!("ブランチ {branch} の Commit グラフに重複した SHA があります: {sha}")
            }
            (Language::Japanese, CommitError::InvalidCommitGraph { branch }) => {
                format!("ブランチ {branch} の Commit グラフは有効な有向非巡回グラフではありません")
            }
        }
    }

    fn telegram_group_preparation_error(self, error: &TelegramGroupPreparationError) -> String {
        if self.language == Language::English {
            return match error {
                TelegramGroupPreparationError::State(failure) => self.state_error(&failure.error),
                TelegramGroupPreparationError::RepositoryIdLookupFailed { repository, source } => {
                    format!(
                        "failed to resolve configured repository {repository}: {}",
                        self.github_error(source)
                    )
                }
                TelegramGroupPreparationError::GroupLookupFailed { source } => format!(
                    "failed to inspect the Telegram group: {}",
                    self.telegram_error(source)
                ),
                TelegramGroupPreparationError::BotIdentityLookupFailed { source } => format!(
                    "failed to verify the Telegram bot identity: {}",
                    self.telegram_error(source)
                ),
                TelegramGroupPreparationError::BotMembershipLookupFailed { source } => format!(
                    "failed to inspect the Telegram bot membership: {}",
                    self.telegram_error(source)
                ),
                TelegramGroupPreparationError::TopicOperationFailed {
                    repository_id,
                    topic_id,
                    source,
                } => format!(
                    "failed to prepare topic {topic_id:?} for repository {repository_id}: {}",
                    self.telegram_error(source)
                ),
                _ => error.to_string(),
            };
        }
        match (self.language, error) {
            (Language::Chinese, TelegramGroupPreparationError::State(failure)) => {
                self.state_error(&failure.error)
            }
            (Language::Chinese, TelegramGroupPreparationError::GroupLookupFailed { source }) => {
                format!("无法检查 Telegram 群组: {}", self.telegram_error(source))
            }
            (Language::Chinese, TelegramGroupPreparationError::NotSupergroup) => {
                "目标 Telegram 聊天不是超级群组".to_owned()
            }
            (Language::Chinese, TelegramGroupPreparationError::ForumTopicsDisabled) => {
                "目标 Telegram 超级群组未开启话题".to_owned()
            }
            (
                Language::Chinese,
                TelegramGroupPreparationError::StateReferencesGeneralTopic { repository_id },
            ) => format!(
                "状态记录 {repository_id} 使用了 Telegram General 话题 ID=1，拒绝重命名 General"
            ),
            (
                Language::Chinese,
                TelegramGroupPreparationError::InvalidStateTopicId {
                    repository_id,
                    topic_id,
                },
            ) => format!("状态记录 {repository_id} 的话题 ID 无效: {topic_id}"),
            (
                Language::Chinese,
                TelegramGroupPreparationError::InvalidStateTopicName { repository_id },
            ) => format!("状态记录 {repository_id} 的话题名称为空"),
            (
                Language::Chinese,
                TelegramGroupPreparationError::InvalidConfiguredTopicName { repository },
            ) => format!("配置仓库 {repository} 的话题名称为空"),
            (
                Language::Chinese,
                TelegramGroupPreparationError::DuplicateConfiguredRepositoryId {
                    repository,
                    conflicting_repository,
                    repository_id,
                },
            ) => format!(
                "配置仓库 {repository} 与 {conflicting_repository} 解析到了相同的 GitHub repository ID={repository_id}"
            ),
            (
                Language::Chinese,
                TelegramGroupPreparationError::StateGroupMismatch {
                    state_group_id,
                    configured_group_id,
                },
            ) => format!(
                "状态文件所属群组 {state_group_id} 与 TelegramClient 配置的群组 {configured_group_id} 不一致"
            ),
            (
                Language::Chinese,
                TelegramGroupPreparationError::BotIdentityLookupFailed { source },
            ) => format!(
                "无法验证 Telegram 机器人身份: {}",
                self.telegram_error(source)
            ),
            (
                Language::Chinese,
                TelegramGroupPreparationError::BotMembershipLookupFailed { source },
            ) => format!(
                "无法检查 Telegram 机器人在群组中的成员状态: {}",
                self.telegram_error(source)
            ),
            (Language::Chinese, TelegramGroupPreparationError::BotNotMember) => {
                "Telegram 机器人不在目标群组中".to_owned()
            }
            (Language::Chinese, TelegramGroupPreparationError::BotNotAdministrator) => {
                "Telegram 机器人不是目标群组管理员".to_owned()
            }
            (Language::Chinese, TelegramGroupPreparationError::BotCannotManageTopics) => {
                "Telegram 机器人缺少 can_manage_topics 权限".to_owned()
            }
            (
                Language::Chinese,
                TelegramGroupPreparationError::RepositoryIdLookupFailed { repository, source },
            ) => format!(
                "无法解析配置仓库 {repository}: {}",
                self.github_error(source)
            ),
            (
                Language::Chinese,
                TelegramGroupPreparationError::TopicOperationFailed {
                    repository_id,
                    topic_id,
                    source,
                },
            ) => format!(
                "无法准备仓库 {repository_id} 的话题 {topic_id:?}: {}",
                self.telegram_error(source)
            ),

            (Language::Japanese, TelegramGroupPreparationError::State(failure)) => {
                self.state_error(&failure.error)
            }
            (Language::Japanese, TelegramGroupPreparationError::GroupLookupFailed { source }) => {
                format!(
                    "Telegram グループを確認できません: {}",
                    self.telegram_error(source)
                )
            }
            (Language::Japanese, TelegramGroupPreparationError::NotSupergroup) => {
                "対象の Telegram チャットはスーパーグループではありません".to_owned()
            }
            (Language::Japanese, TelegramGroupPreparationError::ForumTopicsDisabled) => {
                "対象の Telegram スーパーグループでトピックが有効になっていません".to_owned()
            }
            (
                Language::Japanese,
                TelegramGroupPreparationError::StateReferencesGeneralTopic { repository_id },
            ) => format!(
                "状態記録 {repository_id} が Telegram General トピック ID=1 を使用しているため、General の名前変更を拒否しました"
            ),
            (
                Language::Japanese,
                TelegramGroupPreparationError::InvalidStateTopicId {
                    repository_id,
                    topic_id,
                },
            ) => format!("状態記録 {repository_id} のトピック ID が無効です: {topic_id}"),
            (
                Language::Japanese,
                TelegramGroupPreparationError::InvalidStateTopicName { repository_id },
            ) => format!("状態記録 {repository_id} のトピック名が空です"),
            (
                Language::Japanese,
                TelegramGroupPreparationError::InvalidConfiguredTopicName { repository },
            ) => format!("設定リポジトリ {repository} のトピック名が空です"),
            (
                Language::Japanese,
                TelegramGroupPreparationError::DuplicateConfiguredRepositoryId {
                    repository,
                    conflicting_repository,
                    repository_id,
                },
            ) => format!(
                "設定リポジトリ {repository} と {conflicting_repository} が同じ GitHub repository ID={repository_id} に解決されました"
            ),
            (
                Language::Japanese,
                TelegramGroupPreparationError::StateGroupMismatch {
                    state_group_id,
                    configured_group_id,
                },
            ) => format!(
                "状態ファイルのグループ {state_group_id} と TelegramClient の設定グループ {configured_group_id} が一致しません"
            ),
            (
                Language::Japanese,
                TelegramGroupPreparationError::BotIdentityLookupFailed { source },
            ) => format!(
                "Telegram Bot の識別情報を確認できません: {}",
                self.telegram_error(source)
            ),
            (
                Language::Japanese,
                TelegramGroupPreparationError::BotMembershipLookupFailed { source },
            ) => format!(
                "Telegram グループ内の Bot メンバー状態を確認できません: {}",
                self.telegram_error(source)
            ),
            (Language::Japanese, TelegramGroupPreparationError::BotNotMember) => {
                "Telegram Bot は対象グループのメンバーではありません".to_owned()
            }
            (Language::Japanese, TelegramGroupPreparationError::BotNotAdministrator) => {
                "Telegram Bot は対象グループの管理者ではありません".to_owned()
            }
            (Language::Japanese, TelegramGroupPreparationError::BotCannotManageTopics) => {
                "Telegram Bot に can_manage_topics 権限がありません".to_owned()
            }
            (
                Language::Japanese,
                TelegramGroupPreparationError::RepositoryIdLookupFailed { repository, source },
            ) => format!(
                "設定リポジトリ {repository} を解決できません: {}",
                self.github_error(source)
            ),
            (
                Language::Japanese,
                TelegramGroupPreparationError::TopicOperationFailed {
                    repository_id,
                    topic_id,
                    source,
                },
            ) => format!(
                "リポジトリ {repository_id} のトピック {topic_id:?} を準備できません: {}",
                self.telegram_error(source)
            ),
            (Language::English, _) => unreachable!(),
        }
    }

    fn telegram_report_failure(self, failure: &TelegramReportPublishFailure) -> String {
        format!(
            "[{}] {}",
            failure.code.as_str(),
            self.telegram_report_error(&failure.error)
        )
    }

    fn telegram_report_error(self, error: &TelegramReportPublishError) -> String {
        if self.language == Language::English {
            return match error {
                TelegramReportPublishError::Download { file_name, source } => format!(
                    "failed to download attachment {file_name}: {}",
                    self.github_error(source)
                ),
                TelegramReportPublishError::SendText(source) => format!(
                    "failed to send Telegram report text: {}",
                    self.telegram_error(source)
                ),
                TelegramReportPublishError::SendAttachment { file_name, source } => format!(
                    "failed to send Telegram report attachment {file_name}: {}",
                    self.telegram_error(source)
                ),
                _ => error.to_string(),
            };
        }
        match (self.language, error) {
            (Language::Chinese, TelegramReportPublishError::EmptyText) => {
                "Telegram 报告文本不能为空".to_owned()
            }
            (Language::Chinese, TelegramReportPublishError::InvalidFileName { file_name }) => {
                format!("附件文件名不是安全的单个文件名: {file_name}")
            }
            (Language::Chinese, TelegramReportPublishError::DuplicateFileName { file_name }) => {
                format!("Telegram 报告包含重复附件文件名: {file_name}")
            }
            (
                Language::Chinese,
                TelegramReportPublishError::InvalidDownloadUrl { file_name, source },
            ) => format!("附件 {file_name} 的下载地址无效: {source}"),
            (
                Language::Chinese,
                TelegramReportPublishError::InvalidFingerprint {
                    file_name,
                    fingerprint,
                },
            ) => format!("附件 {file_name} 的指纹格式无效: {fingerprint}"),
            (Language::Chinese, TelegramReportPublishError::TemporaryDirectory(source)) => {
                format!("无法创建附件临时目录: {source}")
            }
            (Language::Chinese, TelegramReportPublishError::Download { file_name, source }) => {
                format!("无法下载附件 {file_name}: {}", self.github_error(source))
            }
            (
                Language::Chinese,
                TelegramReportPublishError::ReadForVerification { file_name, source },
            ) => format!("无法读取附件 {file_name} 以计算指纹: {source}"),
            (
                Language::Chinese,
                TelegramReportPublishError::FingerprintMismatch {
                    file_name,
                    expected,
                    actual,
                },
            ) => format!("附件 {file_name} 的 SHA-256 不匹配，预期 {expected}，实际 {actual}"),
            (Language::Chinese, TelegramReportPublishError::SendText(source)) => format!(
                "无法发送 Telegram 报告文本: {}",
                self.telegram_error(source)
            ),
            (
                Language::Chinese,
                TelegramReportPublishError::SendAttachment { file_name, source },
            ) => format!(
                "无法发送 Telegram 报告附件 {file_name}: {}",
                self.telegram_error(source)
            ),

            (Language::Japanese, TelegramReportPublishError::EmptyText) => {
                "Telegram レポートのテキストは空にできません".to_owned()
            }
            (Language::Japanese, TelegramReportPublishError::InvalidFileName { file_name }) => {
                format!("添付ファイル名は安全な単一ファイル名ではありません: {file_name}")
            }
            (Language::Japanese, TelegramReportPublishError::DuplicateFileName { file_name }) => {
                format!("Telegram レポートに重複した添付ファイル名があります: {file_name}")
            }
            (
                Language::Japanese,
                TelegramReportPublishError::InvalidDownloadUrl { file_name, source },
            ) => format!("添付ファイル {file_name} のダウンロード URL が無効です: {source}"),
            (
                Language::Japanese,
                TelegramReportPublishError::InvalidFingerprint {
                    file_name,
                    fingerprint,
                },
            ) => format!(
                "添付ファイル {file_name} のフィンガープリント形式が無効です: {fingerprint}"
            ),
            (Language::Japanese, TelegramReportPublishError::TemporaryDirectory(source)) => {
                format!("添付ファイル用の一時ディレクトリを作成できません: {source}")
            }
            (Language::Japanese, TelegramReportPublishError::Download { file_name, source }) => {
                format!(
                    "添付ファイル {file_name} をダウンロードできません: {}",
                    self.github_error(source)
                )
            }
            (
                Language::Japanese,
                TelegramReportPublishError::ReadForVerification { file_name, source },
            ) => format!(
                "フィンガープリント計算のため添付ファイル {file_name} を読み込めません: {source}"
            ),
            (
                Language::Japanese,
                TelegramReportPublishError::FingerprintMismatch {
                    file_name,
                    expected,
                    actual,
                },
            ) => format!(
                "添付ファイル {file_name} の SHA-256 が一致しません。期待値 {expected}、実際の値 {actual}"
            ),
            (Language::Japanese, TelegramReportPublishError::SendText(source)) => format!(
                "Telegram レポートのテキストを送信できません: {}",
                self.telegram_error(source)
            ),
            (
                Language::Japanese,
                TelegramReportPublishError::SendAttachment { file_name, source },
            ) => format!(
                "Telegram レポートの添付ファイル {file_name} を送信できません: {}",
                self.telegram_error(source)
            ),
            (Language::English, _) => unreachable!(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Feature {
    Commit,
    Release,
    Actions,
}

impl Feature {
    const fn name(self) -> &'static str {
        match self {
            Self::Commit => "Commit",
            Self::Release => "Release",
            Self::Actions => "Actions",
        }
    }
}

pub enum DownloadRetryReason<'a> {
    RequestFailed(&'a str),
    Http(&'a str),
    ResponseReadFailed(&'a str),
}

pub enum ConsoleMessage<'a> {
    RunStarted,
    ConfigPath(&'a Path),
    StatePath(&'a Path),
    ConfigLoaded {
        schema_version: u32,
        repository_count: usize,
    },
    StateLoaded {
        code: &'a str,
    },
    TelegramPrepared {
        code: &'a str,
        checked: usize,
        created: usize,
        recreated: usize,
        renamed: usize,
        locked: usize,
    },
    StateFileSaved {
        changed: bool,
        code: &'a str,
    },
    RunSummary,
    RepositorySummaryFailed {
        repository: &'a str,
        error: &'a str,
    },
    RepositorySummarySucceeded {
        repository: &'a str,
        published: usize,
        changed: bool,
    },
    RunSummaryTotal {
        published: usize,
        changed: bool,
    },
    TopicCreating {
        owner: &'a str,
        topic_name: &'a str,
    },
    TopicRecreating {
        owner: &'a str,
        thread_id: i32,
        topic_name: &'a str,
    },
    RepositoryStarted {
        repository: &'a str,
    },
    RepositoryFailed {
        repository: &'a str,
        error: &'a str,
    },
    RepositoryCompleted {
        repository: &'a str,
        published: usize,
        changed: bool,
    },
    FeatureDisabled {
        repository: &'a str,
        feature: Feature,
    },
    CommitQuery {
        repository: &'a str,
        branches: usize,
    },
    CommitCheck {
        repository: &'a str,
        baselines: usize,
        rewritten: usize,
        updates: usize,
    },
    CommitHistoryRewritten {
        repository: &'a str,
        branch: &'a str,
        sha: &'a str,
    },
    CommitPublishing {
        repository: &'a str,
        branch: &'a str,
        sha: &'a str,
    },
    ReleaseQuery {
        repository: &'a str,
        specific_tag: bool,
    },
    ReleaseNotFound {
        repository: &'a str,
    },
    ReleaseBaselineEstablished {
        repository: &'a str,
        release_id: u64,
    },
    ReleaseUnchanged {
        repository: &'a str,
    },
    ReleasePublishing {
        repository: &'a str,
        updated: bool,
        release_id: u64,
        attachments: usize,
        uploads: usize,
    },
    ActionsQuery {
        repository: &'a str,
        targets: usize,
    },
    ActionsCheck {
        repository: &'a str,
        baselines: usize,
        updates: usize,
    },
    ActionsPublishing {
        repository: &'a str,
        workflow: &'a str,
        branch: &'a str,
        run_id: u64,
        kind: WorkflowRunUpdateKind,
        artifacts: usize,
    },
    DownloadStarted {
        file_name: &'a str,
    },
    DownloadRetry {
        file_name: &'a str,
        attempt: usize,
        max_attempts: usize,
        reason: DownloadRetryReason<'a>,
    },
    DownloadCompleted {
        file_name: &'a str,
        bytes: u64,
        elapsed_seconds: f64,
        speed: &'a str,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Text {
    Unknown,
    NotPublished,
    CommitTitle,
    ReleaseTitle,
    ReleaseUpdatedTitle,
    ActionsTitle,
    Branch,
    Commit,
    Author,
    Time,
    Tag,
    Name,
    ReleaseId,
    Status,
    CreatedAt,
    PublishedAt,
    UpdatedAt,
    Target,
    Attachments,
    AttachmentList,
    Workflow,
    Result,
    Run,
    Draft,
    Prerelease,
    Stable,
    Immutable,
    Attachment,
    AttachmentCountSuffix,
}

pub fn initialize() -> Translator {
    let language = *LANGUAGE.get_or_init(Language::from_environment);
    Translator::new(language)
}

pub fn current() -> Translator {
    Translator::new(LANGUAGE.get().copied().unwrap_or_default())
}

fn parse_locale(value: &str) -> Option<Language> {
    let normalized = value.trim().to_ascii_lowercase();
    let language = normalized
        .split(['_', '-', '.', '@'])
        .next()
        .unwrap_or_default();
    match language {
        "en" | "c" | "posix" => Some(Language::English),
        "zh" => Some(Language::Chinese),
        "ja" => Some(Language::Japanese),
        _ => None,
    }
}

fn plural(count: usize, singular: &'static str, plural: &'static str) -> &'static str {
    if count == 1 { singular } else { plural }
}

fn console_english(message: ConsoleMessage<'_>) -> String {
    match message {
        ConsoleMessage::RunStarted => "GH2TG started".to_owned(),
        ConsoleMessage::ConfigPath(path) => format!("Configuration file: {}", path.display()),
        ConsoleMessage::StatePath(path) => format!("State file: {}", path.display()),
        ConsoleMessage::ConfigLoaded {
            schema_version,
            repository_count,
        } => format!(
            "Configuration loaded: schema_version={schema_version}, repositories={repository_count}"
        ),
        ConsoleMessage::StateLoaded { code } => format!("State file loaded [{code}]"),
        ConsoleMessage::TelegramPrepared {
            code,
            checked,
            created,
            recreated,
            renamed,
            locked,
        } => format!(
            "Telegram group prepared: status_code={code}, checked {checked} {}, created {created}, recreated {recreated}, renamed {renamed}, locked {locked}",
            plural(checked, "topic", "topics")
        ),
        ConsoleMessage::StateFileSaved { changed, code } => format!(
            "State file: {} [{code}]",
            if changed { "written" } else { "unchanged" }
        ),
        ConsoleMessage::RunSummary => "Run summary:".to_owned(),
        ConsoleMessage::RepositorySummaryFailed { repository, error } => {
            format!("{repository}: failed: {error}")
        }
        ConsoleMessage::RepositorySummarySucceeded {
            repository,
            published,
            changed,
        } => format!(
            "{repository}: succeeded, published {published} {}{}",
            plural(published, "update", "updates"),
            if changed { ", state updated" } else { "" }
        ),
        ConsoleMessage::RunSummaryTotal { published, changed } => format!(
            "Published {published} {} in total; {}",
            plural(published, "update", "updates"),
            if changed {
                "state changed"
            } else {
                "no new changes"
            }
        ),
        ConsoleMessage::TopicCreating { owner, topic_name } => {
            format!("[{owner}] Topic: no state record, creating {topic_name}")
        }
        ConsoleMessage::TopicRecreating {
            owner,
            thread_id,
            topic_name,
        } => format!(
            "[{owner}] Topic: thread_id={thread_id} does not exist, recreating {topic_name}"
        ),
        ConsoleMessage::RepositoryStarted { repository } => {
            format!("[{repository}] Processing started")
        }
        ConsoleMessage::RepositoryFailed { repository, error } => {
            format!("[{repository}] Processing failed: {error}")
        }
        ConsoleMessage::RepositoryCompleted {
            repository,
            published,
            changed,
        } => format!(
            "[{repository}] Processing completed: published {published} {}, state {}",
            plural(published, "update", "updates"),
            if changed { "changed" } else { "unchanged" }
        ),
        ConsoleMessage::FeatureDisabled {
            repository,
            feature,
        } => format!("[{repository}] {}: disabled", feature.name()),
        ConsoleMessage::CommitQuery {
            repository,
            branches,
        } => format!(
            "[{repository}] Commit: querying {branches} {}",
            plural(branches, "branch", "branches")
        ),
        ConsoleMessage::CommitCheck {
            repository,
            baselines,
            rewritten,
            updates,
        } => format!(
            "[{repository}] Commit: established {baselines} baselines, reset {rewritten} cursors, found {updates} {}",
            plural(updates, "update", "updates")
        ),
        ConsoleMessage::CommitHistoryRewritten {
            repository,
            branch,
            sha,
        } => format!(
            "[{repository}] Commit: branch {branch} history was rewritten, resetting cursor to {sha}"
        ),
        ConsoleMessage::CommitPublishing {
            repository,
            branch,
            sha,
        } => format!("[{repository}] Commit: publishing commit {sha} from branch {branch}"),
        ConsoleMessage::ReleaseQuery {
            repository,
            specific_tag,
        } => format!(
            "[{repository}] Release: querying {}",
            if specific_tag {
                "the configured tag"
            } else {
                "the latest release"
            }
        ),
        ConsoleMessage::ReleaseNotFound { repository } => {
            format!("[{repository}] Release: not found")
        }
        ConsoleMessage::ReleaseBaselineEstablished {
            repository,
            release_id,
        } => format!("[{repository}] Release: baseline established at Release ID {release_id}"),
        ConsoleMessage::ReleaseUnchanged { repository } => {
            format!("[{repository}] Release: unchanged")
        }
        ConsoleMessage::ReleasePublishing {
            repository,
            updated,
            release_id,
            attachments,
            uploads,
        } => format!(
            "[{repository}] Release: {} ID {release_id}, {attachments} {}, uploading {uploads}",
            if updated { "updating" } else { "publishing" },
            plural(attachments, "attachment", "attachments")
        ),
        ConsoleMessage::ActionsQuery {
            repository,
            targets,
        } => format!(
            "[{repository}] Actions: querying {targets} workflow/branch {}",
            plural(targets, "target", "targets")
        ),
        ConsoleMessage::ActionsCheck {
            repository,
            baselines,
            updates,
        } => format!(
            "[{repository}] Actions: established {baselines} baselines, found {updates} {}",
            plural(updates, "update", "updates")
        ),
        ConsoleMessage::ActionsPublishing {
            repository,
            workflow,
            branch,
            run_id,
            kind,
            artifacts,
        } => format!(
            "[{repository}] Actions: publishing {workflow} / {branch} Run #{run_id} ({}), {artifacts} {}",
            match kind {
                WorkflowRunUpdateKind::NewWorkflowRun => "new run",
                WorkflowRunUpdateKind::NewArtifacts => "new artifacts",
            },
            plural(artifacts, "artifact", "artifacts")
        ),
        ConsoleMessage::DownloadStarted { file_name } => {
            format!("Attachment download started: file={file_name}")
        }
        ConsoleMessage::DownloadRetry {
            file_name,
            attempt,
            max_attempts,
            reason,
        } => format!(
            "Attachment download retry: file={file_name}, attempt={attempt}/{max_attempts}, reason={}",
            match reason {
                DownloadRetryReason::RequestFailed(source) => {
                    format!("request failed: {source}")
                }
                DownloadRetryReason::Http(status) => format!("HTTP {status}"),
                DownloadRetryReason::ResponseReadFailed(source) => {
                    format!("response read failed: {source}")
                }
            }
        ),
        ConsoleMessage::DownloadCompleted {
            file_name,
            bytes,
            elapsed_seconds,
            speed,
        } => format!(
            "Attachment download completed: file={file_name}, bytes={bytes}, elapsed={elapsed_seconds:.1}s, speed={speed}/s"
        ),
    }
}

fn console_chinese(message: ConsoleMessage<'_>) -> String {
    match message {
        ConsoleMessage::RunStarted => "GH2TG 开始运行".to_owned(),
        ConsoleMessage::ConfigPath(path) => format!("配置文件: {}", path.display()),
        ConsoleMessage::StatePath(path) => format!("状态文件: {}", path.display()),
        ConsoleMessage::ConfigLoaded {
            schema_version,
            repository_count,
        } => format!("配置加载完成: schema_version={schema_version}, 仓库数={repository_count}"),
        ConsoleMessage::StateLoaded { code } => format!("状态文件已加载 [{code}]"),
        ConsoleMessage::TelegramPrepared {
            code,
            checked,
            created,
            recreated,
            renamed,
            locked,
        } => format!(
            "Telegram 群组准备完成: status_code={code}, 检查 {checked} 个话题，创建 {created} 个，重建 {recreated} 个，重命名 {renamed} 个，锁定 {locked} 个"
        ),
        ConsoleMessage::StateFileSaved { changed, code } => format!(
            "状态文件: {} [{code}]",
            if changed { "已写入" } else { "无变化" }
        ),
        ConsoleMessage::RunSummary => "运行摘要:".to_owned(),
        ConsoleMessage::RepositorySummaryFailed { repository, error } => {
            format!("{repository}: 失败: {error}")
        }
        ConsoleMessage::RepositorySummarySucceeded {
            repository,
            published,
            changed,
        } => format!(
            "{repository}: 成功，发布 {published} 条更新{}",
            if changed { "，状态已更新" } else { "" }
        ),
        ConsoleMessage::RunSummaryTotal { published, changed } => format!(
            "本次共发布 {published} 条更新，{}",
            if changed {
                "状态有变化"
            } else {
                "没有新变化"
            }
        ),
        ConsoleMessage::TopicCreating { owner, topic_name } => {
            format!("[{owner}] 话题: 状态记录不存在，正在创建 {topic_name}")
        }
        ConsoleMessage::TopicRecreating {
            owner,
            thread_id,
            topic_name,
        } => format!("[{owner}] 话题: thread_id={thread_id} 不存在，正在重建 {topic_name}"),
        ConsoleMessage::RepositoryStarted { repository } => {
            format!("[{repository}] 开始处理")
        }
        ConsoleMessage::RepositoryFailed { repository, error } => {
            format!("[{repository}] 处理失败: {error}")
        }
        ConsoleMessage::RepositoryCompleted {
            repository,
            published,
            changed,
        } => format!(
            "[{repository}] 处理完成: 发布 {published} 条，状态{}变化",
            if changed { "有" } else { "无" }
        ),
        ConsoleMessage::FeatureDisabled {
            repository,
            feature,
        } => format!("[{repository}] {}: 未启用", feature.name()),
        ConsoleMessage::CommitQuery {
            repository,
            branches,
        } => format!("[{repository}] Commit: 查询 {branches} 个分支"),
        ConsoleMessage::CommitCheck {
            repository,
            baselines,
            rewritten,
            updates,
        } => format!(
            "[{repository}] Commit: 新建 {baselines} 个基线，重置 {rewritten} 个游标，发现 {updates} 条更新"
        ),
        ConsoleMessage::CommitHistoryRewritten {
            repository,
            branch,
            sha,
        } => format!("[{repository}] Commit: 分支 {branch} 历史已重写，重置游标到 {sha}"),
        ConsoleMessage::CommitPublishing {
            repository,
            branch,
            sha,
        } => format!("[{repository}] Commit: 发布分支 {branch} 的提交 {sha}"),
        ConsoleMessage::ReleaseQuery {
            repository,
            specific_tag,
        } => format!(
            "[{repository}] Release: 查询{}",
            if specific_tag {
                "指定 Tag"
            } else {
                "最新项"
            }
        ),
        ConsoleMessage::ReleaseNotFound { repository } => {
            format!("[{repository}] Release: 未找到")
        }
        ConsoleMessage::ReleaseBaselineEstablished {
            repository,
            release_id,
        } => format!("[{repository}] Release: 已建立基线，Release ID {release_id}"),
        ConsoleMessage::ReleaseUnchanged { repository } => {
            format!("[{repository}] Release: 无变化")
        }
        ConsoleMessage::ReleasePublishing {
            repository,
            updated,
            release_id,
            attachments,
            uploads,
        } => format!(
            "[{repository}] Release: {} ID {release_id}，附件 {attachments} 个，上传 {uploads} 个",
            if updated { "滚动更新" } else { "发布" }
        ),
        ConsoleMessage::ActionsQuery {
            repository,
            targets,
        } => format!("[{repository}] Actions: 查询 {targets} 个工作流/分支组合"),
        ConsoleMessage::ActionsCheck {
            repository,
            baselines,
            updates,
        } => format!("[{repository}] Actions: 新建 {baselines} 个基线，发现 {updates} 条更新"),
        ConsoleMessage::ActionsPublishing {
            repository,
            workflow,
            branch,
            run_id,
            kind,
            artifacts,
        } => format!(
            "[{repository}] Actions: 发布 {workflow} / {branch} 的 Run #{run_id}（{}），Artifact {artifacts} 个",
            match kind {
                WorkflowRunUpdateKind::NewWorkflowRun => "新 Run",
                WorkflowRunUpdateKind::NewArtifacts => "新增 Artifact",
            }
        ),
        ConsoleMessage::DownloadStarted { file_name } => {
            format!("开始下载附件: file={file_name}")
        }
        ConsoleMessage::DownloadRetry {
            file_name,
            attempt,
            max_attempts,
            reason,
        } => format!(
            "附件下载重试: file={file_name}, attempt={attempt}/{max_attempts}, reason={}",
            match reason {
                DownloadRetryReason::RequestFailed(source) => {
                    format!("建立请求失败: {source}")
                }
                DownloadRetryReason::Http(status) => format!("HTTP {status}"),
                DownloadRetryReason::ResponseReadFailed(source) => {
                    format!("读取响应失败: {source}")
                }
            }
        ),
        ConsoleMessage::DownloadCompleted {
            file_name,
            bytes,
            elapsed_seconds,
            speed,
        } => format!(
            "附件下载完成: file={file_name}, bytes={bytes}, elapsed={elapsed_seconds:.1}s, speed={speed}/s"
        ),
    }
}

fn console_japanese(message: ConsoleMessage<'_>) -> String {
    match message {
        ConsoleMessage::RunStarted => "GH2TG を開始しました".to_owned(),
        ConsoleMessage::ConfigPath(path) => format!("設定ファイル: {}", path.display()),
        ConsoleMessage::StatePath(path) => format!("状態ファイル: {}", path.display()),
        ConsoleMessage::ConfigLoaded {
            schema_version,
            repository_count,
        } => format!(
            "設定を読み込みました: schema_version={schema_version}, リポジトリ数={repository_count}"
        ),
        ConsoleMessage::StateLoaded { code } => {
            format!("状態ファイルを読み込みました [{code}]")
        }
        ConsoleMessage::TelegramPrepared {
            code,
            checked,
            created,
            recreated,
            renamed,
            locked,
        } => format!(
            "Telegram グループの準備が完了しました: status_code={code}, トピック確認 {checked} 件、作成 {created} 件、再作成 {recreated} 件、名前変更 {renamed} 件、ロック {locked} 件"
        ),
        ConsoleMessage::StateFileSaved { changed, code } => format!(
            "状態ファイル: {} [{code}]",
            if changed {
                "書き込み済み"
            } else {
                "変更なし"
            }
        ),
        ConsoleMessage::RunSummary => "実行結果:".to_owned(),
        ConsoleMessage::RepositorySummaryFailed { repository, error } => {
            format!("{repository}: 失敗: {error}")
        }
        ConsoleMessage::RepositorySummarySucceeded {
            repository,
            published,
            changed,
        } => format!(
            "{repository}: 成功、更新を {published} 件公開{}",
            if changed { "、状態を更新" } else { "" }
        ),
        ConsoleMessage::RunSummaryTotal { published, changed } => format!(
            "合計 {published} 件の更新を公開、{}",
            if changed {
                "状態に変更あり"
            } else {
                "新しい変更なし"
            }
        ),
        ConsoleMessage::TopicCreating { owner, topic_name } => {
            format!("[{owner}] トピック: 状態記録がないため {topic_name} を作成します")
        }
        ConsoleMessage::TopicRecreating {
            owner,
            thread_id,
            topic_name,
        } => format!(
            "[{owner}] トピック: thread_id={thread_id} が存在しないため {topic_name} を再作成します"
        ),
        ConsoleMessage::RepositoryStarted { repository } => {
            format!("[{repository}] 処理を開始しました")
        }
        ConsoleMessage::RepositoryFailed { repository, error } => {
            format!("[{repository}] 処理に失敗しました: {error}")
        }
        ConsoleMessage::RepositoryCompleted {
            repository,
            published,
            changed,
        } => format!(
            "[{repository}] 処理が完了しました: {published} 件公開、状態{}",
            if changed {
                "変更あり"
            } else {
                "変更なし"
            }
        ),
        ConsoleMessage::FeatureDisabled {
            repository,
            feature,
        } => format!("[{repository}] {}: 無効", feature.name()),
        ConsoleMessage::CommitQuery {
            repository,
            branches,
        } => format!("[{repository}] Commit: {branches} ブランチを確認します"),
        ConsoleMessage::CommitCheck {
            repository,
            baselines,
            rewritten,
            updates,
        } => format!(
            "[{repository}] Commit: ベースライン {baselines} 件を作成、カーソル {rewritten} 件をリセット、更新 {updates} 件を検出"
        ),
        ConsoleMessage::CommitHistoryRewritten {
            repository,
            branch,
            sha,
        } => format!(
            "[{repository}] Commit: ブランチ {branch} の履歴が書き換えられたため、カーソルを {sha} にリセットします"
        ),
        ConsoleMessage::CommitPublishing {
            repository,
            branch,
            sha,
        } => format!("[{repository}] Commit: ブランチ {branch} のコミット {sha} を公開します"),
        ConsoleMessage::ReleaseQuery {
            repository,
            specific_tag,
        } => format!(
            "[{repository}] Release: {}を確認します",
            if specific_tag {
                "指定タグ"
            } else {
                "最新リリース"
            }
        ),
        ConsoleMessage::ReleaseNotFound { repository } => {
            format!("[{repository}] Release: 見つかりません")
        }
        ConsoleMessage::ReleaseBaselineEstablished {
            repository,
            release_id,
        } => {
            format!("[{repository}] Release: Release ID {release_id} でベースラインを作成しました")
        }
        ConsoleMessage::ReleaseUnchanged { repository } => {
            format!("[{repository}] Release: 変更なし")
        }
        ConsoleMessage::ReleasePublishing {
            repository,
            updated,
            release_id,
            attachments,
            uploads,
        } => format!(
            "[{repository}] Release: ID {release_id} を{}、添付ファイル {attachments} 件、アップロード {uploads} 件",
            if updated { "更新" } else { "公開" }
        ),
        ConsoleMessage::ActionsQuery {
            repository,
            targets,
        } => format!(
            "[{repository}] Actions: ワークフロー/ブランチの組み合わせ {targets} 件を確認します"
        ),
        ConsoleMessage::ActionsCheck {
            repository,
            baselines,
            updates,
        } => format!(
            "[{repository}] Actions: ベースライン {baselines} 件を作成、更新 {updates} 件を検出"
        ),
        ConsoleMessage::ActionsPublishing {
            repository,
            workflow,
            branch,
            run_id,
            kind,
            artifacts,
        } => format!(
            "[{repository}] Actions: {workflow} / {branch} の Run #{run_id}（{}）を公開、Artifact {artifacts} 件",
            match kind {
                WorkflowRunUpdateKind::NewWorkflowRun => "新しい Run",
                WorkflowRunUpdateKind::NewArtifacts => "新しい Artifact",
            }
        ),
        ConsoleMessage::DownloadStarted { file_name } => {
            format!("添付ファイルのダウンロードを開始: file={file_name}")
        }
        ConsoleMessage::DownloadRetry {
            file_name,
            attempt,
            max_attempts,
            reason,
        } => format!(
            "添付ファイルのダウンロードを再試行: file={file_name}, attempt={attempt}/{max_attempts}, reason={}",
            match reason {
                DownloadRetryReason::RequestFailed(source) => {
                    format!("リクエスト作成失敗: {source}")
                }
                DownloadRetryReason::Http(status) => format!("HTTP {status}"),
                DownloadRetryReason::ResponseReadFailed(source) => {
                    format!("レスポンス読み込み失敗: {source}")
                }
            }
        ),
        ConsoleMessage::DownloadCompleted {
            file_name,
            bytes,
            elapsed_seconds,
            speed,
        } => format!(
            "添付ファイルのダウンロード完了: file={file_name}, bytes={bytes}, elapsed={elapsed_seconds:.1}s, speed={speed}/s"
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{ConsoleMessage, Language, Text, Translator};

    #[test]
    fn resolves_supported_locale_forms() {
        assert_eq!(
            Language::from_locale_values(None, None, Some("zh_CN.UTF-8")),
            Language::Chinese
        );
        assert_eq!(
            Language::from_locale_values(None, None, Some("zh-Hans")),
            Language::Chinese
        );
        assert_eq!(
            Language::from_locale_values(None, None, Some("ja_JP.UTF-8")),
            Language::Japanese
        );
        assert_eq!(
            Language::from_locale_values(None, None, Some("en_US.UTF-8")),
            Language::English
        );
        assert_eq!(
            Language::from_locale_values(None, None, Some("C.UTF-8")),
            Language::English
        );
    }

    #[test]
    fn locale_precedence_and_unknown_values_fall_back_to_english() {
        assert_eq!(
            Language::from_locale_values(Some("ja_JP"), Some("zh_CN"), Some("en_US")),
            Language::Japanese
        );
        assert_eq!(
            Language::from_locale_values(None, Some("zh_CN"), Some("ja_JP")),
            Language::Chinese
        );
        assert_eq!(
            Language::from_locale_values(None, None, Some("fr_FR")),
            Language::English
        );
        assert_eq!(
            Language::from_locale_values(None, None, None),
            Language::English
        );
    }

    #[test]
    fn renders_console_messages_in_all_languages() {
        let path = Path::new("config.json");
        assert_eq!(
            Translator::new(Language::English).console(ConsoleMessage::ConfigPath(path)),
            "Configuration file: config.json"
        );
        assert_eq!(
            Translator::new(Language::Chinese).console(ConsoleMessage::ConfigPath(path)),
            "配置文件: config.json"
        );
        assert_eq!(
            Translator::new(Language::Japanese).console(ConsoleMessage::ConfigPath(path)),
            "設定ファイル: config.json"
        );
    }

    #[test]
    fn exposes_telegram_labels_in_all_languages() {
        assert_eq!(
            Translator::new(Language::English).text(Text::Branch),
            "Branch"
        );
        assert_eq!(
            Translator::new(Language::Chinese).text(Text::Branch),
            "分支"
        );
        assert_eq!(
            Translator::new(Language::Japanese).text(Text::Branch),
            "ブランチ"
        );
    }
}
