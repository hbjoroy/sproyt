mod postgres;
mod sqlite;

#[allow(unused_imports)]
pub use postgres::PostgresChatRepository;
#[allow(unused_imports)]
pub use sqlite::SqliteChatRepository;

use std::sync::Arc;

use crate::agent::{AgentRepository, SharedAgentRepository};
use crate::config::{DatabaseConfig, DatabaseKind};
use crate::domain::{ChatRepository, RepositoryError};
use crate::process::{ProcessRepository, SharedProcessRepository};

pub struct Repositories {
    pub chat: Arc<dyn ChatRepository>,
    pub process: SharedProcessRepository,
    pub agent: SharedAgentRepository,
}

pub async fn migrate(config: &DatabaseConfig) -> Result<(), RepositoryError> {
    match config.kind() {
        DatabaseKind::Sqlite => {
            let repository = SqliteChatRepository::connect(config.url()).await?;
            repository.migrate().await
        }
        DatabaseKind::Postgres => {
            let repository = PostgresChatRepository::connect(config.url()).await?;
            repository.migrate().await
        }
    }
}

pub async fn connect_repositories(
    config: &DatabaseConfig,
) -> Result<Repositories, RepositoryError> {
    match config.kind() {
        DatabaseKind::Sqlite => {
            let repository = Arc::new(SqliteChatRepository::connect(config.url()).await?);
            let chat: Arc<dyn ChatRepository> = repository.clone();
            let process: Arc<dyn ProcessRepository> = repository.clone();
            let agent: Arc<dyn AgentRepository> = repository;
            Ok(Repositories {
                chat,
                process,
                agent,
            })
        }
        DatabaseKind::Postgres => {
            let repository = Arc::new(PostgresChatRepository::connect(config.url()).await?);
            let chat: Arc<dyn ChatRepository> = repository.clone();
            let process: Arc<dyn ProcessRepository> = repository.clone();
            let agent: Arc<dyn AgentRepository> = repository;
            Ok(Repositories {
                chat,
                process,
                agent,
            })
        }
    }
}

fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}

fn sql_error(error: sqlx::Error) -> RepositoryError {
    match &error {
        sqlx::Error::Database(database) if database.is_unique_violation() => {
            RepositoryError::Conflict
        }
        sqlx::Error::Database(database) if database.is_foreign_key_violation() => {
            RepositoryError::PermissionDenied
        }
        _ => storage(error),
    }
}
