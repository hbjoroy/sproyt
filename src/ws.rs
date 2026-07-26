use std::collections::HashMap;
use std::time::Duration;

use axum::extract::ws::{CloseFrame, Message, WebSocket};
use tokio::sync::{broadcast, mpsc, watch};

use crate::{
    auth::{AuthService, AuthenticatedPrincipal},
    chat::{ChatEngine, ChatError, ConnectionId},
    domain::{
        ChannelId, ChannelSequence, ChannelSlug, DisplayName, MessageBody, MessageLimit,
        RepositoryError, UserId,
    },
    protocol::{ClientCommand, ClientEnvelope, PROTOCOL_ID, ServerEnvelope, ServerEvent},
};

pub struct SocketAuthentication {
    service: AuthService,
    principal: AuthenticatedPrincipal,
    requested_name: Option<String>,
    session_cookie: Option<String>,
}

impl SocketAuthentication {
    pub fn new(
        service: AuthService,
        principal: AuthenticatedPrincipal,
        requested_name: Option<String>,
        session_cookie: Option<String>,
    ) -> Self {
        Self {
            service,
            principal,
            requested_name,
            session_cookie,
        }
    }
}

pub async fn handle_socket(
    chat: ChatEngine,
    authentication: SocketAuthentication,
    mut socket: WebSocket,
    mut shutdown: watch::Receiver<bool>,
    idle_timeout: Duration,
) {
    let SocketAuthentication {
        service: auth,
        principal,
        requested_name,
        session_cookie,
    } = authentication;
    let participant_id = principal.user.id.clone();
    if *shutdown.borrow() {
        let _ = socket
            .send(Message::Close(Some(CloseFrame {
                code: 1001,
                reason: "server shutdown".into(),
            })))
            .await;
        return;
    }
    if let Err(error) = chat.ensure_user(principal.user).await {
        let _ = send(&mut socket, &ServerEnvelope::event(error_event(error))).await;
        return;
    }

    let (outbound, mut outbound_events) = mpsc::channel::<ServerEnvelope>(256);
    let mut subscriptions: HashMap<ChannelId, ActiveSubscription> = HashMap::new();
    let idle = tokio::time::sleep(idle_timeout);
    tokio::pin!(idle);
    let mut reauthentication = tokio::time::interval(Duration::from_secs(30));
    reauthentication.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    reauthentication.tick().await;

    loop {
        tokio::select! {
            _ = reauthentication.tick() => {
                if auth
                    .revalidate_request(requested_name.clone(), session_cookie.as_deref())
                    .await
                    .is_err()
                {
                    let _ = socket.send(Message::Close(Some(CloseFrame {
                        code: 1008,
                        reason: "authentication expired".into(),
                    }))).await;
                    break;
                }
            }
            () = &mut idle => {
                let _ = socket.send(Message::Close(Some(CloseFrame {
                    code: 1001,
                    reason: "idle timeout".into(),
                }))).await;
                break;
            }
            result = shutdown.changed() => {
                if result.is_err() || *shutdown.borrow() {
                    let _ = socket.send(Message::Close(Some(CloseFrame {
                        code: 1001,
                        reason: "server shutdown".into(),
                    }))).await;
                    break;
                }
            }
            Some(message) = outbound_events.recv() => {
                if send(&mut socket, &message).await.is_err() {
                    break;
                }
            }
            frame = socket.recv() => {
                idle.as_mut().reset(tokio::time::Instant::now() + idle_timeout);
                match frame {
                    Some(Ok(Message::Text(text))) => {
                        let envelope = match serde_json::from_str::<ClientEnvelope>(&text) {
                            Ok(envelope) => envelope,
                            Err(_) => {
                                let message = ServerEnvelope::event(ServerEvent::Error {
                                    code: "invalid_envelope",
                                    message: "invalid JSON command envelope".to_owned(),
                                });
                                if send(&mut socket, &message).await.is_err() {
                                    break;
                                }
                                continue;
                            }
                        };
                        let request_id = envelope.request_id.clone();
                        let response = if envelope.protocol != PROTOCOL_ID {
                            ServerEnvelope::response(request_id, ServerEvent::Error {
                                code: "unsupported_protocol",
                                message: format!("expected {PROTOCOL_ID}"),
                            })
                        } else if envelope.request_id.trim().is_empty()
                            || envelope.request_id.len() > 128
                        {
                            ServerEnvelope::response(request_id, ServerEvent::Error {
                                code: "invalid_request_id",
                                message: "request_id must contain 1 to 128 bytes".to_owned(),
                            })
                        } else {
                            execute_command(
                                &chat,
                                &participant_id,
                                envelope,
                                &outbound,
                                &mut subscriptions,
                            )
                            .await
                        };
                        if send(&mut socket, &response).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(payload))) => {
                        if socket.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Binary(_))) | Some(Ok(Message::Pong(_))) => {}
                    Some(Err(_)) => break,
                }
            }
        }
    }

    for (channel_id, subscription) in subscriptions {
        subscription.task.abort();
        let _ = chat
            .leave(
                channel_id,
                participant_id.clone(),
                subscription.connection_id,
            )
            .await;
    }
}

