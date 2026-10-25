use super::{NativeError, Result};
use sqlx::{PgPool, Row, SqlitePool};
#[derive(Clone)]
pub(super) enum Db {
    Pg(PgPool),
    Sqlite(SqlitePool),
}
pub(super) enum Tx {
    Pg(sqlx::Transaction<'static, sqlx::Postgres>),
    Sqlite(sqlx::Transaction<'static, sqlx::Sqlite>),
}
macro_rules! query {
    ($sql:expr,$args:expr) => {{
        let mut query = sqlx::query($sql);
        for value in $args {
            query = query.bind((*value).to_owned());
        }
        query
    }};
}
impl Db {
    pub async fn begin(&self) -> Result<Tx> {
        match self {
            Self::Pg(pool) => Ok(Tx::Pg(
                pool.begin().await.map_err(|_| NativeError::Unavailable)?,
            )),
            Self::Sqlite(pool) => Ok(Tx::Sqlite(
                pool.begin().await.map_err(|_| NativeError::Unavailable)?,
            )),
        }
    }
    pub async fn rows(&self, sql: &str, args: &[&str]) -> Result<Vec<Vec<String>>> {
        match self {
            Self::Pg(pool) => query!(sql, args)
                .fetch_all(pool)
                .await
                .map_err(|_| NativeError::Unavailable)?
                .into_iter()
                .map(|row| {
                    (0..row.len())
                        .map(|i| {
                            row.try_get::<String, _>(i)
                                .map_err(|_| NativeError::Unavailable)
                        })
                        .collect()
                })
                .collect(),
            Self::Sqlite(pool) => query!(sql, args)
                .fetch_all(pool)
                .await
                .map_err(|_| NativeError::Unavailable)?
                .into_iter()
                .map(|row| {
                    (0..row.len())
                        .map(|i| {
                            row.try_get::<String, _>(i)
                                .map_err(|_| NativeError::Unavailable)
                        })
                        .collect()
                })
                .collect(),
        }
    }
}
impl Tx {
    pub async fn rows(&mut self, sql: &str, args: &[&str]) -> Result<Vec<Vec<String>>> {
        match self {
            Self::Pg(tx) => query!(sql, args)
                .fetch_all(&mut **tx)
                .await
                .map_err(|_| NativeError::Unavailable)?
                .into_iter()
                .map(|row| {
                    (0..row.len())
                        .map(|i| {
                            row.try_get::<String, _>(i)
                                .map_err(|_| NativeError::Unavailable)
                        })
                        .collect()
                })
                .collect(),
            Self::Sqlite(tx) => query!(sql, args)
                .fetch_all(&mut **tx)
                .await
                .map_err(|_| NativeError::Unavailable)?
                .into_iter()
                .map(|row| {
                    (0..row.len())
                        .map(|i| {
                            row.try_get::<String, _>(i)
                                .map_err(|_| NativeError::Unavailable)
                        })
                        .collect()
                })
                .collect(),
        }
    }
    pub async fn exec(&mut self, sql: &str, args: &[&str]) -> Result<u64> {
        match self {
            Self::Pg(tx) => query!(sql, args)
                .execute(&mut **tx)
                .await
                .map(|r| r.rows_affected())
                .map_err(|_| NativeError::Unavailable),
            Self::Sqlite(tx) => query!(sql, args)
                .execute(&mut **tx)
                .await
                .map(|r| r.rows_affected())
                .map_err(|_| NativeError::Unavailable),
        }
    }
    pub async fn commit(self) -> Result<()> {
        match self {
            Self::Pg(tx) => tx.commit().await.map_err(|_| NativeError::Unavailable),
            Self::Sqlite(tx) => tx.commit().await.map_err(|_| NativeError::Unavailable),
        }
    }
}
