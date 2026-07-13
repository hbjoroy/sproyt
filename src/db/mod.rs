mod postgres;
mod sqlite;

#[allow(unused_imports)]
pub use postgres::PostgresChatRepository;
#[allow(unused_imports)]
pub use sqlite::SqliteChatRepository;

use std::sync::Arc;

use crate::agent::{AgentRepository, SharedAgentRepository};
use crate::config::{DatabaseConfig, DatabaseKind};
use crate::domain::{ChatRepository, RepositoryError};
use crate::process::{ProcessRepository, SharedProcessRepository};

pub struct Repositories {
    pub chat: Arc<dyn ChatRepository>,
    pub process: SharedProcessRepository,
    pub agent: SharedAgentRepository,
}

pub async fn migrate(config: &DatabaseConfig) -> Result<(), RepositoryError> {
    match config.kind() {
        DatabaseKind::Sqlite => {
            let repository = SqliteChatRepository::connect(config.url()).await?;
            repository.migrate().await
        }
        DatabaseKind::Postgres => {
            let repository = PostgresChatRepository::connect(config.url()).await?;
            repository.migrate().await
        }
    }
}

pub async fn connect_repositories(
    config: &DatabaseConfig,
) -> Result<Repositories, RepositoryError> {
    match config.kind() {
        DatabaseKind::Sqlite => {
            let repository = Arc::new(SqliteChatRepository::connect(config.url()).await?);
            let chat: Arc<dyn ChatRepository> = repository.clone();
            let process: Arc<dyn ProcessRepository> = repository.clone();
            let agent: Arc<dyn AgentRepository> = repository;
            Ok(Repositories {
                chat,
                process,
                agent,
            })
        }
        DatabaseKind::Postgres => {
            let repository = Arc::new(PostgresChatRepository::connect(config.url()).await?);
            let chat: Arc<dyn ChatRepository> = repository.clone();
            let process: Arc<dyn ProcessRepository> = repository.clone();
            let agent: Arc<dyn AgentRepository> = repository;
            Ok(Repositories {
                chat,
                process,
                agent,
            })
        }
    }
}

fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}

fn sql_error(error: sqlx::Error) -> RepositoryError {
    match &error {
        sqlx::Error::Database(database) if database.is_unique_violation() => {
            RepositoryError::Conflict
        }
        sqlx::Error::Database(database) if database.is_foreign_key_violation() => {
            RepositoryError::PermissionDenied
        }
        _ => storage(error),
    }
}

