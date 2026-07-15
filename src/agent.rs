use std::{future::Future, pin::Pin, sync::Arc};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::{ChannelId, CircleId, MessageId, RepositoryError, UserId};

pub type AgentFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, RepositoryError>> + Send + 'a>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentScope {
    ReadHistory,
    SendMessages,
    StartProcesses,
    CompleteProcessWork,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityProvenance {
    Human,
    Generated,
    Delegated,
    HumanApproved,
}
impl ActivityProvenance {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "human" => Some(Self::Human),
            "generated" => Some(Self::Generated),
            "delegated" => Some(Self::Delegated),
            "human_approved" => Some(Self::HumanApproved),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct MessageProvenance {
    pub message_id: MessageId,
    pub provenance: ActivityProvenance,
    pub agent_id: Option<UserId>,
    pub owner_id: Option<UserId>,
    pub approved_by: Option<UserId>,
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
    fn consume_rate_limit<'a>(
        &'a self,
        agent_id: UserId,
        limit_per_minute: u16,
    ) -> AgentFuture<'a, ()>;
    fn has_scope<'a>(
        &'a self,
        agent_id: UserId,
        circle_id: Option<CircleId>,
        channel_id: Option<ChannelId>,
        scope: AgentScope,
    ) -> AgentFuture<'a, bool>;
    fn mark_delegated<'a>(&'a self, agent_id: UserId, message_id: MessageId)
    -> AgentFuture<'a, ()>;
    fn approve_message<'a>(&'a self, actor: UserId, message_id: MessageId) -> AgentFuture<'a, ()>;
    fn message_provenance<'a>(
        &'a self,
        message_id: MessageId,
    ) -> AgentFuture<'a, MessageProvenance>;
}

pub type SharedAgentRepository = Arc<dyn AgentRepository>;

#[derive(Clone)]
pub struct AgentService {
    repository: SharedAgentRepository,
}

impl AgentService {
    pub fn new(repository: SharedAgentRepository) -> Self {
        Self { repository }
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
    pub async fn require_scope(
        &self,
        principal: &AgentPrincipal,
        circle_id: Option<CircleId>,
        channel_id: Option<ChannelId>,
        scope: AgentScope,
    ) -> Result<(), RepositoryError> {
        if self
            .repository
            .has_scope(principal.agent_id.clone(), circle_id, channel_id, scope)
            .await?
        {
            Ok(())
        } else {
            Err(RepositoryError::PermissionDenied)
        }
    }

    pub async fn has_any_scope(
        &self,
        principal: &AgentPrincipal,
        circle_id: Option<CircleId>,
        channel_id: Option<ChannelId>,
    ) -> Result<bool, RepositoryError> {
        for scope in [
            AgentScope::ReadHistory,
            AgentScope::SendMessages,
            AgentScope::StartProcesses,
            AgentScope::CompleteProcessWork,
        ] {
            if self
                .repository
                .has_scope(
                    principal.agent_id.clone(),
                    circle_id.clone(),
                    channel_id.clone(),
                    scope,
                )
                .await?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub async fn authenticate(&self, credential: &str) -> Result<AgentPrincipal, RepositoryError> {
        let principal = self.repository.authenticate_agent(credential).await?;
        self.repository
            .consume_rate_limit(principal.agent_id.clone(), principal.rate_limit_per_minute)
            .await?;
        tracing::debug!(agent_id=%principal.agent_id, owner_id=%principal.owner_id, purpose_declared=!principal.purpose.is_empty(), "authorized agent request");
        Ok(principal)
    }
    pub async fn mark_delegated(
        &self,
        agent_id: UserId,
        message_id: MessageId,
    ) -> Result<(), RepositoryError> {
        self.repository.mark_delegated(agent_id, message_id).await
    }
    pub async fn approve_message(
        &self,
        actor: UserId,
        message_id: MessageId,
    ) -> Result<(), RepositoryError> {
        self.repository.approve_message(actor, message_id).await
    }
    pub async fn message_provenance(
        &self,
        message_id: MessageId,
    ) -> Result<MessageProvenance, RepositoryError> {
        self.repository.message_provenance(message_id).await
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
