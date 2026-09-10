use reqwest::StatusCode;
use serde::Deserialize;

use crate::commit::{CommitCandidate, CommitInfo};

use super::{GitHubClient, GitHubError};

const COMMIT_PAGE_SIZE: usize = 100;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum CommitComparisonStatus {
    Ahead,
    Behind,
    Diverged,
    Identical,
}

pub(crate) enum FetchedCommitComparison {
    BaseUnavailable,
    Compared {
        status: CommitComparisonStatus,
        commits: Vec<CommitCandidate>,
    },
}

#[derive(Deserialize)]
struct ApiCommitHead {
    sha: String,
}

#[derive(Deserialize)]
struct ApiCommit {
    sha: String,
    html_url: String,
    commit: ApiCommitDetails,
    parents: Vec<ApiCommitParent>,
}

#[derive(Deserialize)]
struct ApiCommitDetails {
    message: String,
    author: ApiCommitAuthor,
}

#[derive(Deserialize)]
struct ApiCommitAuthor {
    name: String,
    date: String,
}

#[derive(Deserialize)]
struct ApiCommitParent {
    sha: String,
}

#[derive(Deserialize)]
struct CompareResponse {
    status: CommitComparisonStatus,
    commits: Vec<ApiCommit>,
}

pub(crate) async fn fetch_branch_head(
    github: &GitHubClient,
    repository: &str,
    branch: &str,
) -> Result<String, GitHubError> {
    let url = github.repository_url(repository, &["commits", branch])?;
    let commit: ApiCommitHead = github.get_json(url).await?;
    Ok(commit.sha)
}

pub(crate) async fn fetch_commit_comparison(
    github: &GitHubClient,
    repository: &str,
    previous_sha: &str,
    head_sha: &str,
) -> Result<FetchedCommitComparison, GitHubError> {
    match fetch_comparison_pages(github, repository, previous_sha, head_sha).await {
        Ok(comparison) => Ok(comparison),
        Err(error) if is_stale_cursor_error(&error) => Ok(FetchedCommitComparison::BaseUnavailable),
        Err(error) => Err(error),
    }
}

async fn fetch_comparison_pages(
    github: &GitHubClient,
    repository: &str,
    previous_sha: &str,
    head_sha: &str,
) -> Result<FetchedCommitComparison, GitHubError> {
    let comparison = format!("{previous_sha}...{head_sha}");
    let base_url = github.repository_url(repository, &["compare", &comparison])?;
    let mut status = None;
    let mut commits = Vec::new();

    for page in 1.. {
        let mut url = base_url.clone();
        url.query_pairs_mut()
            .append_pair("per_page", &COMMIT_PAGE_SIZE.to_string())
            .append_pair("page", &page.to_string());
        let response: CompareResponse = github.get_json(url).await?;
        status.get_or_insert(response.status);
        let is_last_page = response.commits.len() < COMMIT_PAGE_SIZE;
        commits.extend(response.commits.into_iter().map(commit_candidate));

        if is_last_page {
            break;
        }
    }

    Ok(FetchedCommitComparison::Compared {
        status: status.expect("a comparison must fetch at least one page"),
        commits,
    })
}

fn is_stale_cursor_error(error: &GitHubError) -> bool {
    match error {
        GitHubError::Http { status, .. } if *status == StatusCode::NOT_FOUND => true,
        GitHubError::Http { status, message } if *status == StatusCode::UNPROCESSABLE_ENTITY => {
            let message = message.to_ascii_lowercase();
            message.contains("no common ancestor")
                || message.contains("commit not found")
                || message.contains("bad object")
        }
        _ => false,
    }
}

fn commit_candidate(commit: ApiCommit) -> CommitCandidate {
    CommitCandidate::new(
        CommitInfo::new(
            commit.sha,
            commit.commit.message,
            commit.commit.author.name,
            commit.commit.author.date,
            commit.html_url,
        ),
        commit
            .parents
            .into_iter()
            .map(|parent| parent.sha)
            .collect(),
    )
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

    use url::Url;

    use crate::github::GitHubClient;

    use super::{CommitComparisonStatus, FetchedCommitComparison};

    #[tokio::test]
    async fn comparison_uses_the_pinned_head_sha() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("test server must bind");
        let address = listener
            .local_addr()
            .expect("test server must have an address");
        let (request_sender, request_receiver) = mpsc::channel();
        let server = thread::spawn(move || {
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
                        .expect("request must be UTF-8")
                        .to_ascii_lowercase(),
                )
                .expect("request must be reported");
            let body = concat!(
                "{\"status\":\"ahead\",\"commits\":[{",
                "\"sha\":\"head-sha\",",
                "\"html_url\":\"https://github.com/owner/repo/commit/head-sha\",",
                "\"commit\":{\"message\":\"Commit head\",",
                "\"author\":{\"name\":\"octocat\",",
                "\"date\":\"2026-09-08T00:00:00Z\"}},",
                "\"parents\":[{\"sha\":\"previous-sha\"}]}]}"
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .expect("response must be written");
        });

        let api_root = Url::parse(&format!("http://{address}/")).expect("test URL must parse");
        let github = GitHubClient::with_api_root(Some("test-token".to_owned()), api_root)
            .expect("GitHub client must be created");
        let fetched =
            super::fetch_commit_comparison(&github, "owner/repo", "previous-sha", "head-sha")
                .await
                .expect("comparison must succeed");

        let FetchedCommitComparison::Compared { status, commits } = fetched else {
            panic!("a successful comparison must return commits");
        };
        assert_eq!(status, CommitComparisonStatus::Ahead);
        assert_eq!(commits.len(), 1);
        let request = request_receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("request must be captured");
        assert!(request.starts_with(
            "get /repos/owner/repo/compare/previous-sha...head-sha?per_page=100&page=1 http/1.1"
        ));
        server.join().expect("test server must stop");
    }
}
