use crate::{
    actions::WorkflowRunUpdate,
    commit::CommitInfo,
    i18n::{Text, Translator},
    release::ReleaseInfo,
    telegram_report::TelegramRichText,
};

const BODY_LIMIT: usize = 2500;
const RELEASE_BODY_LIMIT: usize = 1800;
const RELEASE_ASSET_LIST_LIMIT: usize = 1000;

pub fn format_commit(
    translator: Translator,
    repository: &str,
    branch: &str,
    commit: &CommitInfo,
) -> TelegramRichText {
    let title = commit.message().lines().next().unwrap_or_default();
    TelegramRichText::markdown_v2(format!(
        "{} {}\n{}: {}\n{}: {}\n{}: {}\n{}: {}\n\n{}\n\n{}",
        escape_markdown_v2(translator.text(Text::CommitTitle)),
        escape_markdown_v2(repository),
        translator.text(Text::Branch),
        escape_markdown_v2(branch),
        translator.text(Text::Commit),
        short_sha(commit.sha()),
        translator.text(Text::Author),
        escape_markdown_v2(commit.author()),
        translator.text(Text::Time),
        escape_markdown_v2(commit.committed_at()),
        code_block(title, BODY_LIMIT),
        escape_markdown_v2(commit.html_url())
    ))
}

pub fn format_release(
    translator: Translator,
    repository: &str,
    release: &ReleaseInfo,
    updated: bool,
) -> TelegramRichText {
    let title = if updated {
        translator.text(Text::ReleaseUpdatedTitle)
    } else {
        translator.text(Text::ReleaseTitle)
    };
    let name = release.name().unwrap_or_default();
    let author = release
        .author_login()
        .unwrap_or(translator.text(Text::Unknown));
    let published_at = release
        .published_at()
        .unwrap_or(translator.text(Text::NotPublished));
    let updated_at = release
        .updated_at()
        .unwrap_or(translator.text(Text::Unknown));
    let status = release_status(translator, release);
    let asset_names = release
        .assets()
        .iter()
        .map(|asset| asset.file_name().as_str())
        .collect::<Vec<_>>()
        .join(", ");

    let title = escape_markdown_v2(title);
    let status = escape_markdown_v2(&status);
    TelegramRichText::markdown_v2(format!(
        "{title} {repository}\n\
{tag_label}: {tag}\n\
{name_label}: {name}\n\
{release_id_label}: {release_id}\n\
{status_label}: {status}\n\
{author_label}: {author}\n\
{created_at_label}: {created_at}\n\
{published_at_label}: {published_at}\n\
{updated_at_label}: {updated_at}\n\
{target_label}: {target}\n\
{attachments_label}: {attachment_count}{attachment_count_suffix}\n\
{attachment_list_label}: {attachment_list}\n\n\
{body}\n\n\
{url}",
        repository = escape_markdown_v2(repository),
        tag_label = translator.text(Text::Tag),
        tag = escape_markdown_v2(release.tag_name()),
        name_label = translator.text(Text::Name),
        name = escape_markdown_v2(name),
        release_id_label = translator.text(Text::ReleaseId),
        release_id = release.id().value(),
        status_label = translator.text(Text::Status),
        author_label = translator.text(Text::Author),
        author = escape_markdown_v2(author),
        created_at_label = translator.text(Text::CreatedAt),
        created_at = escape_markdown_v2(release.created_at()),
        published_at_label = translator.text(Text::PublishedAt),
        published_at = escape_markdown_v2(published_at),
        updated_at_label = translator.text(Text::UpdatedAt),
        updated_at = escape_markdown_v2(updated_at),
        target_label = translator.text(Text::Target),
        target = escape_markdown_v2(release.target_commitish()),
        attachments_label = translator.text(Text::Attachments),
        attachment_count = release.assets().len(),
        attachment_count_suffix = translator.text(Text::AttachmentCountSuffix),
        attachment_list_label = translator.text(Text::AttachmentList),
        attachment_list = escape_markdown_v2(&truncate(&asset_names, RELEASE_ASSET_LIST_LIMIT)),
        body = code_block(release.body().unwrap_or_default(), RELEASE_BODY_LIMIT),
        url = escape_markdown_v2(release.html_url()),
    ))
}

fn release_status(translator: Translator, release: &ReleaseInfo) -> String {
    let mut statuses = Vec::new();
    if release.draft() {
        statuses.push(translator.text(Text::Draft));
    }
    if release.prerelease() {
        statuses.push(translator.text(Text::Prerelease));
    } else {
        statuses.push(translator.text(Text::Stable));
    }
    if release.immutable() == Some(true) {
        statuses.push(translator.text(Text::Immutable));
    }
    statuses.join(", ")
}

pub fn format_workflow_run(
    translator: Translator,
    repository: &str,
    update: &WorkflowRunUpdate,
) -> TelegramRichText {
    let target = update.target();
    let run = update.run();
    TelegramRichText::plain(format!(
        "{} {repository}\n{}: {}\n{}: {}\n{}: {}\n{}: #{}\n{}: {}\n\n{}\n\n{}",
        translator.text(Text::ActionsTitle),
        translator.text(Text::Workflow),
        target.workflow_file(),
        translator.text(Text::Branch),
        target.branch(),
        translator.text(Text::Result),
        run.conclusion(),
        translator.text(Text::Run),
        run.id(),
        translator.text(Text::Time),
        run.created_at(),
        truncate(run.title(), BODY_LIMIT),
        run.html_url()
    ))
}

fn short_sha(sha: &str) -> &str {
    sha.get(..7).unwrap_or(sha)
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn code_block(value: &str, max_chars: usize) -> String {
    format!(
        "```\n{}\n```",
        escape_markdown_v2_code(&truncate(value, max_chars))
    )
}

fn escape_markdown_v2(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' | '_' | '*' | '[' | ']' | '(' | ')' | '~' | '`' | '>' | '#' | '+' | '-' | '='
            | '|' | '{' | '}' | '.' | '!' => {
                escaped.push('\\');
                escaped.push(character);
            }
            _ => escaped.push(character),
        }
    }
    escaped
}

fn escape_markdown_v2_code(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' | '`' => {
                escaped.push('\\');
                escaped.push(character);
            }
            _ => escaped.push(character),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use crate::{
        commit::CommitInfo,
        i18n::{Language, Translator},
    };

    use super::format_commit;

    fn commit() -> CommitInfo {
        CommitInfo::new(
            "1234567890".to_owned(),
            "Fix markdown [output]".to_owned(),
            "octocat".to_owned(),
            "2026-09-09T00:00:00Z".to_owned(),
            "https://github.com/owner/repo/commit/1234567890".to_owned(),
        )
    }

    #[test]
    fn formats_commit_labels_in_all_supported_languages() {
        let commit = commit();
        let english = format_commit(
            Translator::new(Language::English),
            "owner/repo",
            "main",
            &commit,
        );
        let chinese = format_commit(
            Translator::new(Language::Chinese),
            "owner/repo",
            "main",
            &commit,
        );
        let japanese = format_commit(
            Translator::new(Language::Japanese),
            "owner/repo",
            "main",
            &commit,
        );

        assert!(english.text().contains("Branch: main"));
        assert!(chinese.text().contains("分支: main"));
        assert!(japanese.text().contains("ブランチ: main"));
        assert!(english.text().starts_with("\\[Commit\\] owner/repo"));
        assert!(english.text().contains("Fix markdown [output]"));
    }
}
