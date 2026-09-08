use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    config::ReleaseConfig,
    github::{
        GitHubClient, GitHubError, RepositoryId,
        releases::{
            self as github_releases, FetchedRelease, ReleaseAsset, ReleaseId, ReleaseSnapshot,
        },
    },
    state::ReleaseCursor,
};

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ReleaseTag(String);

impl ReleaseTag {
    pub fn new(value: impl Into<String>) -> Result<Self, ReleaseTagError> {
        let value = value.into();
        if value.is_empty() {
            return Err(ReleaseTagError::Empty);
        }

        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum ReleaseTagError {
    #[error("Release Tag must not be empty")]
    Empty,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReleaseSelector {
    LatestStable,
    LatestIncludingPrereleases,
    Tag(ReleaseTag),
}

impl ReleaseSelector {
    pub fn scope(&self) -> ReleaseSelectorScope {
        match self {
            Self::LatestStable => ReleaseSelectorScope::LatestStable,
            Self::LatestIncludingPrereleases => ReleaseSelectorScope::LatestIncludingPrereleases,
            Self::Tag(tag) => ReleaseSelectorScope::Tag {
                tag: tag.as_str().to_owned(),
            },
        }
    }

    pub fn tag(&self) -> Option<&ReleaseTag> {
        match self {
            Self::Tag(tag) => Some(tag),
            Self::LatestStable | Self::LatestIncludingPrereleases => None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReleaseSelectorScope {
    LatestStable,
    LatestIncludingPrereleases,
    Tag { tag: String },
}

#[derive(Clone, Debug)]
pub struct ReleaseInfo {
    snapshot: ReleaseSnapshot,
}

impl ReleaseInfo {
    fn from_snapshot(snapshot: ReleaseSnapshot) -> Self {
        Self { snapshot }
    }

    pub fn id(&self) -> ReleaseId {
        self.snapshot.id()
    }

    pub fn tag_name(&self) -> &str {
        self.snapshot.tag_name()
    }

    pub fn name(&self) -> Option<&str> {
        self.snapshot.name()
    }

    pub fn body(&self) -> Option<&str> {
        self.snapshot.body()
    }

    pub fn html_url(&self) -> &str {
        self.snapshot.html_url()
    }

    pub fn target_commitish(&self) -> &str {
        self.snapshot.target_commitish()
    }

    pub fn draft(&self) -> bool {
        self.snapshot.draft()
    }

    pub fn prerelease(&self) -> bool {
        self.snapshot.prerelease()
    }

    pub fn immutable(&self) -> Option<bool> {
        self.snapshot.immutable()
    }

    pub fn author_login(&self) -> Option<&str> {
        self.snapshot.author_login()
    }

    pub fn created_at(&self) -> &str {
        self.snapshot.created_at()
    }

    pub fn updated_at(&self) -> Option<&str> {
        self.snapshot.updated_at()
    }

    pub fn published_at(&self) -> Option<&str> {
        self.snapshot.published_at()
    }

    pub fn assets(&self) -> &[ReleaseAsset] {
        self.snapshot.assets()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReleaseStatusCode {
    NoError,
    ReleaseNotFound,
    Etag,
}

impl ReleaseStatusCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoError => "NO_ERROR",
            Self::ReleaseNotFound => "RELEASE_NOT_FOUND",
            Self::Etag => "ETAG",
        }
    }
}

#[derive(Clone, Debug)]
pub enum ReleaseCheck {
    NotFound {
        status_code: ReleaseStatusCode,
        checkpoint: ReleaseCursor,
    },
    BaselineEstablished {
        status_code: ReleaseStatusCode,
        release_id: ReleaseId,
        checkpoint: ReleaseCursor,
    },
    Unchanged {
        status_code: ReleaseStatusCode,
        checkpoint: ReleaseCursor,
    },
    NewRelease {
        status_code: ReleaseStatusCode,
        release: ReleaseInfo,
        checkpoint: ReleaseCursor,
    },
    UpdatedRelease {
        status_code: ReleaseStatusCode,
        release: ReleaseInfo,
        checkpoint: ReleaseCursor,
    },
}

pub async fn check_for_new_release(
    github: &GitHubClient,
    repository_id: &RepositoryId,
    config: &ReleaseConfig,
    current: &ReleaseCursor,
) -> Result<ReleaseCheck, GitHubError> {
    let selector_scope = config.selector.scope();
    let etag = current.etag_for(&selector_scope);
    let fetched =
        github_releases::fetch_selected_release(github, repository_id, &config.selector, etag)
            .await?;

    match fetched {
        FetchedRelease::NotModified => Ok(ReleaseCheck::Unchanged {
            status_code: ReleaseStatusCode::Etag,
            checkpoint: current.clone(),
        }),
        FetchedRelease::NotFound { etag } => Ok(ReleaseCheck::NotFound {
            status_code: ReleaseStatusCode::ReleaseNotFound,
            checkpoint: ReleaseCursor::new(selector_scope, None, None, etag),
        }),
        FetchedRelease::Found { release, etag } => {
            Ok(classify_release(*release, etag, selector_scope, current))
        }
    }
}

fn classify_release(
    release: ReleaseSnapshot,
    etag: Option<String>,
    selector_scope: ReleaseSelectorScope,
    current: &ReleaseCursor,
) -> ReleaseCheck {
    let checkpoint = release_checkpoint(&release, etag, selector_scope.clone());

    if current.selector_scope() != Some(&selector_scope) {
        return ReleaseCheck::BaselineEstablished {
            status_code: ReleaseStatusCode::NoError,
            release_id: release.id(),
            checkpoint,
        };
    }

    let release_changed = current.release_id() != Some(release.id().value());
    if !release_changed && !release_updated(current, &release) {
        return ReleaseCheck::Unchanged {
            status_code: ReleaseStatusCode::NoError,
            checkpoint,
        };
    }

    let release = ReleaseInfo::from_snapshot(release);
    if release_changed {
        ReleaseCheck::NewRelease {
            status_code: ReleaseStatusCode::NoError,
            release,
            checkpoint,
        }
    } else {
        ReleaseCheck::UpdatedRelease {
            status_code: ReleaseStatusCode::NoError,
            release,
            checkpoint,
        }
    }
}

fn release_checkpoint(
    release: &ReleaseSnapshot,
    etag: Option<String>,
    selector_scope: ReleaseSelectorScope,
) -> ReleaseCursor {
    ReleaseCursor::new(
        selector_scope,
        Some(release.id().value()),
        release.updated_at().map(str::to_owned),
        etag,
    )
}

fn release_updated(current: &ReleaseCursor, release: &ReleaseSnapshot) -> bool {
    match (current.updated_at(), release.updated_at()) {
        (Some(previous), Some(next)) => previous != next,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        github::releases::{ReleaseAsset, ReleaseSnapshot},
        state::ReleaseCursor,
    };

    use super::{ReleaseCheck, ReleaseSelectorScope, classify_release};

    #[test]
    fn selector_change_establishes_a_new_baseline() {
        let current = ReleaseCursor::new(
            ReleaseSelectorScope::LatestStable,
            Some(10),
            Some("2026-09-01T00:00:00Z".to_owned()),
            Some("\"stable-10\"".to_owned()),
        );

        let check = classify_release(
            release_snapshot(
                11,
                Some("2026-09-02T00:00:00Z"),
                vec![asset(21, "preview.zip", "sha256:preview")],
            ),
            Some("\"preview-11\"".to_owned()),
            ReleaseSelectorScope::LatestIncludingPrereleases,
            &current,
        );

        let ReleaseCheck::BaselineEstablished {
            release_id,
            checkpoint,
            ..
        } = check
        else {
            panic!("a different selector must establish a baseline");
        };
        assert_eq!(release_id.value(), 11);
        assert_eq!(
            checkpoint.selector_scope(),
            Some(&ReleaseSelectorScope::LatestIncludingPrereleases)
        );
    }

    #[test]
    fn changed_release_id_is_a_new_release_with_all_assets() {
        let current = ReleaseCursor::new(
            ReleaseSelectorScope::LatestStable,
            Some(10),
            Some("2026-09-01T00:00:00Z".to_owned()),
            Some("\"release-10-old\"".to_owned()),
        );

        let check = classify_release(
            release_snapshot(
                11,
                Some("2026-09-02T00:00:00Z"),
                vec![
                    asset(20, "archive.zip", "sha256:first"),
                    asset(21, "notes.txt", "sha256:first"),
                ],
            ),
            Some("\"release-11-new\"".to_owned()),
            ReleaseSelectorScope::LatestStable,
            &current,
        );

        let ReleaseCheck::NewRelease {
            release,
            checkpoint,
            ..
        } = check
        else {
            panic!("a changed release ID must produce a new release");
        };
        assert_eq!(release.assets().len(), 2);
        assert_eq!(release.assets()[0].file_name().as_str(), "archive.zip");
        assert_eq!(checkpoint.release_id(), Some(11));
        assert_eq!(checkpoint.updated_at(), Some("2026-09-02T00:00:00Z"));
    }

    #[test]
    fn changed_updated_at_is_a_rolling_update_with_all_assets() {
        let current = ReleaseCursor::new(
            ReleaseSelectorScope::LatestStable,
            Some(10),
            Some("2026-09-01T00:00:00Z".to_owned()),
            Some("\"release-10-old\"".to_owned()),
        );

        let check = classify_release(
            release_snapshot(
                10,
                Some("2026-09-02T00:00:00Z"),
                vec![asset(20, "archive.zip", "sha256:second")],
            ),
            Some("\"release-10-new\"".to_owned()),
            ReleaseSelectorScope::LatestStable,
            &current,
        );

        let ReleaseCheck::UpdatedRelease {
            release,
            checkpoint,
            ..
        } = check
        else {
            panic!("a changed updated_at must produce a rolling update");
        };
        assert_eq!(release.assets().len(), 1);
        assert_eq!(release.tag_name(), "v10");
        assert_eq!(checkpoint.updated_at(), Some("2026-09-02T00:00:00Z"));
    }

    #[test]
    fn missing_updated_at_is_a_rolling_update() {
        let current = ReleaseCursor::new(
            ReleaseSelectorScope::LatestStable,
            Some(10),
            None,
            Some("\"release-10-old\"".to_owned()),
        );

        let check = classify_release(
            release_snapshot(10, None, vec![]),
            Some("\"release-10-new\"".to_owned()),
            ReleaseSelectorScope::LatestStable,
            &current,
        );

        assert!(matches!(check, ReleaseCheck::UpdatedRelease { .. }));
    }

    #[test]
    fn same_updated_at_is_unchanged_but_refreshes_etag() {
        let current = ReleaseCursor::new(
            ReleaseSelectorScope::LatestStable,
            Some(10),
            Some("2026-09-01T00:00:00Z".to_owned()),
            Some("\"release-10-old\"".to_owned()),
        );

        let check = classify_release(
            release_snapshot(10, Some("2026-09-01T00:00:00Z"), vec![]),
            Some("\"release-10-new\"".to_owned()),
            ReleaseSelectorScope::LatestStable,
            &current,
        );

        let ReleaseCheck::Unchanged { checkpoint, .. } = check else {
            panic!("an unchanged updated_at must not produce a report");
        };
        assert_eq!(checkpoint.etag(), Some("\"release-10-new\""));
    }

    fn release_snapshot(
        id: u64,
        updated_at: Option<&str>,
        assets: Vec<ReleaseAsset>,
    ) -> ReleaseSnapshot {
        ReleaseSnapshot::new(
            id,
            format!("v{id}"),
            Some(format!("Release {id}")),
            Some("Release notes".to_owned()),
            format!("https://github.com/owner/repo/releases/{id}"),
            "main".to_owned(),
            false,
            false,
            Some(false),
            Some("octocat".to_owned()),
            "2026-09-01T00:00:00Z".to_owned(),
            updated_at.map(str::to_owned),
            Some("2026-09-01T00:00:00Z".to_owned()),
            assets,
        )
    }

    fn asset(id: u64, name: &str, digest: &str) -> ReleaseAsset {
        ReleaseAsset::new(
            id,
            name.to_owned(),
            None,
            "application/octet-stream".to_owned(),
            10,
            Some(digest.to_owned()),
            0,
            "2026-09-01T00:00:00Z".to_owned(),
            "2026-09-01T00:00:00Z".to_owned(),
            format!("https://example.com/{name}"),
        )
    }
}
