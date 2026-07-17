use std::{env, fmt, net::SocketAddr, time::Duration};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

const DEFAULT_BIND_ADDRESS: &str = "127.0.0.1:9010";
const DEFAULT_DATABASE_URL: &str = "sqlite://.local/sproyt.sqlite";
const DEFAULT_ENVIRONMENT: &str = "development";
const DEFAULT_LOG_FORMAT: &str = "pretty";
const DEFAULT_AUTH_MODE: &str = "development";
const DEFAULT_WS_IDLE_TIMEOUT_SECONDS: u64 = 60;
const PRODUCTION_OIDC_ISSUER: &str = "https://sproyt-security.bjoroy.me/application/o/sproyt/";
const PRODUCTION_OIDC_REDIRECT_URL: &str = "https://sproyt.bjoroy.me/auth/callback";
const PRODUCTION_POST_LOGOUT_REDIRECT_URL: &str = "https://sproyt.bjoroy.me/";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppConfig {
    bind_address: SocketAddr,
    database: DatabaseConfig,
    environment: DeploymentEnvironment,
    log_format: LogFormat,
    auth_mode: AuthMode,
    oidc: Option<OidcConfig>,
    websocket_idle_timeout: Duration,
}

impl AppConfig {
    pub fn database_from_env() -> Result<DatabaseConfig, ConfigError> {
        DatabaseConfig::new(
            env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_owned()),
        )
    }

    pub fn log_format_from_env() -> Result<LogFormat, ConfigError> {
        LogFormat::parse(
            &env::var("SPROYT_LOG_FORMAT").unwrap_or_else(|_| DEFAULT_LOG_FORMAT.to_owned()),
        )
    }

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
            config.oidc = Some(OidcConfig::from_env(config.environment)?);
        }
        config.websocket_idle_timeout =
            parse_idle_timeout(env::var("SPROYT_WS_IDLE_TIMEOUT_SECONDS").ok().as_deref())?;
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
            websocket_idle_timeout: Duration::from_secs(DEFAULT_WS_IDLE_TIMEOUT_SECONDS),
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

    pub const fn websocket_idle_timeout(&self) -> Duration {
        self.websocket_idle_timeout
    }
}

