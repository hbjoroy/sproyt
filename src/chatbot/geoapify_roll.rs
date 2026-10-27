//! Explicit source-author consent, durable roll outcome, idempotent publication.
use super::place_lookup::PlaceLookupDescriptor;
use super::*;
use crate::domain::SendMessage;

#[derive(Clone, Deserialize)]
struct Record {
    id: String,
    original_reply_id: String,
    author_id: String,
    descriptor_snapshot: String,
    state: String,
    origin_query: Option<String>,
    origin_json: Option<String>,
    geocodes: Option<String>,
    lease_token: Option<String>,
    leased_until: i64,
    outcome: Option<String>,
    reply_body: Option<String>,
    result_message_id: Option<String>,
    error_code: Option<String>,
    channel_id: String,
    agent_id: String,
    parent_message_id: Option<String>,
}
#[derive(Clone)]
pub(super) struct GeoapifyRollJob {
    pub id: String,
    pub lease_token: String,
    pub author_id: String,
    pub channel_id: String,
    pub agent_id: String,
    pub parent_message_id: Option<String>,
    pub outcome: Option<Value>,
}
#[derive(Serialize)]
pub(crate) struct RollChoice {
    index: usize,
    label: String,
}
#[derive(Serialize)]
pub(crate) struct RollView {
    pub id: String,
    pub status: String,
    pub choices: Vec<RollChoice>,
    pub error_code: Option<String>,
    pub result_message_id: Option<String>,
    pub origin_text: Option<String>,
    pub can_retry_geocoding: bool,
}
#[derive(Serialize)]
pub(crate) struct RollPreview {
    provider: &'static str,
    preview_binding: String,
    origin_mode: String,
    suggested_origin: Option<String>,
    radius_m: u32,
    categories: [&'static str; 2],
    state: Option<RollView>,
}
fn view(r: Record) -> RollView {
    let choices = if r.state == "choose_origin" {
        r.geocodes
            .as_deref()
            .and_then(|v| serde_json::from_str::<Value>(v).ok())
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .enumerate()
            .filter_map(|(index, v)| {
                Some(RollChoice {
                    index,
                    label: v["label"].as_str()?.to_owned(),
                })
            })
            .collect()
    } else {
        vec![]
    };
    let can_retry_geocoding = r.origin_query.is_some()
        && ((r.state == "geocoding" && r.leased_until <= chrono::Utc::now().timestamp())
            || (r.state == "failed"
                && matches!(
                    r.error_code.as_deref(),
                    Some("geocode_failed" | "geoapify_busy")
                )));
    RollView {
        origin_text: r.origin_query,
        can_retry_geocoding,
        id: r.id,
        status: r.state,
        choices,
        error_code: r.error_code,
        result_message_id: r.result_message_id,
    }
}
fn consent_binding(d: &PlaceLookupDescriptor) -> String {
    use sha2::{Digest, Sha256};
    format!(
        "{:x}",
        Sha256::digest(format!(
            "geoapify-roll-v1:1500:bar,pub:{}",
            d.preview_binding()
        ))
    )
}
impl CircleChatAgents {
    fn roll_now(&self) -> &'static str {
        if matches!(&self.store, Store::Pg(_)) {
            "cast(extract(epoch from clock_timestamp()) as bigint)"
        } else {
            "cast(strftime('%s','now') as bigint)"
        }
    }
    async fn roll_record(&self, id: &str) -> Result<Option<Record>> {
        let pg = matches!(&self.store, Store::Pg(_));
        let object = if pg {
            "json_build_object"
        } else {
            "json_object"
        };
        let query = format!(
            "select cast({object}('id',cast(r.id as text),'original_reply_id',cast(r.original_reply_id as text),'author_id',cast(r.author_id as text),'descriptor_snapshot',r.descriptor_snapshot,'state',r.state,'origin_query',r.origin_query,'origin_json',r.origin_json,'geocodes',r.geocodes,'lease_token',r.lease_token,'leased_until',r.leased_until,'outcome',r.outcome,'reply_body',r.reply_body,'result_message_id',cast(r.result_message_id as text),'error_code',r.error_code,'channel_id',cast(j.channel_id as text),'agent_id',cast(j.agent_id as text),'parent_message_id',cast(source.parent_message_id as text)) as text) from agent_place_rolls r join circle_chat_agent_jobs j on j.id=r.source_job_id join messages source on source.id=j.source_message_id where r.id=?uuid and r.expires_at>{}",
            self.roll_now()
        );
        self.store
            .values(&query, &[id.to_owned()])
            .await?
            .into_iter()
            .next()
            .map(|s| serde_json::from_str(&s).map_err(storage))
            .transpose()
    }
    async fn authorized_roll(&self, id: &str) -> Result<(Record, PlaceLookupDescriptor)> {
        let r = self
            .roll_record(id)
            .await?
            .ok_or(RepositoryError::PermissionDenied)?;
        let owner = UserId::new(&r.author_id).map_err(storage)?;
        let actual = self
            .place_lookup_for_message(&owner, &r.channel_id, &r.original_reply_id)
            .await?
            .ok_or(RepositoryError::PermissionDenied)?;
        let expected: PlaceLookupDescriptor =
            serde_json::from_str(&r.descriptor_snapshot).map_err(storage)?;
        if actual != expected || actual.source_author_id != r.author_id {
            return Err(RepositoryError::PermissionDenied);
        }
        Ok((r, actual))
    }
    async fn roll_descriptor(
        &self,
        viewer: &UserId,
        channel: &str,
        message: &str,
    ) -> Result<PlaceLookupDescriptor> {
        if geoapify::GeoapifyService::from_env()
            .map_err(storage)?
            .is_none()
        {
            return Err(RepositoryError::Conflict);
        }
        let d = self
            .place_lookup_for_message(viewer, channel, message)
            .await?
            .ok_or(RepositoryError::PermissionDenied)?;
        if d.source_author_id != viewer.to_string() {
            return Err(RepositoryError::PermissionDenied);
        }
        Ok(d)
    }
    pub(crate) async fn geoapify_roll_messages(
        &self,
        viewer: &UserId,
        channel: &str,
        ids: &[String],
    ) -> Result<Vec<String>> {
        if !matches!(geoapify::GeoapifyService::from_env(), Ok(Some(_))) {
            return Ok(vec![]);
        }
        Ok(self
            .place_lookup_rows(viewer, channel, ids)
            .await?
            .into_iter()
            .filter(|(_, descriptor)| descriptor.source_author_id == viewer.to_string())
            .map(|(id, _)| id)
            .collect())
    }

