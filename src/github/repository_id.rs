use serde::Deserialize;

use super::{GitHubClient, GitHubError, RepositoryName};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct RepositoryId {
    database_id: u64,
    full_name: String,
}

impl RepositoryId {
    pub fn database_id(&self) -> u64 {
        self.database_id
    }

    pub fn full_name(&self) -> &str {
        &self.full_name
    }

    pub(crate) fn from_database_id(
        database_id: u64,
        repository: RepositoryName<'_>,
    ) -> Result<Self, GitHubError> {
        if database_id == 0 {
            return Err(GitHubError::InvalidRepositoryId);
        }

        Ok(Self {
            database_id,
            full_name: format!("{}/{}", repository.owner, repository.name),
        })
    }
}

#[derive(Deserialize)]
struct ApiRepository {
    id: u64,
}

pub async fn resolve_github_repository_id(
    github: &GitHubClient,
    repository: RepositoryName<'_>,
) -> Result<RepositoryId, GitHubError> {
    let full_name = repository.full_name();
    let url = github.repository_url(&full_name, &[])?;
    let response: ApiRepository = github.get_json(url).await?;
    RepositoryId::from_database_id(response.id, repository)
}

impl RepositoryName<'_> {
    fn full_name(self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}
