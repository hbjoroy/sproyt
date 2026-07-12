use std::{
    collections::{HashMap, VecDeque},
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::{ChannelId, CircleId, RepositoryError, UserId};

pub type AgentFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, RepositoryError>> + Send + 'a>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentScope {
    ReadHistory,
    SendMessages,
    StartProcesses,
    CompleteProcessWork,
}

impl AgentScope {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReadHistory => "read_history",
            Self::SendMessages => "send_messages",
            Self::StartProcesses => "start_processes",
            Self::CompleteProcessWork => "complete_process_work",
        }
    }
}

#[derive(Clone, Debug)]
pub struct CreateAgent {
    pub actor: UserId,
    pub owner_id: UserId,
    pub display_name: String,
    pub provider: String,
    pub service_identity: String,
    pub purpose: String,
    pub rate_limit_per_minute: u16,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CreatedAgent {
    pub agent_id: UserId,
    pub credential: String,
    pub credential_expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub struct GrantAgent {
    pub actor: UserId,
    pub agent_id: UserId,
    pub circle_id: Option<CircleId>,
    pub channel_id: Option<ChannelId>,
    pub scope: AgentScope,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug)]
pub struct AgentPrincipal {
    pub agent_id: UserId,
    pub owner_id: UserId,
    pub purpose: String,
    pub rate_limit_per_minute: u16,
}

pub trait AgentRepository: Send + Sync + 'static {
    fn create_agent<'a>(&'a self, command: CreateAgent) -> AgentFuture<'a, CreatedAgent>;
    fn grant_agent<'a>(&'a self, command: GrantAgent) -> AgentFuture<'a, Uuid>;
    fn revoke_grant<'a>(&'a self, actor: UserId, grant_id: Uuid) -> AgentFuture<'a, ()>;
    fn authenticate_agent<'a>(&'a self, credential: &'a str) -> AgentFuture<'a, AgentPrincipal>;
    fn has_scope<'a>(
        &'a self,
        agent_id: UserId,
        circle_id: Option<CircleId>,
        channel_id: Option<ChannelId>,
        scope: AgentScope,
    ) -> AgentFuture<'a, bool>;
}

pub type SharedAgentRepository = Arc<dyn AgentRepository>;

#[derive(Clone)]
pub struct AgentService {
    repository: SharedAgentRepository,
    requests: Arc<Mutex<HashMap<UserId, VecDeque<Instant>>>>,
}

impl AgentService {
    pub fn new(repository: SharedAgentRepository) -> Self {
        Self {
            repository,
            requests: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    pub async fn create(&self, command: CreateAgent) -> Result<CreatedAgent, RepositoryError> {
        self.repository.create_agent(command).await
    }
    pub async fn grant(&self, command: GrantAgent) -> Result<Uuid, RepositoryError> {
        self.repository.grant_agent(command).await
    }
    pub async fn revoke(&self, actor: UserId, id: Uuid) -> Result<(), RepositoryError> {
        self.repository.revoke_grant(actor, id).await
    }
    pub async fn authorize(
        &self,
        credential: &str,
        circle_id: Option<CircleId>,
        channel_id: Option<ChannelId>,
        scope: AgentScope,
    ) -> Result<AgentPrincipal, RepositoryError> {
        let principal = self.authenticate(credential).await?;
        if !self
            .repository
            .has_scope(principal.agent_id.clone(), circle_id, channel_id, scope)
            .await?
        {
            return Err(RepositoryError::PermissionDenied);
        }
        Ok(principal)
    }
    pub async fn authenticate(&self, credential: &str) -> Result<AgentPrincipal, RepositoryError> {
        let principal = self.repository.authenticate_agent(credential).await?;
        let mut requests = self
            .requests
            .lock()
            .map_err(|_| RepositoryError::Storage("agent rate limiter poisoned".into()))?;
        let now = Instant::now();
        let window = Duration::from_secs(60);
        let bucket = requests.entry(principal.agent_id.clone()).or_default();
        while bucket
            .front()
            .is_some_and(|at| now.duration_since(*at) >= window)
        {
            bucket.pop_front();
        }
        if bucket.len() >= usize::from(principal.rate_limit_per_minute) {
            return Err(RepositoryError::Conflict);
        }
        bucket.push_back(now);
        drop(requests);
        tracing::debug!(agent_id=%principal.agent_id, owner_id=%principal.owner_id, purpose_declared=!principal.purpose.is_empty(), "authorized agent request");
        Ok(principal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_have_stable_storage_names() {
        assert_eq!(AgentScope::ReadHistory.as_str(), "read_history");
        assert_eq!(
            AgentScope::CompleteProcessWork.as_str(),
            "complete_process_work"
        );
    }
}