    pub(crate) async fn geoapify_roll_preview(
        &self,
        viewer: &UserId,
        channel: &str,
        message: &str,
    ) -> Result<RollPreview> {
        let d = self.roll_descriptor(viewer, channel, message).await?;
        let ids = self
            .store
            .values(
                "select cast(id as text) from agent_place_rolls where source_job_id=?uuid",
                std::slice::from_ref(&d.job_id),
            )
            .await?;
        let state = if let Some(id) = ids.first() {
            Some(
                self.roll_record(id)
                    .await?
                    .map(view)
                    .unwrap_or_else(|| RollView {
                        id: id.clone(),
                        status: "cancelled".into(),
                        choices: vec![],
                        error_code: Some("expired".into()),
                        result_message_id: None,
                        origin_text: None,
                        can_retry_geocoding: false,
                    }),
            )
        } else {
            None
        };
        Ok(RollPreview {
            provider: "geoapify",
            preview_binding: consent_binding(&d),
            origin_mode: d.origin_mode.clone(),
            suggested_origin: d.named_origin.clone(),
            radius_m: 1500,
            categories: ["catering.bar", "catering.pub"],
            state,
        })
    }
    pub(crate) async fn consent_geoapify_roll(
        &self,
        viewer: &UserId,
        channel: &str,
        message: &str,
        binding: &str,
        allow_shared: bool,
        origin_text: Option<&str>,
    ) -> Result<RollView> {
        let d = self.roll_descriptor(viewer, channel, message).await?;
        if consent_binding(&d) != binding || (d.origin_mode == "shared" && !allow_shared) {
            return Err(RepositoryError::PermissionDenied);
        }
        let shared = d.origin_mode == "shared";
        let origin = if shared {
            None
        } else {
            let q = origin_text.unwrap_or_default().trim();
            if q.is_empty()
                || q.chars().count() > 240
                || q.chars().any(char::is_control)
                || q.contains("://")
                || q.contains('@')
            {
                return Err(RepositoryError::Conflict);
            }
            Some(q.to_owned())
        };
        let id = Uuid::now_v7().to_string();
        let geocode_token = Uuid::now_v7().to_string();
        let snapshot = serde_json::to_string(&d).map_err(storage)?;
        let now = self.roll_now();
        let geocode_deadline = if shared {
            "0".to_owned()
        } else {
            format!("{now}+30")
        };
        let q = format!(
            "insert into agent_place_rolls(id,source_job_id,original_reply_id,author_id,descriptor_snapshot,plan_snapshot,state,origin_query,expires_at,lease_token,leased_until) select ?uuid,j.id,?uuid,?uuid,?,j.place_lookup_plan,?,nullif(?,''),{now}+900,nullif(?,''),{geocode_deadline} from circle_chat_agent_jobs j where j.id=?uuid and j.place_lookup_plan is not null on conflict(source_job_id) do nothing"
        );
        let changed = self
            .store
            .execute(
                &q,
                &[
                    id.clone(),
                    message.to_owned(),
                    viewer.to_string(),
                    snapshot,
                    if shared { "pending" } else { "geocoding" }.into(),
                    origin.clone().unwrap_or_default(),
                    if shared {
                        String::new()
                    } else {
                        geocode_token.clone()
                    },
                    d.job_id.clone(),
                ],
            )
            .await?;
        let ids = self
            .store
            .values(
                "select cast(id as text) from agent_place_rolls where source_job_id=?uuid",
                std::slice::from_ref(&d.job_id),
            )
            .await?;
        let actual = ids.first().ok_or(RepositoryError::Conflict)?;
        if changed == 0 {
            let (existing, _) = self.authorized_roll(actual).await?;
            if existing.origin_query != origin {
                return Err(RepositoryError::Conflict);
            }
            if shared {
                return Ok(view(existing));
            }
            // Reuse the original consent only. An active request cannot be duplicated;
            // an expired geocode lease may be reclaimed after a process crash.
            let reclaimed = self.store.execute(&format!("update agent_place_rolls set state='geocoding',lease_token=?,leased_until={now}+30,error_code=null where id=?uuid and expires_at>{now} and ((state='geocoding' and leased_until<={now}) or (state='failed' and error_code in ('geocode_failed','geoapify_busy')))"), &[geocode_token.clone(),actual.clone()]).await?;
            if reclaimed == 0 {
                return self.authorized_roll(actual).await.map(|(r, _)| view(r));
            }
        }
        let id = actual.clone();
        if !shared {
            // The request author consented above, and that consent is durable
            // before this first external request. Recheck after admission wait.
            let permit = self.claim_place_lookup(viewer).await?;
            let Some(permit) = permit else {
                self.store.execute("update agent_place_rolls set state='failed',error_code='geoapify_busy',lease_token=null,leased_until=0 where id=?uuid and state='geocoding' and lease_token=?",&[id.clone(),geocode_token.clone()]).await?;
                return self
                    .roll_record(&id)
                    .await?
                    .map(view)
                    .ok_or(RepositoryError::Conflict);
            };
            let result=async {
                let (record,_) = self.authorized_roll(&id).await?;
                if record.lease_token.as_deref()!=Some(geocode_token.as_str()) || record.leased_until<=chrono::Utc::now().timestamp() { return Err(RepositoryError::Conflict); }
                let service=geoapify::GeoapifyService::from_env().map_err(storage)?.ok_or(RepositoryError::Conflict)?;
                let candidates=service.geocode(origin.as_deref().unwrap_or_default()).await.map_err(storage)?;
                self.authorized_roll(&id).await?;
                if candidates.is_empty(){return Err(RepositoryError::NotFound);}
                let data=serde_json::to_string(&candidates).map_err(storage)?;
                let changed=self.store.execute(&format!("update agent_place_rolls set state='choose_origin',geocodes=?,lease_token=null,leased_until=0 where id=?uuid and state='geocoding' and lease_token=? and leased_until>{now} and expires_at>{now}"),&[data,id.clone(),geocode_token.clone()]).await?;
                if changed==0 {return Err(RepositoryError::Conflict);}
                Ok(())
            }.await;
            self.release_place_lookup(viewer, &permit).await?;
            if result.is_err() {
                self.store.execute("update agent_place_rolls set state='failed',error_code='geocode_failed',lease_token=null,leased_until=0 where id=?uuid and state='geocoding' and lease_token=?",&[id.clone(),geocode_token.clone()]).await?;
            }
        }
        self.authorized_roll(&id).await.map(|(r, _)| view(r))
    }
    pub(crate) async fn choose_geoapify_roll_origin(
        &self,
        viewer: &UserId,
        channel: &str,
        message: &str,
        id: &str,
        index: usize,
    ) -> Result<RollView> {
        let (r, d) = self.authorized_roll(id).await?;
        if r.author_id != viewer.to_string()
            || r.channel_id != channel
            || r.original_reply_id != message
            || d.origin_mode == "shared"
        {
            return Err(RepositoryError::PermissionDenied);
        }
        if r.state != "choose_origin" {
            return Ok(view(r));
        }
        let candidates: Value =
            serde_json::from_str(r.geocodes.as_deref().ok_or(RepositoryError::Conflict)?)
                .map_err(storage)?;
        let candidate = candidates
            .as_array()
            .and_then(|v| v.get(index))
            .ok_or(RepositoryError::Conflict)?;
        let selected = serde_json::to_string(candidate).map_err(storage)?;
        self.store.execute("update agent_place_rolls set origin_json=?,geocodes=null,state='pending' where id=?uuid and state='choose_origin'",&[selected,id.to_owned()]).await?;
        self.authorized_roll(id).await.map(|(r, _)| view(r))
    }
    pub(super) async fn claim_geoapify_roll(&self) -> Result<Option<GeoapifyRollJob>> {
        let now = self.roll_now();
        // Expired private workflow material is discarded; public chat outcomes
        // retain their ordinary history and source attribution.
        self.store.execute(&format!("update agent_place_rolls set state=case when state='published' then state else 'cancelled' end,origin_query=null,origin_json=null,geocodes=null,outcome=null,lease_token=null where expires_at<={now} and (origin_query is not null or origin_json is not null or geocodes is not null or outcome is not null or lease_token is not null)"),&[]).await?;
        let token = Uuid::now_v7().to_string();
        let lock = if matches!(&self.store, Store::Pg(_)) {
            "for update skip locked"
        } else {
            ""
        };
        let q = format!(
            "update agent_place_rolls set state=case when outcome is not null then 'ready' else 'leased' end,lease_token=?,leased_until={now}+180,attempts=attempts+1 where id=(select id from agent_place_rolls where expires_at>{now} and (attempts<3 or outcome is not null) and ((state in ('pending','ready') and leased_until<={now}) or (state='leased' and leased_until<={now})) order by expires_at,id {lock} limit 1) returning cast(id as text)"
        );
        let ids = self.store.values(&q, std::slice::from_ref(&token)).await?;
        let Some(id) = ids.first() else {
            return Ok(None);
        };
        let r = self
            .roll_record(id)
            .await?
            .ok_or(RepositoryError::Conflict)?;
        Ok(Some(GeoapifyRollJob {
            id: id.clone(),
            lease_token: token,
            author_id: r.author_id,
            channel_id: r.channel_id,
            agent_id: r.agent_id,
            parent_message_id: r.parent_message_id,
            outcome: r
                .outcome
                .as_deref()
                .map(serde_json::from_str)
                .transpose()
                .map_err(storage)?,
        }))
    }
    pub(super) async fn geoapify_roll_origin(
        &self,
        job: &GeoapifyRollJob,
    ) -> Result<geoapify::Point> {
        let (r, d) = self.authorized_roll(&job.id).await?;
        if r.lease_token.as_deref() != Some(&job.lease_token)
            || r.leased_until <= Utc::now().timestamp()
            || !matches!(r.state.as_str(), "leased" | "ready")
        {
            return Err(RepositoryError::PermissionDenied);
        }
        if d.origin_mode == "shared" {
            let point = self
                .place_lookup_location(&d)
                .await?
                .ok_or(RepositoryError::PermissionDenied)?
                .context();
            Ok(geoapify::Point {
                latitude: point["latitude"]
                    .as_f64()
                    .ok_or(RepositoryError::Conflict)?,
                longitude: point["longitude"]
                    .as_f64()
                    .ok_or(RepositoryError::Conflict)?,
            })
        } else {
            let point: Value =
                serde_json::from_str(r.origin_json.as_deref().ok_or(RepositoryError::Conflict)?)
                    .map_err(storage)?;
            Ok(geoapify::Point {
                latitude: point["point"]["latitude"]
                    .as_f64()
                    .ok_or(RepositoryError::Conflict)?,
                longitude: point["point"]["longitude"]
                    .as_f64()
                    .ok_or(RepositoryError::Conflict)?,
            })
        }
    }
    pub(super) async fn save_geoapify_roll_result(
        &self,
        job: &GeoapifyRollJob,
        search: &geoapify::PlaceSearch,
        roll: u32,
    ) -> Result<()> {
        self.geoapify_roll_origin(job).await?;
        let selected = search
            .places
            .get(roll.checked_sub(1).ok_or(RepositoryError::Conflict)? as usize)
            .ok_or(RepositoryError::Conflict)?;
        let count = search.places.len();
        if count > 100
            || search.radius_m != 1500
            || !selected.point.latitude.is_finite()
            || !selected.point.longitude.is_finite()
            || !(-90.0..=90.0).contains(&selected.point.latitude)
            || !(-180.0..=180.0).contains(&selected.point.longitude)
        {
            return Err(RepositoryError::Conflict);
        }
        let address = selected
            .address
            .as_deref()
            .map(safe_place_name)
            .map(|value| format!("\n{value}"))
            .unwrap_or_default();
        let destination = format!(
            "https://www.google.com/maps/search/?api=1&query={},{}",
            selected.point.latitude, selected.point.longitude
        );
        let body = format!(
            "Eg slo d{count} og fekk {roll}: {}.{address}\nGoogle Maps: {destination}\nTilfeldig valt blant {count} treff innan 1,5 km.{}\n© OpenStreetMap contributors — https://www.openstreetmap.org/copyright\nPowered by Geoapify — https://www.geoapify.com/",
            safe_place_name(&selected.name),
            if search.potentially_truncated {
                " Utvalet kan vere avgrensa."
            } else {
                ""
            }
        );
        let outcome = json!({"sides":count,"value":roll,"selected":selected,"candidate_count":count,"radius_m":search.radius_m,"potentially_truncated":search.potentially_truncated,"attribution":search.attribution});
        let q = format!(
            "update agent_place_rolls set state='ready',outcome=?,reply_body=? where id=?uuid and lease_token=? and state='leased' and outcome is null and leased_until>{} and expires_at>{}",
            self.roll_now(),
            self.roll_now()
        );
        macro_rules! save {
            ($pool:expr,$pg:expr,$fence:path) => {{
                let mut tx = $pool.begin().await.map_err(storage)?;
                let authority = $fence(&mut tx, &job.id).await?;
                if authority["state"] != "leased" || authority["lease_token"] != job.lease_token {
                    return Err(RepositoryError::PermissionDenied);
                }
                let changed = sqlx::query(&sql(&q, $pg))
                    .bind(outcome.to_string())
                    .bind(&body)
                    .bind(&job.id)
                    .bind(&job.lease_token)
                    .execute(&mut *tx)
                    .await
                    .map_err(storage)?
                    .rows_affected();
                if changed != 1 {
                    return Err(RepositoryError::Conflict);
                }
                tx.commit().await.map_err(storage)?;
            }};
        }
        match &self.store {
            Store::Pg(pool) => save!(pool, true, fence_postgres),
            Store::Sqlite(pool) => save!(pool, false, fence_sqlite),
        }
        Ok(())
    }
    pub(super) async fn fail_geoapify_roll(&self, job: &GeoapifyRollJob, code: &str) -> Result<()> {
        if matches!(code, "model_busy" | "geoapify_busy") {
            // Admission contention did not perform work. Keep the retry budget,
            // but delay eligibility so multiple workers cannot spin on it.
            let now = self.roll_now();
            self.store.execute(&format!("update agent_place_rolls set state=case when outcome is not null then 'ready' else 'pending' end,attempts=case when attempts>0 then attempts-1 else 0 end,error_code=?,lease_token=null,leased_until={now}+10 where id=?uuid and lease_token=? and state in ('leased','ready')"), &[code.into(),job.id.clone(),job.lease_token.clone()]).await?;
        } else {
            self.store.execute("update agent_place_rolls set state=case when outcome is not null then 'ready' else 'failed' end,error_code=?,lease_token=null,leased_until=0 where id=?uuid and lease_token=? and state in ('leased','ready')",&[code.into(),job.id.clone(),job.lease_token.clone()]).await?;
        }
        Ok(())
    }
    pub(super) async fn publish_geoapify_roll(
        &self,
        job: &GeoapifyRollJob,
        chat: &ChatEngine,
    ) -> Result<()> {
        self.geoapify_roll_origin(job).await?;
        let (r, _) = self.authorized_roll(&job.id).await?;
        let body =
            MessageBody::new(r.reply_body.ok_or(RepositoryError::Conflict)?).map_err(storage)?;
        let channel = ChannelId::new(&job.channel_id).map_err(storage)?;
        let actor = UserId::new(&job.agent_id).map_err(storage)?;
        let request = format!("circle-place-roll:{}", job.id);
        let sent = if let Some(parent) = &job.parent_message_id {
            chat.send_thread_reply_idempotent(
                channel,
                actor,
                crate::domain::MessageId::from_uuid(Uuid::parse_str(parent).map_err(storage)?),
                body,
                request,
            )
            .await
        } else {
            chat.send_message_idempotent(channel, actor, body, request)
                .await
        }
        .map_err(storage)?;
        self.store.execute("update agent_place_rolls set state='published',result_message_id=?uuid,lease_token=null,leased_until=0,origin_json=null,geocodes=null where id=?uuid and state='ready'",&[sent.id.as_uuid().to_string(),job.id.clone()]).await?;
        Ok(())
    }
}