struct ActiveSubscription {
    connection_id: ConnectionId,
    task: tokio::task::JoinHandle<()>,
}

async fn execute_command(
    chat: &ChatEngine,
    participant_id: &UserId,
    envelope: ClientEnvelope,
    outbound: &mpsc::Sender<ServerEnvelope>,
    subscriptions: &mut HashMap<ChannelId, ActiveSubscription>,
) -> ServerEnvelope {
    let request_id = envelope.request_id;
    let command_request_id = request_id.clone();
    let result: Result<ServerEvent, ChatError> = match envelope.command {
        ClientCommand::Hello => Ok(ServerEvent::Hello {
            participant_id: participant_id.clone(),
        }),
        ClientCommand::ListUsers => chat
            .list_users(participant_id.clone())
            .await
            .map(|users| ServerEvent::UsersListed { users }),
        ClientCommand::OpenDirectChannel { user_id } => chat
            .open_direct_channel(participant_id.clone(), user_id)
            .await
            .map(|channel| ServerEvent::DirectChannelOpened { channel }),
        ClientCommand::CreateChannel {
            slug,
            name,
            kind,
            circle_id,
        } => {
            async {
                let channel = chat
                    .create_channel(
                        participant_id.clone(),
                        ChannelSlug::new(slug)?,
                        DisplayName::new(name)?,
                        kind,
                        circle_id,
                    )
                    .await?;
                Ok(ServerEvent::ChannelCreated { channel })
            }
            .await
        }
        ClientCommand::JoinChannel { channel } => chat
            .join_channel(participant_id.clone(), channel)
            .await
            .map(|membership| ServerEvent::MembershipJoined { membership }),
        ClientCommand::LeaveChannel { channel_id } => {
            async {
                disconnect(chat, participant_id, &channel_id, subscriptions).await;
                chat.leave_channel(participant_id.clone(), channel_id.clone())
                    .await?;
                Ok(ServerEvent::MembershipLeft { channel_id })
            }
            .await
        }
        ClientCommand::ListMyChannels => chat
            .list_channels(participant_id.clone())
            .await
            .map(|channels| ServerEvent::ChannelsListed { channels }),
        ClientCommand::ListJoinableChannels { circle_id } => chat
            .list_joinable_channels(participant_id.clone(), circle_id)
            .await
            .map(|channels| ServerEvent::JoinableChannelsListed { channels }),
        ClientCommand::AddChannelMember {
            channel_id,
            user_id,
        } => chat
            .add_channel_member(participant_id.clone(), channel_id, user_id)
            .await
            .map(|membership| ServerEvent::ChannelMemberAdded { membership }),
        ClientCommand::LoadRecentMessages {
            channel_id,
            limit,
            after,
            before,
        } => chat
            .load_messages(
                participant_id.clone(),
                channel_id.clone(),
                MessageLimit::new(limit.unwrap_or(50)),
                after,
                before,
            )
            .await
            .map(|messages| ServerEvent::MessagesLoaded {
                channel_id,
                messages,
            }),
        ClientCommand::SubscribeChannel { channel_id } => {
            disconnect(chat, participant_id, &channel_id, subscriptions).await;
            match chat
                .subscribe(channel_id.clone(), participant_id.clone())
                .await
            {
                Ok(subscription) => {
                    let connection_id = subscription.connection_id;
                    let history = subscription.history;
                    let last_seen = history
                        .last()
                        .map_or(ChannelSequence::new(0), |message| message.sequence);
                    let task = spawn_subscription(
                        chat.clone(),
                        channel_id.clone(),
                        subscription.receiver,
                        last_seen,
                        outbound.clone(),
                    );
                    subscriptions.insert(
                        channel_id.clone(),
                        ActiveSubscription {
                            connection_id,
                            task,
                        },
                    );
                    Ok(ServerEvent::SubscriptionStarted {
                        channel_id,
                        history,
                    })
                }
                Err(error) => Err(error),
            }
        }
        ClientCommand::UnsubscribeChannel { channel_id } => {
            disconnect(chat, participant_id, &channel_id, subscriptions).await;
            Ok(ServerEvent::SubscriptionEnded { channel_id })
        }
        ClientCommand::SendMessage { channel_id, body } => {
            async {
                let message = chat
                    .send_message_idempotent(
                        channel_id,
                        participant_id.clone(),
                        MessageBody::new(body)?,
                        command_request_id,
                    )
                    .await?;
                Ok(ServerEvent::MessageAccepted { message })
            }
            .await
        }
        ClientCommand::MarkRead {
            channel_id,
            sequence,
        } => chat
            .mark_read(participant_id.clone(), channel_id, sequence)
            .await
            .map(|membership| ServerEvent::ReadMarkerUpdated { membership }),
        ClientCommand::ListMentions => chat
            .list_mentions(participant_id.clone())
            .await
            .map(|mentions| ServerEvent::MentionsListed { mentions }),
        ClientCommand::MarkMentionRead { message_id } => chat
            .mark_mention_read(participant_id.clone(), message_id)
            .await
            .map(|()| ServerEvent::MentionRead { message_id }),
        ClientCommand::CreateTask {
            source_message_id,
            assignee_id,
            title,
            process_link_id,
        } => chat
            .create_task(
                participant_id.clone(),
                source_message_id,
                assignee_id,
                title,
                process_link_id,
            )
            .await
            .map(|task| ServerEvent::TaskCreated { task }),
        ClientCommand::ListTasks => chat
            .list_tasks(participant_id.clone())
            .await
            .map(|tasks| ServerEvent::TasksListed { tasks }),
        ClientCommand::SetTaskDone { task_id, done } => chat
            .set_task_done(participant_id.clone(), task_id, done)
            .await
            .map(|task| ServerEvent::TaskUpdated { task }),
        ClientCommand::CreateCircle { slug, name } => {
            async {
                let circle = chat
                    .create_circle(
                        participant_id.clone(),
                        ChannelSlug::new(slug)?,
                        DisplayName::new(name)?,
                    )
                    .await?;
                Ok(ServerEvent::CircleCreated { circle })
            }
            .await
        }
        ClientCommand::ListMyCircles => chat
            .list_circles(participant_id.clone())
            .await
            .map(|circles| ServerEvent::CirclesListed { circles }),
        ClientCommand::DeleteCircle { circle_id } => chat
            .delete_circle(participant_id.clone(), circle_id.clone())
            .await
            .map(|()| ServerEvent::CircleDeleted { circle_id }),
        ClientCommand::CreateCircleInvitation { circle_id } => chat
            .create_circle_invitation(participant_id.clone(), circle_id)
            .await
            .map(|invitation| ServerEvent::CircleInvitationCreated { invitation }),
        ClientCommand::AcceptCircleInvitation { token } => chat
            .accept_circle_invitation(participant_id.clone(), token)
            .await
            .map(|membership| ServerEvent::CircleInvitationAccepted { membership }),
        ClientCommand::Ping => {
            let active = subscriptions
                .iter()
                .map(|(channel_id, subscription)| (channel_id.clone(), subscription.connection_id))
                .collect();
            chat.renew_presence(participant_id.clone(), active)
                .await
                .map(|()| ServerEvent::Pong)
        }
    };

    match result {
        Ok(event) => ServerEnvelope::response(request_id, event),
        Err(error) => ServerEnvelope::response(request_id, error_event(error)),
    }
}

