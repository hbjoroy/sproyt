//! Admission and acknowledgement for an explicitly enabled public feedback channel.
use super::*;
use crate::domain::{ChatEvent, MessageReactionChange};

#[cfg(test)]
#[path = "channel_tests.rs"]
pub(super) mod tests;

// Reuse this relation at admission and immediately before external publication.
// Felles channels have no circle_id. A circle agent needs an explicit grant here.
pub(super) const SOURCE_AUTHORITY: &str = "
    feedback_routes r
    join channels c on c.id=r.channel_id and r.enabled and c.circle_id is null
        and c.kind='public' and not c.hidden
    join channel_chat_agent_settings grant_setting on grant_setting.channel_id=c.id
        and grant_setting.agent_id=r.agent_id and grant_setting.enabled
    join circle_chat_agents a on a.agent_id=r.agent_id and a.enabled
    join users bot on bot.id=a.agent_id and bot.kind='agent'
    join agent_profiles profile on profile.agent_id=bot.id and profile.provider='sproyt-circle-chat'
        and profile.revoked_at is null and (profile.expires_at is null or profile.expires_at>current_timestamp)
    join work_applications app on app.id=r.application_id and app.enabled
    join work_github_bindings b on b.application_id=app.id and b.enabled
    join application_processors processor on processor.application_id=app.id and processor.user_id=r.reviewer_id
        and processor.can_review and processor.can_export
    join application_process_roles pr on pr.application_id=app.id and pr.user_id=r.reviewer_id
        and pr.process_role='product-handler'
    join users reviewer on reviewer.id=r.reviewer_id and reviewer.kind='human'
    join channel_memberships cm on cm.channel_id=c.id and cm.user_id=r.reviewer_id and cm.role<>'observer'
    join messages m on m.channel_id=c.id and m.parent_message_id is null and m.deleted_at is null
    join users author on author.id=m.sender_id and author.kind='human'
    join message_provenance provenance on provenance.message_id=m.id and provenance.provenance='human'";

pub(super) const PINNED_JOB: &str = "
    j.source_message_id=m.id and j.source_body=m.body and j.source_sequence=m.sequence
    and j.channel_id=r.channel_id and j.agent_id=r.agent_id and j.application_id=r.application_id
    and j.reviewer_id=r.reviewer_id and j.repository_id=b.repository_id and j.binding_revision=b.revision
    and j.installation_id=b.installation_id and j.repository_name=b.repository_name and j.bot_login=b.bot_login";

const ADMIT: &str = "insert into feedback_jobs(source_message_id,channel_id,source_sequence,source_body,agent_id,
                reviewer_id,application_id,binding_revision,repository_id,installation_id,repository_name,bot_login)
            select m.id,c.id,m.sequence,m.body,r.agent_id,r.reviewer_id,r.application_id,
                b.revision,b.repository_id,b.installation_id,b.repository_name,b.bot_login";

const ACTIVATE_SOURCE: &str =
    "m.id=?uuid and c.id=?uuid and r.reviewer_id=?uuid and m.sequence<=r.after_sequence";

impl WorkItems {
    pub(crate) async fn feedback_activation(
        &self,
        actor: &UserId,
        channel: Uuid,
        message: Uuid,
        activate: bool,
    ) -> Result<Option<String>> {
        // Both the offer and the admission use the route's current product-owner and channel grants.
        let offer = format!(
            "select coalesce(j.status,'available') from {SOURCE_AUTHORITY}
            left join feedback_jobs j on j.source_message_id=m.id where {ACTIVATE_SOURCE}"
        );
        let enqueue = format!(
            "{ADMIT} from {SOURCE_AUTHORITY} where {ACTIVATE_SOURCE}
            on conflict(source_message_id) do nothing"
        );
        macro_rules! activation {
            ($pool:expr,$pg:expr) => {{
                let mut tx = $pool.begin().await.map_err(storage)?;
                if activate {
                    sqlx::query(&sql(&enqueue, $pg))
                        .bind(message.to_string())
                        .bind(channel.to_string())
                        .bind(actor.to_string())
                        .execute(&mut *tx)
                        .await
                        .map_err(storage)?;
                }
                let status = sqlx::query_scalar::<_, String>(&sql(&offer, $pg))
                    .bind(message.to_string())
                    .bind(channel.to_string())
                    .bind(actor.to_string())
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(storage)?;
                tx.commit().await.map_err(storage)?;
                Ok(status)
            }};
        }
        match &self.store {
            Store::Pg(pool) => activation!(pool, true),
            Store::Sqlite(pool) => activation!(pool, false),
        }
    }