fn parse_idle_timeout(value: Option<&str>) -> Result<Duration, ConfigError> {
    let seconds = match value {
        None => DEFAULT_WS_IDLE_TIMEOUT_SECONDS,
        Some(value) => value
            .parse::<u64>()
            .map_err(|_| ConfigError::InvalidWebSocketIdleTimeout(value.to_owned()))?,
    };
    if !(5..=3600).contains(&seconds) {
        return Err(ConfigError::InvalidWebSocketIdleTimeout(
            seconds.to_string(),
        ));
    }
    Ok(Duration::from_secs(seconds))
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
    fn from_env(environment: DeploymentEnvironment) -> Result<Self, ConfigError> {
        let config = Self {
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
        };
        config.validate(environment)?;
        Ok(config)
    }

    fn validate(&self, environment: DeploymentEnvironment) -> Result<(), ConfigError> {
        let issuer = validate_oidc_url("SPROYT_OIDC_ISSUER", &self.issuer, environment)?;
        if environment == DeploymentEnvironment::Production
            && issuer.as_str() != PRODUCTION_OIDC_ISSUER
        {
            return Err(ConfigError::InvalidOidcConfig(
                "SPROYT_OIDC_ISSUER",
                "expected https://sproyt-security.bjoroy.me/application/o/sproyt/",
            ));
        }
        let redirect =
            validate_oidc_url("SPROYT_OIDC_REDIRECT_URL", &self.redirect_url, environment)?;
        if !redirect.path().ends_with("/auth/callback") {
            return Err(ConfigError::InvalidOidcConfig(
                "SPROYT_OIDC_REDIRECT_URL",
                "path must end with /auth/callback",
            ));
        }
        if environment == DeploymentEnvironment::Production
            && redirect.as_str() != PRODUCTION_OIDC_REDIRECT_URL
        {
            return Err(ConfigError::InvalidOidcConfig(
                "SPROYT_OIDC_REDIRECT_URL",
                "expected https://sproyt.bjoroy.me/auth/callback",
            ));
        }
        let post_logout = validate_oidc_url(
            "SPROYT_OIDC_POST_LOGOUT_REDIRECT_URL",
            &self.post_logout_redirect_url,
            environment,
        )?;
        if redirect.scheme() != post_logout.scheme()
            || redirect.host_str() != post_logout.host_str()
            || redirect.port_or_known_default() != post_logout.port_or_known_default()
        {
            return Err(ConfigError::InvalidOidcConfig(
                "SPROYT_OIDC_POST_LOGOUT_REDIRECT_URL",
                "must use the same origin as SPROYT_OIDC_REDIRECT_URL",
            ));
        }
        if environment == DeploymentEnvironment::Production
            && post_logout.as_str() != PRODUCTION_POST_LOGOUT_REDIRECT_URL
        {
            return Err(ConfigError::InvalidOidcConfig(
                "SPROYT_OIDC_POST_LOGOUT_REDIRECT_URL",
                "expected https://sproyt.bjoroy.me/",
            ));
        }
        validate_session_key("SPROYT_SESSION_KEY", &self.session_key)?;
        for key in &self.session_previous_keys {
            validate_session_key("SPROYT_SESSION_PREVIOUS_KEYS", key)?;
        }
        Ok(())
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

    #[cfg(test)]
    pub fn for_test(
        issuer: String,
        client_id: String,
        client_secret: String,
        redirect_url: String,
        post_logout_redirect_url: String,
        session_key: String,
        session_previous_keys: Vec<String>,
    ) -> Self {
        Self {
            issuer,
            client_id,
            client_secret,
            redirect_url,
            post_logout_redirect_url,
            session_key,
            session_previous_keys,
        }
    }
}

fn validate_oidc_url(
    name: &'static str,
    value: &str,
    environment: DeploymentEnvironment,
) -> Result<reqwest::Url, ConfigError> {
    let url = reqwest::Url::parse(value)
        .map_err(|_| ConfigError::InvalidOidcConfig(name, "must be an absolute URL"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ConfigError::InvalidOidcConfig(
            name,
            "must be an absolute HTTP(S) URL without credentials, query, or fragment",
        ));
    }
    if environment == DeploymentEnvironment::Production && url.scheme() != "https" {
        return Err(ConfigError::InvalidOidcConfig(
            name,
            "must use HTTPS in production",
        ));
    }
    Ok(url)
}

fn validate_session_key(name: &'static str, value: &str) -> Result<(), ConfigError> {
    let valid = URL_SAFE_NO_PAD
        .decode(value)
        .is_ok_and(|decoded| decoded.len() == 32);
    if !valid {
        return Err(ConfigError::InvalidOidcConfig(
            name,
            "must be URL-safe base64 without padding for exactly 32 bytes",
        ));
    }
    Ok(())
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
    InvalidWebSocketIdleTimeout(String),
    InvalidOidcConfig(&'static str, &'static str),
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
            Self::InvalidWebSocketIdleTimeout(value) => write!(
                formatter,
                "invalid SPROYT_WS_IDLE_TIMEOUT_SECONDS value: {value}; expected 5 to 3600"
            ),
            Self::InvalidOidcConfig(name, reason) => {
                write!(formatter, "invalid {name}: {reason}")
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
    fn websocket_idle_timeout_is_bounded() {
        assert_eq!(parse_idle_timeout(None).unwrap(), Duration::from_secs(60));
        assert_eq!(
            parse_idle_timeout(Some("30")).unwrap(),
            Duration::from_secs(30)
        );
        assert!(parse_idle_timeout(Some("0")).is_err());
        assert!(parse_idle_timeout(Some("3601")).is_err());
        assert!(parse_idle_timeout(Some("not-a-number")).is_err());
    }

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

    #[test]
    fn validates_production_oidc_urls_and_session_keys_before_discovery() {
        let key = URL_SAFE_NO_PAD.encode([42_u8; 32]);
        let config = |issuer: &str, redirect: &str, logout: &str, session_key: &str| OidcConfig {
            issuer: issuer.to_owned(),
            client_id: "sproyt".to_owned(),
            client_secret: "secret".to_owned(),
            redirect_url: redirect.to_owned(),
            post_logout_redirect_url: logout.to_owned(),
            session_key: session_key.to_owned(),
            session_previous_keys: vec![URL_SAFE_NO_PAD.encode([41_u8; 32])],
        };
        let issuer = PRODUCTION_OIDC_ISSUER;
        let redirect = "https://sproyt.bjoroy.me/auth/callback";
        let logout = "https://sproyt.bjoroy.me/";

        assert!(
            config(issuer, redirect, logout, &key)
                .validate(DeploymentEnvironment::Production)
                .is_ok()
        );
        assert!(
            config(
                "https://identity.example/application/o/sproyt/",
                redirect,
                logout,
                &key
            )
            .validate(DeploymentEnvironment::Production)
            .is_err()
        );
        assert!(
            config(
                "https://identity.limani-parou.com/application/o/sproyt/",
                redirect,
                logout,
                &key
            )
            .validate(DeploymentEnvironment::Production)
            .is_err()
        );
        assert!(
            config(
                issuer,
                "https://user:secret@sproyt.bjoroy.me/auth/callback",
                logout,
                &key
            )
            .validate(DeploymentEnvironment::Production)
            .is_err()
        );
        assert!(
            config(issuer, redirect, "https://other.example/", &key)
                .validate(DeploymentEnvironment::Production)
                .is_err()
        );
        assert!(
            config(
                issuer,
                "https://other.example/auth/callback",
                "https://other.example/",
                &key
            )
            .validate(DeploymentEnvironment::Production)
            .is_err()
        );
        assert!(
            config(issuer, redirect, logout, "not-a-key")
                .validate(DeploymentEnvironment::Production)
                .is_err()
        );
    }
}
