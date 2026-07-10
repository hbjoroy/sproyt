mod postgres;
mod sqlite;

#[allow(unused_imports)]
pub use postgres::PostgresChatRepository;
#[allow(unused_imports)]
pub use sqlite::SqliteChatRepository;

use std::sync::Arc;

use crate::config::{DatabaseConfig, DatabaseKind};
use crate::domain::{ChatRepository, RepositoryError};

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

pub async fn connect_repository(
    config: &DatabaseConfig,
) -> Result<Arc<dyn ChatRepository>, RepositoryError> {
    match config.kind() {
        DatabaseKind::Sqlite => Ok(Arc::new(SqliteChatRepository::connect(config.url()).await?)),
        DatabaseKind::Postgres => Ok(Arc::new(
            PostgresChatRepository::connect(config.url()).await?,
        )),
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