#[cfg(test)]
pub async fn verify_chat_repository_contract<R>(repository: &R, suffix: &str)
where
    R: ChatRepository,
{
    use crate::domain::{
        AcceptCircleInvitation, ChannelKind, ChannelSequence, ChannelSlug, CreateChannel,
        CreateCircle, CreateCircleInvitation, DisplayName, LoadRecentMessages, MarkRead,
        MessageBody, MessageLimit, PrincipalKind, SendMessage, User, UserId,
    };
    use chrono::Utc;

    repository.health_check().await.unwrap();
    let actor = UserId::named(format!("chat-contract-actor-{suffix}"));
    repository
        .upsert_user(User {
            id: actor.clone(),
            kind: PrincipalKind::Human,
            display_name: DisplayName::new("Chat contract actor").unwrap(),
            external_provider: None,
            external_subject: None,
            created_at: Utc::now(),
        })
        .await
        .unwrap();
    let channel = repository
        .create_channel(CreateChannel {
            actor: actor.clone(),
            slug: ChannelSlug::new(format!("chat-contract-{suffix}")).unwrap(),
            name: DisplayName::new("Chat contract channel").unwrap(),
            kind: ChannelKind::Private,
            circle_id: None,
        })
        .await
        .unwrap();
    let first = repository
        .append_message_idempotent(
            SendMessage {
                actor: actor.clone(),
                channel_id: channel.id.clone(),
                body: MessageBody::new("first").unwrap(),
            },
            "same-request".to_owned(),
        )
        .await
        .unwrap();
    let replay = repository
        .append_message_idempotent(
            SendMessage {
                actor: actor.clone(),
                channel_id: channel.id.clone(),
                body: MessageBody::new("different").unwrap(),
            },
            "same-request".to_owned(),
        )
        .await
        .unwrap();
    assert_eq!(first, replay);
    assert_eq!(
        repository
            .load_recent_messages(LoadRecentMessages {
                actor: actor.clone(),
                channel_id: channel.id.clone(),
                limit: MessageLimit::DEFAULT,
                after: None,
            })
            .await
            .unwrap(),
        vec![first.clone()]
    );
    assert_eq!(
        repository
            .mark_read(MarkRead {
                actor: actor.clone(),
                channel_id: channel.id.clone(),
                sequence: first.sequence,
            })
            .await
            .unwrap()
            .last_read_sequence,
        first.sequence
    );
    let second = repository
        .append_message_idempotent(
            SendMessage {
                actor: actor.clone(),
                channel_id: channel.id.clone(),
                body: MessageBody::new("second").unwrap(),
            },
            "second-request".to_owned(),
        )
        .await
        .unwrap();
    let third = repository
        .append_message_idempotent(
            SendMessage {
                actor: actor.clone(),
                channel_id: channel.id.clone(),
                body: MessageBody::new("third").unwrap(),
            },
            "third-request".to_owned(),
        )
        .await
        .unwrap();
    assert_eq!(
        repository
            .load_recent_messages(LoadRecentMessages {
                actor: actor.clone(),
                channel_id: channel.id.clone(),
                limit: MessageLimit::new(2),
                after: Some(ChannelSequence::new(0)),
            })
            .await
            .unwrap(),
        vec![first, second.clone()]
    );
    assert_eq!(
        repository
            .load_recent_messages(LoadRecentMessages {
                actor: actor.clone(),
                channel_id: channel.id,
                limit: MessageLimit::new(2),
                after: Some(second.sequence),
            })
            .await
            .unwrap(),
        vec![third]
    );

    let circle = repository
        .create_circle(CreateCircle {
            actor: actor.clone(),
            slug: ChannelSlug::new(format!("chat-circle-{suffix}")).unwrap(),
            name: DisplayName::new("Chat contract circle").unwrap(),
        })
        .await
        .unwrap();
    let invite = repository
        .create_circle_invitation(CreateCircleInvitation {
            actor: actor.clone(),
            circle_id: circle.id,
        })
        .await
        .unwrap();
    let member = UserId::named(format!("chat-contract-member-{suffix}"));
    repository
        .upsert_user(User {
            id: member.clone(),
            kind: PrincipalKind::Human,
            display_name: DisplayName::new("Chat contract member").unwrap(),
            external_provider: None,
            external_subject: None,
            created_at: Utc::now(),
        })
        .await
        .unwrap();
    repository
        .accept_circle_invitation(AcceptCircleInvitation {
            actor: member.clone(),
            token: invite.token.clone(),
        })
        .await
        .unwrap();
    assert_eq!(
        repository
            .accept_circle_invitation(AcceptCircleInvitation {
                actor: member,
                token: invite.token,
            })
            .await,
        Err(RepositoryError::NotFound)
    );
}

