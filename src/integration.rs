use std::{future::Future, pin::Pin, sync::Arc};

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::domain::{ChatMessage, MessageBody, RepositoryError, UserId};

pub const GRAFANA_PROVIDER: &str = "grafana-webhook";

pub type IntegrationFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, RepositoryError>> + Send + 'a>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlertState {
    Firing,
    Resolved,
}

impl AlertState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Firing => "firing",
            Self::Resolved => "resolved",
        }
    }
}

#[derive(Clone, Debug)]
pub struct IncomingAlert {
    pub agent_id: UserId,
    pub credential_id: Uuid,
    pub fingerprint: String,
    pub starts_at: DateTime<Utc>,
    pub state: AlertState,
    pub body: MessageBody,
}

#[derive(Clone, Debug)]
pub struct IncomingReport {
    pub agent_id: UserId,
    pub credential_id: Uuid,
    pub report_id: String,
    pub body: MessageBody,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeliveryResult {
    Accepted(ChatMessage),
    Duplicate(ChatMessage),
    IgnoredResolved,
}

pub trait IntegrationRepository: Send + Sync + 'static {
    fn deliver_alert<'a>(&'a self, alert: IncomingAlert) -> IntegrationFuture<'a, DeliveryResult>;
    fn deliver_report<'a>(
        &'a self,
        report: IncomingReport,
    ) -> IntegrationFuture<'a, DeliveryResult>;
}

pub type SharedIntegrationRepository = Arc<dyn IntegrationRepository>;

#[derive(Clone)]
pub struct IntegrationService {
    repository: SharedIntegrationRepository,
}

impl IntegrationService {
    pub fn new(repository: SharedIntegrationRepository) -> Self {
        Self { repository }
    }

    pub async fn deliver_alert(
        &self,
        alert: IncomingAlert,
    ) -> Result<DeliveryResult, RepositoryError> {
        self.repository.deliver_alert(alert).await
    }

    pub async fn deliver_report(
        &self,
        report: IncomingReport,
    ) -> Result<DeliveryResult, RepositoryError> {
        self.repository.deliver_report(report).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        agent::{AgentScope, AgentService, CreateAgent, GrantAgent},
        chat::ChatEngine,
        db::SqliteChatRepository,
        domain::{ChannelKind, ChannelSlug, DisplayName, PrincipalKind, User},
    };

    #[tokio::test]
    async fn delivery_is_idempotent_and_resolved_never_reopens() {
        let repository = Arc::new(
            SqliteChatRepository::connect("sqlite::memory:")
                .await
                .unwrap(),
        );
        repository.migrate().await.unwrap();
        let chat = ChatEngine::start(repository.clone());
        let agents = AgentService::new(repository.clone());
        let integrations = IntegrationService::new(repository);
        let owner = UserId::named("integration-owner");
        chat.ensure_user(User {
            id: owner.clone(),
            kind: PrincipalKind::Human,
            display_name: DisplayName::new("Integration owner").unwrap(),
            handle: Some(crate::domain::Handle::new("integration-owner").unwrap()),
            external_provider: None,
            external_subject: None,
            created_at: Utc::now(),
        })
        .await
        .unwrap();
        let circle = chat
            .create_circle(
                owner.clone(),
                ChannelSlug::new("integration-circle").unwrap(),
                DisplayName::new("Integration circle").unwrap(),
            )
            .await
            .unwrap();
        let channel = chat
            .create_channel(
                owner.clone(),
                ChannelSlug::new("alerts").unwrap(),
                DisplayName::new("Alerts").unwrap(),
                ChannelKind::Private,
                Some(circle.id),
            )
            .await
            .unwrap();
        let created = agents
            .create(CreateAgent {
                actor: owner.clone(),
                owner_id: owner.clone(),
                display_name: "Grafana".to_owned(),
                provider: GRAFANA_PROVIDER.to_owned(),
                service_identity: "integration-test".to_owned(),
                purpose: "test".to_owned(),
                rate_limit_per_minute: 60,
                expires_at: None,
            })
            .await
            .unwrap();
        agents
            .grant(GrantAgent {
                actor: owner.clone(),
                agent_id: created.agent_id.clone(),
                circle_id: None,
                channel_id: Some(channel.id.clone()),
                scope: AgentScope::SendMessages,
                expires_at: None,
            })
            .await
            .unwrap();
        let principal = agents.authenticate(&created.credential).await.unwrap();
        let starts_at = Utc::now();
        let command = |state, body: &str| IncomingAlert {
            agent_id: principal.agent_id.clone(),
            credential_id: principal.credential_id,
            fingerprint: "same-alert".to_owned(),
            starts_at,
            state,
            body: MessageBody::new(body).unwrap(),
        };

        let transition_start = starts_at - chrono::Duration::minutes(5);
        for state in [AlertState::Firing, AlertState::Resolved] {
            let transition = integrations
                .deliver_alert(IncomingAlert {
                    agent_id: principal.agent_id.clone(),
                    credential_id: principal.credential_id,
                    fingerprint: "normal-transition".to_owned(),
                    starts_at: transition_start,
                    state,
                    body: MessageBody::new(state.as_str()).unwrap(),
                })
                .await
                .unwrap();
            assert!(matches!(transition, DeliveryResult::Accepted(_)));
        }

        let resolved = integrations
            .deliver_alert(command(AlertState::Resolved, "resolved"))
            .await
            .unwrap();
        assert!(matches!(resolved, DeliveryResult::Accepted(_)));
        let replay = integrations
            .deliver_alert(command(AlertState::Resolved, "ignored body change"))
            .await
            .unwrap();
        assert!(matches!(replay, DeliveryResult::Duplicate(_)));
        let late_firing = integrations
            .deliver_alert(command(AlertState::Firing, "late firing"))
            .await
            .unwrap();
        assert_eq!(late_firing, DeliveryResult::IgnoredResolved);

        let next = integrations
            .deliver_alert(IncomingAlert {
                starts_at: starts_at + chrono::Duration::minutes(5),
                ..command(AlertState::Firing, "new occurrence")
            })
            .await
            .unwrap();
        assert!(matches!(next, DeliveryResult::Accepted(_)));

        let rotated = agents
            .rotate_credential(owner, principal.agent_id.clone())
            .await
            .unwrap();
        assert!(agents.authenticate(&created.credential).await.is_err());
        assert_eq!(
            integrations
                .deliver_alert(IncomingAlert {
                    starts_at: starts_at + chrono::Duration::minutes(10),
                    ..command(AlertState::Firing, "stale credential")
                })
                .await,
            Err(RepositoryError::PermissionDenied)
        );
        let rotated_principal = agents.authenticate(&rotated.credential).await.unwrap();
        agents
            .grant(GrantAgent {
                actor: UserId::named("integration-owner"),
                agent_id: rotated_principal.agent_id.clone(),
                circle_id: None,
                channel_id: Some(channel.id),
                scope: AgentScope::ReadHistory,
                expires_at: None,
            })
            .await
            .unwrap();
        assert_eq!(
            integrations
                .deliver_report(IncomingReport {
                    agent_id: rotated_principal.agent_id,
                    credential_id: rotated_principal.credential_id,
                    report_id: "broader-grant".to_owned(),
                    body: MessageBody::new("must fail closed").unwrap(),
                })
                .await,
            Err(RepositoryError::PermissionDenied)
        );
    }
}