async fn disconnect(
    chat: &ChatEngine,
    participant_id: &UserId,
    channel_id: &ChannelId,
    subscriptions: &mut HashMap<ChannelId, ActiveSubscription>,
) {
    if let Some(active) = subscriptions.remove(channel_id) {
        active.task.abort();
        let _ = chat
            .leave(
                channel_id.clone(),
                participant_id.clone(),
                active.connection_id,
            )
            .await;
    }
}

fn spawn_subscription(
    chat: ChatEngine,
    channel_id: ChannelId,
    mut receiver: broadcast::Receiver<crate::domain::ChatEvent>,
    mut last_seen_sequence: ChannelSequence,
    outbound: mpsc::Sender<ServerEnvelope>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let message = match receiver.recv().await {
                Ok(event) => {
                    if let crate::domain::ChatEvent::MessageAccepted { message } = &event {
                        last_seen_sequence = message.sequence;
                    }
                    ServerEnvelope::event(ServerEvent::Chat { event })
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    let latest_known_sequence = match chat.latest_sequence(channel_id.clone()).await
                    {
                        Ok(sequence) => sequence,
                        Err(error) => {
                            let _ = outbound
                                .send(ServerEnvelope::event(error_event(error)))
                                .await;
                            break;
                        }
                    };
                    ServerEnvelope::event(ServerEvent::Lagged {
                        channel_id: channel_id.clone(),
                        last_seen_sequence,
                        latest_known_sequence,
                        skipped,
                        hint: "load_recent_messages_after",
                    })
                }
                Err(broadcast::error::RecvError::Closed) => break,
            };
            if outbound.send(message).await.is_err() {
                break;
            }
        }
    })
}

