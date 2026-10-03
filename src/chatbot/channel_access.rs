//! Explicit channel choices do not change a circle agent's global configuration.
use super::*;

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct ChannelAgentView {
    pub agent_id: String,
    pub display_name: String,
    pub agent_enabled: bool,
    pub enabled: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct ChannelAgentsView {
    pub access_revision: i64,
    pub agents: Vec<ChannelAgentView>,
    #[serde(default)]
    pub selection_available: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChannelAgentInput {
    pub enabled: bool,
    pub access_revision: i64,
}

// Always require actual channel membership. Circle authority covers open circle
// channels (public/local), never a private channel or a direct conversation.
const MANAGER: &str = "c.circle_id is not null and exists(select 1 from channel_memberships cm join users manager on manager.id=cm.user_id where cm.channel_id=c.id and cm.user_id=?uuid and manager.kind='human' and (cm.role in ('owner','moderator') or (c.kind!='private' and cm.role='member' and exists(select 1 from circle_memberships r where r.circle_id=c.circle_id and r.user_id=cm.user_id and r.role in ('owner','moderator')))))";

impl CircleChatAgents {
    pub(crate) async fn list_channel(
        &self,
        actor: &UserId,
        channel: &str,
    ) -> Result<ChannelAgentsView> {
        let pg = matches!(self.store, Store::Pg(_));
        let agent = if pg {
            "json_build_object('agent_id',cast(a.agent_id as text),'display_name',u.display_name,'agent_enabled',a.enabled,'enabled',coalesce(s.enabled,c.kind!='private'))"
        } else {
            "json_object('agent_id',a.agent_id,'display_name',u.display_name,'agent_enabled',json(case when a.enabled then 'true' else 'false' end),'enabled',json(case when coalesce(s.enabled,c.kind!='private') then 'true' else 'false' end))"
        };
        let agents = format!(
            "select {agent} as item from circle_chat_agents a join users u on u.id=a.agent_id left join channel_chat_agent_settings s on s.agent_id=a.agent_id and s.channel_id=c.id where a.circle_id=c.circle_id order by u.display_name,a.agent_id"
        );
        let object = if pg {
            format!(
                "cast(json_build_object('access_revision',c.chat_agent_access_revision,'agents',coalesce((select json_agg(items.item) from ({agents}) items(item)),'[]'::json)) as text)"
            )
        } else {
            format!(
                "json_object('access_revision',c.chat_agent_access_revision,'agents',json(coalesce((select json_group_array(json(item)) from ({agents}) items),'[]')))"
            )
        };
        // Permission and data are read by one statement, so a separate preflight
        // check cannot expose configuration after membership revocation.
        let query = format!("select {object} from channels c where c.id=?uuid and {MANAGER}");
        let values = self
            .store
            .values(&query, &[channel.into(), actor.to_string()])
            .await?;
        let value = values.first().ok_or(RepositoryError::PermissionDenied)?;
        let mut view: ChannelAgentsView = serde_json::from_str(value).map_err(storage)?;
        view.selection_available = Self::channel_selection_available();
        Ok(view)
    }

    pub(crate) fn channel_selection_available() -> bool {
        std::env::var("SPROYT_CHANNEL_CHAT_AGENTS_ENABLED").as_deref() == Ok("true")
    }

    pub(crate) async fn update_channel(
        &self,
        actor: &UserId,
        channel: &str,
        agent: &str,
        input: ChannelAgentInput,
    ) -> Result<ChannelAgentsView> {
        if input.access_revision < 1 {
            return Err(RepositoryError::Conflict);
        }
        macro_rules! change {
            ($pool:expr, $pg:expr) => {{
                let mut tx = $pool.begin().await.map_err(storage)?;
                // Circle authority precedes channel membership, matching
                // leave_circle. Publication needs only channel -> agent.
                let circle_query = sql("select cast(circle_id as text) from channels where id=?uuid", $pg);
                let circle: Option<String> = sqlx::query_scalar(&circle_query).bind(channel)
                    .fetch_optional(&mut *tx).await.map_err(storage)?.flatten();
                let circle = circle.ok_or(RepositoryError::PermissionDenied)?;
                let authority = sql("select role from circle_memberships where circle_id=?uuid and user_id=?uuid", $pg)
                    + if $pg { " for share" } else { "" };
                let circle_role: Option<String> = sqlx::query_scalar(&authority).bind(&circle).bind(actor.to_string())
                    .fetch_optional(&mut *tx).await.map_err(storage)?;
                let query = sql("select cast(circle_id as text) as circle_id,kind,chat_agent_access_revision from channels where id=?uuid", $pg)
                    + if $pg { " for update" } else { "" };
                let channel_row = sqlx::query(&query).bind(channel)
                    .fetch_optional(&mut *tx).await.map_err(storage)?
                    .ok_or(RepositoryError::PermissionDenied)?;
                let locked_circle: Option<String> = channel_row.try_get("circle_id").map_err(storage)?;
                if locked_circle.as_deref() != Some(circle.as_str()) { return Err(RepositoryError::Conflict); }
                let kind: String = channel_row.try_get("kind").map_err(storage)?;
                let revision: i64 = channel_row.try_get("chat_agent_access_revision").map_err(storage)?;
                let authority = sql("select cm.role from channel_memberships cm join users u on u.id=cm.user_id where cm.channel_id=?uuid and cm.user_id=?uuid and u.kind='human'", $pg)
                    + if $pg { " for share of cm" } else { "" };
                let role: Option<String> = sqlx::query_scalar(&authority).bind(channel).bind(actor.to_string())
                    .fetch_optional(&mut *tx).await.map_err(storage)?;
                let mut allowed = matches!(role.as_deref(), Some("owner" | "moderator"));
                if !allowed && kind != "private" && role.as_deref() == Some("member") {
                    allowed = matches!(circle_role.as_deref(), Some("owner" | "moderator"));
                }
                if !allowed { return Err(RepositoryError::PermissionDenied); }
                if revision != input.access_revision { return Err(RepositoryError::Conflict); }
                let query = sql("select cast(agent_id as text) from circle_chat_agents where agent_id=?uuid and circle_id=?uuid", $pg)
                    + if $pg { " for share" } else { "" };
                let existing: Option<String> = sqlx::query_scalar(&query).bind(agent).bind(&circle)
                    .fetch_optional(&mut *tx).await.map_err(storage)?;
                if existing.is_none() { return Err(RepositoryError::PermissionDenied); }
                sqlx::query(&sql("insert into channel_chat_agent_settings(channel_id,agent_id,enabled,updated_by,updated_at) values(?uuid,?uuid,case when ?='true' then true else false end,?uuid,?int) on conflict(channel_id,agent_id) do update set enabled=excluded.enabled,updated_by=excluded.updated_by,updated_at=excluded.updated_at", $pg))
                    .bind(channel).bind(agent).bind(input.enabled.to_string()).bind(actor.to_string())
                    .bind(Utc::now().timestamp().to_string()).execute(&mut *tx).await.map_err(storage)?;
                sqlx::query(&sql("update channels set chat_agent_access_revision=chat_agent_access_revision+1 where id=?uuid", $pg))
                    .bind(channel).execute(&mut *tx).await.map_err(storage)?;
                let payload = json!({"agent_id":agent,"enabled":input.enabled,"access_revision":revision+1});
                sqlx::query(&sql("insert into audit_events(actor_id,action,target_kind,target_id,payload) values(?uuid,'channel.chat_agent_access_changed','channel',?,?)", $pg))
                    .bind(actor.to_string()).bind(channel).bind(sqlx::types::Json(payload)).execute(&mut *tx).await.map_err(storage)?;
                tx.commit().await.map_err(storage)?;
            }};
        }
        match &self.store {
            Store::Pg(pool) => change!(pool, true),
            Store::Sqlite(pool) => change!(pool, false),
        }
        self.list_channel(actor, channel).await
    }
}
