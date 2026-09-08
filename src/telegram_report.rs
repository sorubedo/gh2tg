use std::{
    collections::BTreeSet,
    fmt::Write as _,
    io,
    path::{Component, Path, PathBuf},
};

use sha2::{Digest, Sha256};
use teloxide::types::ParseMode;
use tempfile::TempDir;
use thiserror::Error;
use tokio::{fs::File, io::AsyncReadExt, time::sleep};
use url::Url;

use crate::{
    github::{GitHubClient, GitHubError},
    telegram::{MessageThreadId, TelegramClient, TelegramError, TelegramMessageId},
};

const MAX_TELEGRAM_ATTEMPTS: usize = 3;
const MAX_MEDIA_GROUP_SIZE: usize = 10;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TelegramRichTextFormat {
    PlainText,
    MarkdownV2,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TelegramRichText {
    text: String,
    format: TelegramRichTextFormat,
}

impl TelegramRichText {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            format: TelegramRichTextFormat::PlainText,
        }
    }

    pub fn markdown_v2(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            format: TelegramRichTextFormat::MarkdownV2,
        }
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    fn parse_mode(&self) -> Option<ParseMode> {
        match self.format {
            TelegramRichTextFormat::PlainText => None,
            TelegramRichTextFormat::MarkdownV2 => Some(ParseMode::MarkdownV2),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttachmentDownloadMethod {
    GitHubReleaseAsset,
    GitHubActionsArtifact,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TelegramReportAttachment {
    file_name: String,
    download_url: String,
    download_method: AttachmentDownloadMethod,
    fingerprint: Option<String>,
}

impl TelegramReportAttachment {
    pub fn new(
        file_name: impl Into<String>,
        download_url: impl Into<String>,
        download_method: AttachmentDownloadMethod,
        fingerprint: Option<String>,
    ) -> Self {
        Self {
            file_name: file_name.into(),
            download_url: download_url.into(),
            download_method,
            fingerprint,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TelegramReportPublishStatusCode {
    InvalidReportText,
    InvalidAttachment,
    TemporaryDirectoryFailed,
    AttachmentDownloadFailed,
    AttachmentVerificationFailed,
    TelegramMessageSendFailed,
    TelegramAttachmentSendFailed,
}

impl TelegramReportPublishStatusCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidReportText => "INVALID_REPORT_TEXT",
            Self::InvalidAttachment => "INVALID_ATTACHMENT",
            Self::TemporaryDirectoryFailed => "TEMPORARY_DIRECTORY_FAILED",
            Self::AttachmentDownloadFailed => "ATTACHMENT_DOWNLOAD_FAILED",
            Self::AttachmentVerificationFailed => "ATTACHMENT_VERIFICATION_FAILED",
            Self::TelegramMessageSendFailed => "TELEGRAM_MESSAGE_SEND_FAILED",
            Self::TelegramAttachmentSendFailed => "TELEGRAM_ATTACHMENT_SEND_FAILED",
        }
    }
}

#[derive(Debug, Error)]
pub enum TelegramReportPublishError {
    #[error("Telegram report text must not be empty")]
    EmptyText,
    #[error("attachment file name is not a safe single file name: {file_name}")]
    InvalidFileName { file_name: String },
    #[error("Telegram report contains a duplicate attachment file name: {file_name}")]
    DuplicateFileName { file_name: String },
    #[error("attachment {file_name} has an invalid download URL: {source}")]
    InvalidDownloadUrl {
        file_name: String,
        #[source]
        source: url::ParseError,
    },
    #[error("attachment {file_name} has an invalid fingerprint format: {fingerprint}")]
    InvalidFingerprint {
        file_name: String,
        fingerprint: String,
    },
    #[error("failed to create attachment temporary directory: {0}")]
    TemporaryDirectory(#[source] io::Error),
    #[error("failed to download attachment {file_name}: {source}")]
    Download {
        file_name: String,
        #[source]
        source: GitHubError,
    },
    #[error("failed to read attachment {file_name} for fingerprint verification: {source}")]
    ReadForVerification {
        file_name: String,
        #[source]
        source: io::Error,
    },
    #[error("attachment {file_name} SHA-256 mismatch; expected {expected}, actual {actual}")]
    FingerprintMismatch {
        file_name: String,
        expected: String,
        actual: String,
    },
    #[error("failed to send Telegram report text: {0}")]
    SendText(#[source] TelegramError),
    #[error("failed to send Telegram report attachment {file_name}: {source}")]
    SendAttachment {
        file_name: String,
        #[source]
        source: TelegramError,
    },
}

impl TelegramReportPublishError {
    const fn status_code(&self) -> TelegramReportPublishStatusCode {
        match self {
            Self::EmptyText => TelegramReportPublishStatusCode::InvalidReportText,
            Self::InvalidFileName { .. }
            | Self::DuplicateFileName { .. }
            | Self::InvalidDownloadUrl { .. }
            | Self::InvalidFingerprint { .. } => TelegramReportPublishStatusCode::InvalidAttachment,
            Self::TemporaryDirectory(_) => {
                TelegramReportPublishStatusCode::TemporaryDirectoryFailed
            }
            Self::Download { .. } => TelegramReportPublishStatusCode::AttachmentDownloadFailed,
            Self::ReadForVerification { .. } | Self::FingerprintMismatch { .. } => {
                TelegramReportPublishStatusCode::AttachmentVerificationFailed
            }
            Self::SendText(_) => TelegramReportPublishStatusCode::TelegramMessageSendFailed,
            Self::SendAttachment { .. } => {
                TelegramReportPublishStatusCode::TelegramAttachmentSendFailed
            }
        }
    }
}

#[derive(Debug)]
pub struct TelegramReportPublishFailure {
    pub code: TelegramReportPublishStatusCode,
    pub published_message_ids: Vec<TelegramMessageId>,
    pub error: TelegramReportPublishError,
}

impl std::fmt::Display for TelegramReportPublishFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "[{}] {}", self.code.as_str(), self.error)
    }
}

impl std::error::Error for TelegramReportPublishFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

impl TelegramReportPublishFailure {
    fn new(
        error: TelegramReportPublishError,
        published_message_ids: Vec<TelegramMessageId>,
    ) -> Self {
        Self {
            code: error.status_code(),
            published_message_ids,
            error,
        }
    }

    fn before_send(error: TelegramReportPublishError) -> Self {
        Self::new(error, Vec::new())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublishedTelegramReport {
    pub root_message_id: TelegramMessageId,
    pub attachment_message_ids: Vec<TelegramMessageId>,
}

impl PublishedTelegramReport {
    pub fn message_count(&self) -> usize {
        1 + self.attachment_message_ids.len()
    }
}

pub struct TelegramReportPublisher<'a> {
    github: &'a GitHubClient,
    telegram: &'a TelegramClient,
    thread_id: MessageThreadId,
}

impl<'a> TelegramReportPublisher<'a> {
    pub fn new(
        github: &'a GitHubClient,
        telegram: &'a TelegramClient,
        thread_id: MessageThreadId,
    ) -> Self {
        Self {
            github,
            telegram,
            thread_id,
        }
    }

    pub async fn publish_telegram_report(
        &self,
        rich_text: TelegramRichText,
        attachments: Vec<TelegramReportAttachment>,
    ) -> Result<PublishedTelegramReport, TelegramReportPublishFailure> {
        if rich_text.text().trim().is_empty() {
            return Err(TelegramReportPublishFailure::before_send(
                TelegramReportPublishError::EmptyText,
            ));
        }

        let prepared = self
            .prepare_attachments(attachments)
            .await
            .map_err(TelegramReportPublishFailure::before_send)?;

        let root_message_id = self
            .send_text_with_retry(&rich_text)
            .await
            .map_err(|source| {
                TelegramReportPublishFailure::before_send(TelegramReportPublishError::SendText(
                    source,
                ))
            })?;
        let mut published_message_ids = vec![root_message_id];
        let mut attachment_message_ids = Vec::with_capacity(prepared.attachments.len());

        for attachments in prepared.attachments.chunks(MAX_MEDIA_GROUP_SIZE) {
            let file_name = attachments
                .iter()
                .map(|attachment| attachment.file_name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let message_ids = self
                .send_attachments_with_retry(attachments)
                .await
                .map_err(|source| {
                    TelegramReportPublishFailure::new(
                        TelegramReportPublishError::SendAttachment {
                            file_name: file_name.clone(),
                            source,
                        },
                        published_message_ids.clone(),
                    )
                })?;
            published_message_ids.extend(message_ids.iter().copied());
            attachment_message_ids.extend(message_ids);
        }

        Ok(PublishedTelegramReport {
            root_message_id,
            attachment_message_ids,
        })
    }

    async fn prepare_attachments(
        &self,
        attachments: Vec<TelegramReportAttachment>,
    ) -> Result<PreparedTelegramAttachments, TelegramReportPublishError> {
        if attachments.is_empty() {
            return Ok(PreparedTelegramAttachments {
                _directory: None,
                attachments: Vec::new(),
            });
        }

        let directory = TempDir::new().map_err(TelegramReportPublishError::TemporaryDirectory)?;
        let mut file_names = BTreeSet::new();
        let mut prepared = Vec::with_capacity(attachments.len());

        for attachment in attachments {
            validate_file_name(&attachment.file_name)?;
            if !file_names.insert(attachment.file_name.clone()) {
                return Err(TelegramReportPublishError::DuplicateFileName {
                    file_name: attachment.file_name,
                });
            }

            let download_url = Url::parse(&attachment.download_url).map_err(|source| {
                TelegramReportPublishError::InvalidDownloadUrl {
                    file_name: attachment.file_name.clone(),
                    source,
                }
            })?;
            let expected_sha256 = attachment
                .fingerprint
                .as_deref()
                .map(|fingerprint| parse_sha256(&attachment.file_name, fingerprint))
                .transpose()?;
            let path = directory.path().join(&attachment.file_name);

            match attachment.download_method {
                AttachmentDownloadMethod::GitHubReleaseAsset => {
                    self.github
                        .download_release_asset_to(&download_url, &path)
                        .await
                }
                AttachmentDownloadMethod::GitHubActionsArtifact => {
                    self.github
                        .download_actions_artifact_to(&download_url, &path)
                        .await
                }
            }
            .map_err(|source| TelegramReportPublishError::Download {
                file_name: attachment.file_name.clone(),
                source,
            })?;

            if let Some(expected) = expected_sha256 {
                let actual = sha256_file(&path).await.map_err(|source| {
                    TelegramReportPublishError::ReadForVerification {
                        file_name: attachment.file_name.clone(),
                        source,
                    }
                })?;
                if actual != expected {
                    return Err(TelegramReportPublishError::FingerprintMismatch {
                        file_name: attachment.file_name,
                        expected,
                        actual,
                    });
                }
            }

            prepared.push(PreparedTelegramAttachment {
                file_name: attachment.file_name,
                path,
            });
        }

        Ok(PreparedTelegramAttachments {
            _directory: Some(directory),
            attachments: prepared,
        })
    }

    async fn send_text_with_retry(
        &self,
        rich_text: &TelegramRichText,
    ) -> Result<TelegramMessageId, TelegramError> {
        for attempt in 0..MAX_TELEGRAM_ATTEMPTS {
            match self
                .telegram
                .send_text(self.thread_id, rich_text.text(), rich_text.parse_mode())
                .await
            {
                Ok(message_id) => return Ok(message_id),
                Err(error) if attempt + 1 < MAX_TELEGRAM_ATTEMPTS => {
                    let Some(delay) = error.retry_after() else {
                        return Err(error);
                    };
                    sleep(delay).await;
                }
                Err(error) => return Err(error),
            }
        }

        unreachable!("Telegram send attempts must eventually terminate")
    }

    async fn send_file_with_retry(
        &self,
        path: &Path,
        file_name: &str,
    ) -> Result<TelegramMessageId, TelegramError> {
        for attempt in 0..MAX_TELEGRAM_ATTEMPTS {
            match self
                .telegram
                .send_file(self.thread_id, path, Some(file_name))
                .await
            {
                Ok(message_id) => return Ok(message_id),
                Err(error) if attempt + 1 < MAX_TELEGRAM_ATTEMPTS => {
                    let Some(delay) = error.retry_after() else {
                        return Err(error);
                    };
                    sleep(delay).await;
                }
                Err(error) => return Err(error),
            }
        }

        unreachable!("Telegram upload attempts must eventually terminate")
    }

    async fn send_attachments_with_retry(
        &self,
        attachments: &[PreparedTelegramAttachment],
    ) -> Result<Vec<TelegramMessageId>, TelegramError> {
        if attachments.len() == 1 {
            return Ok(vec![
                self.send_file_with_retry(&attachments[0].path, &attachments[0].file_name)
                    .await?,
            ]);
        }

        let files = attachments
            .iter()
            .map(|attachment| (attachment.path.as_path(), attachment.file_name.as_str()))
            .collect::<Vec<_>>();
        for attempt in 0..MAX_TELEGRAM_ATTEMPTS {
            match self.telegram.send_files(self.thread_id, &files).await {
                Ok(message_ids) => return Ok(message_ids),
                Err(error) if attempt + 1 < MAX_TELEGRAM_ATTEMPTS => {
                    let Some(delay) = error.retry_after() else {
                        return Err(error);
                    };
                    sleep(delay).await;
                }
                Err(error) => return Err(error),
            }
        }

        unreachable!("Telegram media group attempts must eventually terminate")
    }
}

struct PreparedTelegramAttachments {
    _directory: Option<TempDir>,
    attachments: Vec<PreparedTelegramAttachment>,
}

struct PreparedTelegramAttachment {
    file_name: String,
    path: PathBuf,
}

fn validate_file_name(file_name: &str) -> Result<(), TelegramReportPublishError> {
    let path = Path::new(file_name);
    let mut components = path.components();
    let valid = !file_name.is_empty()
        && matches!(components.next(), Some(Component::Normal(_)))
        && components.next().is_none();

    if valid {
        Ok(())
    } else {
        Err(TelegramReportPublishError::InvalidFileName {
            file_name: file_name.to_owned(),
        })
    }
}

fn parse_sha256(file_name: &str, fingerprint: &str) -> Result<String, TelegramReportPublishError> {
    let Some((algorithm, digest)) = fingerprint.split_once(':') else {
        return Err(TelegramReportPublishError::InvalidFingerprint {
            file_name: file_name.to_owned(),
            fingerprint: fingerprint.to_owned(),
        });
    };
    let valid = algorithm.eq_ignore_ascii_case("sha256")
        && digest.len() == 64
        && digest.bytes().all(|byte| byte.is_ascii_hexdigit());
    if !valid {
        return Err(TelegramReportPublishError::InvalidFingerprint {
            file_name: file_name.to_owned(),
            fingerprint: fingerprint.to_owned(),
        });
    }

    Ok(digest.to_ascii_lowercase())
}

async fn sha256_file(path: &Path) -> Result<String, io::Error> {
    let mut file = File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];

    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    let digest = hasher.finalize();
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Ok(encoded)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        thread,
    };

    use tempfile::TempDir;
    use url::Url;

    use crate::{
        github::GitHubClient,
        telegram::{MessageThreadId, TelegramClient, TelegramMessageId},
    };

    use super::{
        AttachmentDownloadMethod, MAX_MEDIA_GROUP_SIZE, PreparedTelegramAttachment,
        TelegramReportAttachment, TelegramReportPublishStatusCode, TelegramReportPublisher,
        TelegramRichText, parse_sha256, sha256_file, validate_file_name,
    };

    #[test]
    fn accepts_a_sha256_fingerprint_case_insensitively() {
        let digest = "A".repeat(64);
        assert_eq!(
            parse_sha256("archive.zip", &format!("SHA256:{digest}"))
                .expect("fingerprint must be valid"),
            "a".repeat(64)
        );
    }

    #[test]
    fn rejects_unsafe_attachment_file_names() {
        assert!(validate_file_name("../archive.zip").is_err());
        assert!(validate_file_name("folder/archive.zip").is_err());
        assert!(validate_file_name("").is_err());
        assert!(validate_file_name("archive.zip").is_ok());
    }

    #[test]
    fn media_groups_are_limited_to_ten_attachments() {
        let directory = TempDir::new().expect("temporary directory must be created");
        let attachments = (0..21)
            .map(|index| PreparedTelegramAttachment {
                file_name: format!("{index}.zip"),
                path: directory.path().join(format!("{index}.zip")),
            })
            .collect::<Vec<_>>();

        let group_sizes = attachments
            .chunks(MAX_MEDIA_GROUP_SIZE)
            .map(<[_]>::len)
            .collect::<Vec<_>>();

        assert_eq!(group_sizes, vec![10, 10, 1]);
    }

    #[tokio::test]
    async fn calculates_file_sha256() {
        let directory = TempDir::new().expect("temporary directory must be created");
        let path = directory.path().join("archive.zip");
        fs::write(&path, b"hello world").expect("test file must be written");

        assert_eq!(
            sha256_file(&path).await.expect("hash must be calculated"),
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[tokio::test]
    async fn publishes_text_and_returns_the_telegram_message_id() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("test server must bind");
        let address = listener
            .local_addr()
            .expect("test server must have address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("Telegram request must connect");
            let request = read_http_request(&mut stream);
            let body = r#"{"ok":true,"result":{"message_id":42,"date":0,"chat":{"id":-1001234567890,"type":"supergroup","title":"Test"},"text":"hello"}}"#;
            write_http_response(&mut stream, 200, body);
            request
        });

        let api_url = Url::parse(&format!("http://{address}/")).expect("API URL must parse");
        let github = GitHubClient::with_api_root("github-token".to_owned(), api_url.clone())
            .expect("GitHub client must be created");
        let telegram =
            TelegramClient::with_api_url("telegram-token".to_owned(), -1001234567890, api_url);
        let publisher = TelegramReportPublisher::new(&github, &telegram, MessageThreadId(123));

        let published = publisher
            .publish_telegram_report(TelegramRichText::plain("hello"), Vec::new())
            .await
            .expect("Telegram report must be published");

        assert_eq!(published.root_message_id, TelegramMessageId(42));
        assert!(published.attachment_message_ids.is_empty());
        assert_eq!(published.message_count(), 1);
        let request = server.join().expect("Telegram server must stop");
        assert!(
            request
                .lines()
                .next()
                .is_some_and(|line| line.to_ascii_lowercase().contains("sendmessage")),
            "unexpected Telegram request: {request}"
        );
        assert!(request.contains("\"message_thread_id\":123"));
        assert!(request.contains("\"text\":\"hello\""));
    }

    #[tokio::test]
    async fn fingerprint_mismatch_fails_before_any_telegram_message_is_sent() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("test server must bind");
        let address = listener
            .local_addr()
            .expect("test server must have address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("download request must connect");
            let _request = read_http_request(&mut stream);
            write_http_response(&mut stream, 200, "archive");
        });

        let api_url = Url::parse(&format!("http://{address}/")).expect("API URL must parse");
        let github = GitHubClient::with_api_root("github-token".to_owned(), api_url.clone())
            .expect("GitHub client must be created");
        let telegram = TelegramClient::new("telegram-token".to_owned(), -1001234567890);
        let publisher = TelegramReportPublisher::new(&github, &telegram, MessageThreadId(123));
        let attachment = TelegramReportAttachment::new(
            "archive.zip",
            format!("http://{address}/archive.zip"),
            AttachmentDownloadMethod::GitHubReleaseAsset,
            Some(format!("sha256:{}", "0".repeat(64))),
        );

        let failure = publisher
            .publish_telegram_report(TelegramRichText::plain("release"), vec![attachment])
            .await
            .expect_err("a mismatched fingerprint must fail publication");

        assert_eq!(
            failure.code,
            TelegramReportPublishStatusCode::AttachmentVerificationFailed
        );
        assert!(failure.published_message_ids.is_empty());
        server.join().expect("download server must stop");
    }

    fn read_http_request(stream: &mut TcpStream) -> String {
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        let header_end = loop {
            let read = stream.read(&mut buffer).expect("request must be readable");
            assert_ne!(read, 0, "request ended before its headers");
            request.extend_from_slice(&buffer[..read]);
            if let Some(index) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let headers = String::from_utf8_lossy(&request[..header_end]);
        let content_length = headers
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .and_then(|value| value.trim().parse::<usize>().ok())
            })
            .unwrap_or(0);
        while request.len() < header_end + content_length {
            let read = stream
                .read(&mut buffer)
                .expect("request body must be readable");
            assert_ne!(read, 0, "request ended before its body");
            request.extend_from_slice(&buffer[..read]);
        }
        String::from_utf8(request).expect("request must be UTF-8")
    }

    fn write_http_response(stream: &mut TcpStream, status: u16, body: &str) {
        let response = format!(
            "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream
            .write_all(response.as_bytes())
            .expect("response must be written");
    }
}