fn error_event(error: ChatError) -> ServerEvent {
    let code = match &error {
        ChatError::EngineStopped => "engine_stopped",
        ChatError::Repository(RepositoryError::Conflict) => "conflict",
        ChatError::Repository(RepositoryError::NotFound) => "not_found",
        ChatError::Repository(RepositoryError::PermissionDenied) => "permission_denied",
        ChatError::Repository(RepositoryError::Storage(_)) => "storage_error",
        ChatError::Validation(_) => "validation_error",
    };
    ServerEvent::Error {
        code,
        message: error.public_message(),
    }
}

async fn send(socket: &mut WebSocket, event: &ServerEnvelope) -> Result<(), axum::Error> {
    let payload = serde_json::to_string(event).expect("server messages must serialize");
    socket.send(Message::Text(payload.into())).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_protocol_with_stable_error_shape() {
        let envelope: ClientEnvelope =
            serde_json::from_str(r#"{"protocol":"old","request_id":"r1","type":"ping"}"#).unwrap();
        assert_eq!(envelope.protocol, "old");
        assert!(matches!(envelope.command, ClientCommand::Ping));
    }

    #[test]
    fn storage_errors_are_redacted_from_websocket_events() {
        let event = error_event(ChatError::Repository(RepositoryError::Storage(
            "postgres://admin:provider-secret@database".to_owned(),
        )));
        let ServerEvent::Error { message, .. } = event else {
            panic!("expected error event");
        };
        assert_eq!(message, "internal storage error");
        assert!(!message.contains("provider-secret"));
    }
}
