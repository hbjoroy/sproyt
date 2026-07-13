use std::{env, fmt, net::SocketAddr};

const DEFAULT_BIND_ADDRESS: &str = "127.0.0.1:9010";
const DEFAULT_DATABASE_URL: &str = "sqlite://.local/sproyt.sqlite";
const DEFAULT_ENVIRONMENT: &str = "development";
const DEFAULT_LOG_FORMAT: &str = "pretty";
const DEFAULT_AUTH_MODE: &str = "development";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppConfig {
    bind_address: SocketAddr,
    database: DatabaseConfig,
    environment: DeploymentEnvironment,
    log_format: LogFormat,
    auth_mode: AuthMode,
    oidc: Option<OidcConfig>,
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
        let auth_mode =
            env::var("SPROYT_AUTH_MODE").unwrap_or_else(|_| DEFAULT_AUTH_MODE.to_owned());
        let mut config = Self::from_values(
            bind_address,
            database_url,
            environment,
            log_format,
            auth_mode,
        )?;
        if config.auth_mode == AuthMode::Oidc {
            config.oidc = Some(OidcConfig::from_env()?);
        }
        Ok(config)
    }

    pub fn from_values(
        bind_address: impl AsRef<str>,
        database_url: impl Into<String>,
        environment: impl AsRef<str>,
        log_format: impl AsRef<str>,
        auth_mode: impl AsRef<str>,
    ) -> Result<Self, ConfigError> {
        let bind_address = bind_address
            .as_ref()
            .parse()
            .map_err(|_| ConfigError::InvalidBindAddress(bind_address.as_ref().to_owned()))?;
        let database = DatabaseConfig::new(database_url.into())?;
        let environment = DeploymentEnvironment::parse(environment.as_ref())?;
        let log_format = LogFormat::parse(log_format.as_ref())?;
        let auth_mode = AuthMode::parse(auth_mode.as_ref())?;
        if environment == DeploymentEnvironment::Production && auth_mode == AuthMode::Development {
            return Err(ConfigError::DevelopmentAuthInProduction);
        }
        Ok(Self {
            bind_address,
            database,
            environment,
            log_format,
            auth_mode,
            oidc: None,
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

    pub const fn auth_mode(&self) -> AuthMode {
        self.auth_mode
    }

    pub const fn oidc(&self) -> Option<&OidcConfig> {
        self.oidc.as_ref()
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct OidcConfig {
    issuer: String,
    client_id: String,
    client_secret: String,
    redirect_url: String,
    post_logout_redirect_url: String,
    session_key: String,
    session_previous_keys: Vec<String>,
}

impl OidcConfig {
    fn from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            issuer: required_env("SPROYT_OIDC_ISSUER")?,
            client_id: required_env("SPROYT_OIDC_CLIENT_ID")?,
            client_secret: required_env("SPROYT_OIDC_CLIENT_SECRET")?,
            redirect_url: required_env("SPROYT_OIDC_REDIRECT_URL")?,
            post_logout_redirect_url: required_env("SPROYT_OIDC_POST_LOGOUT_REDIRECT_URL")?,
            session_key: required_env("SPROYT_SESSION_KEY")?,
            session_previous_keys: std::env::var("SPROYT_SESSION_PREVIOUS_KEYS")
                .unwrap_or_default()
                .split(',')
                .map(str::trim)
                .filter(|key| !key.is_empty())
                .map(str::to_owned)
                .collect(),
        })
    }

    pub fn issuer(&self) -> &str {
        &self.issuer
    }
    pub fn client_id(&self) -> &str {
        &self.client_id
    }
    pub fn client_secret(&self) -> &str {
        &self.client_secret
    }
    pub fn redirect_url(&self) -> &str {
        &self.redirect_url
    }
    pub fn post_logout_redirect_url(&self) -> &str {
        &self.post_logout_redirect_url
    }
    pub fn session_key(&self) -> &str {
        &self.session_key
    }
    pub fn session_previous_keys(&self) -> &[String] {
        &self.session_previous_keys
    }
}

impl fmt::Debug for OidcConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OidcConfig")
            .field("issuer", &self.issuer)
            .field("client_id", &self.client_id)
            .field("client_secret", &"<redacted>")
            .field("redirect_url", &self.redirect_url)
            .field("post_logout_redirect_url", &self.post_logout_redirect_url)
            .field("session_key", &"<redacted>")
            .field("session_previous_keys", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthMode {
    Development,
    Oidc,
}

impl AuthMode {
    fn parse(value: &str) -> Result<Self, ConfigError> {
        match value {
            "development" => Ok(Self::Development),
            "oidc" => Ok(Self::Oidc),
            _ => Err(ConfigError::InvalidAuthMode(value.to_owned())),
        }
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

#[derive(Clone, Eq, PartialEq)]
pub struct DatabaseConfig {
    url: String,
    kind: DatabaseKind,
}

impl fmt::Debug for DatabaseConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DatabaseConfig")
            .field("url", &redact_database_url(&self.url))
            .field("kind", &self.kind)
            .finish()
    }
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
    DevelopmentAuthInProduction,
    InvalidAuthMode(String),
    InvalidEnvironment(String),
    InvalidLogFormat(String),
    MissingEnvironmentVariable(&'static str),
    UnsupportedDatabaseUrl(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBindAddress(value) => {
                write!(formatter, "invalid SPROYT_ADDR value: {value}")
            }
            Self::DevelopmentAuthInProduction => {
                formatter.write_str("development authentication is forbidden in production")
            }
            Self::InvalidAuthMode(value) => {
                write!(formatter, "invalid SPROYT_AUTH_MODE value: {value}")
            }
            Self::InvalidEnvironment(value) => {
                write!(formatter, "invalid SPROYT_ENV value: {value}")
            }
            Self::InvalidLogFormat(value) => {
                write!(formatter, "invalid SPROYT_LOG_FORMAT value: {value}")
            }
            Self::MissingEnvironmentVariable(name) => {
                write!(formatter, "required environment variable {name} is missing")
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

fn required_env(name: &'static str) -> Result<String, ConfigError> {
    env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or(ConfigError::MissingEnvironmentVariable(name))
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
            "development",
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
            "oidc",
        )
        .expect("postgres config should parse");

        assert_eq!(config.bind_address().port(), 9011);
        assert_eq!(config.database().kind(), DatabaseKind::Postgres);
        assert_eq!(config.environment(), DeploymentEnvironment::Production);
        assert_eq!(config.log_format(), LogFormat::Json);
        assert_eq!(config.auth_mode(), AuthMode::Oidc);
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
        let environment = AppConfig::from_values(
            "127.0.0.1:9010",
            "sqlite://dev.sqlite",
            "staging",
            "pretty",
            "development",
        )
        .unwrap_err();
        assert_eq!(
            environment,
            ConfigError::InvalidEnvironment("staging".to_owned())
        );

        let log_format = AppConfig::from_values(
            "127.0.0.1:9010",
            "sqlite://dev.sqlite",
            "test",
            "compact",
            "development",
        )
        .unwrap_err();
        assert_eq!(
            log_format,
            ConfigError::InvalidLogFormat("compact".to_owned())
        );
    }

    #[test]
    fn refuses_development_auth_in_production() {
        let error = AppConfig::from_values(
            "127.0.0.1:9010",
            "postgres://localhost/sproyt",
            "production",
            "json",
            "development",
        )
        .unwrap_err();
        assert_eq!(error, ConfigError::DevelopmentAuthInProduction);
    }
}
