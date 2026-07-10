use std::{env, fmt, net::SocketAddr};

const DEFAULT_BIND_ADDRESS: &str = "127.0.0.1:9010";
const DEFAULT_DATABASE_URL: &str = "sqlite://.local/sproyt.sqlite";
const DEFAULT_ENVIRONMENT: &str = "development";
const DEFAULT_LOG_FORMAT: &str = "pretty";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppConfig {
    bind_address: SocketAddr,
    database: DatabaseConfig,
    environment: DeploymentEnvironment,
    log_format: LogFormat,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        let bind_address =
            env::var("SPROYT_ADDR").unwrap_or_else(|_| DEFAULT_BIND_ADDRESS.to_owned());
        let database_url =
            env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_owned());
        let environment = env::var("SPROYT_ENV").unwrap_or_else(|_| DEFAULT_ENVIRONMENT.to_owned());
        let log_format =
            env::var("SPROYT_LOG_FORMAT").unwrap_or_else(|_| DEFAULT_LOG_FORMAT.to_owned());
        Self::from_values(bind_address, database_url, environment, log_format)
    }

    pub fn from_values(
        bind_address: impl AsRef<str>,
        database_url: impl Into<String>,
        environment: impl AsRef<str>,
        log_format: impl AsRef<str>,
    ) -> Result<Self, ConfigError> {
        let bind_address = bind_address
            .as_ref()
            .parse()
            .map_err(|_| ConfigError::InvalidBindAddress(bind_address.as_ref().to_owned()))?;
        let database = DatabaseConfig::new(database_url.into())?;
        let environment = DeploymentEnvironment::parse(environment.as_ref())?;
        let log_format = LogFormat::parse(log_format.as_ref())?;
        Ok(Self {
            bind_address,
            database,
            environment,
            log_format,
        })
    }

    pub const fn bind_address(&self) -> SocketAddr {
        self.bind_address
    }

    pub const fn database(&self) -> &DatabaseConfig {
        &self.database
    }

    pub const fn environment(&self) -> DeploymentEnvironment {
        self.environment
    }

    pub const fn log_format(&self) -> LogFormat {
        self.log_format
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeploymentEnvironment {
    Development,
    Production,
    Test,
}

impl DeploymentEnvironment {
    fn parse(value: &str) -> Result<Self, ConfigError> {
        match value {
            "development" => Ok(Self::Development),
            "production" => Ok(Self::Production),
            "test" => Ok(Self::Test),
            _ => Err(ConfigError::InvalidEnvironment(value.to_owned())),
        }
    }
}

impl fmt::Display for DeploymentEnvironment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Development => formatter.write_str("development"),
            Self::Production => formatter.write_str("production"),
            Self::Test => formatter.write_str("test"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogFormat {
    Json,
    Pretty,
}

impl LogFormat {
    fn parse(value: &str) -> Result<Self, ConfigError> {
        match value {
            "json" => Ok(Self::Json),
            "pretty" => Ok(Self::Pretty),
            _ => Err(ConfigError::InvalidLogFormat(value.to_owned())),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatabaseConfig {
    url: String,
    kind: DatabaseKind,
}

impl DatabaseConfig {
    pub fn new(url: impl Into<String>) -> Result<Self, ConfigError> {
        let url = url.into();
        let kind = DatabaseKind::from_url(&url)?;
        Ok(Self { url, kind })
    }

    pub const fn kind(&self) -> DatabaseKind {
        self.kind
    }

    pub fn url(&self) -> &str {
        &self.url
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DatabaseKind {
    Postgres,
    Sqlite,
}

impl DatabaseKind {
    fn from_url(url: &str) -> Result<Self, ConfigError> {
        if url.starts_with("sqlite:") {
            Ok(Self::Sqlite)
        } else if url.starts_with("postgres://") || url.starts_with("postgresql://") {
            Ok(Self::Postgres)
        } else {
            Err(ConfigError::UnsupportedDatabaseUrl(redact_database_url(
                url,
            )))
        }
    }
}

impl fmt::Display for DatabaseKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Postgres => formatter.write_str("postgres"),
            Self::Sqlite => formatter.write_str("sqlite"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfigError {
    InvalidBindAddress(String),
    InvalidEnvironment(String),
    InvalidLogFormat(String),
    UnsupportedDatabaseUrl(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBindAddress(value) => {
                write!(formatter, "invalid SPROYT_ADDR value: {value}")
            }
            Self::InvalidEnvironment(value) => {
                write!(formatter, "invalid SPROYT_ENV value: {value}")
            }
            Self::InvalidLogFormat(value) => {
                write!(formatter, "invalid SPROYT_LOG_FORMAT value: {value}")
            }
            Self::UnsupportedDatabaseUrl(value) => {
                write!(formatter, "unsupported DATABASE_URL value: {value}")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

fn redact_database_url(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_owned();
    };
    let Some((_, host_and_path)) = rest.rsplit_once('@') else {
        return url.to_owned();
    };
    format!("{scheme}://<credentials>@{host_and_path}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_sqlite_database_url() {
        let config = AppConfig::from_values(
            "127.0.0.1:9010",
            "sqlite://.local/dev.sqlite",
            "development",
            "pretty",
        )
        .expect("sqlite config should parse");

        assert_eq!(config.bind_address().port(), 9010);
        assert_eq!(config.database().kind(), DatabaseKind::Sqlite);
    }

    #[test]
    fn detects_postgres_database_url() {
        let config = AppConfig::from_values(
            "127.0.0.1:9011",
            "postgres://user:secret@localhost/sproyt",
            "production",
            "json",
        )
        .expect("postgres config should parse");

        assert_eq!(config.bind_address().port(), 9011);
        assert_eq!(config.database().kind(), DatabaseKind::Postgres);
        assert_eq!(config.environment(), DeploymentEnvironment::Production);
        assert_eq!(config.log_format(), LogFormat::Json);
    }

    #[test]
    fn rejects_unknown_database_url() {
        let error = DatabaseConfig::new("mysql://localhost/sproyt").unwrap_err();

        assert_eq!(
            error,
            ConfigError::UnsupportedDatabaseUrl("mysql://localhost/sproyt".to_owned())
        );
    }

    #[test]
    fn redacts_database_credentials_in_errors() {
        let error = DatabaseConfig::new("mysql://user:secret@localhost/sproyt").unwrap_err();

        assert_eq!(
            error,
            ConfigError::UnsupportedDatabaseUrl(
                "mysql://<credentials>@localhost/sproyt".to_owned()
            )
        );
    }

    #[test]
    fn rejects_unknown_environment_and_log_format() {
        let environment =
            AppConfig::from_values("127.0.0.1:9010", "sqlite://dev.sqlite", "staging", "pretty")
                .unwrap_err();
        assert_eq!(
            environment,
            ConfigError::InvalidEnvironment("staging".to_owned())
        );

        let log_format =
            AppConfig::from_values("127.0.0.1:9010", "sqlite://dev.sqlite", "test", "compact")
                .unwrap_err();
        assert_eq!(
            log_format,
            ConfigError::InvalidLogFormat("compact".to_owned())
        );
    }
}
