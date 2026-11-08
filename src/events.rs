use crate::{
    chatbot::sql,
    db::{sql_error, storage},
    domain::RepositoryError,
    topics::TopicStore,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Datelike, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Column, Row};
use std::collections::BTreeMap;

type Result<T> = std::result::Result<T, RepositoryError>;
type Record = BTreeMap<String, Option<String>>;
#[derive(Clone)]
pub(crate) struct EventStore(pub TopicStore);

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EventCursor {
    v: u8,
    as_of: String,
    bucket: u8,
    starts_at: String,
    id: String,
}
impl EventCursor {
    fn decode(raw: &str) -> Result<Self> {
        if raw.len() > 512 {
            return Err(RepositoryError::Conflict);
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(raw)
            .map_err(|_| RepositoryError::Conflict)?;
        let value: Self = serde_json::from_slice(&bytes).map_err(|_| RepositoryError::Conflict)?;
        let canonical = |s: &str| {
            DateTime::parse_from_rfc3339(s).is_ok_and(|date| {
                date.with_timezone(&Utc)
                    .to_rfc3339_opts(SecondsFormat::Millis, true)
                    == s
                    && (1..=9999).contains(&date.year())
            })
        };
        if value.v != 1
            || value.bucket > 2
            || !uuid::Uuid::parse_str(&value.id).is_ok_and(|id| id.to_string() == value.id)
            || !canonical(&value.as_of)
            || !canonical(&value.starts_at)
        {
            return Err(RepositoryError::Conflict);
        }
        Ok(value)
    }
    fn encode(&self) -> Result<String> {
        Ok(URL_SAFE_NO_PAD.encode(serde_json::to_vec(self).map_err(storage)?))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EventInput {
    pub title: String,
    pub code: String,
    pub starts_at: DateTime<Utc>,
    pub timezone: String,
    pub ends_at: Option<DateTime<Utc>>,
    pub location: Option<String>,
    pub description: String,
}
impl EventInput {
    fn validate(&mut self) -> Result<()> {
        self.title = self.title.trim().into();
        self.code = self.code.trim().to_ascii_lowercase();
        if self.title.is_empty()
            || self.title.chars().count() > 160
            || !valid_code(&self.code)
            || self.description.len() > 20000
            || self
                .location
                .as_ref()
                .is_some_and(|v| v.chars().count() > 300)
            || self.timezone.parse::<chrono_tz::Tz>().is_err()
            || !(1..=9999).contains(&self.starts_at.year())
            || self
                .ends_at
                .is_some_and(|end| end <= self.starts_at || !(1..=9999).contains(&end.year()))
        {
            return Err(RepositoryError::Conflict);
        }
        Ok(())
    }
}
pub(crate) fn valid_code(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 48
        && s.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}
fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}
fn value(row: &Record, key: &str) -> String {
    row.get(key).and_then(Clone::clone).unwrap_or_default()
}
fn optional(row: &Record, key: &str) -> Value {
    row.get(key)
        .and_then(Clone::clone)
        .map(Value::String)
        .unwrap_or(Value::Null)
}

impl EventStore {
    async fn query(&self, statement: &str, args: &[String]) -> Result<Vec<Record>> {
        macro_rules! fetch {
            ($pool:expr,$pg:expr) => {{
                let statement = sql(statement, $pg);
                let mut q = sqlx::query(&statement);
                for arg in args {
                    q = q.bind(arg);
                }
                q.fetch_all($pool)
                    .await
                    .map_err(sql_error)?
                    .into_iter()
                    .map(|row| {
                        row.columns()
                            .iter()
                            .map(|c| {
                                Ok((
                                    c.name().to_owned(),
                                    row.try_get::<Option<String>, _>(c.name())
                                        .map_err(sql_error)?,
                                ))
                            })
                            .collect::<Result<Record>>()
                    })
                    .collect()
            }};
        }
        match &self.0 {
            TopicStore::Postgres(p) => fetch!(p, true),
            TopicStore::Sqlite(p) => fetch!(p, false),
        }
    }
    async fn execute(&self, statement: &str, args: &[String]) -> Result<u64> {
        macro_rules! run {
            ($pool:expr,$pg:expr) => {{
                let statement = sql(statement, $pg);
                let mut q = sqlx::query(&statement);
                for arg in args {
                    q = q.bind(arg);
                }
                q.execute($pool).await.map_err(sql_error)?.rows_affected()
            }};
        }
        Ok(match &self.0 {
            TopicStore::Postgres(p) => run!(p, true),
            TopicStore::Sqlite(p) => run!(p, false),
        })
    }
    async fn mutate(
        &self,
        statement: &str,
        mut args: Vec<String>,
        actor: &str,
        channel: &str,
        event: Option<&str>,
        manage: bool,
    ) -> Result<u64> {
        self.require_enabled(actor, channel).await?;
        let mut guard=" exists(select 1 from channel_memberships em join channels ec on ec.id=em.channel_id where em.user_id=?uuid and em.channel_id=?uuid and (ec.circle_id is null or exists(select 1 from circle_memberships ecm where ecm.circle_id=ec.circle_id and ecm.user_id=em.user_id)) and coalesce(ec.events_enabled,exists(select 1 from channel_events current_event where current_event.channel_id=ec.id))".to_owned();
        args.extend([actor.into(), channel.into()]);
        if manage {
            guard.push_str(" and (em.role in ('owner','moderator') or exists(select 1 from channel_events ee where ee.id=? and ee.creator_id=em.user_id))");
            args.push(event.unwrap_or("").into());
        }
        let pg = matches!(&self.0, TopicStore::Postgres(_));
        if pg {
            guard = guard.replace(
                "ecm.user_id=em.user_id))",
                "ecm.user_id=em.user_id for share))",
            );
            guard.push_str(" and ec.id in (select id from event_channel_gate) for share of em");
        }
        guard.push(')');
        let mut statement = statement.replace("/*ACCESS*/", &guard);
        if pg {
            // Materialization and a SHARE row lock recheck availability after
            // a competing channel toggle, and retain it through the mutation.
            statement = format!(
                "with event_channel_gate as materialized (select c.id from channels c where c.id=?uuid and coalesce(c.events_enabled,exists(select 1 from channel_events current_event where current_event.channel_id=c.id)) for share of c) {statement}"
            );
            args.insert(0, channel.into());
        }
        let changed = self.execute(&statement, &args).await?;
        if changed == 0 {
            self.require_enabled(actor, channel).await?;
        }
        Ok(changed)
    }
    pub(crate) async fn availability(&self, actor: &str, channel: &str) -> Result<bool> {
        self.access(actor, channel).await?;
        let rows=self.query("select cast(coalesce(c.events_enabled,exists(select 1 from channel_events e where e.channel_id=c.id)) as text) as enabled from channels c where c.id=?uuid",&[channel.into()]).await?;
        Ok(matches!(
            value(rows.first().ok_or(RepositoryError::NotFound)?, "enabled").as_str(),
            "1" | "true"
        ))
    }
    async fn require_enabled(&self, actor: &str, channel: &str) -> Result<()> {
        if self.availability(actor, channel).await? {
            Ok(())
        } else {
            Err(RepositoryError::EventsDisabled)
        }
    }
    pub(crate) async fn access(&self, actor: &str, channel: &str) -> Result<String> {
        let r=self.query("select m.role from channel_memberships m join channels c on c.id=m.channel_id where m.channel_id=?uuid and m.user_id=?uuid and (c.circle_id is null or exists(select 1 from circle_memberships cm where cm.circle_id=c.circle_id and cm.user_id=m.user_id)) and not exists(select 1 from personal_spaces p where p.channel_id=c.id) and not exists(select 1 from direct_conversations d where d.channel_id=c.id) and not exists(select 1 from direct_group_conversations d where d.channel_id=c.id)",&[channel.into(),actor.into()]).await?;
        r.first()
            .map(|v| value(v, "role"))
            .ok_or(RepositoryError::PermissionDenied)
    }
    async fn manager(&self, actor: &str, channel: &str, id: &str) -> Result<()> {
        self.require_enabled(actor, channel).await?;
        let role = self.access(actor, channel).await?;
        let r=self.query("select cast(creator_id as text) as creator_id from channel_events where channel_id=?uuid and id=?",&[channel.into(),id.into()]).await?;
        let e = r.first().ok_or(RepositoryError::NotFound)?;
        if value(e, "creator_id") != actor && role != "owner" && role != "moderator" {
            return Err(RepositoryError::PermissionDenied);
        }
        Ok(())
    }
    fn event_value(r: &Record, actor: &str, role: &str) -> Result<Value> {
        let mut out = json!({});
        for k in [
            "id",
            "channel_id",
            "creator_id",
            "title",
            "code",
            "starts_at",
            "timezone",
            "description",
            "updated_at",
            "my_rsvp",
        ] {
            out[k] = json!(value(r, k));
        }
        for k in ["ends_at", "location", "cover_media_id"] {
            out[k] = optional(r, k);
        }
        let revision = value(r, "revision").parse::<i64>().map_err(storage)?;
        out["revision"] = json!(revision);
        out["cancelled"] = json!(value(r, "cancelled") == "1");
        let enabled = matches!(value(r, "events_enabled").as_str(), "1" | "true");
        out["events_enabled"] = json!(enabled);
        out["can_manage"] = json!(
            enabled && (value(r, "creator_id") == actor || role == "owner" || role == "moderator")
        );
        out["needs_attention"] = json!(
            matches!(value(r, "my_rsvp").as_str(), "attending" | "following")
                && value(r, "seen_revision").parse::<i64>().unwrap_or(revision) < revision
        );
        out["images"] = json!([]);
        out["images_next_after"] = Value::Null;
        out["aliases"] = json!([]);
        Ok(out)
    }
    async fn summaries(
        &self,
        actor: &str,
        channel: &str,
        clause: &str,
        extra: Vec<String>,
    ) -> Result<Vec<Value>> {
        let role = self.access(actor, channel).await?;
        let mut args = vec![actor.into(), channel.into()];
        args.extend(extra);
        let statement = format!(
            "select cast((select coalesce(c.events_enabled,exists(select 1 from channel_events ce where ce.channel_id=c.id)) from channels c where c.id=e.channel_id) as text) as events_enabled,e.id,cast(e.channel_id as text) as channel_id,cast(e.creator_id as text) as creator_id,e.title,e.code,e.starts_at,e.ends_at,e.timezone,e.location,e.description,cast(e.cancelled as text) as cancelled,cast(e.revision as text) as revision,e.updated_at,cast(e.cover_media_id as text) as cover_media_id,coalesce(r.status,'unanswered') as my_rsvp,cast(r.seen_revision as text) as seen_revision from channel_events e left join channel_event_responses r on r.event_id=e.id and r.user_id=?uuid where e.channel_id=?uuid and {clause}"
        );
        let rows = self.query(&statement, &args).await?;
        let mut events = rows
            .iter()
            .map(|r| Self::event_value(r, actor, &role))
            .collect::<Result<Vec<_>>>()?;
        if !events.is_empty() {
            let ids = events
                .iter()
                .map(|e| e["id"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>();
            let placeholders = vec!["?"; ids.len()].join(",");
            let aliases=self.query(&format!("select event_id,code from channel_event_codes where event_id in ({placeholders}) order by code limit 20200"),&ids).await?;
            for e in &mut events {
                e["aliases"] = json!(
                    aliases
                        .iter()
                        .filter(|r| value(r, "event_id") == e["id"].as_str().unwrap())
                        .map(|r| value(r, "code"))
                        .collect::<Vec<_>>()
                );
            }
        }
        self.access(actor, channel).await?;
        Ok(events)
    }
    pub(crate) async fn detail(&self, actor: &str, channel: &str, id: &str) -> Result<Value> {
        let mut events = self
            .summaries(actor, channel, "e.id=?", vec![id.into()])
            .await?;
        let mut event = events.pop().ok_or(RepositoryError::NotFound)?;
        let library = self.images(actor, channel, id, "").await?;
        event["images"] = library["images"].clone();
        event["images_next_after"] = library["next_after"].clone();
        Ok(event)
    }
    pub(crate) async fn images(
        &self,
        actor: &str,
        channel: &str,
        id: &str,
        after: &str,
    ) -> Result<Value> {
        self.access(actor, channel).await?;
        let images=self.query("select cast(i.media_id as text) as media_id,cast(i.contributor_id as text) as contributor_id from channel_event_images i join channel_events e on e.id=i.event_id where i.event_id=? and e.channel_id=?uuid and cast(i.media_id as text)>? order by i.media_id limit 201",&[id.into(),channel.into(),after.into()]).await?;
        self.access(actor, channel).await?;
        Ok(
            json!({"images":images.iter().take(200).map(|r|json!({"media_id":value(r,"media_id"),"contributor_id":value(r,"contributor_id")})).collect::<Vec<_>>(),"next_after":if images.len()>200{Some(value(&images[199],"media_id"))}else{None}}),
        )
    }
    async fn attention_count(&self, actor: &str, channel: &str) -> Result<i64> {
        let rows=self.query("select cast(count(*) as text) as count from channel_events e join channel_event_responses r on r.event_id=e.id where e.channel_id=?uuid and r.user_id=?uuid and r.status in ('attending','following') and r.seen_revision<e.revision",&[channel.into(),actor.into()]).await?;
        value(&rows[0], "count").parse().map_err(storage)
    }
    pub(crate) async fn list(&self, actor: &str, channel: &str, after: &str) -> Result<Value> {
        if !self.availability(actor, channel).await? {
            return Ok(
                json!({"events":[],"next_after":null,"attention_count":0,"events_enabled":false}),
            );
        }
        let cursor = if after.is_empty() {
            None
        } else {
            Some(EventCursor::decode(after)?)
        };
        let as_of = cursor.as_ref().map(|c| c.as_of.clone()).unwrap_or_else(now);
        let bucket = "(case when e.cancelled<>0 then 2 when coalesce(e.ends_at,e.starts_at)>? then 0 else 1 end)";
        let (condition, mut args) = if let Some(cursor) = cursor {
            let compare = if cursor.bucket == 0 { ">" } else { "<" };
            (
                format!(
                    "({bucket}>cast(? as integer) or ({bucket}=cast(? as integer) and (e.starts_at{compare}? or (e.starts_at=? and e.id>?))))"
                ),
                vec![
                    as_of.clone(),
                    cursor.bucket.to_string(),
                    as_of.clone(),
                    cursor.bucket.to_string(),
                    cursor.starts_at.clone(),
                    cursor.starts_at,
                    cursor.id,
                ],
            )
        } else {
            ("true".into(), Vec::new())
        };
        let clause = format!(
            "{condition} order by {bucket},case when e.cancelled=0 and coalesce(e.ends_at,e.starts_at)>? then e.starts_at end asc,case when e.cancelled<>0 or coalesce(e.ends_at,e.starts_at)<=? then e.starts_at end desc,e.id asc limit 101"
        );
        args.extend([as_of.clone(), as_of.clone(), as_of.clone()]);
        let mut events = self.summaries(actor, channel, &clause, args).await?;
        let next = if events.len() > 100 {
            let event = &events[99];
            let start = event["starts_at"].as_str().unwrap();
            let end = event["ends_at"].as_str().unwrap_or(start);
            let bucket = if event["cancelled"] == true {
                2
            } else if end > as_of.as_str() {
                0
            } else {
                1
            };
            Some(
                EventCursor {
                    v: 1,
                    as_of,
                    bucket,
                    starts_at: start.into(),
                    id: event["id"].as_str().unwrap().into(),
                }
                .encode()?,
            )
        } else {
            None
        };
        events.truncate(100);
        let attention = self.attention_count(actor, channel).await?;
        self.access(actor, channel).await?;
        if !self.availability(actor, channel).await? {
            return Ok(
                json!({"events":[],"next_after":null,"attention_count":0,"events_enabled":false}),
            );
        }
        Ok(
            json!({"events":events,"next_after":next,"attention_count":attention,"events_enabled":true}),
        )
    }
    pub(crate) async fn suggestions(
        &self,
        actor: &str,
        channel: &str,
        prefix: &str,
    ) -> Result<Value> {
        if !self.availability(actor, channel).await? {
            return Ok(json!({"events":[],"next_after":null,"events_enabled":false}));
        }
        let prefix = prefix.to_ascii_lowercase();
        if prefix.len() > 48
            || !prefix
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        {
            return Err(RepositoryError::Conflict);
        }
        // The alias primary key supplies a bounded index range, including old
        // codes. Empty prefix still reads at most twelve alias rows.
        let upper = format!("{prefix}~");
        let events=self.summaries(actor,channel,"e.id in (select event_id from channel_event_codes where channel_id=?uuid and code>=? and code<? order by code limit 12) order by e.code",vec![channel.into(),prefix,upper]).await?;
        if !self.availability(actor, channel).await? {
            return Ok(json!({"events":[],"next_after":null,"events_enabled":false}));
        }
        Ok(json!({"events":events,"next_after":null,"events_enabled":true}))
    }
    pub(crate) async fn save(
        &self,
        actor: &str,
        channel: &str,
        id: Option<&str>,
        mut input: EventInput,
    ) -> Result<Value> {
        self.require_enabled(actor, channel).await?;
        input.validate()?;
        self.access(actor, channel).await?;
        let event_id = id
            .map(str::to_owned)
            .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
        let fields = vec![
            input.title,
            input.code,
            input.starts_at.to_rfc3339_opts(SecondsFormat::Millis, true),
            input
                .ends_at
                .map(|v| v.to_rfc3339_opts(SecondsFormat::Millis, true))
                .unwrap_or_default(),
            input.timezone,
            input.location.unwrap_or_default(),
            input.description,
            now(),
        ];
        if id.is_some() {
            self.manager(actor, channel, &event_id).await?;
            let mut args = fields.clone();
            args.extend([
                fields[2].clone(),
                fields[3].clone(),
                fields[4].clone(),
                fields[5].clone(),
                channel.into(),
                event_id.clone(),
            ]);
            let changed=self.mutate("update channel_events set title=?,code=?,starts_at=?,ends_at=nullif(?,''),timezone=?,location=nullif(?,''),description=?,updated_at=?,revision=revision+case when starts_at<>? or coalesce(ends_at,'')<>? or timezone<>? or coalesce(location,'')<>? then 1 else 0 end where channel_id=?uuid and id=? and /*ACCESS*/",args,actor,channel,Some(&event_id),true).await?;
            if changed == 0 {
                return Err(RepositoryError::PermissionDenied);
            }
        } else {
            let mut args = vec![event_id.clone(), channel.into(), actor.into()];
            args.extend(fields);
            self.mutate("insert into channel_events(id,channel_id,creator_id,title,code,starts_at,ends_at,timezone,location,description,updated_at) select ?,?uuid,?uuid,?,?,?,nullif(?,''),?,nullif(?,''),?,? where /*ACCESS*/",args,actor,channel,None,false).await?;
        }
        self.detail(actor, channel, &event_id).await
    }
    pub(crate) async fn cancel(&self, actor: &str, channel: &str, id: &str) -> Result<Value> {
        self.manager(actor, channel, id).await?;
        let changed=self.mutate("update channel_events set cancelled=1,revision=revision+1,updated_at=? where id=? and channel_id=?uuid and cancelled=0 and /*ACCESS*/",vec![now(),id.into(),channel.into()],actor,channel,Some(id),true).await?;
        if changed == 0 {
            self.manager(actor, channel, id).await?;
        }
        self.detail(actor, channel, id).await
    }
    pub(crate) async fn rsvp(
        &self,
        actor: &str,
        channel: &str,
        id: &str,
        status: &str,
    ) -> Result<Value> {
        self.require_enabled(actor, channel).await?;
        if !["attending", "declined", "following", "unanswered"].contains(&status) {
            return Err(RepositoryError::Conflict);
        }
        if status == "unanswered" {
            self.mutate("delete from channel_event_responses where event_id=? and user_id=?uuid and exists(select 1 from channel_events where id=? and channel_id=?uuid) and /*ACCESS*/",vec![id.into(),actor.into(),id.into(),channel.into()],actor,channel,Some(id),false).await?;
        } else {
            let count=self.mutate("insert into channel_event_responses(event_id,user_id,status,updated_at,seen_revision) select e.id,?uuid,?,?,e.revision from channel_events e where e.id=? and e.channel_id=?uuid and e.cancelled=0 and /*ACCESS*/ on conflict(event_id,user_id) do update set status=excluded.status,updated_at=excluded.updated_at,seen_revision=excluded.seen_revision",vec![actor.into(),status.into(),now(),id.into(),channel.into()],actor,channel,Some(id),false).await?;
            if count == 0 {
                return Err(RepositoryError::Conflict);
            }
        }
        self.detail(actor, channel, id).await
    }
    pub(crate) async fn responses(
        &self,
        actor: &str,
        channel: &str,
        id: &str,
        after: &str,
    ) -> Result<Value> {
        self.detail(actor, channel, id).await?;
        // Current humans default to unanswered; former members only retain history.
        let candidates = "with candidates as (select m.user_id from channel_memberships m join channels c on c.id=m.channel_id join users u on u.id=m.user_id where m.channel_id=?uuid and u.kind='human' and (c.circle_id is null or exists(select 1 from circle_memberships cm where cm.circle_id=c.circle_id and cm.user_id=m.user_id)) union select r.user_id from channel_event_responses r join channel_events e on e.id=r.event_id join users u on u.id=r.user_id where r.event_id=? and e.channel_id=?uuid and coalesce(e.ends_at,e.starts_at)<=? and u.kind='human')";
        let base_args = vec![channel.into(), id.into(), channel.into(), now()];
        let mut args = base_args.clone();
        args.extend([id.into(), after.into()]);
        let rows=self.query(&format!("{candidates} select cast(u.id as text) as user_id,u.display_name,coalesce(r.status,'unanswered') as status from candidates a join users u on u.id=a.user_id left join channel_event_responses r on r.user_id=a.user_id and r.event_id=? where cast(u.id as text)>? order by u.id limit 201"),&args).await?;
        let mut args = base_args;
        args.push(id.into());
        let totals=self.query(&format!("{candidates} select coalesce(r.status,'unanswered') as status,cast(count(*) as text) as count from candidates a left join channel_event_responses r on r.user_id=a.user_id and r.event_id=? group by coalesce(r.status,'unanswered')"),&args).await?;
        let mut counts = json!({"attending":0,"declined":0,"following":0,"unanswered":0});
        for row in totals {
            counts[value(&row, "status")] =
                json!(value(&row, "count").parse::<i64>().map_err(storage)?);
        }
        self.access(actor, channel).await?;
        Ok(
            json!({"responses":rows.iter().take(200).map(|r|json!({"user_id":value(r,"user_id"),"display_name":value(r,"display_name"),"status":value(r,"status")})).collect::<Vec<_>>(),"counts":counts,"next_after":if rows.len()>200{Some(value(&rows[199],"user_id"))}else{None}}),
        )
    }
    pub(crate) async fn image(
        &self,
        actor: &str,
        channel: &str,
        id: &str,
        media: &str,
        remove: bool,
    ) -> Result<Value> {
        self.require_enabled(actor, channel).await?;
        self.detail(actor, channel, id).await?;
        if remove {
            let manager = self.manager(actor, channel, id).await.is_ok();
            self.mutate("delete from channel_event_images where event_id=? and media_id=?uuid and (contributor_id=?uuid or ?='yes') and /*ACCESS*/",vec![id.into(),media.into(),actor.into(),if manager{"yes"}else{"no"}.into()],actor,channel,Some(id),manager).await?;
        } else {
            let n=self.mutate("insert into channel_event_images(event_id,media_id,contributor_id) select e.id,m.id,?uuid from channel_events e join media_objects m on m.channel_id=e.channel_id where e.id=? and e.channel_id=?uuid and m.id=?uuid and m.owner_id=?uuid and m.content_type in ('image/jpeg','image/png','image/webp','image/gif') and /*ACCESS*/ on conflict do nothing",vec![actor.into(),id.into(),channel.into(),media.into(),actor.into()],actor,channel,Some(id),false).await?;
            if n==0 && self.query("select event_id from channel_event_images where event_id=? and media_id=?uuid",&[id.into(),media.into()]).await?.is_empty() {return Err(RepositoryError::PermissionDenied)}
        }
        self.detail(actor, channel, id).await
    }
    pub(crate) async fn cover(
        &self,
        actor: &str,
        channel: &str,
        id: &str,
        media: Option<&str>,
    ) -> Result<Value> {
        self.manager(actor, channel, id).await?;
        if let Some(media) = media {
            let n=self.mutate("update channel_events set cover_media_id=?uuid,updated_at=? where id=? and channel_id=?uuid and exists(select 1 from channel_event_images where event_id=? and media_id=?uuid) and /*ACCESS*/",vec![media.into(),now(),id.into(),channel.into(),id.into(),media.into()],actor,channel,Some(id),true).await?;
            if n == 0 {
                return Err(RepositoryError::Conflict);
            }
        } else {
            let changed=self.mutate("update channel_events set cover_media_id=null,updated_at=? where id=? and channel_id=?uuid and /*ACCESS*/",vec![now(),id.into(),channel.into()],actor,channel,Some(id),true).await?;
            if changed == 0 {
                return Err(RepositoryError::PermissionDenied);
            }
        }
        self.detail(actor, channel, id).await
    }
    pub(crate) async fn references(
        &self,
        actor: &str,
        channel: &str,
        codes: Vec<String>,
    ) -> Result<Value> {
        self.access(actor, channel).await?;
        if codes.len() > 50 {
            return Err(RepositoryError::Conflict);
        }
        let codes = codes
            .into_iter()
            .map(|c| c.to_ascii_lowercase())
            .filter(|c| valid_code(c))
            .collect::<Vec<_>>();
        if codes.is_empty() {
            return Ok(json!({"events":[]}));
        }
        let placeholders = vec!["?"; codes.len()].join(",");
        let events=self.summaries(actor,channel,&format!("exists(select 1 from channel_event_codes a where a.event_id=e.id and a.code in ({placeholders})) order by e.id"),codes).await?;
        Ok(json!({"events":events}))
    }
    pub(crate) async fn acknowledge(&self, actor: &str, channel: &str, id: &str) -> Result<Value> {
        self.access(actor, channel).await?;
        self.mutate("update channel_event_responses set seen_revision=(select revision from channel_events where id=?) where event_id=? and user_id=?uuid and exists(select 1 from channel_events where id=? and channel_id=?uuid) and /*ACCESS*/",vec![id.into(),id.into(),actor.into(),id.into(),channel.into()],actor,channel,Some(id),false).await?;
        self.detail(actor, channel, id).await
    }
    pub(crate) async fn codes(&self, actor: &str, channel: &str, id: &str) -> Result<Vec<String>> {
        self.detail(actor, channel, id).await?;
        Ok(self.query("select code from channel_event_codes where channel_id=?uuid and event_id=? order by code limit 1000",&[channel.into(),id.into()]).await?.iter().map(|r|value(r,"code")).collect())
    }
}

// Read actual text, including writes from older production binaries. Retired codes
// remain reserved aliases, so old references retain identity after event renames.
pub(crate) fn reference_codes(body: &str) -> Vec<String> {
    use pulldown_cmark::{Event, Options, Parser, Tag};
    let mut out = Vec::new();
    let mut excluded = 0usize;
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut stack = Vec::new();
    for (event, range) in Parser::new_ext(body, options).into_offset_iter() {
        match event {
            Event::Start(tag) => {
                let skip = matches!(
                    tag,
                    Tag::CodeBlock(_) | Tag::Link { .. } | Tag::Image { .. } | Tag::HtmlBlock
                );
                stack.push(skip);
                if skip {
                    excluded += 1;
                }
            }
            Event::End(_) => {
                if stack.pop().unwrap_or(false) {
                    excluded = excluded.saturating_sub(1);
                }
            }
            Event::Text(_) if excluded == 0 => {
                // Inspect source offsets, not unescaped parser text: \#arr is literal.
                let absolute_start = range.start;
                let absolute_end = range.end;
                let raw = &body[range];
                let bytes = raw.as_bytes();
                let mut i = 0;
                while i + 5 <= bytes.len() {
                    if bytes[i..i + 5].eq_ignore_ascii_case(b"#arr:") {
                        let before = raw[..i].chars().next_back();
                        let escaped_start = i == 0 && body[..absolute_start].ends_with('\\');
                        let boundary = !escaped_start
                            && before
                                .is_none_or(|c| c.is_whitespace() || matches!(c, '(' | '[' | '{'));
                        let start = i + 5;
                        let mut end = start;
                        while end < bytes.len()
                            && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'-')
                        {
                            end += 1;
                        }
                        let following = raw[end..]
                            .chars()
                            .next()
                            .or_else(|| body[absolute_end..].chars().next());
                        let complete = following.is_none_or(|c| {
                            !c.is_alphanumeric() && c != '_' && c != '-' && c != '&'
                        });
                        let code = raw[start..end].to_ascii_lowercase();
                        if boundary && complete && valid_code(&code) && !out.contains(&code) {
                            out.push(code);
                        }
                        i = end.max(i + 1);
                    } else {
                        i += 1;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn references_skip_code_and_escaped_text() {
        assert_eq!(
            reference_codes("#arr:gig `#arr:no` \\#arr:no2\n\n```\n#arr:no3\n```\n#arr:HELLO"),
            vec!["gig", "hello"]
        );
    }
    #[test]
    fn references_skip_all_markdown_code_forms() {
        assert_eq!(
            reference_codes(
                "~~~rust\n#arr:hidden\n~~~\n    #arr:indented\n\t#arr:tab\n#arr:visible\n"
            ),
            vec!["visible"]
        );
    }
    #[test]
    fn list_cursor_requires_canonical_utc_and_identity() {
        let mut cursor = EventCursor {
            v: 1,
            as_of: "2026-11-07T12:00:00.000Z".into(),
            bucket: 0,
            starts_at: "2026-11-08T12:00:00.000Z".into(),
            id: "abcdefab-cdef-4abc-8abc-abcdefabcdef".into(),
        };
        assert!(EventCursor::decode(&cursor.encode().unwrap()).is_ok());
        cursor.as_of = "2026-11-07T13:00:00.000+01:00".into();
        assert!(EventCursor::decode(&cursor.encode().unwrap()).is_err());
        cursor.as_of = "2026-11-07T12:00:00.000Z".into();
        cursor.id.make_ascii_uppercase();
        assert!(EventCursor::decode(&cursor.encode().unwrap()).is_err());
    }
    #[test]
    fn event_validation() {
        let mut e = EventInput {
            title: "Gig".into(),
            code: "GIG".into(),
            starts_at: Utc::now(),
            ends_at: None,
            timezone: "Europe/Oslo".into(),
            location: None,
            description: String::new(),
        };
        assert!(e.validate().is_ok());
        e.timezone = "Oslo".into();
        assert!(e.validate().is_err());
    }
}

#[cfg(test)]
mod database_tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    async fn fixture() -> (EventStore, String, String, String) {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .in_memory(true)
                    .foreign_keys(true),
            )
            .await
            .unwrap();
        sqlx::migrate!("./migrations/sqlite")
            .run(&pool)
            .await
            .unwrap();
        seed(EventStore(TopicStore::Sqlite(pool))).await
    }
    async fn seed(s: EventStore) -> (EventStore, String, String, String) {
        let owner = uuid::Uuid::now_v7().to_string();
        let member = uuid::Uuid::now_v7().to_string();
        let channel = uuid::Uuid::now_v7().to_string();
        for id in [&owner, &member] {
            s.execute(
                "insert into users(id,kind,display_name) values(?uuid,'human','Event test')",
                std::slice::from_ref(id),
            )
            .await
            .unwrap();
        }
        s.execute("insert into channels(id,slug,name,kind,created_by) values(?uuid,?,'Events','private',?uuid)",&[channel.clone(),format!("events-{channel}"),owner.clone()]).await.unwrap();
        for (id, role) in [(&owner, "owner"), (&member, "member")] {
            s.execute(
                "insert into channel_memberships(channel_id,user_id,role) values(?uuid,?uuid,?)",
                &[channel.clone(), id.clone(), role.into()],
            )
            .await
            .unwrap();
        }
        s.execute(
            "update channels set events_enabled=true where id=?uuid",
            std::slice::from_ref(&channel),
        )
        .await
        .unwrap();
        (s, owner, member, channel)
    }
    fn input(code: &str, days: i64) -> EventInput {
        EventInput {
            title: "Concert".into(),
            code: code.into(),
            starts_at: Utc::now() + chrono::Duration::days(days),
            ends_at: None,
            timezone: "Europe/Oslo".into(),
            location: None,
            description: "**A concert**".into(),
        }
    }
    async fn aliases_rights_material_notice_and_acknowledgment_contract(
        setup: (EventStore, String, String, String),
    ) {
        let (s, owner, member, c) = setup;
        let e = s
            .save(&member, &c, None, input("concert", 2))
            .await
            .unwrap();
        let id = e["id"].as_str().unwrap();
        assert_eq!(e["can_manage"], true);
        s.rsvp(&owner, &c, id, "following").await.unwrap();
        let mut renamed = input("gig", 2);
        renamed.starts_at = DateTime::parse_from_rfc3339(e["starts_at"].as_str().unwrap())
            .unwrap()
            .with_timezone(&Utc);
        s.save(&member, &c, Some(id), renamed).await.unwrap();
        assert_eq!(
            s.detail(&owner, &c, id).await.unwrap()["needs_attention"],
            false
        );
        let refs = s
            .references(&owner, &c, vec!["concert".into()])
            .await
            .unwrap();
        assert_eq!(refs["events"][0]["id"], id);
        assert!(s.save(&owner, &c, None, input("concert", 3)).await.is_err());
        s.save(&member, &c, Some(id), input("gig", 4))
            .await
            .unwrap();
        assert_eq!(
            s.detail(&owner, &c, id).await.unwrap()["needs_attention"],
            true
        );
        let ack = s.acknowledge(&owner, &c, id).await.unwrap();
        assert_eq!(ack["needs_attention"], false);
        assert_eq!(ack["my_rsvp"], "following");
        s.cancel(&member, &c, id).await.unwrap();
        assert_eq!(
            s.detail(&owner, &c, id).await.unwrap()["needs_attention"],
            true
        );
        assert!(s.rsvp(&owner, &c, id, "attending").await.is_err());
    }
    async fn old_membership_sql_cleans_future_only_and_rejoin_does_not_restore_contract(
        setup: (EventStore, String, String, String),
    ) {
        let (s, owner, member, c) = setup;
        let future = s.save(&owner, &c, None, input("future", 2)).await.unwrap();
        let past = s.save(&owner, &c, None, input("past", -2)).await.unwrap();
        let future = future["id"].as_str().unwrap();
        let past = past["id"].as_str().unwrap();
        s.rsvp(&member, &c, future, "attending").await.unwrap();
        s.rsvp(&member, &c, past, "following").await.unwrap();
        s.execute(
            "update channels set events_enabled=false where id=?uuid",
            std::slice::from_ref(&c),
        )
        .await
        .unwrap();
        // The old binary knows nothing about events: this is its unchanged delete.
        s.execute(
            "delete from channel_memberships where channel_id=?uuid and user_id=?uuid",
            &[c.clone(), member.clone()],
        )
        .await
        .unwrap();
        assert!(s.rsvp(&member, &c, future, "attending").await.is_err());
        assert!(s.detail(&member, &c, past).await.is_err());
        let future_roster = s.responses(&owner, &c, future, "").await.unwrap();
        assert_eq!(future_roster["responses"].as_array().unwrap().len(), 1);
        assert_eq!(future_roster["counts"]["unanswered"], 1);
        let past_roster = s.responses(&owner, &c, past, "").await.unwrap();
        assert_eq!(past_roster["responses"].as_array().unwrap().len(), 2);
        assert_eq!(past_roster["counts"]["following"], 1);

        s.execute(
            "insert into channel_memberships(channel_id,user_id,role) values(?uuid,?uuid,'member')",
            &[c.clone(), member.clone()],
        )
        .await
        .unwrap();
        assert_eq!(
            s.detail(&member, &c, future).await.unwrap()["my_rsvp"],
            "unanswered"
        );
        assert_eq!(
            s.detail(&member, &c, past).await.unwrap()["my_rsvp"],
            "following"
        );
        s.execute(
            "update channels set events_enabled=true where id=?uuid",
            std::slice::from_ref(&c),
        )
        .await
        .unwrap();
        s.rsvp(&member, &c, future, "attending").await.unwrap();
        s.rsvp(&member, &c, future, "attending").await.unwrap();
        let responses = s.responses(&owner, &c, future, "").await.unwrap();
        assert_eq!(responses["counts"]["attending"], 1);
        assert_eq!(responses["counts"]["unanswered"], 1);
        s.rsvp(&member, &c, future, "unanswered").await.unwrap();
        assert_eq!(
            s.detail(&member, &c, future).await.unwrap()["my_rsvp"],
            "unanswered"
        );
    }
    async fn first_event_enables_topics_only_once_contract(
        setup: (EventStore, String, String, String),
    ) {
        let (s, owner, _, c) = setup;
        s.save(&owner, &c, None, input("one", 1)).await.unwrap();
        s.execute(
            "update channel_topic_settings set enabled=false where channel_id=?uuid",
            std::slice::from_ref(&c),
        )
        .await
        .unwrap();
        s.save(&owner, &c, None, input("two", 2)).await.unwrap();
        let r=s.query("select cast(enabled as text) as enabled from channel_topic_settings where channel_id=?uuid",&[c]).await.unwrap();
        assert!(["0", "false"].contains(&value(&r[0], "enabled").as_str()));
    }
    async fn image_channel_owner_and_cover_guards_contract(
        setup: (EventStore, String, String, String),
    ) {
        let (s, owner, member, c) = setup;
        let event = s.save(&owner, &c, None, input("images", 2)).await.unwrap();
        let id = event["id"].as_str().unwrap();
        let media = uuid::Uuid::now_v7().to_string();
        s.execute("insert into media_objects(id,owner_id,channel_id,storage_key,original_filename,content_type,size_bytes,sha256,created_at) values(?uuid,?uuid,?uuid,?,'image.png','image/png',1,'test',current_timestamp)",&[media.clone(),member.clone(),c.clone(),media.clone()]).await.unwrap();
        assert!(s.image(&owner, &c, id, &media, false).await.is_err());
        s.image(&member, &c, id, &media, false).await.unwrap();
        assert!(s.cover(&member, &c, id, Some(&media)).await.is_err());
        assert_eq!(
            s.cover(&owner, &c, id, Some(&media)).await.unwrap()["cover_media_id"],
            media
        );
        let removed = s.image(&member, &c, id, &media, true).await.unwrap();
        assert!(removed["cover_media_id"].is_null());
    }
    async fn prefix_suggestions_beyond_first_page_contract(
        setup: (EventStore, String, String, String),
    ) {
        let (s, owner, _, c) = setup;
        for n in 0..101 {
            s.save(&owner, &c, None, input(&format!("event-{n:03}"), 1))
                .await
                .unwrap();
        }
        let event = s.save(&owner, &c, None, input("zebra", 2)).await.unwrap();
        let id = event["id"].as_str().unwrap();
        s.save(&owner, &c, Some(id), input("renamed-zebra", 2))
            .await
            .unwrap();
        let first = s.list(&owner, &c, "").await.unwrap();
        assert_eq!(first["events"].as_array().unwrap().len(), 100);
        assert!(
            !first["events"]
                .as_array()
                .unwrap()
                .iter()
                .any(|event| event["id"] == id)
        );
        let matched = s.suggestions(&owner, &c, "ZEB").await.unwrap();
        assert_eq!(matched["events"].as_array().unwrap().len(), 1);
        assert_eq!(matched["events"][0]["id"], id);
        assert!(s.suggestions(&owner, &c, "%bad").await.is_err());
        assert!(
            s.suggestions(&owner, &c, "").await.unwrap()["events"]
                .as_array()
                .unwrap()
                .len()
                <= 12
        );
    }
    async fn globally_ordered_pagination_contract(setup: (EventStore, String, String, String)) {
        let (s, owner, _, c) = setup;
        let past_start = Utc::now() - chrono::Duration::days(2);
        let mut expected = std::collections::HashSet::new();
        for n in 0..101 {
            let mut event = input(&format!("past-{n:03}"), -2);
            event.starts_at = past_start;
            let event = s.save(&owner, &c, None, event).await.unwrap();
            expected.insert(event["id"].as_str().unwrap().to_owned());
        }
        let first = s.save(&owner, &c, None, input("soon", 1)).await.unwrap();
        let second = s.save(&owner, &c, None, input("later", 2)).await.unwrap();
        let cancelled = s
            .save(&owner, &c, None, input("cancelled", 1))
            .await
            .unwrap();
        let cancelled_id = cancelled["id"].as_str().unwrap();
        s.rsvp(&owner, &c, cancelled_id, "following").await.unwrap();
        s.cancel(&owner, &c, cancelled_id).await.unwrap();
        for event in [&first, &second, &cancelled] {
            expected.insert(event["id"].as_str().unwrap().to_owned());
        }
        let page = s.list(&owner, &c, "").await.unwrap();
        assert_eq!(page["events"][0]["id"], first["id"]);
        assert_eq!(page["events"][1]["id"], second["id"]);
        assert_eq!(page["attention_count"], 1);
        assert!(
            !page["events"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["id"] == cancelled_id)
        );
        let mut seen = Vec::new();
        let mut after = String::new();
        for _ in 0..10 {
            let page = s.list(&owner, &c, &after).await.unwrap();
            seen.extend(
                page["events"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|e| e["id"].as_str().unwrap().to_owned()),
            );
            match page["next_after"].as_str() {
                Some(cursor) => after = cursor.into(),
                None => break,
            }
        }
        assert_eq!(seen.len(), expected.len());
        assert_eq!(
            seen.iter()
                .cloned()
                .collect::<std::collections::HashSet<_>>(),
            expected
        );
        assert_eq!(seen.last().unwrap(), cancelled_id);
        assert!(s.list(&owner, &c, "not-a-valid-cursor").await.is_err());
    }
    async fn availability_archive_and_rolling_writer_contract(
        setup: (EventStore, String, String, String),
    ) {
        let (s, owner, member, c) = setup;
        s.execute(
            "update channels set events_enabled=null where id=?uuid",
            std::slice::from_ref(&c),
        )
        .await
        .unwrap();
        assert!(!s.availability(&owner, &c).await.unwrap());
        assert_eq!(
            s.save(&owner, &c, None, input("new-denied", 2))
                .await
                .unwrap_err(),
            RepositoryError::EventsDisabled
        );
        let id = uuid::Uuid::now_v7().to_string();
        let starts =
            (Utc::now() + chrono::Duration::days(2)).to_rfc3339_opts(SecondsFormat::Millis, true);
        // Unchanged 417 SQL can still create during overlap. NULL derives ON;
        // an explicit false must stay false even after another old-writer insert.
        let old_insert = "insert into channel_events(id,channel_id,creator_id,title,code,starts_at,timezone,description,updated_at) values(?,?uuid,?uuid,'Old writer',?,?,'Europe/Oslo','Retained Markdown',?)";
        s.execute(
            old_insert,
            &[
                id.clone(),
                c.clone(),
                owner.clone(),
                "old-writer".into(),
                starts.clone(),
                now(),
            ],
        )
        .await
        .unwrap();
        assert!(s.availability(&owner, &c).await.unwrap());
        s.rsvp(&member, &c, &id, "attending").await.unwrap();
        let media = uuid::Uuid::now_v7().to_string();
        s.execute("insert into media_objects(id,owner_id,channel_id,storage_key,original_filename,content_type,size_bytes,sha256,created_at) values(?uuid,?uuid,?uuid,?,'image.png','image/png',1,'test',current_timestamp)",&[media.clone(),member.clone(),c.clone(),media.clone()]).await.unwrap();
        s.image(&member, &c, &id, &media, false).await.unwrap();
        s.cover(&owner, &c, &id, Some(&media)).await.unwrap();
        let before = s.detail(&member, &c, &id).await.unwrap();
        s.execute(
            "update channels set events_enabled=false where id=?uuid",
            std::slice::from_ref(&c),
        )
        .await
        .unwrap();
        let archived = s.detail(&member, &c, &id).await.unwrap();
        assert_eq!(archived["events_enabled"], false);
        assert_eq!(archived["can_manage"], false);
        for key in [
            "id",
            "description",
            "my_rsvp",
            "images",
            "cover_media_id",
            "revision",
            "updated_at",
        ] {
            assert_eq!(archived[key], before[key], "disabled preserves {key}");
        }
        assert_eq!(
            s.references(&member, &c, vec!["old-writer".into()])
                .await
                .unwrap()["events"][0]["id"],
            id
        );
        assert_eq!(
            s.responses(&member, &c, &id, "").await.unwrap()["counts"]["attending"],
            1
        );
        assert!(
            s.list(&member, &c, "").await.unwrap()["events"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(
            s.suggestions(&member, &c, "old").await.unwrap()["events"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            s.save(&owner, &c, None, input("blocked", 2))
                .await
                .unwrap_err(),
            RepositoryError::EventsDisabled
        );
        assert_eq!(
            s.save(&owner, &c, Some(&id), input("old-writer", 2))
                .await
                .unwrap_err(),
            RepositoryError::EventsDisabled
        );
        assert_eq!(
            s.cancel(&owner, &c, &id).await.unwrap_err(),
            RepositoryError::EventsDisabled
        );
        for status in ["attending", "declined", "following", "unanswered"] {
            assert_eq!(
                s.rsvp(&member, &c, &id, status).await.unwrap_err(),
                RepositoryError::EventsDisabled
            );
        }
        assert_eq!(
            s.image(&member, &c, &id, &media, false).await.unwrap_err(),
            RepositoryError::EventsDisabled
        );
        assert_eq!(
            s.image(&member, &c, &id, &media, true).await.unwrap_err(),
            RepositoryError::EventsDisabled
        );
        assert_eq!(
            s.cover(&owner, &c, &id, None).await.unwrap_err(),
            RepositoryError::EventsDisabled
        );
        assert_eq!(
            s.acknowledge(&member, &c, &id).await.unwrap_err(),
            RepositoryError::EventsDisabled
        );
        let second = uuid::Uuid::now_v7().to_string();
        s.execute(
            old_insert,
            &[
                second,
                c.clone(),
                owner.clone(),
                "old-while-off".into(),
                starts,
                now(),
            ],
        )
        .await
        .unwrap();
        assert!(!s.availability(&owner, &c).await.unwrap());
        s.execute(
            "update channels set events_enabled=true where id=?uuid",
            std::slice::from_ref(&c),
        )
        .await
        .unwrap();
        assert_eq!(s.detail(&owner, &c, &id).await.unwrap()["can_manage"], true);
        assert_eq!(
            s.detail(&member, &c, &id).await.unwrap()["my_rsvp"],
            "attending"
        );
        assert_eq!(
            s.list(&owner, &c, "").await.unwrap()["events"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }
    #[tokio::test]
    async fn availability_archive_and_rolling_writer() {
        availability_archive_and_rolling_writer_contract(fixture().await).await;
    }
    #[tokio::test]
    async fn globally_ordered_pagination() {
        globally_ordered_pagination_contract(fixture().await).await;
    }
    #[tokio::test]
    async fn prefix_suggestions_beyond_first_page() {
        prefix_suggestions_beyond_first_page_contract(fixture().await).await;
    }
    #[tokio::test]
    async fn aliases_rights_material_notice_and_acknowledgment() {
        aliases_rights_material_notice_and_acknowledgment_contract(fixture().await).await;
    }
    #[tokio::test]
    async fn old_membership_sql_cleans_future_only_and_rejoin_does_not_restore() {
        old_membership_sql_cleans_future_only_and_rejoin_does_not_restore_contract(fixture().await)
            .await;
    }
    #[tokio::test]
    async fn first_event_enables_topics_only_once() {
        first_event_enables_topics_only_once_contract(fixture().await).await;
    }
    #[tokio::test]
    async fn image_channel_owner_and_cover_guards() {
        image_channel_owner_and_cover_guards_contract(fixture().await).await;
    }

    #[tokio::test]
    async fn postgres_channel_events_contract() {
        let Ok(url) = std::env::var("SPROYT_POSTGRES_TEST_URL") else {
            return;
        };
        let admin = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .unwrap();
        let schema = format!("event_contract_{}", uuid::Uuid::now_v7().simple());
        sqlx::query(&format!("create schema {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        let search_path = format!("set search_path to {schema}");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(3)
            .after_connect(move |connection, _| {
                let statement = search_path.clone();
                Box::pin(async move {
                    sqlx::query(&statement).execute(connection).await?;
                    Ok(())
                })
            })
            .connect(&url)
            .await
            .unwrap();
        sqlx::migrate!("./migrations/postgres")
            .run(&pool)
            .await
            .unwrap();
        let s = EventStore(TopicStore::Postgres(pool.clone()));
        aliases_rights_material_notice_and_acknowledgment_contract(seed(s.clone()).await).await;
        old_membership_sql_cleans_future_only_and_rejoin_does_not_restore_contract(
            seed(s.clone()).await,
        )
        .await;
        first_event_enables_topics_only_once_contract(seed(s.clone()).await).await;
        image_channel_owner_and_cover_guards_contract(seed(s.clone()).await).await;
        prefix_suggestions_beyond_first_page_contract(seed(s.clone()).await).await;
        globally_ordered_pagination_contract(seed(s.clone()).await).await;
        availability_archive_and_rolling_writer_contract(seed(s.clone()).await).await;

        pool.close().await;
        sqlx::query(&format!("drop schema {schema} cascade"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
    }
}

#[cfg(test)]
mod reference_corpus_tests {
    #[test]
    fn shared_markdown_reference_corpus() {
        let cases: serde_json::Value =
            serde_json::from_str(include_str!("../docs/fixtures/event-references.json")).unwrap();
        for case in cases.as_array().unwrap() {
            let expected = case["codes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_owned())
                .collect::<Vec<_>>();
            assert_eq!(
                super::reference_codes(case["source"].as_str().unwrap()),
                expected,
                "{}",
                case["name"]
            );
        }
    }
}

#[cfg(test)]
mod http_tests {
    use super::*;
    #[tokio::test]
    async fn events_require_current_human_account_and_channel_membership() {
        let auth = crate::auth::AuthService::development();
        let alice = auth
            .authenticate_development(Some("event-http-alice".into()))
            .unwrap()
            .user;
        let bob = auth
            .authenticate_development(Some("event-http-bob".into()))
            .unwrap()
            .user;
        let repository = std::sync::Arc::new(
            crate::db::SqliteChatRepository::connect("sqlite::memory:")
                .await
                .unwrap(),
        );
        repository.migrate().await.unwrap();
        let store = repository.event_store_for_test();
        let TopicStore::Sqlite(pool) = store.0.clone() else {
            unreachable!()
        };
        for user in [&alice, &bob] {
            sqlx::query("insert into users(id,kind,display_name) values(?,'human','Event HTTP')")
                .bind(user.id.to_string())
                .execute(&pool)
                .await
                .unwrap();
        }
        let channel = uuid::Uuid::now_v7().to_string();
        sqlx::query("insert into channels(id,slug,name,kind,created_by) values(?,'event-http','Events','private',?)").bind(&channel).bind(alice.id.to_string()).execute(&pool).await.unwrap();
        sqlx::query("insert into channel_memberships(channel_id,user_id,role) values(?,?,'owner')")
            .bind(&channel)
            .bind(alice.id.to_string())
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("update channels set events_enabled=true where id=?")
            .bind(&channel)
            .execute(&pool)
            .await
            .unwrap();
        let mut state = crate::web::native::test_state(auth, None).await;
        state.topics = Some(TopicStore::Sqlite(pool.clone()));
        state.chat = crate::chat::ChatEngine::start(repository);
        let app = crate::server::build_router(state.clone(), state.operations.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!(
            "http://{}/api/v1/channels/{}/events",
            listener.local_addr().unwrap(),
            channel
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::new();
        let url = format!("{endpoint}?participant=event-http-alice");
        assert_eq!(
            client.get(&url).send().await.unwrap().status(),
            axum::http::StatusCode::FORBIDDEN
        );
        assert_eq!(
            client
                .get(&url)
                .header("x-sproyt-account", bob.id.to_string())
                .send()
                .await
                .unwrap()
                .status(),
            axum::http::StatusCode::FORBIDDEN
        );
        assert_eq!(
            client
                .get(format!("{endpoint}?participant=event-http-bob"))
                .header("x-sproyt-account", bob.id.to_string())
                .send()
                .await
                .unwrap()
                .status(),
            axum::http::StatusCode::FORBIDDEN
        );
        let response = client
            .get(&url)
            .header("x-sproyt-account", alice.id.to_string())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        let event=client.post(&url).header("x-sproyt-account",alice.id.to_string()).json(&json!({"title":"Gig","code":"gig","starts_at":(Utc::now()+chrono::Duration::days(1)).to_rfc3339(),"timezone":"Europe/Oslo","ends_at":null,"location":null,"description":"**Rich**"})).send().await.unwrap();
        assert_eq!(event.status(), axum::http::StatusCode::OK);
        let event: Value = event.json().await.unwrap();
        let event_id = event["id"].as_str().unwrap();
        let message_id = uuid::Uuid::now_v7().to_string();
        let body = format!(
            "**Rich message** #arr:gig {}end",
            "full content ".repeat(200)
        );
        for (sequence, text) in [(1, body.as_str()), (2, "`#arr:gig`"), (3, "\\#arr:gig")] {
            let id = if sequence == 1 {
                message_id.clone()
            } else {
                uuid::Uuid::now_v7().to_string()
            };
            sqlx::query("insert into messages(id,channel_id,sender_id,sender_display_name,sequence,body,created_at) values(?,?,?,'Alice',?,?,?)").bind(id).bind(&channel).bind(alice.id.to_string()).bind(sequence).bind(text).bind(now()).execute(&pool).await.unwrap();
        }
        let renamed = EventInput {
            title: "Gig".into(),
            code: "concert".into(),
            starts_at: DateTime::parse_from_rfc3339(event["starts_at"].as_str().unwrap())
                .unwrap()
                .with_timezone(&Utc),
            ends_at: None,
            timezone: "Europe/Oslo".into(),
            location: None,
            description: String::new(),
        };
        store
            .save(&alice.id.to_string(), &channel, Some(event_id), renamed)
            .await
            .unwrap();
        let matches_url = format!("{endpoint}/{event_id}/messages?participant=event-http-alice");
        let page: Value = client
            .get(&matches_url)
            .header("x-sproyt-account", alice.id.to_string())
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(page["messages"].as_array().unwrap().len(), 1);
        assert_eq!(page["messages"][0]["id"], message_id);
        assert_eq!(page["messages"][0]["body"], body);
        assert!(page["messages"][0].get("sender_id").is_some());
        sqlx::query("update messages set body='Reference removed' where id=?")
            .bind(&message_id)
            .execute(&pool)
            .await
            .unwrap();
        let page: Value = client
            .get(&matches_url)
            .header("x-sproyt-account", alice.id.to_string())
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(page["messages"].as_array().unwrap().is_empty());
        server.abort();
    }
}

#[cfg(test)]
mod export_tests {
    use super::*;
    use crate::domain::ChatRepository;
    #[tokio::test]
    async fn portable_export_includes_own_events_answers_and_image_contributions() {
        let repository = crate::db::SqliteChatRepository::connect("sqlite::memory:")
            .await
            .unwrap();
        repository.migrate().await.unwrap();
        let s = repository.event_store_for_test();
        let owner = uuid::Uuid::now_v7().to_string();
        let channel = uuid::Uuid::now_v7().to_string();
        s.execute(
            "insert into users(id,kind,display_name) values(?uuid,'human','Exporter')",
            std::slice::from_ref(&owner),
        )
        .await
        .unwrap();
        s.execute("insert into channels(id,slug,name,kind,created_by) values(?uuid,'event-export','Export','private',?uuid)",&[channel.clone(),owner.clone()]).await.unwrap();
        s.execute(
            "insert into channel_memberships(channel_id,user_id,role) values(?uuid,?uuid,'owner')",
            &[channel.clone(), owner.clone()],
        )
        .await
        .unwrap();
        s.execute(
            "update channels set events_enabled=true where id=?uuid",
            std::slice::from_ref(&channel),
        )
        .await
        .unwrap();
        let event = s
            .save(
                &owner,
                &channel,
                None,
                EventInput {
                    title: "Own concert".into(),
                    code: "concert".into(),
                    starts_at: Utc::now() + chrono::Duration::days(1),
                    ends_at: None,
                    timezone: "Europe/Oslo".into(),
                    location: None,
                    description: "Own description".into(),
                },
            )
            .await
            .unwrap();
        let id = event["id"].as_str().unwrap();
        s.rsvp(&owner, &channel, id, "attending").await.unwrap();
        let media = uuid::Uuid::now_v7().to_string();
        s.execute("insert into media_objects(id,owner_id,channel_id,storage_key,original_filename,content_type,size_bytes,sha256,created_at) values(?uuid,?uuid,?uuid,?,'image.png','image/png',1,'test',?)",&[media.clone(),owner.clone(),channel.clone(),media.clone(),now()]).await.unwrap();
        s.image(&owner, &channel, id, &media, false).await.unwrap();
        let export = repository
            .export_user_data(crate::domain::UserId::new(owner).unwrap())
            .await
            .unwrap();
        assert_eq!(export.events_created.len(), 1);
        assert_eq!(export.events_created[0]["description"], "Own description");
        assert_eq!(export.event_responses[0]["status"], "attending");
        assert_eq!(export.event_image_contributions[0]["media_id"], media);
    }
}