    pub(super) async fn admit_feedback(&self) -> Result<()> {
        let query = format!("{ADMIT} from {SOURCE_AUTHORITY}
            where m.sequence>r.after_sequence and m.sequence>coalesce((select max(prior.source_sequence) from feedback_jobs prior
                where prior.channel_id=c.id),r.after_sequence)
            order by m.sequence limit 32 on conflict(source_message_id) do nothing");
        match &self.store {
            Store::Pg(pool) => sqlx::query(&query)
                .execute(pool)
                .await
                .map_err(storage)?
                .rows_affected(),
            Store::Sqlite(pool) => sqlx::query(&query)
                .execute(pool)
                .await
                .map_err(storage)?
                .rows_affected(),
        };
        Ok(())
    }

    pub(super) async fn acknowledge_feedback(&self, chat: &ChatEngine, source: Uuid) -> Result<()> {
        // Both chat mutations, their notification outbox and the job's receipt
        // commit together. Another worker either performs them all or sees the receipt.
        macro_rules! deliver {
            ($tx:expr, $pg:expr, $notify:path) => {{
                let mut tx = $tx.map_err(storage)?;
                let mut query = sql(&format!("select cast(j.channel_id as text) as channel,
                    cast(j.agent_id as text) as agent, bot.display_name, j.issue_number,j.repository_name,
                    j.issue_url,j.publication_kind,cast(j.ack_message_id as text) as receipt
                    from {SOURCE_AUTHORITY} join feedback_jobs j on {PINNED_JOB}
                    where j.source_message_id=?uuid and j.status in ('published','done')"), $pg);
                if $pg {
                    query.push_str(" for update of j for share of r,c,grant_setting,a,profile,app,b,processor,pr,cm,m");
                }
                let row = sqlx::query(&query).bind(source.to_string()).fetch_optional(&mut *tx)
                    .await.map_err(storage)?.ok_or(RepositoryError::PermissionDenied)?;
                if row.try_get::<Option<String>,_>("receipt").map_err(storage)?.is_some() {
                    tx.commit().await.map_err(storage)?;
                    return Ok(());
                }
                let channel: String = row.try_get("channel").map_err(storage)?;
                let agent: String = row.try_get("agent").map_err(storage)?;
                let name: String = row.try_get("display_name").map_err(storage)?;
                let number: i64 = row.try_get("issue_number").map_err(storage)?;
                let repository: String = row.try_get("repository_name").map_err(storage)?;
                let url: String = row.try_get("issue_url").map_err(storage)?;
                let kind: String = row.try_get("publication_kind").map_err(storage)?;
                if number < 1 || url != format!("https://github.com/{repository}/issues/{number}") {
                    return Err(RepositoryError::Conflict);
                }
                let (emoji, action) = match kind.as_str() {
                    "create" => ("📋", "Eg har oppretta eit forslag"),
                    "comment" => ("💬", "Eg har lagt til dei nye opplysningane i saka"),
                    "duplicate" => ("🔗", "Denne tilbakemeldinga er allereie dekt i saka"),
                    _ => return Err(RepositoryError::Conflict),
                };
                let reserve = sql("update channel_sequences set next_sequence=next_sequence+1
                    where channel_id=?uuid returning next_sequence-1", $pg);
                let sequence: i64 = sqlx::query_scalar(&reserve).bind(&channel)
                    .fetch_one(&mut *tx).await.map_err(storage)?;
                let message = ChatMessage {
                    id: MessageId::generate(), channel_id: ChannelId::new(&channel).map_err(storage)?,
                    parent_message_id: Some(MessageId::from_uuid(source)), sender_id: UserId::new(&agent).map_err(storage)?,
                    sender_display_name: DisplayName::new(name).map_err(storage)?,
                    sequence: ChannelSequence::try_from(sequence).map_err(storage)?,
                    body: MessageBody::new(format!("{action}. {url}")).map_err(storage)?,
                    sent_at: Utc::now(), edited_at: None, deleted_at: None,
                };
                let insert = sql("insert into messages(id,channel_id,parent_message_id,sender_id,
                    sender_display_name,sequence,body,created_at) values(?uuid,?uuid,?uuid,?uuid,?,?,?,?)",$pg);
                sqlx::query(&insert).bind(message.id.as_uuid().to_string()).bind(&channel)
                    .bind(source.to_string()).bind(&agent).bind(message.sender_display_name.as_str())
                    .bind(sequence).bind(message.body.as_str()).bind(message.sent_at)
                    .execute(&mut *tx).await.map_err(storage)?;
                $notify(&mut tx, &message).await?;
                let react = sql("insert into message_reactions(message_id,user_id,emoji,created_at)
                    values(?uuid,?uuid,?,current_timestamp) on conflict(message_id,user_id,emoji) do nothing",$pg);
                sqlx::query(&react).bind(source.to_string()).bind(&agent).bind(emoji)
                    .execute(&mut *tx).await.map_err(storage)?;
                let count_query = sql("select count(*) from message_reactions where message_id=?uuid and emoji=?", $pg);
                let count: i64 = sqlx::query_scalar(&count_query).bind(source.to_string()).bind(emoji)
                    .fetch_one(&mut *tx).await.map_err(storage)?;
                let change = MessageReactionChange {
                    message_id: MessageId::from_uuid(source), channel_id: message.channel_id.clone(),
                    user_id: message.sender_id.clone(), emoji: emoji.to_owned(), added: true,
                    count: u32::try_from(count).map_err(storage)?,
                };
                let record = sql("update feedback_jobs set ack_message_id=?uuid where source_message_id=?uuid",$pg);
                sqlx::query(&record).bind(message.id.as_uuid().to_string()).bind(source.to_string())
                    .execute(&mut *tx).await.map_err(storage)?;
                if $pg {
                    let event = serde_json::to_string(&ChatEvent::MessageReactionChanged { change: change.clone() }).map_err(storage)?;
                    sqlx::query("select pg_notify('sproyt_messages',$1),pg_notify('sproyt_reactions',$2)")
                        .bind(message.id.as_uuid().to_string()).bind(event).execute(&mut *tx).await.map_err(storage)?;
                }
                tx.commit().await.map_err(storage)?;
                if !$pg {
                    chat.announce_persisted_message(message.id).await.map_err(storage)?;
                    chat.announce_persisted_reaction(change).await.map_err(storage)?;
                }
                Ok(())
            }};
        }
        match &self.store {
            Store::Pg(pool) => deliver!(
                pool.begin().await,
                true,
                crate::notification::enqueue_message_postgres
            ),
            Store::Sqlite(pool) => deliver!(
                pool.begin_with("BEGIN IMMEDIATE").await,
                false,
                crate::notification::enqueue_message_sqlite
            ),
        }
    }
}
