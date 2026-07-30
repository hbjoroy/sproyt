mod postgres;
mod sqlite;

#[allow(unused_imports)]
pub use postgres::PostgresChatRepository;
#[allow(unused_imports)]
pub use sqlite::SqliteChatRepository;

use std::sync::Arc;

use crate::agent::{AgentRepository, SharedAgentRepository};
use crate::config::{DatabaseConfig, DatabaseKind};
use crate::domain::{ChatRepository, MediaId, MessageBody, RepositoryError};
use crate::process::{ProcessRepository, SharedProcessRepository};

pub struct Repositories {
    pub chat: Arc<dyn ChatRepository>,
    pub process: SharedProcessRepository,
    pub agent: SharedAgentRepository,
}

fn media_ids_from_body(body: &MessageBody) -> Result<Vec<MediaId>, RepositoryError> {
    let mut ids = Vec::new();
    let mut rest = body.as_str();
    while let Some(start) = rest.find("[[media:") {
        rest = &rest[start + 8..];
        let end = rest.find('|').ok_or(RepositoryError::Conflict)?;
        let id = MediaId::new(&rest[..end])
            .map_err(|error| RepositoryError::Storage(error.to_string()))?;
        if ids.contains(&id) {
            return Err(RepositoryError::Conflict);
        }
        ids.push(id);
        rest = &rest[end + 1..];
    }
    if ids.len() > 10 {
        return Err(RepositoryError::Conflict);
    }
    Ok(ids)
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
        AcceptCircleInvitation, AddChannelMember, ChannelKind, ChannelRef, ChannelSequence,
        ChannelSlug, CreateChannel, CreateCircle, CreateCircleInvitation, DeleteCircle,
        DeleteMessage, DisplayName, EditMessage, JoinChannel, LeaveChannel, LoadRecentMessages,
        MarkRead, MessageBody, MessageLimit, PORTABLE_USER_EXPORT_FORMAT, PrincipalKind,
        SendMessage, User, UserId,
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
            kind: ChannelKind::Local,
            circle_id: None,
        })
        .await
        .unwrap();
    let general_id = match repository
        .create_channel(CreateChannel {
            actor: actor.clone(),
            slug: ChannelSlug::new("general").unwrap(),
            name: DisplayName::new("General").unwrap(),
            kind: ChannelKind::Public,
            circle_id: None,
        })
        .await
    {
        Ok(channel) => channel.id,
        Err(RepositoryError::Conflict) => {
            repository
                .list_channels_for_user(actor.clone())
                .await
                .unwrap()
                .into_iter()
                .find(|channel| channel.slug.as_str() == "general")
                .expect("bootstrapped general must be visible to authenticated users")
                .id
        }
        Err(error) => panic!("could not create or load general: {error}"),
    };
    let general_member = UserId::named(format!("chat-general-member-{suffix}"));
    repository
        .upsert_user(User {
            id: general_member.clone(),
            kind: PrincipalKind::Human,
            display_name: DisplayName::new("General member").unwrap(),
            external_provider: None,
            external_subject: None,
            created_at: Utc::now(),
        })
        .await
        .unwrap();
    assert!(
        repository
            .list_channels_for_user(general_member)
            .await
            .unwrap()
            .iter()
            .any(|summary| summary.id == general_id),
        "new authenticated users must inherit general"
    );
    let mut first = repository
        .append_message_idempotent(
            SendMessage {
                actor: actor.clone(),
                channel_id: channel.id.clone(),
                parent_message_id: None,
                body: MessageBody::new("first").unwrap(),
            },
            "same-request".to_owned(),
        )
        .await
        .unwrap();
    assert_eq!(first.sender_display_name.as_str(), "Chat contract actor");
    let added_reaction = repository
        .toggle_message_reaction(actor.clone(), first.id, "👍".to_owned())
        .await
        .unwrap();
    assert!(added_reaction.added);
    assert_eq!(added_reaction.count, 1);
    assert_eq!(
        repository
            .list_channel_reactions(actor.clone(), channel.id.clone())
            .await
            .unwrap(),
        vec![crate::domain::MessageReactionSummary {
            message_id: first.id,
            emoji: "👍".to_owned(),
            count: 1,
            reacted_by_me: true,
            user_ids: vec![actor.clone()],
        }]
    );
    let removed_reaction = repository
        .toggle_message_reaction(actor.clone(), first.id, "👍".to_owned())
        .await
        .unwrap();
    assert!(!removed_reaction.added);
    assert_eq!(removed_reaction.count, 0);
    assert!(
        repository
            .list_channel_reactions(actor.clone(), channel.id.clone())
            .await
            .unwrap()
            .is_empty()
    );
    repository
        .upsert_user(User {
            id: actor.clone(),
            kind: PrincipalKind::Human,
            display_name: DisplayName::new("Renamed chat contract actor").unwrap(),
            external_provider: None,
            external_subject: None,
            created_at: Utc::now(),
        })
        .await
        .unwrap();
    let replay = repository
        .append_message_idempotent(
            SendMessage {
                actor: actor.clone(),
                channel_id: channel.id.clone(),
                parent_message_id: None,
                body: MessageBody::new("first").unwrap(),
            },
            "same-request".to_owned(),
        )
        .await
        .unwrap();
    assert_eq!(first, replay);
    let mismatched_replay = repository
        .append_message_idempotent(
            SendMessage {
                actor: actor.clone(),
                channel_id: channel.id.clone(),
                parent_message_id: None,
                body: MessageBody::new("different").unwrap(),
            },
            "same-request".to_owned(),
        )
        .await;
    assert_eq!(mismatched_replay, Err(RepositoryError::Conflict));
    assert_eq!(
        repository
            .edit_message(EditMessage {
                actor: UserId::named(format!("{suffix}-other")),
                message_id: first.id,
                body: MessageBody::new("not allowed").unwrap(),
            })
            .await,
        Err(RepositoryError::PermissionDenied)
    );
    let original_sequence = first.sequence;
    first = repository
        .edit_message(EditMessage {
            actor: actor.clone(),
            message_id: first.id,
            body: MessageBody::new("first, edited").unwrap(),
        })
        .await
        .unwrap();
    assert_eq!(first.body.as_str(), "first, edited");
    assert_eq!(first.sequence, original_sequence);
    assert!(first.edited_at.is_some());
    assert_eq!(
        repository
            .load_recent_messages(LoadRecentMessages {
                actor: actor.clone(),
                channel_id: channel.id.clone(),
                limit: MessageLimit::DEFAULT,
                after: None,
                before: None,
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
                parent_message_id: None,
                body: MessageBody::new("second").unwrap(),
            },
            "second-request".to_owned(),
        )
        .await
        .unwrap();
    assert_eq!(
        second.sender_display_name.as_str(),
        "Renamed chat contract actor"
    );
    let third = repository
        .append_message_idempotent(
            SendMessage {
                actor: actor.clone(),
                channel_id: channel.id.clone(),
                parent_message_id: None,
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
                before: None,
            })
            .await
            .unwrap(),
        vec![first.clone(), second.clone()]
    );
    assert_eq!(
        repository
            .load_recent_messages(LoadRecentMessages {
                actor: actor.clone(),
                channel_id: channel.id.clone(),
                limit: MessageLimit::new(2),
                after: Some(second.sequence),
                before: None,
            })
            .await
            .unwrap(),
        vec![third]
    );
    assert_eq!(
        repository
            .load_recent_messages(LoadRecentMessages {
                actor: actor.clone(),
                channel_id: channel.id.clone(),
                limit: MessageLimit::new(2),
                after: None,
                before: Some(ChannelSequence::new(3)),
            })
            .await
            .unwrap(),
        vec![first.clone(), second.clone()]
    );
    assert_eq!(
        repository
            .load_recent_messages(LoadRecentMessages {
                actor: actor.clone(),
                channel_id: channel.id.clone(),
                limit: MessageLimit::new(2),
                after: Some(ChannelSequence::new(0)),
                before: Some(ChannelSequence::new(3)),
            })
            .await,
        Err(RepositoryError::Conflict)
    );
    let summary = repository
        .list_channels_for_user(actor.clone())
        .await
        .unwrap()
        .into_iter()
        .find(|summary| summary.id == channel.id)
        .unwrap();
    assert_eq!(summary.last_read_sequence, ChannelSequence::new(1));
    assert_eq!(summary.latest_sequence, ChannelSequence::new(3));

    assert_eq!(
        repository
            .delete_message(DeleteMessage {
                actor: UserId::named(format!("{suffix}-other")),
                message_id: first.id,
            })
            .await,
        Err(RepositoryError::PermissionDenied)
    );
    let deleted = repository
        .delete_message(DeleteMessage {
            actor: actor.clone(),
            message_id: first.id,
        })
        .await
        .unwrap();
    assert_eq!(deleted.id, first.id);
    assert_eq!(deleted.sequence, first.sequence);
    assert_eq!(deleted.body.as_str(), "Meldinga er sletta.");
    assert!(deleted.edited_at.is_none());
    assert!(deleted.deleted_at.is_some());
    assert!(matches!(
        repository
            .edit_message(EditMessage {
                actor: actor.clone(),
                message_id: first.id,
                body: MessageBody::new("kan ikkje hentast tilbake").unwrap(),
            })
            .await,
        Err(RepositoryError::PermissionDenied) | Err(RepositoryError::Conflict)
    ));
    assert_eq!(repository.load_message(first.id).await.unwrap(), deleted);

    let reply = repository
        .append_message_idempotent(
            SendMessage {
                actor: actor.clone(),
                channel_id: channel.id.clone(),
                parent_message_id: Some(second.id),
                body: MessageBody::new("svar i tråden").unwrap(),
            },
            "thread-reply-request".to_owned(),
        )
        .await
        .unwrap();
    assert_eq!(reply.parent_message_id, Some(second.id));
    assert_eq!(reply.channel_id, second.channel_id);
    assert!(matches!(
        repository
            .append_message(SendMessage {
                actor: actor.clone(),
                channel_id: channel.id.clone(),
                parent_message_id: Some(reply.id),
                body: MessageBody::new("nested reply").unwrap(),
            })
            .await,
        Err(RepositoryError::Conflict)
    ));
    assert_eq!(repository.load_message(reply.id).await.unwrap(), reply);

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
            circle_id: circle.id.clone(),
        })
        .await
        .unwrap();
    let circle_channel = repository
        .create_channel(CreateChannel {
            actor: actor.clone(),
            slug: ChannelSlug::new(format!("chat-circle-channel-{suffix}")).unwrap(),
            name: DisplayName::new("Circle channel").unwrap(),
            kind: ChannelKind::Local,
            circle_id: Some(circle.id.clone()),
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
    assert_eq!(
        repository
            .join_channel(JoinChannel {
                actor: member.clone(),
                channel: ChannelRef::Id(circle_channel.id.clone()),
            })
            .await,
        Err(RepositoryError::PermissionDenied)
    );
    repository
        .accept_circle_invitation(AcceptCircleInvitation {
            actor: member.clone(),
            token: invite.token.clone(),
        })
        .await
        .unwrap();
    let circle_profiles = repository
        .list_circle_user_profiles(actor.clone(), circle.id.clone())
        .await
        .unwrap();
    assert!(
        circle_profiles
            .iter()
            .any(|profile| profile.user.id == actor)
    );
    assert!(
        circle_profiles
            .iter()
            .any(|profile| profile.user.id == member)
    );
    let outsider = UserId::named(format!("chat-contract-outsider-{suffix}"));
    repository
        .upsert_user(User {
            id: outsider.clone(),
            kind: PrincipalKind::Human,
            display_name: DisplayName::new("Chat contract outsider").unwrap(),
            external_provider: None,
            external_subject: None,
            created_at: Utc::now(),
        })
        .await
        .unwrap();
    assert_eq!(
        repository
            .list_circle_user_profiles(outsider, circle.id.clone())
            .await,
        Err(RepositoryError::PermissionDenied)
    );
    assert!(
        !repository
            .list_channels_for_user(member.clone())
            .await
            .unwrap()
            .iter()
            .any(|summary| summary.id == circle_channel.id)
    );
    assert!(
        repository
            .list_joinable_channels(member.clone(), circle.id.clone())
            .await
            .unwrap()
            .iter()
            .any(|channel| channel.id == circle_channel.id)
    );
    repository
        .join_channel(JoinChannel {
            actor: member.clone(),
            channel: ChannelRef::Id(circle_channel.id.clone()),
        })
        .await
        .unwrap();
    let later_circle_channel = repository
        .create_channel(CreateChannel {
            actor: actor.clone(),
            slug: ChannelSlug::new(format!("chat-circle-later-{suffix}")).unwrap(),
            name: DisplayName::new("Later circle channel").unwrap(),
            kind: ChannelKind::Private,
            circle_id: Some(circle.id.clone()),
        })
        .await
        .unwrap();
    assert!(
        !repository
            .list_channels_for_user(member.clone())
            .await
            .unwrap()
            .iter()
            .any(|summary| summary.id == later_circle_channel.id)
    );
    assert_eq!(
        repository
            .join_channel(JoinChannel {
                actor: member.clone(),
                channel: ChannelRef::Id(later_circle_channel.id.clone())
            })
            .await,
        Err(RepositoryError::PermissionDenied)
    );
    repository
        .add_channel_member(AddChannelMember {
            actor: actor.clone(),
            channel_id: later_circle_channel.id.clone(),
            user_id: member.clone(),
        })
        .await
        .unwrap();
    for sequence in 1..=205 {
        repository
            .append_message(SendMessage {
                actor: actor.clone(),
                channel_id: circle_channel.id.clone(),
                parent_message_id: None,
                body: MessageBody::new(format!("portable message {sequence}")).unwrap(),
            })
            .await
            .unwrap();
    }
    let export = repository.export_user_data(member.clone()).await.unwrap();
    assert_eq!(export.format, PORTABLE_USER_EXPORT_FORMAT);
    assert_eq!(export.user.id, member);
    assert_eq!(export.circles.len(), 1);
    assert_eq!(export.circles[0].circle.id, circle.id);
    assert!(
        export
            .channels
            .iter()
            .any(|channel| channel.channel.id == general_id),
        "general must be included in the member export"
    );
    let exported_circle_channel = export
        .channels
        .iter()
        .find(|channel| channel.channel.id == circle_channel.id)
        .expect("accepted circle channel must be exported");
    assert_eq!(
        exported_circle_channel.messages.len(),
        205,
        "portable export must not apply the interactive history limit"
    );
    assert_eq!(
        repository
            .accept_circle_invitation(AcceptCircleInvitation {
                actor: member.clone(),
                token: invite.token,
            })
            .await,
        Err(RepositoryError::NotFound)
    );
    repository
        .leave_channel(LeaveChannel {
            actor: member.clone(),
            channel_id: circle_channel.id.clone(),
        })
        .await
        .unwrap();
    assert_eq!(
        repository
            .load_recent_messages(LoadRecentMessages {
                actor: member.clone(),
                channel_id: circle_channel.id.clone(),
                limit: MessageLimit::DEFAULT,
                after: None,
                before: None,
            })
            .await,
        Err(RepositoryError::PermissionDenied)
    );
    assert_eq!(
        repository
            .delete_circle(DeleteCircle {
                actor: member.clone(),
                circle_id: circle.id.clone(),
            })
            .await,
        Err(RepositoryError::PermissionDenied)
    );
    repository
        .delete_circle(DeleteCircle {
            actor: actor.clone(),
            circle_id: circle.id.clone(),
        })
        .await
        .unwrap();
    assert!(
        repository
            .list_circles_for_user(actor.clone())
            .await
            .unwrap()
            .into_iter()
            .all(|(listed, _)| listed.id != circle.id)
    );
    assert!(
        repository
            .list_circles_for_user(member.clone())
            .await
            .unwrap()
            .into_iter()
            .all(|(listed, _)| listed.id != circle.id)
    );
    assert_eq!(
        repository
            .join_channel(JoinChannel {
                actor,
                channel: ChannelRef::Id(circle_channel.id),
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
            AcceptCircleInvitation, AddChannelMember, ChannelKind, ChannelSlug, CreateChannel,
            CreateCircle, CreateCircleInvitation, DisplayName, LoadRecentMessages, MarkRead,
            MessageBody, MessageLimit, PrincipalKind, SendMessage, User, UserId,
        },
        process::{
            EnqueueCorrelation, EnqueueInspection, EnqueueProcessStart, ProcessError,
            ProcessErrorKind, SetCircleFeature, StartedProcess,
        },
    };
    use chrono::{Duration, Utc};
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
                parent_message_id: None,
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
                parent_message_id: None,
                body: MessageBody::new("first").unwrap(),
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
            before: None,
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
    let process_channel = repository
        .create_channel(CreateChannel {
            actor: actor.clone(),
            slug: ChannelSlug::new(format!("process-{suffix}")).unwrap(),
            name: DisplayName::new("Contract process channel").unwrap(),
            kind: ChannelKind::Private,
            circle_id: Some(circle.id.clone()),
        })
        .await
        .unwrap();
    let process_start = EnqueueProcessStart {
        channel_id: process_channel.id.clone(),
        actor: actor.clone(),
        request_id: "process-contract".to_owned(),
        namespace: "contract".to_owned(),
        definition_name: "contract".to_owned(),
        definition_version: None,
        metadata: serde_json::json!({}),
    };
    assert_eq!(
        repository.enqueue_start(process_start.clone()).await,
        Err(RepositoryError::PermissionDenied),
        "Heart process starts must be gated per circle"
    );
    repository
        .set_circle_feature(SetCircleFeature {
            circle_id: circle.id.clone(),
            actor: actor.clone(),
            feature: "heart.event-planning".to_owned(),
            enabled: true,
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
                actor: member.clone(),
                token: invite.token
            })
            .await,
        Err(RepositoryError::NotFound)
    );
    repository
        .add_channel_member(AddChannelMember {
            actor: actor.clone(),
            channel_id: process_channel.id.clone(),
            user_id: member.clone(),
        })
        .await
        .unwrap();

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
    repository
        .consume_rate_limit(created_agent.agent_id.clone(), 1)
        .await
        .unwrap();
    assert_eq!(
        repository
            .consume_rate_limit(created_agent.agent_id.clone(), 1)
            .await,
        Err(RepositoryError::Conflict),
        "agent rate limit was not shared by the repository"
    );
    let expired_agent = repository
        .create_agent(CreateAgent {
            actor: actor.clone(),
            owner_id: actor.clone(),
            display_name: "Expired contract agent".to_owned(),
            provider: "contract".to_owned(),
            service_identity: format!("expired-agent-{suffix}"),
            purpose: "Expiry conformance".to_owned(),
            rate_limit_per_minute: 10,
            expires_at: Some(Utc::now() - Duration::seconds(1)),
        })
        .await
        .unwrap();
    assert!(matches!(
        repository
            .authenticate_agent(&expired_agent.credential)
            .await,
        Err(RepositoryError::PermissionDenied)
    ));
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
    repository
        .grant_agent(GrantAgent {
            actor: actor.clone(),
            agent_id: created_agent.agent_id.clone(),
            circle_id: None,
            channel_id: Some(channel.id.clone()),
            scope: AgentScope::StartProcesses,
            expires_at: Some(Utc::now() - Duration::seconds(1)),
        })
        .await
        .unwrap();
    assert!(
        !repository
            .has_scope(
                created_agent.agent_id.clone(),
                None,
                Some(channel.id.clone()),
                AgentScope::StartProcesses,
            )
            .await
            .unwrap()
    );
    repository.revoke_grant(actor.clone(), grant).await.unwrap();
    assert!(
        !repository
            .has_scope(
                created_agent.agent_id.clone(),
                None,
                Some(channel.id.clone()),
                AgentScope::ReadHistory
            )
            .await
            .unwrap()
    );
    let reactivated_grant = repository
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
    assert_eq!(reactivated_grant, grant, "regrant returned a phantom id");
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
    repository
        .revoke_grant(actor.clone(), reactivated_grant)
        .await
        .unwrap();
    repository
        .revoke_agent(actor.clone(), created_agent.agent_id.clone())
        .await
        .unwrap();
    assert!(matches!(
        repository
            .authenticate_agent(&created_agent.credential)
            .await,
        Err(RepositoryError::PermissionDenied)
    ));
    assert!(
        !repository
            .has_scope(
                created_agent.agent_id.clone(),
                None,
                Some(channel.id.clone()),
                AgentScope::ReadHistory
            )
            .await
            .unwrap()
    );
    assert!(matches!(
        repository
            .revoke_agent(UserId::named("not-the-owner"), created_agent.agent_id)
            .await,
        Err(RepositoryError::PermissionDenied)
    ));

    let link = repository
        .enqueue_start(process_start.clone())
        .await
        .unwrap();
    let replay = repository.enqueue_start(process_start).await.unwrap();
    assert_eq!(
        replay.id, link.id,
        "process start replay created a new link"
    );
    let abandoned_job = repository
        .lease_next(std::time::Duration::ZERO)
        .await
        .unwrap()
        .unwrap();
    let job = repository
        .lease_next(std::time::Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(job.id, abandoned_job.id, "expired lease was not recovered");
    assert_eq!(job.process_link_id, link.id);
    let completed_job = job.clone();
    let instance_id = uuid::Uuid::now_v7();
    repository
        .complete_start(job, StartedProcess { instance_id })
        .await
        .unwrap();
    repository
        .complete_start(completed_job, StartedProcess { instance_id })
        .await
        .unwrap();
    let response = EnqueueCorrelation {
        process_link_id: link.id,
        actor: member.clone(),
        request_id: "process-response-contract".to_owned(),
        payload: serde_json::json!({"answer":"yes"}),
    };
    let response_job_id = repository
        .enqueue_correlation(response.clone())
        .await
        .unwrap();
    assert_eq!(
        repository.enqueue_correlation(response).await.unwrap(),
        response_job_id,
        "process response replay created a new outbox job"
    );
    let response_job = repository
        .lease_next(std::time::Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response_job.id, response_job_id);
    repository
        .complete_operation(
            response_job,
            "process.correlated",
            serde_json::json!({"matched_instances":1}),
        )
        .await
        .unwrap();
    let view = repository
        .get_process(member.clone(), link.id)
        .await
        .unwrap();
    assert_eq!(view.process.id, link.id);
    assert_eq!(view.process.status, "active");
    assert_eq!(
        view.events
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<Vec<_>>(),
        vec!["process.started", "process.correlated"]
    );
    assert!(
        view.events
            .iter()
            .all(|event| !event.actor_id.to_string().is_empty())
    );
    assert_eq!(
        repository
            .get_process(UserId::named(format!("process-outsider-{suffix}")), link.id)
            .await,
        Err(RepositoryError::PermissionDenied)
    );
    let inspect = EnqueueInspection {
        process_link_id: link.id,
        actor: member.clone(),
        request_id: "process-inspect-contract".to_owned(),
    };
    let inspect_job_id = repository
        .enqueue_inspection(inspect.clone())
        .await
        .unwrap();
    assert_eq!(
        repository.enqueue_inspection(inspect).await.unwrap(),
        inspect_job_id,
        "process inspection replay created a new outbox job"
    );
    let inspect_job = repository
        .lease_next(std::time::Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(inspect_job.id, inspect_job_id);
    assert!(matches!(
        &inspect_job.operation,
        crate::process::OutboxOperation::Inspect { .. }
    ));
    repository
        .complete_operation(
            inspect_job,
            "process.inspected",
            serde_json::json!({"status":"waiting","current_node":"collect-rsvp"}),
        )
        .await
        .unwrap();
    let failing_job_id = repository
        .enqueue_correlation(EnqueueCorrelation {
            process_link_id: link.id,
            actor: member.clone(),
            request_id: "process-failure-contract".to_owned(),
            payload: serde_json::json!({"answer":"maybe"}),
        })
        .await
        .unwrap();
    let failing_job = repository
        .lease_next(std::time::Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(failing_job.id, failing_job_id);
    repository
        .reschedule(
            failing_job,
            ProcessError {
                kind: ProcessErrorKind::Unauthorized,
                message: "Heart rejected participant".to_owned(),
                retryable: false,
            },
            std::time::Duration::ZERO,
        )
        .await
        .unwrap();
    let failed = repository
        .get_process(member.clone(), link.id)
        .await
        .unwrap();
    assert_eq!(failed.process.status, "failed");
    let failure = failed.events.last().unwrap();
    assert_eq!(failure.event_type, "process.failed");
    assert_eq!(failure.actor_id, member);
    assert_eq!(failure.payload["kind"], "unauthorized");
    repository
        .set_circle_feature(SetCircleFeature {
            circle_id: circle.id,
            actor,
            feature: "heart.event-planning".to_owned(),
            enabled: false,
        })
        .await
        .unwrap();
    assert_eq!(
        repository
            .enqueue_correlation(EnqueueCorrelation {
                process_link_id: link.id,
                actor: member,
                request_id: "process-response-after-kill".to_owned(),
                payload: serde_json::json!({"answer":"no"}),
            })
            .await,
        Err(RepositoryError::PermissionDenied),
        "disabled Heart feature accepted process work"
    );
}
