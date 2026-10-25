//! Opt-in canary native identities. Native families NEVER import a browser's
//! refresh grant: every attempt owns a fresh AuthService login transaction.
mod codec;
mod db;
use crate::{
    auth::{
        AuthError, AuthService, AuthenticatedPrincipal, LOGIN_COOKIE, REFRESH_COOKIE,
        SESSION_COOKIE,
    },
    config::{AppConfig, DatabaseKind},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use codec::Codec;
use db::{Db, Tx};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
pub const SCOPE: &str = "sproyt_native_v1";
pub const ENVIRONMENT: &str = "canary";
const ATTEMPT_COLUMNS: &str = "id,state_hash,challenge,expected_user_id,start_epoch,epoch,user_id,callback_port,created_at,expires_at,phase,cookie_hash,confirm_hash,code_hash,handoff_expires,redemption_id,receipt_expires,family,secret,receipt";
const FAMILY_COLUMNS: &str = "family,user_id,epoch,generation,phase,access_hash,refresh_hash,revoke_hash,expires_at,secret,lease_id,lease_expires";
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeError {
    Unauthorized,
    Invalid,
    Unavailable,
    Configuration,
}
impl std::fmt::Display for NativeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Unauthorized => "native authentication required",
            Self::Invalid => "native request rejected",
            Self::Unavailable => "native service unavailable",
            Self::Configuration => "native configuration invalid",
        })
    }
}
impl std::error::Error for NativeError {}
impl NativeError {
    pub fn status(&self) -> axum::http::StatusCode {
        use axum::http::StatusCode;
        match self {
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Invalid => StatusCode::BAD_REQUEST,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
            Self::Configuration => StatusCode::SERVICE_UNAVAILABLE,
        }
    }
}
type Result<T> = std::result::Result<T, NativeError>;
#[derive(Clone)]
pub struct NativeSessions(Arc<Inner>);
struct Inner {
    db: Db,
    codec: Codec,
    origin: String,
    issuer: String,
    instance: String,
}
#[derive(Serialize)]
pub struct Info {
    pub scope: &'static str,
    pub environment: &'static str,
    pub instance_id: String,
    pub issuer: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoginRequest {
    pub challenge: String,
    pub destination_id: String,
    pub callback_port: u16,
    pub expected_user_id: Option<String>,
}
#[derive(Serialize)]
pub struct LoginStarted {
    pub transaction_id: String,
    pub authorization_url: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExchangeRequest {
    pub transaction_id: String,
    pub code: String,
    pub verifier: String,
    pub redemption_id: String,
    pub environment: String,
    pub destination_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CancelRequest {
    pub transaction_id: String,
    pub verifier: String,
    pub environment: String,
    pub destination_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefreshRequest {
    pub operation_id: String,
    pub generation: u64,
}
// Deliberately no Debug implementation on credential-bearing structures.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Grant {
    pub scope: String,
    pub environment: String,
    pub instance_id: String,
    pub user_id: String,
    pub family: String,
    pub generation: u64,
    pub epoch: u64,
    pub access_token: String,
    pub refresh_token: String,
    pub revoke_token: String,
    pub expires_at: u64,
    pub display_name: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ExchangeResult {
    pub user_id: String,
    pub native_session: Grant,
    pub display_name: String,
}
#[derive(Clone)]
pub struct NativeSessionRef {
    pub family: String,
    pub access_hash: String,
    pub generation: u64,
    pub epoch: u64,
}
pub struct Confirmation {
    pub transaction_id: String,
    pub callback_port: u16,
    pub nonce: String,
    pub principal: AuthenticatedPrincipal,
}
#[derive(Serialize)]
pub struct SessionInfo {
    pub user_id: String,
    pub family: String,
    pub generation: u64,
    pub epoch: u64,
    pub display_name: String,
}
#[derive(Default, Serialize, Deserialize)]
struct AttemptSecret {
    authorization_url: String,
    login_cookie: String,
    browser_cookie: String,
    confirmation_nonce: String,
    provider_cookies: String,
    provider_expires: u64,
    display_name: String,
    handoff_code: String,
}
#[derive(Serialize, Deserialize)]
struct FamilySecret {
    cookies: String,
    grant: Grant,
}
struct Attempt(Vec<String>);
impl Attempt {
    fn id(&self) -> &str {
        &self.0[0]
    }
    fn phase(&self) -> &str {
        &self.0[10]
    }
    fn expires(&self) -> Result<u64> {
        number(&self.0[9])
    }
}
struct Family(Vec<String>);
impl Family {
    fn id(&self) -> &str {
        &self.0[0]
    }
    fn user(&self) -> &str {
        &self.0[1]
    }
    fn epoch(&self) -> Result<u64> {
        number(&self.0[2])
    }
    fn generation(&self) -> Result<u64> {
        number(&self.0[3])
    }
    fn phase(&self) -> &str {
        &self.0[4]
    }
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
fn random() -> Result<String> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|_| NativeError::Unavailable)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}
fn hash(value: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(value.as_bytes()))
}
fn proof(value: &str) -> bool {
    value.len() == 43
        && URL_SAFE_NO_PAD
            .decode(value)
            .is_ok_and(|bytes| bytes.len() == 32 && URL_SAFE_NO_PAD.encode(bytes) == value)
}
fn state_valid(value: &str) -> bool {
    (16..=256).contains(&value.len())
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
}
fn verifier(value: &str) -> bool {
    (43..=128).contains(&value.len())
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-._~".contains(&c))
}
fn operation(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|c| c.is_ascii_hexdigit())
}
fn canonical_user(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}
fn number(value: &str) -> Result<u64> {
    value.parse().map_err(|_| NativeError::Unavailable)
}
fn cookie_pair(value: &str, name: &str) -> Result<String> {
    let pair = value.split(';').next().ok_or(NativeError::Unauthorized)?;
    if pair
        .strip_prefix(&format!("{name}="))
        .is_none_or(str::is_empty)
    {
        return Err(NativeError::Unauthorized);
    }
    Ok(pair.into())
}
fn cookie_value<'a>(header: Option<&'a str>, name: &str) -> Result<&'a str> {
    let mut values = header
        .ok_or(NativeError::Unauthorized)?
        .split(';')
        .filter_map(|part| part.trim().split_once('='))
        .filter(|(key, _)| *key == name)
        .map(|(_, value)| value);
    let value = values.next().ok_or(NativeError::Unauthorized)?;
    if values.next().is_some() || !proof(value) {
        return Err(NativeError::Unauthorized);
    }
    Ok(value)
}
fn auth_error(error: AuthError) -> NativeError {
    match error {
        AuthError::Unauthorized => NativeError::Unauthorized,
        _ => NativeError::Unavailable,
    }
}
impl NativeSessions {
    pub async fn from_env(config: &AppConfig, pool: Option<&PgPool>) -> Result<Option<Self>> {
        if std::env::var("SPROYT_NATIVE_ENABLED").as_deref() != Ok("true") {
            return Ok(None);
        }
        let oidc = config.oidc().ok_or(NativeError::Configuration)?;
        let redirect =
            reqwest::Url::parse(oidc.redirect_url()).map_err(|_| NativeError::Configuration)?;
        let origin = redirect.origin().ascii_serialization();
        if origin != "https://sproyt-canary.bjoroy.me"
            || redirect.path() != "/auth/callback"
            || redirect.query().is_some()
            || redirect.fragment().is_some()
            || !redirect.username().is_empty()
            || redirect.password().is_some()
        {
            return Err(NativeError::Configuration);
        }
        let db = match config.database().kind() {
            DatabaseKind::Postgres => Db::Pg(pool.ok_or(NativeError::Configuration)?.clone()),
            DatabaseKind::Sqlite => Db::Sqlite(
                sqlx::sqlite::SqlitePoolOptions::new()
                    .max_connections(config.db_max_connections())
                    .connect(config.database().url())
                    .await
                    .map_err(|_| NativeError::Unavailable)?,
            ),
        };
        Ok(Some(
            Self::new(
                db,
                &origin,
                oidc.issuer(),
                oidc.session_key(),
                oidc.session_previous_keys(),
            )
            .await?,
        ))
    }
    async fn new(
        db: Db,
        origin: &str,
        issuer: &str,
        key: &str,
        previous: &[String],
    ) -> Result<Self> {
        let instance = hash(&format!("sproyt-native-v1\n{origin}\n{issuer}"));
        let service = Self(Arc::new(Inner {
            db,
            codec: Codec::new(key, previous, &instance)?,
            origin: origin.into(),
            issuer: issuer.into(),
            instance,
        }));
        let mut tx = service.0.db.begin().await?;
        tx.exec("INSERT INTO native_service(instance_id,origin,issuer,schema_version) VALUES($1,$2,$3,'1') ON CONFLICT DO NOTHING",&[&service.0.instance,origin,issuer]).await?;
        let rows = tx
            .rows(
                "SELECT instance_id,issuer,schema_version FROM native_service WHERE origin=$1",
                &[origin],
            )
            .await?;
        if rows.len() != 1 || rows[0] != [service.0.instance.clone(), issuer.into(), "1".into()] {
            return Err(NativeError::Configuration);
        }
        tx.commit().await?;
        Ok(service)
    }
    #[cfg(test)]
    pub(crate) async fn for_test(pool: sqlx::SqlitePool, issuer: &str, key: &str) -> Result<Self> {
        sqlx::raw_sql(include_str!(
            "../migrations/sqlite/0068_native_sessions.sql"
        ))
        .execute(&pool)
        .await
        .map_err(|_| NativeError::Unavailable)?;
        Self::new(
            Db::Sqlite(pool),
            "https://sproyt-canary.bjoroy.me",
            issuer,
            key,
            &[],
        )
        .await
    }
    pub fn info(&self) -> Info {
        Info {
            scope: SCOPE,
            environment: ENVIRONMENT,
            instance_id: self.0.instance.clone(),
            issuer: self.0.issuer.clone(),
        }
    }
    pub fn origin(&self) -> &str {
        &self.0.origin
    }
    async fn attempt(&self, tx: &mut Tx, id: &str) -> Result<Attempt> {
        let rows = tx
            .rows(
                &format!("SELECT {ATTEMPT_COLUMNS} FROM native_attempts WHERE id=$1"),
                &[id],
            )
            .await?;
        rows.into_iter()
            .next()
            .map(Attempt)
            .ok_or(NativeError::Unauthorized)
    }
    async fn locked_attempt(&self, tx: &mut Tx, id: &str) -> Result<Attempt> {
        // First write takes a row/write lock before reading mutable phase/family.
        // In particular cancel must observe a family concurrently issued by exchange.
        if tx
            .exec("UPDATE native_attempts SET phase=phase WHERE id=$1", &[id])
            .await?
            != 1
        {
            return Err(NativeError::Unauthorized);
        }
        self.attempt(tx, id).await
    }
    async fn family(&self, tx: &mut Tx, id: &str) -> Result<Family> {
        let rows = tx
            .rows(
                &format!("SELECT {FAMILY_COLUMNS} FROM native_families WHERE family=$1"),
                &[id],
            )
            .await?;
        rows.into_iter()
            .next()
            .map(Family)
            .ok_or(NativeError::Unauthorized)
    }
    async fn epoch(&self, tx: &mut Tx, user: &str) -> Result<(u64, u64)> {
        let rows = tx
            .rows(
                "SELECT epoch,changed_at FROM native_epochs WHERE user_id=$1",
                &[user],
            )
            .await?;
        match rows.first() {
            Some(row) => Ok((number(&row[0])?, number(&row[1])?)),
            None => Ok((0, 0)),
        }
    }
    async fn fence(&self, tx: &mut Tx, family: &Family) -> Result<()> {
        if !matches!(family.phase(), "active" | "refreshing")
            || self.epoch(tx, family.user()).await?.0 != family.epoch()?
        {
            return Err(NativeError::Unauthorized);
        }
        if family.phase() == "refreshing" && number(&family.0[11])? <= now() {
            tx.exec("UPDATE native_families SET phase='reauth',secret='' WHERE family=$1 AND phase='refreshing' AND lease_id=$2",&[family.id(),&family.0[10]]).await?;
            return Err(NativeError::Unauthorized);
        }
        Ok(())
    }
    fn family_secret(&self, family: &Family) -> Result<FamilySecret> {
        let secret: FamilySecret = self
            .0
            .codec
            .open(&format!("family:{}", family.id()), &family.0[9])?;
        let grant = &secret.grant;
        if grant.scope != SCOPE
            || grant.environment != ENVIRONMENT
            || grant.instance_id != self.0.instance
            || grant.user_id != family.user()
            || grant.family != family.id()
            || grant.epoch != family.epoch()?
            || grant.generation != family.generation()?
            || hash(&grant.access_token) != family.0[5]
            || hash(&grant.refresh_token) != family.0[6]
            || hash(&grant.revoke_token) != family.0[7]
            || grant.expires_at != number(&family.0[8])?
        {
            return Err(NativeError::Unauthorized);
        }
        Ok(secret)
    }
    pub async fn login(&self, auth: &AuthService, input: LoginRequest) -> Result<LoginStarted> {
        if !proof(&input.challenge)
            || input.destination_id != "windows-loopback"
            || input.callback_port < 1024
            || input
                .expected_user_id
                .as_deref()
                .is_some_and(|id| !canonical_user(id))
        {
            return Err(NativeError::Invalid);
        }
        // This invokes the existing confidential OIDC client for a NEW transaction.
        // Browser session/refresh cookies are neither accepted nor copied here.
        let login = auth
            .login(Some("/auth/native/confirmed".into()))
            .map_err(auth_error)?;
        let authorization =
            reqwest::Url::parse(&login.authorization_url).map_err(|_| NativeError::Unavailable)?;
        let states: Vec<_> = authorization
            .query_pairs()
            .filter(|(key, _)| key == "state")
            .map(|(_, value)| value.into_owned())
            .collect();
        if states.len() != 1 || !state_valid(&states[0]) {
            return Err(NativeError::Unavailable);
        }
        let id = random()?;
        let created = now();
        let expected = input.expected_user_id.unwrap_or_default();
        let payload = AttemptSecret {
            authorization_url: login.authorization_url,
            login_cookie: cookie_pair(&login.set_cookie, LOGIN_COOKIE)?,
            browser_cookie: random()?,
            ..Default::default()
        };
        let secret = self.0.codec.seal(&format!("attempt:{id}"), &payload)?;
        let mut tx = self.0.db.begin().await?;
        let epoch = self.epoch(&mut tx, &expected).await?.0;
        // A bounded pilot ledger fails closed rather than accepting unbounded attempts.
        let count = tx
            .rows("SELECT CAST(count(*) AS TEXT) FROM native_attempts", &[])
            .await?;
        if number(&count[0][0])? >= 4096 {
            return Err(NativeError::Unavailable);
        }
        tx.exec("INSERT INTO native_attempts(id,state_hash,challenge,expected_user_id,start_epoch,epoch,user_id,callback_port,created_at,expires_at,phase,cookie_hash,confirm_hash,code_hash,handoff_expires,redemption_id,receipt_expires,family,secret,receipt) VALUES($1,$2,$3,$4,$5,'','','',$6,$7,'new','','','','','','','',$8,'')",&[&id,&hash(&states[0]),&input.challenge,&expected,&epoch.to_string(),&created.to_string(),&(created+600).to_string(),&secret]).await?;
        tx.exec(
            "UPDATE native_attempts SET callback_port=$1 WHERE id=$2",
            &[&input.callback_port.to_string(), &id],
        )
        .await?;
        tx.commit().await?;
        Ok(LoginStarted {
            transaction_id: id.clone(),
            authorization_url: format!("{}/auth/native/browser/{id}", self.0.origin),
        })
    }
    pub async fn browser(&self, id: &str) -> Result<(String, String)> {
        if !proof(id) {
            return Err(NativeError::Unauthorized);
        }
        let mut tx = self.0.db.begin().await?;
        let attempt = self.locked_attempt(&mut tx, id).await?;
        if attempt.phase() != "new" || attempt.expires()? <= now() {
            return Err(NativeError::Unauthorized);
        }
        let secret: AttemptSecret = self
            .0
            .codec
            .open(&format!("attempt:{id}"), &attempt.0[18])?;
        if tx.exec("UPDATE native_attempts SET phase='authorizing',cookie_hash=$1 WHERE id=$2 AND phase='new'",&[&hash(&secret.browser_cookie),id]).await?!=1{return Err(NativeError::Unauthorized);}
        tx.commit().await?;
        Ok((
            secret.authorization_url,
            format!(
                "sproyt_native_{id}={}; Path=/auth; HttpOnly; Secure; SameSite=Lax; Max-Age=600",
                secret.browser_cookie
            ),
        ))
    }
    pub async fn owns_state(&self, state: &str) -> Result<bool> {
        Ok(!self
            .0
            .db
            .rows(
                "SELECT id FROM native_attempts WHERE state_hash=$1",
                &[&hash(state)],
            )
            .await?
            .is_empty())
    }
    pub async fn callback(
        &self,
        auth: &AuthService,
        code: String,
        state: String,
        header: Option<&str>,
    ) -> Result<Confirmation> {
        if code.is_empty() || code.len() > 4096 || !state_valid(&state) {
            return Err(NativeError::Unauthorized);
        }
        let rows = self
            .0
            .db
            .rows(
                "SELECT id FROM native_attempts WHERE state_hash=$1",
                &[&hash(&state)],
            )
            .await?;
        let id = rows.first().ok_or(NativeError::Unauthorized)?[0].clone();
        let mut tx = self.0.db.begin().await?;
        let attempt = self.locked_attempt(&mut tx, &id).await?;
        let cookie = cookie_value(header, &format!("sproyt_native_{id}"))?;
        if attempt.phase() != "authorizing"
            || attempt.expires()? <= now()
            || hash(cookie) != attempt.0[11]
        {
            return Err(NativeError::Unauthorized);
        }
        let mut secret: AttemptSecret = self
            .0
            .codec
            .open(&format!("attempt:{id}"), &attempt.0[18])?;
        if tx
            .exec(
                "UPDATE native_attempts SET phase='verifying' WHERE id=$1 AND phase='authorizing'",
                &[&id],
            )
            .await?
            != 1
        {
            return Err(NativeError::Unauthorized);
        }
        tx.commit().await?; // Claim BEFORE provider code exchange; never retry uncertain code use.
        let login = match auth.callback(code, state, Some(&secret.login_cookie)).await {
            Ok(login) => login,
            Err(error) => {
                let mut tx = self.0.db.begin().await?;
                tx.exec("UPDATE native_attempts SET phase='failed',secret='' WHERE id=$1 AND phase='verifying'",&[&id]).await?;
                tx.commit().await?;
                return Err(auth_error(error));
            }
        };
        let user = login.principal.user.id.to_string();
        if (!attempt.0[3].is_empty() && attempt.0[3] != user) || login.set_refresh_cookie.is_none()
        {
            return Err(NativeError::Unauthorized);
        }
        secret.provider_cookies = format!(
            "{}; {}",
            cookie_pair(&login.set_cookie, SESSION_COOKIE)?,
            cookie_pair(
                login
                    .set_refresh_cookie
                    .as_deref()
                    .ok_or(NativeError::Unauthorized)?,
                REFRESH_COOKIE
            )?
        );
        secret.provider_expires = auth
            .native_session_expires_at(&secret.provider_cookies)
            .map_err(auth_error)?;
        secret.display_name = login.principal.user.display_name.to_string();
        secret.confirmation_nonce = random()?;
        secret.login_cookie.clear();
        secret.authorization_url.clear();
        let mut tx = self.0.db.begin().await?;
        let epoch = self.epoch(&mut tx, &user).await?;
        if (!attempt.0[3].is_empty() && number(&attempt.0[4])? != epoch.0)
            || (attempt.0[3].is_empty() && epoch.1 >= number(&attempt.0[8])? && epoch.1 > 0)
        {
            return Err(NativeError::Unauthorized);
        }
        let sealed = self.0.codec.seal(&format!("attempt:{id}"), &secret)?;
        if tx.exec("UPDATE native_attempts SET phase='confirm',user_id=$1,epoch=$2,confirm_hash=$3,secret=$4 WHERE id=$5 AND phase='verifying'",&[&user,&epoch.0.to_string(),&hash(&secret.confirmation_nonce),&sealed,&id]).await?!=1{return Err(NativeError::Unauthorized);}
        tx.commit().await?;
        Ok(Confirmation {
            callback_port: attempt.0[7].parse().map_err(|_| NativeError::Unavailable)?,
            transaction_id: id,
            nonce: secret.confirmation_nonce,
            principal: login.principal,
        })
    }
    pub async fn confirm(&self, id: &str, nonce: &str, header: Option<&str>) -> Result<String> {
        if !proof(id) || !proof(nonce) {
            return Err(NativeError::Unauthorized);
        }
        let mut tx = self.0.db.begin().await?;
        let attempt = self.locked_attempt(&mut tx, id).await?;
        if !matches!(attempt.phase(), "confirm" | "ready")
            || attempt.expires()? <= now()
            || hash(cookie_value(header, &format!("sproyt_native_{id}"))?) != attempt.0[11]
            || hash(nonce) != attempt.0[12]
            || self.epoch(&mut tx, &attempt.0[6]).await?.0 != number(&attempt.0[5])?
        {
            return Err(NativeError::Unauthorized);
        }
        let mut secret: AttemptSecret = self
            .0
            .codec
            .open(&format!("attempt:{id}"), &attempt.0[18])?;
        if secret.provider_expires <= now() {
            return Err(NativeError::Unauthorized);
        }
        if attempt.phase() == "confirm" {
            secret.handoff_code = random()?;
            let sealed = self.0.codec.seal(&format!("attempt:{id}"), &secret)?;
            if tx.exec("UPDATE native_attempts SET phase='ready',code_hash=$1,handoff_expires=$2,secret=$3 WHERE id=$4 AND phase='confirm'",&[&hash(&secret.handoff_code),&(now()+60).min(attempt.expires()?).to_string(),&sealed,id]).await?!=1{return Err(NativeError::Unauthorized);}
        } else if number(&attempt.0[14])? <= now() {
            return Err(NativeError::Unauthorized);
        }
        tx.commit().await?;
        Ok(format!(
            "http://127.0.0.1:{}/return?transaction_id={id}&code={}",
            attempt.0[7], secret.handoff_code
        ))
    }
    pub async fn exchange(&self, input: ExchangeRequest) -> Result<ExchangeResult> {
        if !proof(&input.transaction_id)
            || !proof(&input.code)
            || !verifier(&input.verifier)
            || !operation(&input.redemption_id)
            || input.environment != ENVIRONMENT
            || input.destination_id != "windows-loopback"
        {
            return Err(NativeError::Unauthorized);
        }
        let mut tx = self.0.db.begin().await?;
        let attempt = self.locked_attempt(&mut tx, &input.transaction_id).await?;
        if hash(&input.verifier) != attempt.0[2]
            || hash(&input.code) != attempt.0[13]
            || self.epoch(&mut tx, &attempt.0[6]).await?.0 != number(&attempt.0[5])?
        {
            return Err(NativeError::Unauthorized);
        }
        if attempt.phase() == "redeemed" {
            if attempt.0[15] != input.redemption_id || number(&attempt.0[16])? <= now() {
                return Err(NativeError::Unauthorized);
            }
            let family = self.family(&mut tx, &attempt.0[17]).await?;
            if let Err(error) = self.fence(&mut tx, &family).await {
                tx.commit().await?;
                return Err(error);
            }
            return self
                .0
                .codec
                .open(&format!("handoff:{}", attempt.id()), &attempt.0[19]);
        }
        if attempt.phase() != "ready" || number(&attempt.0[14])? <= now() {
            return Err(NativeError::Unauthorized);
        }
        let secret: AttemptSecret = self
            .0
            .codec
            .open(&format!("attempt:{}", attempt.id()), &attempt.0[18])?;
        let grant = Grant {
            scope: SCOPE.into(),
            environment: ENVIRONMENT.into(),
            instance_id: self.0.instance.clone(),
            user_id: attempt.0[6].clone(),
            family: random()?,
            generation: 0,
            epoch: number(&attempt.0[5])?,
            access_token: format!("sproyt-access.{}", random()?),
            refresh_token: format!("sproyt-refresh.{}", random()?),
            revoke_token: format!("sproyt-revoke.{}", random()?),
            expires_at: (now() + 300).min(secret.provider_expires),
            display_name: secret.display_name,
        };
        if grant.expires_at <= now() {
            return Err(NativeError::Unauthorized);
        }
        let sealed = self.0.codec.seal(
            &format!("family:{}", grant.family),
            &FamilySecret {
                cookies: secret.provider_cookies,
                grant: grant.clone(),
            },
        )?;
        let result = ExchangeResult {
            user_id: grant.user_id.clone(),
            native_session: grant.clone(),
            display_name: grant.display_name.clone(),
        };
        let receipt = self
            .0
            .codec
            .seal(&format!("handoff:{}", attempt.id()), &result)?;
        if tx.exec("UPDATE native_attempts SET phase='redeemed',redemption_id=$1,receipt_expires=$2,family=$3,secret='',receipt=$4 WHERE id=$5 AND phase='ready'",&[&input.redemption_id,&(now()+60).to_string(),&grant.family,&receipt,attempt.id()]).await?!=1{return Err(NativeError::Unauthorized);}
        tx.exec("INSERT INTO native_families(family,user_id,epoch,generation,phase,access_hash,refresh_hash,revoke_hash,expires_at,secret,lease_id,lease_expires) VALUES($1,$2,$3,'0','active',$4,$5,$6,$7,$8,'','0')",&[&grant.family,&grant.user_id,&grant.epoch.to_string(),&hash(&grant.access_token),&hash(&grant.refresh_token),&hash(&grant.revoke_token),&grant.expires_at.to_string(),&sealed]).await?;
        tx.commit().await?;
        Ok(result)
    }
    pub async fn cancel_login(&self, input: CancelRequest) -> Result<()> {
        if !proof(&input.transaction_id)
            || !verifier(&input.verifier)
            || input.environment != ENVIRONMENT
            || input.destination_id != "windows-loopback"
        {
            return Err(NativeError::Unauthorized);
        }
        let mut tx = self.0.db.begin().await?;
        let attempt = self.locked_attempt(&mut tx, &input.transaction_id).await?;
        if hash(&input.verifier) != attempt.0[2] {
            return Err(NativeError::Unauthorized);
        }
        // Lock the attempt with this proof-bound write, then revoke its server-owned
        // family in the same transaction. Never accept a caller-supplied family ID.
        tx.exec(
            "UPDATE native_attempts SET phase='cancelled',secret='' WHERE id=$1",
            &[attempt.id()],
        )
        .await?;
        if !attempt.0[17].is_empty() {
            tx.exec(
                "UPDATE native_families SET phase='revoked',secret='' WHERE family=$1",
                &[&attempt.0[17]],
            )
            .await?;
        }
        tx.commit().await
    }
    pub async fn revoke(&self, token: &str) -> Result<()> {
        if !token.strip_prefix("sproyt-revoke.").is_some_and(proof) {
            return Err(NativeError::Unauthorized);
        }
        let mut tx = self.0.db.begin().await?;
        if tx
            .exec(
                "UPDATE native_families SET phase='revoked',secret='' WHERE revoke_hash=$1",
                &[&hash(token)],
            )
            .await?
            != 1
        {
            return Err(NativeError::Unauthorized);
        }
        tx.commit().await
    }
    async fn referenced(&self, reference: &NativeSessionRef) -> Result<FamilySecret> {
        let mut tx = self.0.db.begin().await?;
        let family = self.family(&mut tx, &reference.family).await?;
        if let Err(error) = self.fence(&mut tx, &family).await {
            tx.commit().await?;
            return Err(error);
        }
        if family.0[5] != reference.access_hash
            || family.generation() != Ok(reference.generation)
            || family.epoch() != Ok(reference.epoch)
            || number(&family.0[8])? <= now()
        {
            return Err(NativeError::Unauthorized);
        }
        self.family_secret(&family)
    }
    pub async fn authenticate_ref(
        &self,
        auth: &AuthService,
        token: &str,
    ) -> Result<(AuthenticatedPrincipal, NativeSessionRef)> {
        if !token.strip_prefix("sproyt-access.").is_some_and(proof) {
            return Err(NativeError::Unauthorized);
        }
        let hash = hash(token);
        let rows = self
            .0
            .db
            .rows(
                "SELECT family,generation,epoch FROM native_families WHERE access_hash=$1",
                &[&hash],
            )
            .await?;
        let row = rows.first().ok_or(NativeError::Unauthorized)?;
        let reference = NativeSessionRef {
            family: row[0].clone(),
            access_hash: hash,
            generation: number(&row[1])?,
            epoch: number(&row[2])?,
        };
        let principal = self.recheck(auth, &reference).await?;
        Ok((principal, reference))
    }
    pub async fn authenticate(
        &self,
        auth: &AuthService,
        token: &str,
    ) -> Result<AuthenticatedPrincipal> {
        self.authenticate_ref(auth, token)
            .await
            .map(|(principal, _)| principal)
    }
    pub async fn recheck(
        &self,
        auth: &AuthService,
        reference: &NativeSessionRef,
    ) -> Result<AuthenticatedPrincipal> {
        let secret = self.referenced(reference).await?;
        let principal = auth
            .authenticate_session(Some(&secret.cookies))
            .await
            .map_err(auth_error)?;
        if principal.user.id.to_string() != secret.grant.user_id {
            return Err(NativeError::Unauthorized);
        }
        self.referenced(reference).await?;
        Ok(principal)
    }
    pub async fn session(&self, auth: &AuthService, token: &str) -> Result<SessionInfo> {
        let (principal, reference) = self.authenticate_ref(auth, token).await?;
        Ok(SessionInfo {
            user_id: principal.user.id.to_string(),
            family: reference.family,
            generation: reference.generation,
            epoch: reference.epoch,
            display_name: principal.user.display_name.to_string(),
        })
    }
    pub async fn refresh(
        &self,
        auth: &AuthService,
        token: &str,
        input: RefreshRequest,
    ) -> Result<Grant> {
        if !token.strip_prefix("sproyt-refresh.").is_some_and(proof)
            || !operation(&input.operation_id)
            || input.generation == u64::MAX
        {
            return Err(NativeError::Unauthorized);
        }
        let old_hash = hash(token);
        let generation = input.generation.to_string();
        let receipt_purpose = format!("refresh:{old_hash}:{}:{generation}", input.operation_id);
        let mut tx = self.0.db.begin().await?;
        let rows=tx.rows("SELECT family,expires_at,result FROM native_refresh_receipts WHERE old_hash=$1 AND operation_id=$2 AND generation=$3",&[&old_hash,&input.operation_id,&generation]).await?;
        if let Some(row) = rows.first() {
            let family = self.family(&mut tx, &row[0]).await?;
            if let Err(error) = self.fence(&mut tx, &family).await {
                tx.commit().await?;
                return Err(error);
            }
            if number(&row[1])? <= now() {
                return Err(NativeError::Unauthorized);
            }
            return self.0.codec.open(&receipt_purpose, &row[2]);
        }
        let rows = tx
            .rows(
                &format!("SELECT {FAMILY_COLUMNS} FROM native_families WHERE refresh_hash=$1"),
                &[&old_hash],
            )
            .await?;
        let family = rows
            .into_iter()
            .next()
            .map(Family)
            .ok_or(NativeError::Unauthorized)?;
        if let Err(error) = self.fence(&mut tx, &family).await {
            tx.commit().await?;
            return Err(error);
        }
        if family.generation()? != input.generation {
            return Err(NativeError::Unauthorized);
        }
        if family.phase() == "refreshing" {
            return Err(NativeError::Unavailable);
        }
        let mut secret = self.family_secret(&family)?;
        let lease = random()?;
        if tx.exec("UPDATE native_families SET phase='refreshing',lease_id=$1,lease_expires=$2 WHERE family=$3 AND phase='active' AND refresh_hash=$4 AND generation=$5",&[&lease,&(now()+30).to_string(),family.id(),&old_hash,&generation]).await?!=1{return Err(NativeError::Unavailable);}
        tx.commit().await?; // Never repeat this upstream grant after an uncertain outcome.
        let renewal = match auth.renew_native_session(Some(&secret.cookies)).await {
            Ok(value) => value,
            Err(error) => {
                self.require_reauthentication(family.id(), &lease).await?;
                return Err(auth_error(error));
            }
        };
        secret.cookies = format!(
            "{}; {}",
            cookie_pair(&renewal.set_cookie, SESSION_COOKIE)?,
            cookie_pair(&renewal.set_refresh_cookie, REFRESH_COOKIE)?
        );
        let principal = auth
            .authenticate_session(Some(&secret.cookies))
            .await
            .map_err(auth_error)?;
        if principal.user.id.to_string() != family.user() {
            self.require_reauthentication(family.id(), &lease).await?;
            return Err(NativeError::Unauthorized);
        }
        secret.grant.generation += 1;
        secret.grant.access_token = format!("sproyt-access.{}", random()?);
        secret.grant.refresh_token = format!("sproyt-refresh.{}", random()?);
        secret.grant.expires_at = (now() + 300).min(
            auth.native_session_expires_at(&secret.cookies)
                .map_err(auth_error)?,
        );
        secret.grant.display_name = principal.user.display_name.to_string();
        let sealed = self
            .0
            .codec
            .seal(&format!("family:{}", family.id()), &secret)?;
        let receipt = self.0.codec.seal(&receipt_purpose, &secret.grant)?;
        let mut tx = self.0.db.begin().await?;
        let current = self.family(&mut tx, family.id()).await?;
        if let Err(error) = self.fence(&mut tx, &current).await {
            tx.commit().await?;
            return Err(error);
        }
        if current.0[10] != lease || current.phase() != "refreshing" {
            return Err(NativeError::Unauthorized);
        }
        if tx.exec("UPDATE native_families SET phase='active',generation=$1,access_hash=$2,refresh_hash=$3,expires_at=$4,secret=$5,lease_id='',lease_expires='0' WHERE family=$6 AND phase='refreshing' AND lease_id=$7",&[&secret.grant.generation.to_string(),&hash(&secret.grant.access_token),&hash(&secret.grant.refresh_token),&secret.grant.expires_at.to_string(),&sealed,family.id(),&lease]).await?!=1{return Err(NativeError::Unauthorized);}
        tx.exec("INSERT INTO native_refresh_receipts(old_hash,operation_id,generation,family,expires_at,result) VALUES($1,$2,$3,$4,$5,$6)",&[&old_hash,&input.operation_id,&generation,family.id(),&(now()+60).to_string(),&receipt]).await?;
        tx.commit().await?;
        Ok(secret.grant)
    }
    async fn require_reauthentication(&self, family: &str, lease: &str) -> Result<()> {
        let mut tx = self.0.db.begin().await?;
        tx.exec("UPDATE native_families SET phase='reauth',secret='' WHERE family=$1 AND phase='refreshing' AND lease_id=$2",&[family,lease]).await?;
        tx.commit().await
    }
}

#[cfg(test)]
mod tests;
