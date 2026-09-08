pub mod actions;
pub mod commits;
pub mod releases;
mod repository_id;

pub use repository_id::RepositoryId;
pub(crate) use repository_id::resolve_github_repository_id;

use std::{
    io::{self, IsTerminal},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use reqwest::{
    Client, StatusCode, Url,
    header::{
        ACCEPT, AUTHORIZATION, ETAG, HeaderMap, HeaderValue, IF_NONE_MATCH, RETRY_AFTER, USER_AGENT,
    },
};
use serde::de::DeserializeOwned;
use thiserror::Error;
use tokio::{fs::File, io::AsyncWriteExt, time::sleep};

const API_ROOT: &str = "https://api.github.com/";
const USER_AGENT_VALUE: &str = "BetterCI/0.1";
const MAX_GET_ATTEMPTS: usize = 3;
const INITIAL_RETRY_DELAY: Duration = Duration::from_millis(500);
const MAX_RETRY_AFTER: u64 = 30;
const RELEASE_ASSET_ACCEPT: &str = "application/octet-stream";
const ACTIONS_ARTIFACT_ACCEPT: &str = "application/vnd.github+json";
const DOWNLOAD_PROGRESS_TEMPLATE: &str =
    "{msg} [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} {bytes_per_sec} {eta}";
const DOWNLOAD_SPINNER_TEMPLATE: &str = "{spinner} {msg} {bytes} {bytes_per_sec}";

#[derive(Clone)]
pub struct GitHubClient {
    client: Client,
    api_root: Url,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RepositoryName<'a> {
    pub owner: &'a str,
    pub name: &'a str,
}

pub(super) enum ConditionalJson<T> {
    NotModified,
    NotFound { etag: Option<String> },
    Modified { value: T, etag: Option<String> },
}

#[derive(Debug, Error)]
pub enum GitHubError {
    #[error("GitHub token cannot be used as an HTTP header: {0}")]
    InvalidToken(#[source] reqwest::header::InvalidHeaderValue),
    #[error("GitHub ETag in state cannot be used as an HTTP header: {0}")]
    InvalidEtag(#[source] reqwest::header::InvalidHeaderValue),
    #[error("failed to create GitHub HTTP client: {0}")]
    BuildClient(#[source] reqwest::Error),
    #[error("invalid GitHub API URL: {0}")]
    InvalidUrl(#[source] url::ParseError),
    #[error("invalid repository name {0}; expected owner/repo format")]
    InvalidRepository(String),
    #[error("GitHub returned an invalid Repository ID")]
    InvalidRepositoryId,
    #[error("failed to construct GitHub API URL")]
    CannotBuildUrl,
    #[error("GitHub request {url} failed: {source}")]
    Request {
        url: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("failed to parse GitHub response {url}: {source}")]
    Decode {
        url: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("GitHub returned HTTP {status}: {message}")]
    Http { status: StatusCode, message: String },
    #[error("failed to create download file {path}: {source}")]
    CreateDownload {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write download file {path}: {source}")]
    WriteDownload {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("remote file name is not a safe single file name: {0}")]
    InvalidFileName(String),
    #[error("workflow {workflow_file} does not exist")]
    WorkflowNotFound { workflow_file: String },
    #[error("workflow run {run_id} does not exist")]
    WorkflowRunNotFound { run_id: u64 },
}

impl GitHubClient {
    pub fn new(token: String) -> Result<Self, GitHubError> {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(USER_AGENT_VALUE));
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );

        let mut authorization =
            HeaderValue::from_str(&format!("Bearer {token}")).map_err(GitHubError::InvalidToken)?;
        authorization.set_sensitive(true);
        headers.insert(AUTHORIZATION, authorization);

        let client = Client::builder()
            .default_headers(headers)
            .build()
            .map_err(GitHubError::BuildClient)?;
        let api_root = Url::parse(API_ROOT).map_err(GitHubError::InvalidUrl)?;

        Ok(Self { client, api_root })
    }

    #[cfg(test)]
    pub(crate) fn with_api_root(token: String, api_root: Url) -> Result<Self, GitHubError> {
        let mut github = Self::new(token)?;
        github.api_root = api_root;
        Ok(github)
    }

    pub(super) fn repository_url(
        &self,
        repository: &str,
        trailing_segments: &[&str],
    ) -> Result<Url, GitHubError> {
        let repository = parse_repository(repository)?;
        let mut url = self.api_root.clone();
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| GitHubError::CannotBuildUrl)?;
        segments.extend(["repos", repository.owner, repository.name]);
        segments.extend(trailing_segments.iter().copied());
        drop(segments);
        Ok(url)
    }

    pub(super) async fn get_json<T>(&self, url: Url) -> Result<T, GitHubError>
    where
        T: DeserializeOwned,
    {
        let request_url = url.to_string();
        for attempt in 0..MAX_GET_ATTEMPTS {
            let response = match self.client.get(url.clone()).send().await {
                Ok(response) => response,
                Err(source) if attempt + 1 < MAX_GET_ATTEMPTS => {
                    wait_before_retry(attempt, None).await;
                    continue;
                }
                Err(source) => {
                    return Err(GitHubError::Request {
                        url: request_url,
                        source,
                    });
                }
            };

            if should_retry_status(response.status()) && attempt + 1 < MAX_GET_ATTEMPTS {
                wait_before_retry(attempt, response.headers().get(RETRY_AFTER)).await;
                continue;
            }

            let response = checked_response(response).await?;
            match response.json().await {
                Ok(value) => return Ok(value),
                Err(source)
                    if attempt + 1 < MAX_GET_ATTEMPTS && should_retry_response_error(&source) =>
                {
                    wait_before_retry(attempt, None).await;
                }
                Err(source) => {
                    return Err(GitHubError::Decode {
                        url: request_url,
                        source,
                    });
                }
            }
        }

        unreachable!("GitHub JSON request attempts must eventually terminate")
    }

    pub(super) async fn get_json_conditionally<T>(
        &self,
        url: Url,
        etag: Option<&str>,
    ) -> Result<ConditionalJson<T>, GitHubError>
    where
        T: DeserializeOwned,
    {
        let request_url = url.to_string();
        let etag_header = etag
            .map(HeaderValue::from_str)
            .transpose()
            .map_err(GitHubError::InvalidEtag)?;

        for attempt in 0..MAX_GET_ATTEMPTS {
            let mut request = self.client.get(url.clone());
            if let Some(etag) = &etag_header {
                request = request.header(IF_NONE_MATCH, etag.clone());
            }

            let response = match request.send().await {
                Ok(response) => response,
                Err(source) if attempt + 1 < MAX_GET_ATTEMPTS => {
                    wait_before_retry(attempt, None).await;
                    continue;
                }
                Err(source) => {
                    return Err(GitHubError::Request {
                        url: request_url,
                        source,
                    });
                }
            };

            if response.status() == StatusCode::NOT_MODIFIED {
                return Ok(ConditionalJson::NotModified);
            }

            if should_retry_status(response.status()) && attempt + 1 < MAX_GET_ATTEMPTS {
                wait_before_retry(attempt, response.headers().get(RETRY_AFTER)).await;
                continue;
            }

            let response_etag = response
                .headers()
                .get(ETAG)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);
            if response.status() == StatusCode::NOT_FOUND {
                return Ok(ConditionalJson::NotFound {
                    etag: response_etag,
                });
            }

            let response = checked_response(response).await?;
            match response.json().await {
                Ok(value) => {
                    return Ok(ConditionalJson::Modified {
                        value,
                        etag: response_etag,
                    });
                }
                Err(source)
                    if attempt + 1 < MAX_GET_ATTEMPTS && should_retry_response_error(&source) =>
                {
                    wait_before_retry(attempt, None).await;
                }
                Err(source) => {
                    return Err(GitHubError::Decode {
                        url: request_url,
                        source,
                    });
                }
            }
        }

        unreachable!("GitHub conditional JSON request attempts must eventually terminate")
    }

    pub(crate) async fn download_release_asset_to(
        &self,
        url: &Url,
        path: &Path,
    ) -> Result<(), GitHubError> {
        self.download_to(url, path, RELEASE_ASSET_ACCEPT).await
    }

    pub(crate) async fn download_actions_artifact_to(
        &self,
        url: &Url,
        path: &Path,
    ) -> Result<(), GitHubError> {
        self.download_to(url, path, ACTIONS_ARTIFACT_ACCEPT).await
    }

    async fn download_to(&self, url: &Url, path: &Path, accept: &str) -> Result<(), GitHubError> {
        let request_url = url.to_string();
        let file_name = download_file_name(path);
        let download_started_at = Instant::now();

        let translator = crate::i18n::current();
        println!(
            "{}",
            translator.console(crate::i18n::ConsoleMessage::DownloadStarted {
                file_name: &file_name,
            })
        );

        for attempt in 0..MAX_GET_ATTEMPTS {
            let mut response = match self
                .client
                .get(url.clone())
                .header(ACCEPT, accept)
                .send()
                .await
            {
                Ok(response) => response,
                Err(source) if attempt + 1 < MAX_GET_ATTEMPTS => {
                    eprintln!(
                        "{}",
                        translator.console(crate::i18n::ConsoleMessage::DownloadRetry {
                            file_name: &file_name,
                            attempt: attempt + 1,
                            max_attempts: MAX_GET_ATTEMPTS,
                            reason: crate::i18n::DownloadRetryReason::RequestFailed(
                                &source.to_string(),
                            ),
                        })
                    );
                    wait_before_retry(attempt, None).await;
                    continue;
                }
                Err(source) => {
                    return Err(GitHubError::Request {
                        url: request_url,
                        source,
                    });
                }
            };

            if should_retry_status(response.status()) && attempt + 1 < MAX_GET_ATTEMPTS {
                let status = response.status().to_string();
                eprintln!(
                    "{}",
                    translator.console(crate::i18n::ConsoleMessage::DownloadRetry {
                        file_name: &file_name,
                        attempt: attempt + 1,
                        max_attempts: MAX_GET_ATTEMPTS,
                        reason: crate::i18n::DownloadRetryReason::Http(&status),
                    })
                );
                wait_before_retry(attempt, response.headers().get(RETRY_AFTER)).await;
                continue;
            }
            response = checked_response(response).await?;
            let progress = create_download_progress(path, response.content_length());

            let mut file = match File::create(path).await {
                Ok(file) => file,
                Err(source) => {
                    progress.finish_and_clear();
                    return Err(GitHubError::CreateDownload {
                        path: path.to_path_buf(),
                        source,
                    });
                }
            };
            let mut retry_download = false;

            loop {
                match response.chunk().await {
                    Ok(Some(chunk)) => {
                        let chunk_length = chunk.len() as u64;
                        if let Err(source) = file.write_all(&chunk).await {
                            progress.finish_and_clear();
                            return Err(GitHubError::WriteDownload {
                                path: path.to_path_buf(),
                                source,
                            });
                        }
                        progress.inc(chunk_length);
                    }
                    Ok(None) => break,
                    Err(source) if attempt + 1 < MAX_GET_ATTEMPTS => {
                        progress.finish_and_clear();
                        eprintln!(
                            "{}",
                            translator.console(crate::i18n::ConsoleMessage::DownloadRetry {
                                file_name: &file_name,
                                attempt: attempt + 1,
                                max_attempts: MAX_GET_ATTEMPTS,
                                reason: crate::i18n::DownloadRetryReason::ResponseReadFailed(
                                    &source.to_string(),
                                ),
                            })
                        );
                        retry_download = true;
                        break;
                    }
                    Err(source) => {
                        progress.finish_and_clear();
                        return Err(GitHubError::Request {
                            url: request_url,
                            source,
                        });
                    }
                }
            }

            if retry_download {
                wait_before_retry(attempt, None).await;
                continue;
            }

            file.flush()
                .await
                .map_err(|source| GitHubError::WriteDownload {
                    path: path.to_path_buf(),
                    source,
                })
                .inspect_err(|_| progress.finish_and_clear())?;
            progress.finish();
            let elapsed = download_started_at.elapsed();
            let speed = format_bytes_per_second(progress.position(), elapsed);
            println!(
                "{}",
                translator.console(crate::i18n::ConsoleMessage::DownloadCompleted {
                    file_name: &file_name,
                    bytes: progress.position(),
                    elapsed_seconds: elapsed.as_secs_f64(),
                    speed: &speed,
                })
            );
            return Ok(());
        }

        unreachable!("GitHub download attempts must eventually terminate")
    }
}

fn create_download_progress(path: &Path, content_length: Option<u64>) -> ProgressBar {
    let progress = match content_length {
        Some(length) => ProgressBar::new(length),
        None => ProgressBar::new_spinner(),
    };
    let draw_target = if io::stderr().is_terminal() {
        ProgressDrawTarget::stderr()
    } else {
        ProgressDrawTarget::hidden()
    };
    let template = if content_length.is_some() {
        DOWNLOAD_PROGRESS_TEMPLATE
    } else {
        DOWNLOAD_SPINNER_TEMPLATE
    };

    progress.set_draw_target(draw_target);
    progress.set_style(
        ProgressStyle::with_template(template).expect("download progress template must be valid"),
    );
    progress.set_message(download_file_name(path));
    if content_length.is_none() {
        progress.enable_steady_tick(Duration::from_millis(100));
    }
    progress
}

fn download_file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            crate::i18n::current()
                .text(crate::i18n::Text::Attachment)
                .to_owned()
        })
}