#[cfg(test)]
pub async fn verify_repository_contract<R>(repository: &R, suffix: &str)
where
    R: ChatRepository + ProcessRepository + AgentRepository,
{
    use crate::{
        agent::{AgentScope, CreateAgent, GrantAgent},
        domain::{
            AcceptCircleInvitation, ChannelKind, ChannelSlug, CreateChannel, CreateCircle,
            CreateCircleInvitation, DisplayName, LoadRecentMessages, MarkRead, MessageBody,
            MessageLimit, PrincipalKind, SendMessage, User, UserId,
        },
        process::{EnqueueProcessStart, StartedProcess},
    };
    use chrono::Utc;
    verify_chat_repository_contract(repository, &format!("{suffix}-shared-chat")).await;
    let actor = UserId::named(format!("contract-actor-{suffix}"));
    repository
        .upsert_user(User {
            id: actor.clone(),
            kind: PrincipalKind::Human,
            display_name: DisplayName::new("Contract actor").unwrap(),
            external_provider: None,
            external_subject: None,
            created_at: Utc::now(),
        })
        .await
        .unwrap();
    let channel = repository
        .create_channel(CreateChannel {
            actor: actor.clone(),
            slug: ChannelSlug::new(format!("contract-{suffix}")).unwrap(),
            name: DisplayName::new("Contract channel").unwrap(),
            kind: ChannelKind::Private,
            circle_id: None,
        })
        .await
        .unwrap();
    let first = repository
        .append_message_idempotent(
            SendMessage {
                actor: actor.clone(),
                channel_id: channel.id.clone(),
                body: MessageBody::new("first").unwrap(),
            },
            "same-request".to_owned(),
        )
        .await
        .unwrap();
    let replay = repository
        .append_message_idempotent(
            SendMessage {
                actor: actor.clone(),
                channel_id: channel.id.clone(),
                body: MessageBody::new("different").unwrap(),
            },
            "same-request".to_owned(),
        )
        .await
        .unwrap();
    assert_eq!(first, replay);
    let loaded = repository
        .load_recent_messages(LoadRecentMessages {
            actor: actor.clone(),
            channel_id: channel.id.clone(),
            limit: MessageLimit::DEFAULT,
            after: None,
        })
        .await
        .unwrap();
    assert_eq!(loaded, vec![first.clone()]);
    let membership = repository
        .mark_read(MarkRead {
            actor: actor.clone(),
            channel_id: channel.id.clone(),
            sequence: first.sequence,
        })
        .await
        .unwrap();
    assert_eq!(membership.last_read_sequence, first.sequence);
    let circle = repository
        .create_circle(CreateCircle {
            actor: actor.clone(),
            slug: ChannelSlug::new(format!("circle-{suffix}")).unwrap(),
            name: DisplayName::new("Contract circle").unwrap(),
        })
        .await
        .unwrap();
    let invite = repository
        .create_circle_invitation(CreateCircleInvitation {
            actor: actor.clone(),
            circle_id: circle.id.clone(),
        })
        .await
        .unwrap();
    let member = UserId::named(format!("contract-member-{suffix}"));
    repository
        .upsert_user(User {
            id: member.clone(),
            kind: PrincipalKind::Human,
            display_name: DisplayName::new("Contract member").unwrap(),
            external_provider: None,
            external_subject: None,
            created_at: Utc::now(),
        })
        .await
        .unwrap();
    repository
        .accept_circle_invitation(AcceptCircleInvitation {
            actor: member.clone(),
            token: invite.token.clone(),
        })
        .await
        .unwrap();
    assert_eq!(
        repository
            .accept_circle_invitation(AcceptCircleInvitation {
                actor: member,
                token: invite.token
            })
            .await,
        Err(RepositoryError::NotFound)
    );

    let created_agent = repository
        .create_agent(CreateAgent {
            actor: actor.clone(),
            owner_id: actor.clone(),
            display_name: "Contract agent".to_owned(),
            provider: "contract".to_owned(),
            service_identity: format!("agent-{suffix}"),
            purpose: "Repository conformance".to_owned(),
            rate_limit_per_minute: 10,
            expires_at: None,
        })
        .await
        .unwrap();
    assert!(repository.authenticate_agent("invalid").await.is_err());
    assert_eq!(
        repository
            .authenticate_agent(&created_agent.credential)
            .await
            .unwrap()
            .agent_id,
        created_agent.agent_id
    );
    let grant = repository
        .grant_agent(GrantAgent {
            actor: actor.clone(),
            agent_id: created_agent.agent_id.clone(),
            circle_id: None,
            channel_id: Some(channel.id.clone()),
            scope: AgentScope::ReadHistory,
            expires_at: None,
        })
        .await
        .unwrap();
    assert!(
        repository
            .has_scope(
                created_agent.agent_id.clone(),
                None,
                Some(channel.id.clone()),
                AgentScope::ReadHistory
            )
            .await
            .unwrap()
    );
    assert_eq!(
        repository
            .enqueue_start(EnqueueProcessStart {
                channel_id: channel.id.clone(),
                actor: created_agent.agent_id.clone(),
                request_id: "observer-must-not-start".to_owned(),
                namespace: "contract".to_owned(),
                definition_name: "contract".to_owned(),
                definition_version: None,
                metadata: serde_json::json!({}),
            })
            .await,
        Err(RepositoryError::PermissionDenied)
    );
    repository.revoke_grant(actor.clone(), grant).await.unwrap();
    assert!(
        !repository
            .has_scope(
                created_agent.agent_id,
                None,
                Some(channel.id.clone()),
                AgentScope::ReadHistory
            )
            .await
            .unwrap()
    );

    let link = repository
        .enqueue_start(EnqueueProcessStart {
            channel_id: channel.id,
            actor,
            request_id: "process-contract".to_owned(),
            namespace: "contract".to_owned(),
            definition_name: "contract".to_owned(),
            definition_version: None,
            metadata: serde_json::json!({}),
        })
        .await
        .unwrap();
    let job = repository
        .lease_next(std::time::Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(job.process_link_id, link.id);
    repository
        .complete_start(
            job,
            StartedProcess {
                instance_id: uuid::Uuid::now_v7(),
            },
        )
        .await
        .unwrap();
}