fn safe_place_name(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '[' => '［',
            ']' => '］',
            '(' => '（',
            ')' => '）',
            '<' => '＜',
            '>' => '＞',
            '@' => '＠',
            '`' => 'ˋ',
            '*' => '＊',
            '_' => '＿',
            '\\' => '／',
            ':' => '：',
            '|' => '｜',
            '{' => '｛',
            '}' => '｝',
            c if c.is_whitespace() => ' ',
            c => c,
        })
        .collect()
}

fn fence_query(pg: bool) -> String {
    let object = if pg {
        "json_build_object"
    } else {
        "json_object"
    };
    let now = if pg {
        "cast(extract(epoch from clock_timestamp()) as bigint)"
    } else {
        "cast(strftime('%s','now') as bigint)"
    };
    let locking = if pg {
        " for update of r for share of c,a,p,source,original,cm,ccm"
    } else {
        ""
    };
    format!(
        "select cast({object}('job_id',cast(j.id as text),'state',r.state,'lease_token',r.lease_token,'body',r.reply_body,'descriptor',r.descriptor_snapshot,'agent_id',cast(j.agent_id as text),'channel_id',cast(j.channel_id as text),'parent_id',cast(source.parent_message_id as text),'has_outcome',case when r.outcome is null then 0 else 1 end) as text) from agent_place_rolls r join circle_chat_agent_jobs j on j.id=r.source_job_id join channels c on c.id=j.channel_id join circle_chat_agents a on a.agent_id=j.agent_id join agent_profiles p on p.agent_id=j.agent_id join messages source on source.id=j.source_message_id join users author on author.id=source.sender_id join users bot on bot.id=j.agent_id join messages original on original.id=r.original_reply_id join command_receipts receipt on receipt.message_id=original.id and receipt.principal_id=j.agent_id and receipt.request_id='circle-chat-agent:' || cast(j.id as text) join message_provenance provenance on provenance.message_id=source.id join channel_memberships cm on cm.channel_id=c.id and cm.user_id=r.author_id join circle_memberships ccm on ccm.circle_id=c.circle_id and ccm.user_id=r.author_id where r.id=?uuid and r.expires_at>{now} and r.leased_until>{now} and r.lease_token is not null and r.state in ('leased','ready') and r.author_id=source.sender_id and r.plan_snapshot=j.place_lookup_plan and original.body=? and original.sender_id=j.agent_id and original.channel_id=c.id and original.edited_at is null and original.deleted_at is null and source.edited_at is null and source.deleted_at is null and source.channel_id=c.id and author.kind='human' and provenance.provenance='human' and bot.kind='agent' and a.enabled=true and a.revision=j.config_revision and c.chat_agent_access_revision=j.access_revision and a.circle_id=c.circle_id and p.revoked_at is null and (p.expires_at is null or p.expires_at>current_timestamp) and coalesce((select s.enabled from channel_chat_agent_settings s where s.channel_id=c.id and s.agent_id=a.agent_id),c.kind!='private'){locking}"
    )
}
macro_rules! fence {
    ($tx:expr,$id:expr,$pg:expr) => {{
        let query=sql(&fence_query($pg),$pg);
        let mut value=Value::Null;
        for grant_locked in [false,true] {
            let text:Option<String>=sqlx::query_scalar(&query).bind($id).bind(place_lookup::NEUTRAL_REPLY).fetch_optional(&mut **$tx).await.map_err(storage)?;
            value=serde_json::from_str(&text.ok_or(RepositoryError::PermissionDenied)?).map_err(storage)?;
            if !grant_locked {
                let d:PlaceLookupDescriptor=serde_json::from_str(value["descriptor"].as_str().ok_or(RepositoryError::PermissionDenied)?).map_err(storage)?;
                if d.origin_mode=="shared" {
                    let expiry=if $pg{"s.expires_at>clock_timestamp()"}else{"julianday(s.expires_at)>julianday('now')"};
                    let locking=if $pg{" for share of s"}else{""};
                    let query=sql(&format!("select cast(s.share_id as text) from agent_location_shares s join circle_chat_agent_jobs j on j.location_share_id=s.share_id where j.id=?uuid and s.share_id=?uuid and s.user_id=?uuid and s.agent_id=j.agent_id and s.channel_id=j.channel_id and {expiry}{locking}"),$pg);
                    let found:Option<String>=sqlx::query_scalar(&query).bind(&d.job_id).bind(d.share_id.as_deref().ok_or(RepositoryError::PermissionDenied)?).bind(&d.source_author_id).fetch_optional(&mut **$tx).await.map_err(storage)?;
                    if found.is_none(){return Err(RepositoryError::PermissionDenied);}
                }
            }
        }
        value
    }};
}
async fn fence_postgres(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, id: &str) -> Result<Value> {
    sqlx::query("select c.id from channels c join circle_chat_agent_jobs j on j.channel_id=c.id join agent_place_rolls r on r.source_job_id=j.id where r.id=$1::text::uuid for share of c").bind(id).fetch_optional(&mut **tx).await.map_err(storage)?.ok_or(RepositoryError::PermissionDenied)?;
    Ok(fence!(tx, id, true))
}
async fn fence_sqlite(tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>, id: &str) -> Result<Value> {
    sqlx::query("update agent_place_rolls set id=id where id=?")
        .bind(id)
        .execute(&mut **tx)
        .await
        .map_err(storage)?;
    Ok(fence!(tx, id, false))
}
fn validate_publication(value: &Value, command: &SendMessage) -> Result<()> {
    if value["state"] != "ready"
        || value["has_outcome"] != 1
        || value["body"].as_str() != Some(command.body.as_str())
        || value["agent_id"].as_str() != Some(command.actor.to_string().as_str())
        || value["channel_id"].as_str() != Some(command.channel_id.to_string().as_str())
        || value["parent_id"].as_str()
            != command
                .parent_message_id
                .map(|id| id.as_uuid().to_string())
                .as_deref()
    {
        return Err(RepositoryError::PermissionDenied);
    }
    Ok(())
}
pub(crate) async fn authorize_postgres(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    command: &SendMessage,
    request: &str,
) -> Result<()> {
    let id = request
        .strip_prefix("circle-place-roll:")
        .ok_or(RepositoryError::PermissionDenied)?;
    Uuid::parse_str(id).map_err(|_| RepositoryError::PermissionDenied)?;
    validate_publication(&fence_postgres(tx, id).await?, command)
}
pub(crate) async fn authorize_sqlite(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    command: &SendMessage,
    request: &str,
) -> Result<()> {
    let id = request
        .strip_prefix("circle-place-roll:")
        .ok_or(RepositoryError::PermissionDenied)?;
    Uuid::parse_str(id).map_err(|_| RepositoryError::PermissionDenied)?;
    validate_publication(&fence_sqlite(tx, id).await?, command)
}

#[cfg(test)]
#[path = "geoapify_roll_tests.rs"]
mod tests;
