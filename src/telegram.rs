use std::{
    io::{self, IsTerminal},
    path::Path,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use teloxide::{
    ApiError, RequestError,
    prelude::*,
    types::{
        ChatFullInfo, ChatMember, InputFile, InputMedia, InputMediaDocument, Me, MessageId,
        ParseMode, ThreadId, UserId,
    },
};
use thiserror::Error;
use tokio::{
    fs::File,
    io::{AsyncRead, ReadBuf},
};

const UPLOAD_PROGRESS_TEMPLATE: &str =
    "{msg} [{wide_bar:.green/blue}] {bytes}/{total_bytes} {bytes_per_sec} {eta}";
const UPLOAD_SPINNER_TEMPLATE: &str = "{spinner} {msg} {bytes} {bytes_per_sec}";

#[derive(Clone)]
pub struct TelegramClient {
    bot: Bot,
    group_id: ChatId,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MessageThreadId(pub i32);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TelegramMessageId(pub i32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TelegramErrorCode {
    GroupNotFound,
    InvalidBotToken,
    BotNotMember,
    PermissionDenied,
    RateLimited,
    NetworkError,
    TelegramApiError,
    InvalidTelegramResponse,
    LocalIoError,
}

#[derive(Debug, Error)]
pub enum TelegramError {
    #[error("Telegram request failed: {0}")]
    Request(#[from] RequestError),
}

impl TelegramError {
    pub fn error_code(&self) -> TelegramErrorCode {
        match self {
            Self::Request(RequestError::Api(ApiError::InvalidToken)) => {
                TelegramErrorCode::InvalidBotToken
            }
            Self::Request(RequestError::Api(ApiError::ChatNotFound)) => {
                TelegramErrorCode::GroupNotFound
            }
            Self::Request(RequestError::Api(
                ApiError::BotKicked
                | ApiError::BotKickedFromSupergroup
                | ApiError::BotKickedFromChannel,
            )) => TelegramErrorCode::BotNotMember,
            Self::Request(RequestError::RetryAfter(_)) => TelegramErrorCode::RateLimited,
            Self::Request(RequestError::Network(_)) => TelegramErrorCode::NetworkError,
            Self::Request(RequestError::InvalidJson { .. }) => {
                TelegramErrorCode::InvalidTelegramResponse
            }
            Self::Request(RequestError::Io(_)) => TelegramErrorCode::LocalIoError,
            Self::Request(RequestError::Api(api_error))
                if Self::api_error_is_permission_denied(api_error) =>
            {
                TelegramErrorCode::PermissionDenied
            }
            Self::Request(RequestError::Api(_))
            | Self::Request(RequestError::MigrateToChatId(_)) => {
                TelegramErrorCode::TelegramApiError
            }
        }
    }

    pub fn is_topic_missing(&self) -> bool {
        let message = self.description().to_ascii_lowercase();
        message.contains("message thread not found")
            || message.contains("thread_id_invalid")
            || message.contains("topic_id_invalid")
            || message.contains("topic not found")
    }

    pub fn is_topic_not_modified(&self) -> bool {
        let message = self.description().to_ascii_lowercase();
        message.contains("topic_not_modified") || message.contains("topic is not modified")
    }

    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::Request(RequestError::RetryAfter(seconds)) => Some(seconds.duration()),
            _ => None,
        }
    }

    fn description(&self) -> String {
        match self {
            Self::Request(RequestError::Api(ApiError::Unknown(message))) => message.clone(),
            _ => self.to_string(),
        }
    }

    fn api_error_is_permission_denied(api_error: &ApiError) -> bool {
        Self::api_error_is_permission_denied_description(&api_error.to_string())
    }

    fn api_error_is_permission_denied_description(message: &str) -> bool {
        let message = message.to_ascii_lowercase();
        message.contains("not enough rights")
            || message.contains("administrator rights")
            || message.contains("forbidden:")
            || message.contains("bot was kicked")
    }
}

impl TelegramClient {
    pub fn new(bot_token: String, group_id: i64) -> Self {
        Self {
            bot: Bot::new(bot_token),
            group_id: ChatId(group_id),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_api_url(bot_token: String, group_id: i64, api_url: url::Url) -> Self {
        Self {
            bot: Bot::new(bot_token).set_api_url(api_url),
            group_id: ChatId(group_id),
        }
    }

    pub fn group_id(&self) -> i64 {
        self.group_id.0
    }

    pub async fn get_chat_info(&self) -> Result<ChatFullInfo, TelegramError> {
        Ok(self.bot.get_chat(self.group_id).await?)
    }

    pub async fn get_bot_identity(&self) -> Result<Me, TelegramError> {
        Ok(self.bot.get_me().await?)
    }

    pub async fn get_chat_member(&self, user_id: UserId) -> Result<ChatMember, TelegramError> {
        Ok(self.bot.get_chat_member(self.group_id, user_id).await?)
    }

    pub async fn create_topic(&self, name: &str) -> Result<MessageThreadId, TelegramError> {
        let topic = self.bot.create_forum_topic(self.group_id, name).await?;
        Ok(MessageThreadId(topic.thread_id.0.0))
    }

    pub async fn rename_topic(
        &self,
        thread_id: MessageThreadId,
        name: &str,
    ) -> Result<(), TelegramError> {
        self.bot
            .edit_forum_topic(self.group_id, thread_id.into())
            .name(name)
            .await?;
        Ok(())
    }

    pub async fn close_topic(&self, thread_id: MessageThreadId) -> Result<(), TelegramError> {
        self.bot
            .close_forum_topic(self.group_id, thread_id.into())
            .await?;
        Ok(())
    }

    pub(crate) async fn send_text(
        &self,
        thread_id: MessageThreadId,
        text: &str,
        parse_mode: Option<ParseMode>,
    ) -> Result<TelegramMessageId, TelegramError> {
        let request = self
            .bot
            .send_message(self.group_id, text)
            .message_thread_id(thread_id.into());
        let message = match parse_mode {
            Some(parse_mode) => request.parse_mode(parse_mode).await?,
            None => request.await?,
        };
        Ok(TelegramMessageId(message.id.0))
    }

    pub(crate) async fn send_file(
        &self,
        thread_id: MessageThreadId,
        path: &Path,
        caption: Option<&str>,
    ) -> Result<TelegramMessageId, TelegramError> {
        let file = File::open(path).await.map_err(telegram_io_error)?;
        let total_bytes = file.metadata().await.map_err(telegram_io_error)?.len();
        let progress = create_upload_progress(total_bytes);
        let document = InputFile::read(UploadProgressReader::new(file, progress.clone()));
        let request = self
            .bot
            .send_document(
                self.group_id,
                document.file_name(
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or_default()
                        .to_owned(),
                ),
            )
            .message_thread_id(thread_id.into());

        let result = match caption {
            Some(caption) => request.caption(caption).await,
            None => request.await,
        };

        match result {
            Ok(message) => {
                progress.finish();
                Ok(TelegramMessageId(message.id.0))
            }
            Err(error) => {
                progress.finish_and_clear();
                Err(error.into())
            }
        }
    }

    pub(crate) async fn send_files(
        &self,
        thread_id: MessageThreadId,
        files: &[(&Path, &str)],
    ) -> Result<Vec<TelegramMessageId>, TelegramError> {
        debug_assert!((2..=10).contains(&files.len()));

        let mut opened_files = Vec::with_capacity(files.len());
        let mut total_bytes = 0_u64;
        for &(path, file_name) in files {
            let file = File::open(path).await.map_err(telegram_io_error)?;
            total_bytes =
                total_bytes.saturating_add(file.metadata().await.map_err(telegram_io_error)?.len());
            opened_files.push((file, file_name.to_owned()));
        }

        let progress = create_upload_progress(total_bytes);
        let media = opened_files
            .into_iter()
            .map(|(file, file_name)| {
                let input_file = InputFile::read(UploadProgressReader::new(file, progress.clone()))
                    .file_name(file_name);
                InputMedia::Document(InputMediaDocument::new(input_file))
            })
            .collect::<Vec<_>>();

        let result = self
            .bot
            .send_media_group(self.group_id, media)
            .message_thread_id(thread_id.into())
            .await;

        match result {
            Ok(messages) => {
                progress.finish();
                Ok(messages
                    .into_iter()
                    .map(|message| TelegramMessageId(message.id.0))
                    .collect())
            }
            Err(error) => {
                progress.finish_and_clear();
                Err(error.into())
            }
        }
    }
}

fn telegram_io_error(error: io::Error) -> TelegramError {
    TelegramError::from(RequestError::Io(Arc::new(error)))
}

fn create_upload_progress(total_bytes: u64) -> ProgressBar {
    let progress = if total_bytes == 0 {
        ProgressBar::new_spinner()
    } else {
        ProgressBar::new(total_bytes)
    };
    let draw_target = if io::stderr().is_terminal() {
        ProgressDrawTarget::stderr()
    } else {
        ProgressDrawTarget::hidden()
    };
    let template = if total_bytes == 0 {
        UPLOAD_SPINNER_TEMPLATE
    } else {
        UPLOAD_PROGRESS_TEMPLATE
    };

    progress.set_draw_target(draw_target);
    progress.set_style(
        ProgressStyle::with_template(template).expect("upload progress template must be valid"),
    );
    progress.set_message("Telegram upload");
    if total_bytes == 0 {
        progress.enable_steady_tick(Duration::from_millis(100));
    }
    progress
}

struct UploadProgressReader<R> {
    reader: R,
    progress: ProgressBar,
}

impl<R> UploadProgressReader<R> {
    fn new(reader: R, progress: ProgressBar) -> Self {
        Self { reader, progress }
    }
}

impl<R> AsyncRead for UploadProgressReader<R>
where
    R: AsyncRead + Unpin,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.as_mut().get_mut();
        let bytes_before = buffer.filled().len();
        let result = Pin::new(&mut this.reader).poll_read(context, buffer);
        if let Poll::Ready(Ok(())) = &result {
            let bytes_read = buffer.filled().len() - bytes_before;
            if bytes_read > 0 {
                this.progress.inc(bytes_read as u64);
            }
        }
        result
    }
}

impl From<MessageThreadId> for ThreadId {
    fn from(value: MessageThreadId) -> Self {
        Self(MessageId(value.0))
    }
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

    use super::*;

    #[test]
    fn classifies_missing_topic_error() {
        let error = TelegramError::from(RequestError::Api(ApiError::Unknown(
            "Bad Request: message thread not found".to_owned(),
        )));

        assert!(error.is_topic_missing());
        assert_eq!(error.error_code(), TelegramErrorCode::TelegramApiError);
    }

    #[test]
    fn classifies_rate_limit_error() {
        let error = TelegramError::from(RequestError::RetryAfter(
            teloxide::types::Seconds::from_seconds(5),
        ));

        assert_eq!(error.error_code(), TelegramErrorCode::RateLimited);
    }

    #[tokio::test]
    async fn sends_document_media_group_and_returns_all_message_ids() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("test server must bind");
        let address = listener
            .local_addr()
            .expect("test server must have address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("Telegram request must connect");
            let request = read_http_request(&mut stream);
            let body = r#"{"ok":true,"result":[{"message_id":43,"date":0,"chat":{"id":-1001234567890,"type":"supergroup","title":"Test"}},{"message_id":44,"date":0,"chat":{"id":-1001234567890,"type":"supergroup","title":"Test"}}]}"#;
            write_http_response(&mut stream, 200, body);
            request
        });

        let directory = TempDir::new().expect("temporary directory must be created");
        let first_path = directory.path().join("first.zip");
        let second_path = directory.path().join("second.zip");
        fs::write(&first_path, b"first").expect("first test file must be written");
        fs::write(&second_path, b"second").expect("second test file must be written");

        let api_url = url::Url::parse(&format!("http://{address}/")).expect("API URL must parse");
        let telegram =
            TelegramClient::with_api_url("telegram-token".to_owned(), -1001234567890, api_url);
        let message_ids = telegram
            .send_files(
                MessageThreadId(123),
                &[(&first_path, "first.zip"), (&second_path, "second.zip")],
            )
            .await
            .expect("Telegram media group must be sent");

        assert_eq!(
            message_ids,
            vec![TelegramMessageId(43), TelegramMessageId(44)]
        );
        let request = server.join().expect("Telegram server must stop");
        assert!(
            request
                .lines()
                .next()
                .is_some_and(|line| line.to_ascii_lowercase().contains("sendmediagroup")),
            "unexpected Telegram request: {request}"
        );
        assert!(request.contains("name=\"message_thread_id\""));
        assert!(request.contains("123"));
        assert!(request.contains("first.zip"), "request: {request}");
        assert!(request.contains("second.zip"), "request: {request}");
        assert!(request.contains("first"), "request: {request}");
        assert!(request.contains("second"), "request: {request}");
    }

    fn read_http_request(stream: &mut TcpStream) -> String {
        let mut headers = Vec::new();
        let mut buffer = [0_u8; 1];
        loop {
            stream
                .read_exact(&mut buffer)
                .expect("request headers must be readable");
            headers.push(buffer[0]);
            if headers.ends_with(b"\r\n\r\n") {
                break;
            }
        }

        let header_text = String::from_utf8_lossy(&headers);
        let content_length = header_text
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .and_then(|value| value.trim().parse::<usize>().ok())
            })
            .unwrap_or_default();
        let is_chunked = header_text.lines().any(|line| {
            line.to_ascii_lowercase()
                .starts_with("transfer-encoding: chunked")
        });

        let mut body = Vec::new();
        if content_length > 0 {
            let mut content = vec![0_u8; content_length];
            stream
                .read_exact(&mut content)
                .expect("request body must be readable");
            body.extend(content);
        } else if is_chunked {
            loop {
                let mut size_line = Vec::new();
                loop {
                    stream
                        .read_exact(&mut buffer)
                        .expect("chunk size must be readable");
                    size_line.push(buffer[0]);
                    if size_line.ends_with(b"\r\n") {
                        break;
                    }
                }
                let size_line = String::from_utf8_lossy(&size_line);
                let size = usize::from_str_radix(size_line.trim(), 16)
                    .expect("chunk size must be hexadecimal");
                if size == 0 {
                    stream
                        .read_exact(&mut [0_u8; 2])
                        .expect("chunk terminator must be readable");
                    break;
                }

                let mut chunk = vec![0_u8; size];
                stream
                    .read_exact(&mut chunk)
                    .expect("chunk body must be readable");
                body.extend(chunk);
                stream
                    .read_exact(&mut [0_u8; 2])
                    .expect("chunk separator must be readable");
            }
        }

        headers.extend(body);
        String::from_utf8(headers).expect("request must be UTF-8")
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
