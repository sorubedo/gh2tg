use serde::Deserialize;
use url::Url;

use crate::release::ReleaseSelector;

use super::{ConditionalJson, GitHubClient, GitHubError, RepositoryId};

const RELEASE_PAGE_SIZE: usize = 1;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ReleaseId(u64);

impl ReleaseId {
    pub fn value(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AssetFileName(String);

impl AssetFileName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct AssetId(u64);

impl AssetId {
    pub fn value(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AssetDigest(String);

impl AssetDigest {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ReleaseAsset {
    file_name: AssetFileName,
    id: AssetId,
    label: Option<String>,
    content_type: String,
    size: u64,
    digest: Option<AssetDigest>,
    download_count: u64,
    created_at: String,
    updated_at: String,
    browser_download_url: String,
}

impl ReleaseAsset {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        id: u64,
        name: String,
        label: Option<String>,
        content_type: String,
        size: u64,
        digest: Option<String>,
        download_count: u64,
        created_at: String,
        updated_at: String,
        browser_download_url: String,
    ) -> Self {
        Self {
            file_name: AssetFileName(name),
            id: AssetId(id),
            label,
            content_type,
            size,
            digest: digest.map(AssetDigest),
            download_count,
            created_at,
            updated_at,
            browser_download_url,
        }
    }

    pub fn file_name(&self) -> &AssetFileName {
        &self.file_name
    }

    pub fn asset_id(&self) -> AssetId {
        self.id
    }

    pub fn digest(&self) -> Option<&AssetDigest> {
        self.digest.as_ref()
    }

    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    pub fn content_type(&self) -> &str {
        &self.content_type
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn download_count(&self) -> u64 {
        self.download_count
    }

    pub fn created_at(&self) -> &str {
        &self.created_at
    }

    pub fn updated_at(&self) -> &str {
        &self.updated_at
    }

    pub fn browser_download_url(&self) -> &str {
        &self.browser_download_url
    }
}

#[derive(Clone, Debug)]
pub struct ReleaseSnapshot {
    id: ReleaseId,
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    html_url: String,
    target_commitish: String,
    draft: bool,
    prerelease: bool,
    immutable: Option<bool>,
    author_login: Option<String>,
    created_at: String,
    updated_at: Option<String>,
    published_at: Option<String>,
    assets: Vec<ReleaseAsset>,
}

impl ReleaseSnapshot {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        id: u64,
        tag_name: String,
        name: Option<String>,
        body: Option<String>,
        html_url: String,
        target_commitish: String,
        draft: bool,
        prerelease: bool,
        immutable: Option<bool>,
        author_login: Option<String>,
        created_at: String,
        updated_at: Option<String>,
        published_at: Option<String>,
        assets: Vec<ReleaseAsset>,
    ) -> Self {
        Self {
            id: ReleaseId(id),
            tag_name,
            name,
            body,
            html_url,
            target_commitish,
            draft,
            prerelease,
            immutable,
            author_login,
            created_at,
            updated_at,
            published_at,
            assets,
        }
    }

    pub fn id(&self) -> ReleaseId {
        self.id
    }

    pub fn tag_name(&self) -> &str {
        &self.tag_name
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub fn body(&self) -> Option<&str> {
        self.body.as_deref()
    }

    pub fn html_url(&self) -> &str {
        &self.html_url
    }

    pub fn target_commitish(&self) -> &str {
        &self.target_commitish
    }

    pub fn draft(&self) -> bool {
        self.draft
    }

    pub fn prerelease(&self) -> bool {
        self.prerelease
    }

    pub fn immutable(&self) -> Option<bool> {
        self.immutable
    }

    pub fn author_login(&self) -> Option<&str> {
        self.author_login.as_deref()
    }

    pub fn created_at(&self) -> &str {
        &self.created_at
    }

    pub fn updated_at(&self) -> Option<&str> {
        self.updated_at.as_deref()
    }

    pub fn published_at(&self) -> Option<&str> {
        self.published_at.as_deref()
    }

    pub fn assets(&self) -> &[ReleaseAsset] {
        &self.assets
    }
}

pub(crate) enum FetchedRelease {
    NotModified,
    NotFound {
        etag: Option<String>,
    },
    Found {
        release: Box<ReleaseSnapshot>,
        etag: Option<String>,
    },
}

pub(crate) async fn fetch_selected_release(
    github: &GitHubClient,
    repository_id: &RepositoryId,
    selector: &ReleaseSelector,
    etag: Option<&str>,
) -> Result<FetchedRelease, GitHubError> {
    let query_url = release_query_url(github, repository_id, selector)?;
    match selector {
        ReleaseSelector::Tag(_) => fetch_release_by_tag(github, query_url, etag).await,
        ReleaseSelector::LatestIncludingPrereleases => {
            fetch_latest_release_including_prereleases(github, repository_id, query_url, etag).await
        }
        ReleaseSelector::LatestStable => fetch_latest_stable_release(github, query_url, etag).await,
    }
}

pub fn release_asset_download_url(
    github: &GitHubClient,
    repository_id: &RepositoryId,
    asset: &ReleaseAsset,
) -> Result<Url, GitHubError> {
    let asset_id = asset.asset_id().value().to_string();
    github.repository_url(
        repository_id.full_name(),
        &["releases", "assets", &asset_id],
    )
}

async fn fetch_latest_stable_release(
    github: &GitHubClient,
    url: Url,
    etag: Option<&str>,
) -> Result<FetchedRelease, GitHubError> {
    fetch_single_release(github, url, etag).await
}

async fn fetch_latest_release_including_prereleases(
    github: &GitHubClient,
    repository_id: &RepositoryId,
    first_page_url: Url,
    etag: Option<&str>,
) -> Result<FetchedRelease, GitHubError> {
    let (releases, etag) = match github
        .get_json_conditionally::<Vec<ApiRelease>>(first_page_url, etag)
        .await?
    {
        ConditionalJson::NotModified => return Ok(FetchedRelease::NotModified),
        ConditionalJson::NotFound { etag } => return Ok(FetchedRelease::NotFound { etag }),
        ConditionalJson::Modified { value, etag } => (value, etag),
    };
    if let Some(release) = first_published_release(&releases) {
        return Ok(FetchedRelease::Found {
            release: Box::new(release_snapshot(release.clone())),
            etag,
        });
    }
    if releases.is_empty() {
        return Ok(FetchedRelease::NotFound { etag });
    }

    for page in 2.. {
        let mut url = github.repository_url(repository_id.full_name(), &["releases"])?;
        url.query_pairs_mut()
            .append_pair("per_page", &RELEASE_PAGE_SIZE.to_string())
            .append_pair("page", &page.to_string());
        let releases: Vec<ApiRelease> = github.get_json(url).await?;
        if let Some(release) = first_published_release(&releases) {
            return Ok(FetchedRelease::Found {
                release: Box::new(release_snapshot(release.clone())),
                etag,
            });
        }
        if releases.is_empty() {
            return Ok(FetchedRelease::NotFound { etag });
        }
    }

    unreachable!("release pagination must eventually terminate")
}

async fn fetch_release_by_tag(
    github: &GitHubClient,
    url: Url,
    etag: Option<&str>,
) -> Result<FetchedRelease, GitHubError> {
    fetch_single_release(github, url, etag).await
}

async fn fetch_single_release(
    github: &GitHubClient,
    url: Url,
    etag: Option<&str>,
) -> Result<FetchedRelease, GitHubError> {
    match github.get_json_conditionally(url, etag).await? {
        ConditionalJson::NotModified => Ok(FetchedRelease::NotModified),
        ConditionalJson::NotFound { etag } => Ok(FetchedRelease::NotFound { etag }),
        ConditionalJson::Modified {
            value: release,
            etag,
        } => Ok(FetchedRelease::Found {
            release: Box::new(release_snapshot(release)),
            etag,
        }),
    }
}

fn release_query_url(
    github: &GitHubClient,
    repository_id: &RepositoryId,
    selector: &ReleaseSelector,
) -> Result<Url, GitHubError> {
    match selector {
        ReleaseSelector::Tag(tag) => github.repository_url(
            repository_id.full_name(),
            &["releases", "tags", tag.as_str()],
        ),
        ReleaseSelector::LatestIncludingPrereleases => {
            let mut url = github.repository_url(repository_id.full_name(), &["releases"])?;
            url.query_pairs_mut()
                .append_pair("per_page", &RELEASE_PAGE_SIZE.to_string())
                .append_pair("page", "1");
            Ok(url)
        }
        ReleaseSelector::LatestStable => {
            github.repository_url(repository_id.full_name(), &["releases", "latest"])
        }
    }
}

fn first_published_release(releases: &[ApiRelease]) -> Option<&ApiRelease> {
    releases.iter().find(|release| !release.draft)
}

fn release_snapshot(release: ApiRelease) -> ReleaseSnapshot {
    ReleaseSnapshot::new(
        release.id,
        release.tag_name,
        release.name,
        release.body,
        release.html_url,
        release.target_commitish,
        release.draft,
        release.prerelease,
        release.immutable,
        release.author.map(|author| author.login),
        release.created_at,
        release.updated_at,
        release.published_at,
        release
            .assets
            .into_iter()
            .map(|asset| {
                ReleaseAsset::new(
                    asset.id,
                    asset.name,
                    asset.label,
                    asset.content_type,
                    asset.size,
                    asset.digest,
                    asset.download_count,
                    asset.created_at,
                    asset.updated_at,
                    asset.browser_download_url,
                )
            })
            .collect(),
    )
}

#[derive(Clone, Deserialize)]
struct ApiRelease {
    id: u64,
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    html_url: String,
    target_commitish: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    immutable: Option<bool>,
    author: Option<ApiUser>,
    created_at: String,
    #[serde(default)]
    updated_at: Option<String>,
    published_at: Option<String>,
    assets: Vec<ApiReleaseAsset>,
}

#[derive(Clone, Deserialize)]
struct ApiReleaseAsset {
    id: u64,
    name: String,
    label: Option<String>,
    content_type: String,
    size: u64,
    digest: Option<String>,
    download_count: u64,
    created_at: String,
    updated_at: String,
    browser_download_url: String,
}

#[derive(Clone, Deserialize)]
struct ApiUser {
    login: String,
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

    use crate::{
        github::{GitHubClient, RepositoryId, parse_repository},
        release::ReleaseSelector,
    };

    use super::{
        ApiRelease, ApiReleaseAsset, ApiUser, FetchedRelease, release_query_url, release_snapshot,
    };

    #[test]
    fn snapshot_keeps_every_release_asset() {
        let snapshot = release_snapshot(ApiRelease {
            id: 10,
            tag_name: "v1.0.0".to_owned(),
            name: Some("Release 1.0.0".to_owned()),
            body: Some("Release notes".to_owned()),
            html_url: "https://github.com/owner/repo/releases/10".to_owned(),
            target_commitish: "main".to_owned(),
            draft: false,
            prerelease: false,
            immutable: Some(false),
            author: Some(ApiUser {
                login: "octocat".to_owned(),
            }),
            created_at: "2026-09-01T00:00:00Z".to_owned(),
            updated_at: Some("2026-09-01T00:00:00Z".to_owned()),
            published_at: Some("2026-09-01T00:00:00Z".to_owned()),
            assets: vec![
                ApiReleaseAsset {
                    id: 20,
                    name: "archive.zip".to_owned(),
                    label: None,
                    content_type: "application/zip".to_owned(),
                    size: 10,
                    digest: Some("sha256:archive".to_owned()),
                    download_count: 0,
                    created_at: "2026-09-01T00:00:00Z".to_owned(),
                    updated_at: "2026-09-01T00:00:00Z".to_owned(),
                    browser_download_url: "https://example.com/archive.zip".to_owned(),
                },
                ApiReleaseAsset {
                    id: 21,
                    name: "notes.txt".to_owned(),
                    label: None,
                    content_type: "text/plain".to_owned(),
                    size: 10,
                    digest: Some("sha256:notes".to_owned()),
                    download_count: 0,
                    created_at: "2026-09-01T00:00:00Z".to_owned(),
                    updated_at: "2026-09-01T00:00:00Z".to_owned(),
                    browser_download_url: "https://example.com/notes.txt".to_owned(),
                },
            ],
        });

        assert_eq!(snapshot.assets().len(), 2);
        assert_eq!(snapshot.assets()[0].file_name().as_str(), "archive.zip");
        assert_eq!(snapshot.assets()[1].file_name().as_str(), "notes.txt");
        assert_eq!(snapshot.tag_name(), "v1.0.0");
        assert_eq!(snapshot.updated_at(), Some("2026-09-01T00:00:00Z"));
    }

    #[test]
    fn latest_release_query_requests_one_release_per_page() {
        let api_root = Url::parse("http://127.0.0.1/").expect("test URL must parse");
        let github = GitHubClient::with_api_root("test-token".to_owned(), api_root)
            .expect("GitHub client must be created");
        let repository = RepositoryId::from_database_id(
            1,
            parse_repository("owner/repo").expect("repository must parse"),
        )
        .expect("repository ID must be created");

        let url = release_query_url(
            &github,
            &repository,
            &ReleaseSelector::LatestIncludingPrereleases,
        )
        .expect("release URL must be built");

        assert_eq!(
            url.query_pairs()
                .find(|(key, _)| key == "per_page")
                .map(|(_, value)| value.into_owned()),
            Some("1".to_owned())
        );
    }

    #[tokio::test]
    async fn sends_etag_for_the_selected_github_resource() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("test server must bind");
        let address = listener
            .local_addr()
            .expect("test server must have address");
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
                .send(String::from_utf8(request).expect("request must be UTF-8"))
                .expect("request must be reported");
            stream
                .write_all(
                    b"HTTP/1.1 304 Not Modified\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .expect("response must be written");
        });

        let api_root = Url::parse(&format!("http://{address}/")).expect("test URL must parse");
        let github = GitHubClient::with_api_root("test-token".to_owned(), api_root)
            .expect("GitHub client must be created");
        let repository = RepositoryId::from_database_id(
            1,
            parse_repository("owner/repo").expect("repository must parse"),
        )
        .expect("repository ID must be created");

        let fetched = super::fetch_selected_release(
            &github,
            &repository,
            &ReleaseSelector::LatestStable,
            Some("\"release-10\""),
        )
        .await
        .expect("304 must be accepted");

        assert!(matches!(fetched, FetchedRelease::NotModified));
        let request = request_receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("request must be captured")
            .to_ascii_lowercase();
        assert!(request.starts_with("get /repos/owner/repo/releases/latest http/1.1"));
        assert!(request.contains("if-none-match: \"release-10\""));
        assert!(!request.contains("asset_regex"));
        server.join().expect("test server must stop");
    }
}