fn format_bytes_per_second(bytes: u64, elapsed: Duration) -> String {
    let seconds = elapsed.as_secs_f64();
    if seconds <= f64::EPSILON {
        return "0 B".to_owned();
    }

    let rate = bytes as f64 / seconds;
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = rate;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

pub fn parse_repository(name: &str) -> Result<RepositoryName<'_>, GitHubError> {
    let Some((owner, repository)) = name.split_once('/') else {
        return Err(GitHubError::InvalidRepository(name.to_owned()));
    };

    if owner.is_empty() || repository.is_empty() || repository.contains('/') {
        return Err(GitHubError::InvalidRepository(name.to_owned()));
    }

    Ok(RepositoryName {
        owner,
        name: repository,
    })
}

async fn checked_response(response: reqwest::Response) -> Result<reqwest::Response, GitHubError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }

    let message = response
        .text()
        .await
        .unwrap_or_else(|error| error.to_string());
    let message: String = message.chars().take(1000).collect();
    Err(GitHubError::Http { status, message })
}

fn should_retry_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

fn should_retry_response_error(error: &reqwest::Error) -> bool {
    error.is_body() || error.is_decode()
}

async fn wait_before_retry(attempt: usize, retry_after: Option<&HeaderValue>) {
    let delay = retry_after
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(|seconds| Duration::from_secs(seconds.min(MAX_RETRY_AFTER)))
        .unwrap_or_else(|| {
            let multiplier = 1_u64 << attempt.min(4);
            Duration::from_millis(INITIAL_RETRY_DELAY.as_millis() as u64 * multiplier)
        });
    sleep(delay).await;
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::{Read, Write},
        net::TcpListener,
        thread,
        time::Duration,
    };

    use tempfile::TempDir;
    use url::Url;

    use super::{ConditionalJson, GitHubClient, format_bytes_per_second};

    #[test]
    fn formats_download_rate() {
        assert_eq!(
            format_bytes_per_second(1536, Duration::from_secs(1)),
            "1.5 KiB"
        );
        assert_eq!(
            format_bytes_per_second(2 * 1024 * 1024, Duration::from_secs(2)),
            "1.0 MiB"
        );
        assert_eq!(format_bytes_per_second(10, Duration::ZERO), "0 B");
    }

    #[tokio::test]
    async fn retries_conditional_json_decode() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("test server must bind");
        let address = listener
            .local_addr()
            .expect("test server must have address");
        let server = thread::spawn(move || {
            for body in ["{", "[]"] {
                let (mut stream, _) = listener.accept().expect("test request must connect");
                let mut request = Vec::new();
                let mut buffer = [0_u8; 1024];
                while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                    let read = stream.read(&mut buffer).expect("request must be readable");
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..read]);
                }

                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream
                    .write_all(response.as_bytes())
                    .expect("response must be written");
            }
        });

        let api_root = Url::parse(&format!("http://{address}/")).expect("test URL must parse");
        let github = GitHubClient::with_api_root("test-token".to_owned(), api_root)
            .expect("GitHub client must be created");
        let url =
            Url::parse(&format!("http://{address}/releases")).expect("release URL must parse");

        let result = github
            .get_json_conditionally::<Vec<serde_json::Value>>(url, None)
            .await
            .expect("a retry must recover from an invalid JSON response");

        assert!(matches!(
            result,
            ConditionalJson::Modified { value, .. } if value.is_empty()
        ));
        server.join().expect("test server must stop");
    }

    #[tokio::test]
    async fn retries_release_asset_download_with_binary_accept() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("test server must bind");
        let address = listener
            .local_addr()
            .expect("test server must have address");
        let server = thread::spawn(move || {
            let mut requests = Vec::new();
            for (status, body) in [(500, "retry"), (200, "archive")] {
                let (mut stream, _) = listener.accept().expect("test request must connect");
                let mut request = Vec::new();
                let mut buffer = [0_u8; 1024];
                while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                    let read = stream.read(&mut buffer).expect("request must be readable");
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..read]);
                }
                requests.push(String::from_utf8(request).expect("request must be UTF-8"));

                let response = format!(
                    "HTTP/1.1 {status} Test\r\nRetry-After: 0\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream
                    .write_all(response.as_bytes())
                    .expect("response must be written");
            }
            requests
        });

        let api_root = Url::parse(&format!("http://{address}/")).expect("test URL must parse");
        let github = GitHubClient::with_api_root("test-token".to_owned(), api_root)
            .expect("GitHub client must be created");
        let url = Url::parse(&format!("http://{address}/asset")).expect("asset URL must parse");
        let directory = TempDir::new().expect("temporary directory must be created");
        let path = directory.path().join("archive.zip");

        github
            .download_release_asset_to(&url, &path)
            .await
            .expect("the retry must download the asset");

        assert_eq!(fs::read(&path).expect("asset must be readable"), b"archive");
        let requests = server.join().expect("test server must stop");
        assert_eq!(requests.len(), 2);
        assert!(requests.iter().all(|request| {
            request
                .to_ascii_lowercase()
                .contains("accept: application/octet-stream")
        }));
    }
}
