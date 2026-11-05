//! Persistent, binding-scoped family app data. All external I/O happens before transactions.
mod db;
#[cfg(test)]
mod tests;

use crate::{
    agent::AgentPrincipal,
    config::{AppConfig, DatabaseConfig, DatabaseKind},
    domain::UserId,
};
use chrono::Utc;
use db::{Db, Tx};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{
    PgPool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::{collections::BTreeMap, str::FromStr, sync::Arc, time::Duration};
use uuid::Uuid;

type Result<T> = std::result::Result<T, FamilyError>;
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FamilyError {
    #[error("app authentication required")]
    Unauthorized,
    #[error("app operation forbidden")]
    Forbidden,
    #[error("binding not found")]
    NotFound,
    #[error("binding disabled")]
    Disabled,
    #[error("invalid app request")]
    Invalid,
    #[error("revision conflict")]
    RevisionConflict,
    #[error("idempotency conflict")]
    Conflict,
    #[error("app service unavailable")]
    Unavailable,
    #[error("invalid app configuration")]
    Configuration,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AppScope {
    #[serde(rename = "responses:read")]
    ResponsesRead,
    #[serde(rename = "attributes:write")]
    AttributesWrite,
    #[serde(rename = "messages:write")]
    MessagesWrite,
    #[serde(rename = "media:write")]
    MediaWrite,
}
impl AppScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ResponsesRead => "responses:read",
            Self::AttributesWrite => "attributes:write",
            Self::MessagesWrite => "messages:write",
            Self::MediaWrite => "media:write",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub app_id: String,
    pub name: String,
    pub version: u32,
    pub agent_id: String,
    pub sharing_notice: String,
    pub notice_version: u32,
    pub attributes: BTreeMap<String, AttributeField>,
    pub scopes: Vec<AppScope>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum AttributeField {
    String { max_length: usize },
    Boolean,
    Integer { minimum: i64, maximum: i64 },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
pub enum ScopeRef {
    Channel(String),
    Circle(String),
}
impl ScopeRef {
    pub fn id(&self) -> &str {
        match self {
            Self::Channel(id) | Self::Circle(id) => id,
        }
    }
    pub fn kind(&self) -> &str {
        match self {
            Self::Channel(_) => "channel",
            Self::Circle(_) => "circle",
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Participation {
    Yes,
    No,
}
impl Participation {
    fn as_str(self) -> &'static str {
        match self {
            Self::Yes => "yes",
            Self::No => "no",
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Binding {
    pub binding_id: String,
    pub app_id: String,
    pub scope: ScopeRef,
    pub resource_id: String,
    pub resource_name: String,
    pub manifest_version: u32,
    pub revision: i64,
    pub status: String,
}
#[derive(Serialize)]
pub struct AuthorizationStarted {
    pub state: String,
    pub binding_id: String,
    pub expires_at: i64,
}
// Only a trusted outbound adapter may construct this; never deserialize client JSON into it.
pub struct AuthorizationProof {
    pub authorization_id: String,
    pub app_user_id: String,
    pub session_id: String,
    pub session_name: String,
    pub verified_email: Option<String>,
    pub expires_at: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemberResponse {
    pub subject_id: String,
    pub display_name: String,
    pub response: Value,
    pub app_attributes: Value,
    pub revision: i64,
    pub updated_at: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct OwnResponse {
    pub participation: Option<Participation>,
    pub sharing: bool,
    pub app_attributes: Value,
    pub revision: i64,
}
#[derive(Clone, Debug, Serialize)]
pub struct Snapshot {
    pub binding_id: String,
    pub items: Vec<MemberResponse>,
    pub changes_cursor: i64,
}
#[derive(Clone, Debug, Serialize)]
pub struct Change {
    pub event_id: String,
    pub sequence: i64,
    pub kind: String,
    pub subject_id: String,
    pub revision: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member: Option<MemberResponse>,
}
#[derive(Clone)]
pub struct FamilyApps {
    db: Db,
    manifests: Arc<BTreeMap<String, Manifest>>,
    environment: String,
}
fn now() -> i64 {
    Utc::now().timestamp()
}
fn opaque() -> String {
    Uuid::now_v7().to_string()
}
fn hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
fn valid(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}
impl FamilyApps {
    pub async fn connect(
        config: &DatabaseConfig,
        pool: Option<&PgPool>,
        manifests: Vec<Manifest>,
    ) -> Result<Self> {
        let db = match config.kind() {
            DatabaseKind::Postgres => Db::Pg(pool.ok_or(FamilyError::Configuration)?.clone()),
            DatabaseKind::Sqlite => {
                let options = SqliteConnectOptions::from_str(config.url())
                    .map_err(|_| FamilyError::Configuration)?
                    .foreign_keys(true)
                    .busy_timeout(Duration::from_secs(5))
                    .create_if_missing(true);
                Db::Sqlite(
                    SqlitePoolOptions::new()
                        .max_connections(1)
                        .connect_with(options)
                        .await
                        .map_err(|_| FamilyError::Unavailable)?,
                )
            }
        };
        let mut configured = BTreeMap::new();
        for manifest in manifests {
            if !valid(&manifest.app_id, 80)
                || !valid(&manifest.name, 120)
                || manifest.version == 0
                || manifest.notice_version == 0
                || !valid(&manifest.sharing_notice, 4000)
                || Uuid::parse_str(&manifest.agent_id).is_err()
                || manifest.attributes.len() > 16
            {
                return Err(FamilyError::Configuration);
            }
            for (name, field) in &manifest.attributes {
                if !valid(name, 64)
                    || name == "participation"
                    || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                    || matches!(field,AttributeField::String{max_length} if *max_length==0||*max_length>512)
                    || matches!(field,AttributeField::Integer{minimum,maximum} if minimum>maximum)
                {
                    return Err(FamilyError::Configuration);
                }
            }
            if configured
                .insert(manifest.app_id.clone(), manifest)
                .is_some()
            {
                return Err(FamilyError::Configuration);
            }
        }
        Ok(Self {
            db,
            manifests: Arc::new(configured),
            environment: "canary".into(),
        })
    }
    pub async fn from_env(config: &AppConfig, pool: Option<&PgPool>) -> Result<Option<Self>> {
        let Some(raw) = std::env::var("SPROYT_FAMILY_APPS_MANIFESTS")
            .ok()
            .filter(|s| !s.trim().is_empty())
        else {
            return Ok(None);
        };
        if config.oidc().is_none_or(|oidc| {
            oidc.redirect_url() != "https://sproyt-canary.bjoroy.me/auth/callback"
        }) {
            return Err(FamilyError::Configuration);
        }
        let manifests: Vec<Manifest> =
            serde_json::from_str(&raw).map_err(|_| FamilyError::Configuration)?;
        Ok(Some(
            Self::connect(config.database(), pool, manifests).await?,
        ))
    }
    pub fn catalog(&self) -> Vec<Manifest> {
        self.manifests.values().cloned().collect()
    }
    fn manifest(&self, app: &str) -> Result<&Manifest> {
        self.manifests.get(app).ok_or(FamilyError::NotFound)
    }
    pub async fn prepare_authorization(
        &self,
        actor: &UserId,
        app: &str,
        scope: ScopeRef,
        resource_id: &str,
    ) -> Result<AuthorizationStarted> {
        self.manifest(app)?;
        if !valid(resource_id, 200) {
            return Err(FamilyError::Invalid);
        }
        let mut tx = self.db.begin().await?;
        self.admin(&mut tx, &actor.to_string(), &scope).await?;
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).map_err(|_| FamilyError::Unavailable)?;
        let state = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let binding = opaque();
        let expiry = now() + 300;
        tx.exec("insert into app_authorizations(state_hash,binding_id,app_id,actor_id,scope_kind,scope_id,resource_id,expires_at) values($1,$2,$3,$4::id,$5,$6,$7,CAST($8 AS bigint))",&[&hash(&state),&binding,app,&actor.to_string(),scope.kind(),scope.id(),resource_id,&expiry.to_string()]).await?;
        tx.commit().await?;
        Ok(AuthorizationStarted {
            state,
            binding_id: binding,
            expires_at: expiry,
        })
    }
    pub async fn create_binding(
        &self,
        actor: &UserId,
        state: &str,
        proof: AuthorizationProof,
        current_verified_email: Option<&str>,
    ) -> Result<Binding> {
        if !valid(&proof.authorization_id, 200)
            || !valid(&proof.app_user_id, 200)
            || !valid(&proof.session_name, 200)
            || proof.expires_at <= now()
        {
            return Err(FamilyError::Invalid);
        }
        let mut tx = self.db.begin().await?;
        // DELETE RETURNING serializes consumption across replicas and rolls back on any later failure.
        let a=tx.rows("delete from app_authorizations where state_hash=$1 and actor_id=$2::id and expires_at>CAST($3 AS bigint) returning binding_id,app_id,scope_kind,scope_id,resource_id",&[&hash(state),&actor.to_string(),&now().to_string()]).await?.pop().ok_or(FamilyError::Forbidden)?;
        if a[4] != proof.session_id {
            return Err(FamilyError::Forbidden);
        }
        let scope = if a[2] == "channel" {
            ScopeRef::Channel(a[3].clone())
        } else {
            ScopeRef::Circle(a[3].clone())
        };
        self.admin(&mut tx, &actor.to_string(), &scope).await?;
        let manifest = self.manifest(&a[1])?;
        let link=tx.rows("select external_user_id from app_identity_links where app_id=$1 and user_id=$2::id",&[&a[1],&actor.to_string()]).await?;
        if let Some(link) = link.first() {
            if link[0] != proof.app_user_id {
                return Err(FamilyError::Forbidden);
            }
        } else {
            let email = current_verified_email
                .ok_or(FamilyError::Forbidden)?
                .trim()
                .to_lowercase();
            if !valid(&email, 320)
                || Some(email.as_str())
                    != proof
                        .verified_email
                        .as_deref()
                        .map(str::trim)
                        .map(str::to_lowercase)
                        .as_deref()
            {
                return Err(FamilyError::Forbidden);
            }
            tx.exec("insert into app_identity_links(app_id,user_id,external_user_id) values($1,$2::id,$3)",&[&a[1],&actor.to_string(),&proof.app_user_id]).await?;
        }
        tx.exec("insert into app_consumed_approvals(app_id,authorization_id,expires_at) values($1,$2,CAST($3 AS bigint))",&[&a[1],&proof.authorization_id,&proof.expires_at.to_string()]).await?;
        let (channel, circle) = match &scope {
            ScopeRef::Channel(id) => (id.as_str(), ""),
            ScopeRef::Circle(id) => ("", id.as_str()),
        };
        tx.exec("insert into app_bindings(id,app_id,channel_id,circle_id,resource_id,resource_name,manifest_version,notice_version,manifest_json,status,revision,created_by,environment) values($1,$2,NULLIF($3,'')::id,NULLIF($4,'')::id,$5,$6,CAST($7 AS bigint),CAST($8 AS bigint),$9::json,'active',0,$10::id,$11)",&[&a[0],&a[1],channel,circle,&proof.session_id,&proof.session_name,&manifest.version.to_string(),&manifest.notice_version.to_string(),&serde_json::to_string(manifest).map_err(|_|FamilyError::Invalid)?,&actor.to_string(),&self.environment]).await?;
        let binding = self.binding(&mut tx, &a[0], false).await?;
        tx.commit().await?;
        Ok(binding)
    }
    async fn admin(&self, tx: &mut Tx, actor: &str, scope: &ScopeRef) -> Result<()> {
        let allowed=match scope{
            ScopeRef::Channel(id)=>tx.rows("select 'ok' from channels c join channel_memberships m on m.channel_id=c.id join users u on u.id=m.user_id where c.id=$1::id and m.user_id=$2::id and u.kind='human' and (m.role='owner' or exists(select 1 from circle_memberships cm where cm.circle_id=c.circle_id and cm.user_id=m.user_id and cm.role in ('owner','moderator')))",&[id,actor]).await?,
            ScopeRef::Circle(id)=>tx.rows("select 'ok' from circle_memberships m join users u on u.id=m.user_id where m.circle_id=$1::id and m.user_id=$2::id and u.kind='human' and m.role in ('owner','moderator')",&[id,actor]).await?,
        };
        if allowed.is_empty() {
            Err(FamilyError::Forbidden)
        } else {
            Ok(())
        }
    }
    async fn binding(&self, tx: &mut Tx, id: &str, lock: bool) -> Result<Binding> {
        if lock {
            tx.exec(
                "update app_bindings set revision=revision where id=$1",
                &[id],
            )
            .await?;
        }
        let r=tx.rows("select app_id,coalesce(CAST(channel_id AS text),''),coalesce(CAST(circle_id AS text),''),resource_id,resource_name,CAST(manifest_version AS text),CAST(revision AS text),status,environment from app_bindings where id=$1",&[id]).await?.pop().ok_or(FamilyError::NotFound)?;
        if r[8] != self.environment {
            return Err(FamilyError::NotFound);
        }
        self.manifest(&r[0])?;
        Ok(Binding {
            binding_id: id.into(),
            app_id: r[0].clone(),
            scope: if r[1].is_empty() {
                ScopeRef::Circle(r[2].clone())
            } else {
                ScopeRef::Channel(r[1].clone())
            },
            resource_id: r[3].clone(),
            resource_name: r[4].clone(),
            manifest_version: r[5].parse().map_err(|_| FamilyError::Unavailable)?,
            revision: r[6].parse().map_err(|_| FamilyError::Unavailable)?,
            status: r[7].clone(),
        })
    }
    async fn epoch(&self, tx: &mut Tx, binding: &Binding, user: &str) -> Result<String> {
        let sql = match binding.scope {
            ScopeRef::Channel(_) => {
                "select app_membership_epoch from channel_memberships m join users u on u.id=m.user_id where m.channel_id=$1::id and m.user_id=$2::id and u.kind='human'"
            }
            ScopeRef::Circle(_) => {
                "select app_membership_epoch from circle_memberships m join users u on u.id=m.user_id where m.circle_id=$1::id and m.user_id=$2::id and u.kind='human'"
            }
        };
        tx.rows(sql, &[binding.scope.id(), user])
            .await?
            .pop()
            .map(|r| r[0].clone())
            .ok_or(FamilyError::Forbidden)
    }
    pub async fn respond(
        &self,
        actor: &UserId,
        id: &str,
        response: Participation,
        notice_version: u32,
    ) -> Result<OwnResponse> {
        let mut tx = self.db.begin().await?;
        let b = self.binding(&mut tx, id, true).await?;
        if b.status != "active" {
            return Err(FamilyError::Disabled);
        }
        let epoch = self.epoch(&mut tx, &b, &actor.to_string()).await?;
        let notice = tx
            .rows(
                "select CAST(notice_version AS text) from app_bindings where id=$1",
                &[id],
            )
            .await?;
        if notice[0][0] != notice_version.to_string() {
            return Err(FamilyError::Invalid);
        }
        tx.exec("insert into app_subjects(app_id,user_id,subject_id) values($1,$2::id,$3) on conflict(app_id,user_id) do nothing",&[&b.app_id,&actor.to_string(),&opaque()]).await?;
        tx.exec("insert into app_member_responses(binding_id,user_id,membership_epoch,participation,sharing,notice_version,revision,updated_at) values($1,$2::id,$3,$4,1,CAST($5 AS bigint),1,$6) on conflict(binding_id,user_id) do update set membership_epoch=excluded.membership_epoch,participation=excluded.participation,sharing=1,notice_version=excluded.notice_version,attributes=case when app_member_responses.membership_epoch=excluded.membership_epoch then app_member_responses.attributes else '{}'::json end,revision=app_member_responses.revision+1,event_kind='response.upserted',updated_at=excluded.updated_at",&[id,&actor.to_string(),&epoch,response.as_str(),&notice_version.to_string(),&Utc::now().to_rfc3339()]).await?;
        let own = self.own_in(&mut tx, &actor.to_string(), id).await?;
        tx.commit().await?;
        Ok(own)
    }
    pub async fn revoke(&self, actor: &UserId, id: &str) -> Result<OwnResponse> {
        let mut tx = self.db.begin().await?;
        self.binding(&mut tx, id, true).await?;
        tx.exec("update app_member_responses set sharing=0,revision=revision+1,updated_at=$3 where binding_id=$1 and user_id=$2::id and sharing=1",&[id,&actor.to_string(),&Utc::now().to_rfc3339()]).await?;
        let own = self.own_in(&mut tx, &actor.to_string(), id).await?;
        tx.commit().await?;
        Ok(own)
    }
    async fn own_in(&self, tx: &mut Tx, user: &str, id: &str) -> Result<OwnResponse> {
        let row=tx.rows("select participation,CAST(sharing AS text),CAST(revision AS text),CAST(attributes AS text) from app_member_responses where binding_id=$1 and user_id=$2::id",&[id,user]).await?.pop();
        Ok(match row {
            None => OwnResponse {
                participation: None,
                sharing: false,
                revision: 0,
                app_attributes: json!({}),
            },
            Some(r) => OwnResponse {
                participation: Some(if r[0] == "yes" {
                    Participation::Yes
                } else {
                    Participation::No
                }),
                sharing: r[1] == "1",
                revision: r[2].parse().map_err(|_| FamilyError::Unavailable)?,
                app_attributes: serde_json::from_str(&r[3])
                    .map_err(|_| FamilyError::Unavailable)?,
            },
        })
    }
    pub async fn own_response(&self, actor: &UserId, id: &str) -> Result<OwnResponse> {
        let mut tx = self.db.begin().await?;
        let b = self.binding(&mut tx, id, false).await?;
        self.epoch(&mut tx, &b, &actor.to_string()).await?;
        self.own_in(&mut tx, &actor.to_string(), id).await
    }
    async fn authorize(
        &self,
        tx: &mut Tx,
        principal: &AgentPrincipal,
        id: &str,
        scope: AppScope,
    ) -> Result<Binding> {
        let b = self.binding(tx, id, true).await?;
        let manifest = self.manifest(&b.app_id)?;
        if manifest.agent_id != principal.agent_id.to_string() {
            return Err(FamilyError::NotFound);
        }
        if b.status != "active" {
            return Err(FamilyError::Disabled);
        }
        if !manifest.scopes.contains(&scope) {
            return Err(FamilyError::Forbidden);
        }
        self.authenticate_principal(tx, principal).await?;
        Ok(b)
    }
    async fn authenticate_principal(&self, tx: &mut Tx, principal: &AgentPrincipal) -> Result<()> {
        // Existing credentials are rechecked here even if a caller accidentally caches a principal.
        if tx.rows("select 'ok' from agent_credentials c join agent_profiles p on p.agent_id=c.agent_id where c.id=$1::id and c.agent_id=$2::id and c.revoked_at is null and c.expires_at>$3::time and p.revoked_at is null and (p.expires_at is null or p.expires_at>$3::time)",&[&principal.credential_id.to_string(),&principal.agent_id.to_string(),&Utc::now().to_rfc3339()]).await?.is_empty(){return Err(FamilyError::Unauthorized)}
        Ok(())
    }
    pub async fn require_scope(
        &self,
        principal: &AgentPrincipal,
        id: &str,
        scope: AppScope,
    ) -> Result<Binding> {
        let mut tx = self.db.begin().await?;
        self.authorize(&mut tx, principal, id, scope).await
    }
    pub async fn bindings(&self, principal: &AgentPrincipal) -> Result<Vec<Binding>> {
        let mut tx = self.db.begin().await?;
        self.authenticate_principal(&mut tx, principal).await?;
        let mut result = vec![];
        for row in tx
            .rows(
                "select id,app_id from app_bindings where environment=$1 order by id limit 1000",
                &[&self.environment],
            )
            .await?
        {
            if self
                .manifests
                .get(&row[1])
                .is_none_or(|m| m.agent_id != principal.agent_id.to_string())
            {
                continue;
            }
            let b = self.binding(&mut tx, &row[0], false).await?;
            if self.manifest(&b.app_id)?.agent_id == principal.agent_id.to_string() {
                result.push(b)
            }
        }
        Ok(result)
    }
    async fn exported(&self, tx: &mut Tx, b: &Binding) -> Result<Vec<MemberResponse>> {
        let scope = match b.scope {
            ScopeRef::Channel(_) => "channel_memberships",
            ScopeRef::Circle(_) => "circle_memberships",
        };
        let column = match b.scope {
            ScopeRef::Channel(_) => "channel_id",
            ScopeRef::Circle(_) => "circle_id",
        };
        let sql = format!(
            "select s.subject_id,u.display_name,r.participation,CAST(r.attributes AS text),CAST(r.revision AS text),r.updated_at from app_member_responses r join app_subjects s on s.user_id=r.user_id and s.app_id=$2 join users u on u.id=r.user_id join {scope} m on m.user_id=r.user_id and m.{column}=$3::id and m.app_membership_epoch=r.membership_epoch where r.binding_id=$1 and r.sharing=1 order by s.subject_id limit 1001"
        );
        let rows = tx
            .rows(&sql, &[&b.binding_id, &b.app_id, b.scope.id()])
            .await?;
        if rows.len() > 1000 {
            return Err(FamilyError::Unavailable);
        }
        rows.into_iter()
            .map(|r| {
                Ok(MemberResponse {
                    subject_id: r[0].clone(),
                    display_name: r[1].clone(),
                    response: json!({"participation":r[2]}),
                    app_attributes: serde_json::from_str(&r[3])
                        .map_err(|_| FamilyError::Unavailable)?,
                    revision: r[4].parse().map_err(|_| FamilyError::Unavailable)?,
                    updated_at: r[5].clone(),
                })
            })
            .collect()
    }
    pub async fn snapshot(&self, principal: &AgentPrincipal, id: &str) -> Result<Snapshot> {
        let mut tx = self.db.begin().await?;
        let b = self
            .authorize(&mut tx, principal, id, AppScope::ResponsesRead)
            .await?;
        let items = self.exported(&mut tx, &b).await?;
        let cursor = b.revision;
        tx.commit().await?;
        Ok(Snapshot {
            binding_id: id.into(),
            items,
            changes_cursor: cursor,
        })
    }
    pub async fn changes(
        &self,
        principal: &AgentPrincipal,
        id: &str,
        cursor: i64,
        limit: usize,
    ) -> Result<Vec<Change>> {
        if cursor < 0 || limit == 0 || limit > 100 {
            return Err(FamilyError::Invalid);
        }
        let mut tx = self.db.begin().await?;
        let b = self
            .authorize(&mut tx, principal, id, AppScope::ResponsesRead)
            .await?;
        let current = self.exported(&mut tx, &b).await?;
        let events=tx.rows("select event_id,CAST(sequence AS text),kind,subject_id,CAST(revision AS text) from app_binding_events where binding_id=$1 and sequence>CAST($2 AS bigint) order by app_binding_events.sequence limit CAST($3 AS bigint)",&[id,&cursor.to_string(),&limit.to_string()]).await?;
        let mut result = vec![];
        for r in events {
            let member = current.iter().find(|m| m.subject_id == r[3]).cloned();
            let kind = if member.is_none() {
                "response.revoked".into()
            } else {
                r[2].clone()
            };
            let revision = member
                .as_ref()
                .map(|m| m.revision)
                .unwrap_or(r[4].parse().map_err(|_| FamilyError::Unavailable)?);
            result.push(Change {
                event_id: r[0].clone(),
                sequence: r[1].parse().map_err(|_| FamilyError::Unavailable)?,
                kind,
                subject_id: r[3].clone(),
                revision,
                member,
            });
        }
        tx.commit().await?;
        Ok(result)
    }
    pub async fn replace_attributes(
        &self,
        principal: &AgentPrincipal,
        id: &str,
        subject: &str,
        expected_revision: i64,
        key: &str,
        attributes: Value,
    ) -> Result<MemberResponse> {
        if !valid(key, 200) {
            return Err(FamilyError::Invalid);
        }
        let mut tx = self.db.begin().await?;
        let b = self
            .authorize(&mut tx, principal, id, AppScope::AttributesWrite)
            .await?;
        let pinned = tx
            .rows(
                "select CAST(manifest_json AS text) from app_bindings where id=$1",
                &[id],
            )
            .await?;
        let manifest: Manifest =
            serde_json::from_str(&pinned[0][0]).map_err(|_| FamilyError::Unavailable)?;
        let fields = attributes.as_object().ok_or(FamilyError::Invalid)?;
        if fields.len() > 16 {
            return Err(FamilyError::Invalid);
        }
        for (name, value) in fields {
            let field = manifest.attributes.get(name).ok_or(FamilyError::Invalid)?;
            let ok = match field {
                AttributeField::String { max_length } => value
                    .as_str()
                    .is_some_and(|s| s.len() <= *max_length && !s.chars().any(char::is_control)),
                AttributeField::Boolean => value.is_boolean(),
                AttributeField::Integer { minimum, maximum } => value
                    .as_i64()
                    .is_some_and(|n| n >= *minimum && n <= *maximum),
            };
            if !ok {
                return Err(FamilyError::Invalid);
            }
        }
        let fingerprint=hash(&json!({"subject":subject,"expected_revision":expected_revision,"attributes":attributes}).to_string());
        let receipt=tx.rows("select request_hash from app_command_receipts where binding_id=$1 and operation='attributes.replace' and idempotency_key=$2",&[id,key]).await?;
        if let Some(r) = receipt.first() {
            if r[0] != fingerprint {
                return Err(FamilyError::Conflict);
            };
            let member = self
                .exported(&mut tx, &b)
                .await?
                .into_iter()
                .find(|m| m.subject_id == subject)
                .ok_or(FamilyError::Forbidden)?;
            tx.commit().await?;
            return Ok(member);
        }
        let member = self
            .exported(&mut tx, &b)
            .await?
            .into_iter()
            .find(|m| m.subject_id == subject)
            .ok_or(FamilyError::Forbidden)?;
        if member.revision != expected_revision {
            return Err(FamilyError::RevisionConflict);
        }
        tx.exec("update app_member_responses set attributes=$3::json,writer_id=$4::id,revision=revision+1,event_kind='attributes.updated',updated_at=$5 where binding_id=$1 and user_id=(select user_id from app_subjects where app_id=$6 and subject_id=$2)",&[id,subject,&attributes.to_string(),&principal.agent_id.to_string(),&Utc::now().to_rfc3339(),&b.app_id]).await?;
        tx.exec("insert into app_command_receipts(binding_id,operation,idempotency_key,request_hash,expires_at) values($1,'attributes.replace',$2,$3,CAST($4 AS bigint))",&[id,key,&fingerprint,&(now()+86400*7).to_string()]).await?;
        let member = self
            .exported(&mut tx, &b)
            .await?
            .into_iter()
            .find(|m| m.subject_id == subject)
            .ok_or(FamilyError::Forbidden)?;
        tx.commit().await?;
        Ok(member)
    }
    pub async fn disable(&self, actor: &UserId, id: &str) -> Result<()> {
        let mut tx = self.db.begin().await?;
        let b = self.binding(&mut tx, id, true).await?;
        self.admin(&mut tx, &actor.to_string(), &b.scope).await?;
        tx.exec(
            "update app_bindings set status='disabled',revision=revision+1 where id=$1",
            &[id],
        )
        .await?;
        tx.commit().await
    }
}

#[derive(Clone, Serialize)]
pub struct PendingAuthorization {
    pub app_id: String,
    pub binding_id: String,
    pub scope: ScopeRef,
    pub resource_id: String,
    pub expires_at: i64,
}
impl FamilyApps {
    pub async fn pending_authorization(
        &self,
        actor: &UserId,
        state: &str,
    ) -> Result<PendingAuthorization> {
        let mut tx = self.db.begin().await?;
        let r=tx.rows("select app_id,binding_id,scope_kind,scope_id,resource_id,CAST(expires_at AS text) from app_authorizations where state_hash=$1 and actor_id=$2::id and expires_at>CAST($3 AS bigint)",&[&hash(state),&actor.to_string(),&now().to_string()]).await?.pop().ok_or(FamilyError::Forbidden)?;
        let scope = if r[2] == "channel" {
            ScopeRef::Channel(r[3].clone())
        } else {
            ScopeRef::Circle(r[3].clone())
        };
        self.admin(&mut tx, &actor.to_string(), &scope).await?;
        Ok(PendingAuthorization {
            app_id: r[0].clone(),
            binding_id: r[1].clone(),
            scope,
            resource_id: r[4].clone(),
            expires_at: r[5].parse().map_err(|_| FamilyError::Unavailable)?,
        })
    }
    pub async fn scope_bindings(&self, actor: &UserId, scope: &ScopeRef) -> Result<Vec<Binding>> {
        let mut tx = self.db.begin().await?;
        // Actual members may find the durable card without seeing anyone else's response.
        let column = match scope {
            ScopeRef::Channel(_) => "channel_id",
            ScopeRef::Circle(_) => "circle_id",
        };
        let table = match scope {
            ScopeRef::Channel(_) => "channel_memberships",
            ScopeRef::Circle(_) => "circle_memberships",
        };
        let query = format!("select 'ok' from {table} where {column}=$1::id and user_id=$2::id");
        if tx
            .rows(&query, &[scope.id(), &actor.to_string()])
            .await?
            .is_empty()
        {
            return Err(FamilyError::Forbidden);
        }
        let query = format!(
            "select id from app_bindings where {column}=$1::id and environment=$2 order by id limit 100"
        );
        let mut result = vec![];
        for r in tx.rows(&query, &[scope.id(), &self.environment]).await? {
            if let Ok(b) = self.binding(&mut tx, &r[0], false).await {
                result.push(b)
            }
        }
        Ok(result)
    }
}
