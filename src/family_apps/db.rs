use super::{FamilyError, Result};
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
fn sql(sql: &str, pg: bool) -> String {
    if pg {
        sql.replace("::id", "::uuid")
            .replace("::json", "::jsonb")
            .replace("::time", "::timestamptz")
    } else {
        sql.replace('$', "?")
            .replace("::id", "")
            .replace("::json", "")
            .replace("::time", "")
    }
}
fn error(e: sqlx::Error) -> FamilyError {
    match e {
        sqlx::Error::Database(ref d) if d.is_unique_violation() => FamilyError::Conflict,
        _ => FamilyError::Unavailable,
    }
}
macro_rules! query {
    ($sql:expr,$args:expr) => {{
        let mut q = sqlx::query($sql);
        for v in $args {
            q = q.bind((*v).to_owned());
        }
        q
    }};
}
impl Db {
    pub async fn begin(&self) -> Result<Tx> {
        match self {
            Self::Pg(p) => Ok(Tx::Pg(p.begin().await.map_err(error)?)),
            Self::Sqlite(p) => Ok(Tx::Sqlite(
                p.begin_with("BEGIN IMMEDIATE").await.map_err(error)?,
            )),
        }
    }
}
impl Tx {
    pub async fn rows(&mut self, statement: &str, args: &[&str]) -> Result<Vec<Vec<String>>> {
        match self {
            Self::Pg(tx) => {
                let sql = sql(statement, true);
                query!(&sql, args)
                    .fetch_all(&mut **tx)
                    .await
                    .map_err(error)?
                    .into_iter()
                    .map(|r| {
                        (0..r.len())
                            .map(|i| r.try_get::<String, _>(i).map_err(error))
                            .collect()
                    })
                    .collect()
            }
            Self::Sqlite(tx) => {
                let sql = sql(statement, false);
                query!(&sql, args)
                    .fetch_all(&mut **tx)
                    .await
                    .map_err(error)?
                    .into_iter()
                    .map(|r| {
                        (0..r.len())
                            .map(|i| r.try_get::<String, _>(i).map_err(error))
                            .collect()
                    })
                    .collect()
            }
        }
    }
    pub async fn exec(&mut self, statement: &str, args: &[&str]) -> Result<u64> {
        match self {
            Self::Pg(tx) => {
                let sql = sql(statement, true);
                query!(&sql, args)
                    .execute(&mut **tx)
                    .await
                    .map(|r| r.rows_affected())
                    .map_err(error)
            }
            Self::Sqlite(tx) => {
                let sql = sql(statement, false);
                query!(&sql, args)
                    .execute(&mut **tx)
                    .await
                    .map(|r| r.rows_affected())
                    .map_err(error)
            }
        }
    }
    pub async fn commit(self) -> Result<()> {
        match self {
            Self::Pg(tx) => tx.commit().await.map_err(error),
            Self::Sqlite(tx) => tx.commit().await.map_err(error),
        }
    }
}
