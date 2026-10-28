//! Durable request plans only; Google content never enters this module.
use super::*;
use sha2::{Digest, Sha256};

pub(super) const NEUTRAL_REPLY: &str = "Opne stadssøket for ferske barforslag.";

#[cfg(test)]
tokio::task_local! { pub(crate) static TEST_ENABLED: bool; }

pub(crate) fn enabled() -> bool {
    #[cfg(test)]
    if let Ok(value) = TEST_ENABLED.try_with(|value| *value) {
        return value;
    }
    ["SPROYT_GOOGLE_MAPS_API_KEY", "SPROYT_GEOAPIFY_API_KEY"]
        .iter()
        .any(|key| std::env::var(key).is_ok_and(|value| !value.trim().is_empty()))
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PlaceLookupPlan {
    pub query: String,
    pub origin_mode: String,
    #[serde(default)]
    pub named_origin: Option<String>,
    #[serde(default)]
    pub choose_random: bool,
}

pub(super) fn parse_plan(arguments: &str) -> std::result::Result<PlaceLookupPlan, &'static str> {
    let mut plan: PlaceLookupPlan =
        serde_json::from_str(arguments).map_err(|_| "model_invalid_tool")?;
    plan.query = plan.query.trim().to_owned();
    fn valid(value: &str) -> bool {
        !value.is_empty()
            && value.chars().count() <= 240
            && !value.chars().any(char::is_control)
            && !value.contains("://")
            && !value.contains('@')
            && !value.contains('°')
            && !value.as_bytes().windows(3).any(|w| {
                w[0].is_ascii_digit() && matches!(w[1], b'.' | b',') && w[2].is_ascii_digit()
            })
    }
    if !valid(&plan.query) || !matches!(plan.origin_mode.as_str(), "none" | "named" | "shared") {
        return Err("model_invalid_tool");
    }
    if let Some(origin) = &mut plan.named_origin {
        *origin = origin.trim().to_owned();
    }
    if (plan.origin_mode == "named") != plan.named_origin.is_some()
        || plan
            .named_origin
            .as_deref()
            .is_some_and(|origin| !valid(origin))
    {
        return Err("model_invalid_tool");
    }
    if provider_query(&plan.query, &plan.origin_mode, plan.named_origin.as_deref()).is_none() {
        return Err("model_invalid_tool");
    }
    Ok(plan)
}

fn provider_query(query: &str, origin_mode: &str, named_origin: Option<&str>) -> Option<String> {
    let actual = match origin_mode {
        "named" => format!("barar og pubar: {query} nær {}", named_origin?),
        "none" | "shared" => format!("barar og pubar: {query}"),
        _ => return None,
    };
    (actual.chars().count() <= 240).then_some(actual)
}

pub(super) const CLARIFICATION_REPLY: &str = "Kva by eller stad vil du søkje etter barar ved? Skriv òg utgangspunktet dersom du ønskjer gangavstand.";

/// Only literal text from this human's target message may leave the channel.
/// Neither memory, earlier messages, image descriptions nor model paraphrases
/// are evidence for a search. Mask markup rather than joining text around it.
pub(super) fn grounded_in_source(plan: &PlaceLookupPlan, body: &str) -> bool {
    let mut plain = body.to_owned();
    while let Some(start) = plain.find("[[") {
        let end = plain[start..]
            .find("]]")
            .map_or(plain.len(), |offset| start + offset + 2);
        plain.replace_range(start..end, "\0");
    }
    let mut masked = String::with_capacity(plain.len());
    let mut mention = false;
    for ch in plain.chars() {
        if ch == '@' {
            mention = true;
        }
        if mention && ch.is_whitespace() {
            mention = false;
        }
        masked.push(if mention { '\0' } else { ch });
    }
    fn contains_span(body: &str, value: &str) -> bool {
        body.match_indices(value).any(|(start, _)| {
            let end = start + value.len();
            !body[..start]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
                && !body[end..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_')
        })
    }
    if !contains_span(&masked, &plan.query) {
        return false;
    }
    if plan.origin_mode == "shared" {
        let folded = masked.to_lowercase();
        return [
            "nær meg",
            "i nærleiken av meg",
            "i nærheten av meg",
            "her eg er",
            "her jeg er",
            "near me",
            "around me",
        ]
        .iter()
        .any(|phrase| contains_span(&folded, phrase));
    }
    contains_span(&masked, &plan.query)
        && plan
            .named_origin
            .as_deref()
            .is_none_or(|origin| contains_span(&masked, origin))
}

#[derive(Serialize)]
pub(crate) struct PlaceLookupPreview {
    pub preview_binding: String,
    pub query: String,
    pub origin_mode: String,
    pub named_origin: Option<String>,
    pub uses_shared_location: bool,
}

pub(super) fn definition() -> Value {
    json!({"type":"function","function":{"name":"find_bars",
        "description":"Search for bars and pubs and publish actual results to everyone in this channel. The server performs the search and renders the sourced reply; never claim that a search card exists. Set choose_random=true when asked to roll/select a random bar. Compose a concise bar-related query using a place named by the target human, or that same human's recent message in this channel. named_origin may normalize spelling and punctuation or omit address details, but must not invent a place. Never use agent messages, memory, coordinates, URLs, attachment tokens or another person's location. origin_mode none for a named city/area, named for a named hotel/station/place (supply named_origin), shared only when the target explicitly asks near me and has a server-provided shared_location. Ask for a place if none is supplied. Make this the only tool call. Never claim a search or roll succeeded without its result.",
        "parameters":{"type":"object","properties":{"query":{"type":"string","maxLength":240},"origin_mode":{"type":"string","enum":["none","named","shared"]},"named_origin":{"type":"string","maxLength":240},"choose_random":{"type":"boolean"}},"required":["query","origin_mode"],"additionalProperties":false}}})
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct PlaceLookupDescriptor {
    pub job_id: String,
    pub agent_id: String,
    pub source_message_id: String,
    pub source_author_id: String,
    pub share_id: Option<String>,
    pub query: String,
    pub origin_mode: String,
    pub named_origin: Option<String>,
}

impl PlaceLookupDescriptor {
    pub(crate) fn provider_query(&self) -> Option<String> {
        provider_query(&self.query, &self.origin_mode, self.named_origin.as_deref())
    }

    /// Consent binds to this precise plan and exact location grant, never to
    /// a later replacement. The digest discloses no coordinate or share ID.
    pub(crate) fn preview_binding(&self) -> String {
        let mut digest = Sha256::new();
        digest.update(b"sproyt-place-lookup-preview-v2\0");
        digest.update(serde_json::to_vec(self).expect("string-only descriptor serializes"));
        digest.update(b"\0");
        digest.update(self.provider_query().unwrap_or_default().as_bytes());
        format!("{:x}", digest.finalize())
    }
}

impl CircleChatAgents {
    // Receipt creation is atomic with message publication. We deliberately do
    // not depend on the worker's later finish() update surviving a crash.
    pub(super) async fn place_lookup_rows(
        &self,
        viewer: &UserId,
        channel: &str,
        ids: &[String],
    ) -> Result<Vec<(String, PlaceLookupDescriptor)>> {
        if !enabled() || ids.is_empty() {
            return Ok(vec![]);
        }
        if ids.len() > 50
            || Uuid::parse_str(channel).is_err()
            || ids.iter().any(|id| Uuid::parse_str(id).is_err())
        {
            return Err(RepositoryError::Conflict);
        }
        let grant_time = if matches!(&self.store, Store::Pg(_)) {
            "l.expires_at>clock_timestamp()"
        } else {
            "julianday(l.expires_at)>julianday('now')"
        };
        let marks = vec!["?uuid"; ids.len()].join(",");
        let query = format!(
            "select cast(case when l.share_id is not null then 1 else 0 end as bigint) share_active,cast(reply.id as text) reply_id,cast(j.id as text) job_id,cast(j.agent_id as text) agent_id,cast(j.source_message_id as text) source_id,cast(source.sender_id as text) author_id,cast(j.location_share_id as text) share_id,j.place_lookup_plan,source.body source_body from circle_chat_agent_jobs j join command_receipts receipt on receipt.principal_id=j.agent_id and receipt.request_id='circle-chat-agent:' || cast(j.id as text) join messages reply on reply.id=receipt.message_id join messages source on source.id=j.source_message_id join message_provenance provenance on provenance.message_id=source.id join users author on author.id=source.sender_id join channels c on c.id=j.channel_id join circle_chat_agents a on a.agent_id=j.agent_id join agent_profiles p on p.agent_id=a.agent_id left join agent_location_shares l on l.share_id=j.location_share_id and l.user_id=source.sender_id and l.agent_id=j.agent_id and l.channel_id=j.channel_id and {grant_time} where reply.id in ({marks}) and c.id=?uuid and exists(select 1 from channel_memberships cm where cm.channel_id=c.id and cm.user_id=?uuid) and exists(select 1 from circle_memberships cm where cm.circle_id=c.circle_id and cm.user_id=?uuid) and exists(select 1 from channel_memberships cm where cm.channel_id=c.id and cm.user_id=source.sender_id) and exists(select 1 from circle_memberships cm where cm.circle_id=c.circle_id and cm.user_id=source.sender_id) and a.enabled=true and a.revision=j.config_revision and c.chat_agent_access_revision=j.access_revision and a.circle_id=c.circle_id and p.revoked_at is null and (p.expires_at is null or p.expires_at>current_timestamp) and coalesce((select s.enabled from channel_chat_agent_settings s where s.channel_id=c.id and s.agent_id=a.agent_id),c.kind!='private') and reply.channel_id=c.id and source.channel_id=c.id and reply.sender_id=j.agent_id and reply.body=? and reply.edited_at is null and reply.deleted_at is null and source.edited_at is null and source.deleted_at is null and author.kind='human' and provenance.provenance='human' and j.place_lookup_plan is not null"
        );
        let mut args = ids.to_vec();
        args.extend([
            channel.to_owned(),
            viewer.to_string(),
            viewer.to_string(),
            NEUTRAL_REPLY.to_owned(),
        ]);
        macro_rules! load {
            ($pool:expr,$pg:expr) => {{
                let query = sql(&query, $pg);
                let mut statement = sqlx::query(&query);
                for arg in &args {
                    statement = statement.bind(arg);
                }
                let rows = statement.fetch_all($pool).await.map_err(storage)?;
                let mut result = Vec::new();
                for row in rows {
                    let plan_text: String = row.try_get("place_lookup_plan").map_err(storage)?;
                    let Ok(plan) = parse_plan(&plan_text) else {
                        continue;
                    };
                    let source_body: String = row.try_get("source_body").map_err(storage)?;
                    if !grounded_in_source(&plan, &source_body) {
                        continue;
                    }
                    let author: String = row.try_get("author_id").map_err(storage)?;
                    if plan.origin_mode == "shared" && author != viewer.to_string() {
                        continue;
                    }
                    let descriptor = PlaceLookupDescriptor {
                        job_id: row.try_get("job_id").map_err(storage)?,
                        agent_id: row.try_get("agent_id").map_err(storage)?,
                        source_message_id: row.try_get("source_id").map_err(storage)?,
                        source_author_id: author,
                        share_id: row.try_get("share_id").map_err(storage)?,
                        query: plan.query,
                        origin_mode: plan.origin_mode,
                        named_origin: plan.named_origin,
                    };
                    // A shared card disappears when its exact grant is revoked.
                    if descriptor.origin_mode == "shared"
                        && (!location::enabled()
                            || row.try_get::<i64, _>("share_active").map_err(storage)? != 1)
                    {
                        continue;
                    }
                    result.push((row.try_get("reply_id").map_err(storage)?, descriptor));
                }
                result
            }};
        }
        Ok(match &self.store {
            Store::Pg(pool) => load!(pool, true),
            Store::Sqlite(pool) => load!(pool, false),
        })
    }

    pub(crate) async fn place_lookup_messages(
        &self,
        viewer: &UserId,
        channel: &str,
        ids: &[String],
    ) -> Result<Vec<String>> {
        Ok(self
            .place_lookup_rows(viewer, channel, ids)
            .await?
            .into_iter()
            .map(|(id, _)| id)
            .collect())
    }

    /// Provider-specific availability never exposes a card for an unusable key.
    /// Keep the generic descriptor list independent so Geoapify still works.
    pub(crate) async fn google_place_lookup_messages(
        &self,
        viewer: &UserId,
        channel: &str,
        ids: &[String],
    ) -> Result<Vec<String>> {
        #[cfg(test)]
        if super::maps_lookup::TEST_PROVIDER_CALLS
            .try_with(|_| ())
            .is_ok()
        {
            return self.place_lookup_messages(viewer, channel, ids).await;
        }
        if !matches!(super::maps::MapsService::from_env(), Ok(Some(_))) {
            return Ok(vec![]);
        }
        self.place_lookup_messages(viewer, channel, ids).await
    }

    pub(crate) async fn place_lookup_for_message(
        &self,
        viewer: &UserId,
        channel: &str,
        message: &str,
    ) -> Result<Option<PlaceLookupDescriptor>> {
        Ok(self
            .place_lookup_rows(viewer, channel, &[message.to_owned()])
            .await?
            .into_iter()
            .next()
            .map(|(_, value)| value))
    }

    pub(crate) async fn place_lookup_preview(
        &self,
        viewer: &UserId,
        channel: &str,
        message: &str,
    ) -> Result<Option<PlaceLookupPreview>> {
        let Some(descriptor) = self
            .place_lookup_for_message(viewer, channel, message)
            .await?
        else {
            return Ok(None);
        };
        let Some(query) = descriptor.provider_query() else {
            return Ok(None);
        };
        Ok(Some(PlaceLookupPreview {
            preview_binding: descriptor.preview_binding(),
            uses_shared_location: descriptor.origin_mode == "shared",
            query,
            origin_mode: descriptor.origin_mode,
            named_origin: descriptor.named_origin,
        }))
    }

    pub(crate) async fn place_lookup_location(
        &self,
        descriptor: &PlaceLookupDescriptor,
    ) -> Result<Option<location::JobLocation>> {
        if descriptor.origin_mode != "shared" {
            return Ok(None);
        }
        if descriptor.share_id.is_none() {
            return Err(RepositoryError::PermissionDenied);
        }
        let job = Job {
            id: descriptor.job_id.clone(),
            agent_id: descriptor.agent_id.clone(),
            source_message_id: descriptor.source_message_id.clone(),
            channel_id: String::new(),
            attempts: 0,
            reply_body: None,
            location_share_id: descriptor.share_id.clone(),
            lease_token: String::new(),
        };
        self.location_for_job(&job)
            .await?
            .map(Some)
            .ok_or(RepositoryError::PermissionDenied)
    }
}

impl CircleChatAgents {
    /// Shared database admission: one active call per user, ten/minute,
    /// three-second cooldown, eight globally active, sixty-second crash lease.
    pub(crate) async fn claim_place_lookup(&self, viewer: &UserId) -> Result<Option<String>> {
        let token = Uuid::now_v7().to_string();
        macro_rules! claim { ($pool:expr,$pg:expr) => {{
            let mut tx=$pool.begin().await.map_err(storage)?;
            // Serializes admission across all application replicas.
            sqlx::query("update place_lookup_admission_gate set id=1 where id=1").execute(&mut *tx).await.map_err(storage)?;
            let clock=if $pg {"select cast(extract(epoch from clock_timestamp()) as bigint)"} else {"select cast(strftime('%s','now') as bigint)"};
            let now:i64=sqlx::query_scalar(clock).fetch_one(&mut *tx).await.map_err(storage)?;
            let active:i64=sqlx::query_scalar(&sql("select count(*) from place_lookup_admissions where leased_until>?int",$pg)).bind(now.to_string()).fetch_one(&mut *tx).await.map_err(storage)?;
            if active>=8 {tx.rollback().await.map_err(storage)?; return Ok(None);}
            let query=sql("insert into place_lookup_admissions(user_id,token,leased_until,next_allowed,window_started,calls) values(?uuid,?,?int,?int,?int,1) on conflict(user_id) do update set token=excluded.token,leased_until=excluded.leased_until,next_allowed=excluded.next_allowed,window_started=case when place_lookup_admissions.window_started<=?int then excluded.window_started else place_lookup_admissions.window_started end,calls=case when place_lookup_admissions.window_started<=?int then 1 else place_lookup_admissions.calls+1 end where place_lookup_admissions.leased_until<=?int and place_lookup_admissions.next_allowed<=?int and (place_lookup_admissions.window_started<=?int or place_lookup_admissions.calls<10)",$pg);
            let changed=sqlx::query(&query).bind(viewer.to_string()).bind(&token).bind((now+60).to_string()).bind((now+3).to_string()).bind(now.to_string()).bind((now-60).to_string()).bind((now-60).to_string()).bind(now.to_string()).bind(now.to_string()).bind((now-60).to_string()).execute(&mut *tx).await.map_err(storage)?.rows_affected();
            tx.commit().await.map_err(storage)?;
            if changed==1 {Some(token)} else {None}
        }}; }
        Ok(match &self.store {
            Store::Pg(pool) => claim!(pool, true),
            Store::Sqlite(pool) => claim!(pool, false),
        })
    }

    pub(crate) async fn release_place_lookup(&self, viewer: &UserId, token: &str) -> Result<()> {
        self.store
            .execute(
                "update place_lookup_admissions set leased_until=0 where user_id=?uuid and token=?",
                &[viewer.to_string(), token.to_owned()],
            )
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn place_lookup_only_uses_literal_target_text_and_preview_has_no_internal_ids() {
        let plan = parse_plan(
            r#"{"query":"bars in Bergen","origin_mode":"named","named_origin":"Bergen station"}"#,
        )
        .unwrap();
        assert!(grounded_in_source(
            &plan,
            "@Maria find bars in Bergen from Bergen station"
        ));
        assert!(!grounded_in_source(&plan, "@Maria find bars near my hotel"));
        assert!(!grounded_in_source(
            &plan,
            "bars in Bergen (the station is only known from memory)"
        ));
        assert!(!grounded_in_source(
            &plan,
            "[[media:bars in Bergen]] Bergen station"
        ));
        let shared = parse_plan(r#"{"query":"barar","origin_mode":"shared"}"#).unwrap();
        assert!(grounded_in_source(&shared, "Finn barar nær meg"));
        assert!(!grounded_in_source(&shared, "Finn ein bar i Bergen"));
        let craft =
            parse_plan(r#"{"query":"craft beer og mat nær meg","origin_mode":"shared"}"#).unwrap();
        assert_eq!(craft.query, "craft beer og mat nær meg");
        assert!(grounded_in_source(
            &craft,
            "@Maria finn craft beer og mat nær meg"
        ));
        assert!(!grounded_in_source(&craft, "Finn barar nær meg"));
        assert!(!grounded_in_source(
            &craft,
            "[[media:craft beer og mat nær meg]]"
        ));
        let preview = PlaceLookupPreview {
            preview_binding: "opaque-binding".into(),
            query: plan.query,
            origin_mode: plan.origin_mode,
            named_origin: plan.named_origin,
            uses_shared_location: false,
        };
        let json = serde_json::to_value(preview).unwrap();
        assert_eq!(json.as_object().unwrap().len(), 5);
        assert!(json.get("share_id").is_none());
        assert!(json.get("source_author_id").is_none());
    }

    #[test]
    fn place_lookup_provider_query_matches_preview_and_bounds_combined_named_text() {
        assert_eq!(
            provider_query("bars", "named", Some("Bergen station")).as_deref(),
            Some("barar og pubar: bars nær Bergen station")
        );
        assert_eq!(
            provider_query("bars in Bergen", "none", None).as_deref(),
            Some("barar og pubar: bars in Bergen")
        );
        assert_eq!(
            provider_query("København", "none", None).as_deref(),
            Some("barar og pubar: København")
        );
        assert_eq!(
            provider_query("craft beer og mat nær meg", "shared", None).as_deref(),
            Some("barar og pubar: craft beer og mat nær meg")
        );
        assert!(provider_query("bars", "named", None).is_none());
        // The limit counts Unicode characters, including the fixed prefix and separator.
        assert!(provider_query(&"ø".repeat(218), "named", Some("Å")).is_some());
        assert!(provider_query(&"ø".repeat(219), "named", Some("Å")).is_none());
        let too_long =
            json!({"query":"a".repeat(200),"origin_mode":"named","named_origin":"b".repeat(40)});
        assert!(parse_plan(&too_long.to_string()).is_err());
    }

    #[test]
    fn place_lookup_preview_binding_changes_with_plan_and_exact_grant() {
        let original = PlaceLookupDescriptor {
            job_id: "job".into(),
            agent_id: "agent".into(),
            source_message_id: "source".into(),
            source_author_id: "author".into(),
            share_id: Some("grant-one".into()),
            query: "Barar i nærleiken".into(),
            origin_mode: "shared".into(),
            named_origin: None,
        };
        let binding = original.preview_binding();
        assert_eq!(binding.len(), 64);
        assert_eq!(binding, original.clone().preview_binding());
        let mut changed = original.clone();
        changed.share_id = Some("grant-two".into());
        assert_ne!(binding, changed.preview_binding());
        changed = original.clone();
        changed.query = "another place".into();
        assert_ne!(binding, changed.preview_binding());
        changed = original.clone();
        changed.named_origin = Some("station".into());
        assert_ne!(binding, changed.preview_binding());
        changed = original;
        changed.source_author_id = "another author".into();
        assert_ne!(binding, changed.preview_binding());
    }

    #[test]
    fn place_lookup_plan_is_bounded_and_has_no_model_supplied_authority() {
        for bad in [
            r#"{"query":"bars","origin_mode":"shared","latitude":1}"#,
            r#"{"query":"https://example.com","origin_mode":"none"}"#,
            r#"{"query":"bars","origin_mode":"named"}"#,
            r#"{"query":"bars","origin_mode":"none","named_origin":"station"}"#,
            r#"{"query":"bars","origin_mode":"guessed"}"#,
        ] {
            assert!(parse_plan(bad).is_err());
        }
        assert!(parse_plan(r#"{"query":"bars in Bergen","origin_mode":"none"}"#).is_ok());
        assert!(parse_plan(r#"{"query":"bars near Bergen station","origin_mode":"named","named_origin":"Bergen station"}"#).is_ok());
    }
}

#[cfg(test)]
pub(crate) mod database_tests {
    use super::*;
    pub(crate) async fn fixture() -> (CircleChatAgents, SqlitePool, UserId) {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations/sqlite")
            .run(&pool)
            .await
            .unwrap();
        let actor = UserId::from_uuid(Uuid::now_v7());
        sqlx::query("insert into users(id,kind,display_name) values(?,'human','Owner')")
            .bind(actor.to_string())
            .execute(&pool)
            .await
            .unwrap();
        let service = CircleChatAgents {
            store: Store::Sqlite(pool.clone()),
            model: None,
            worker_enabled: false,
            weather: None,
            sea_temperature: None,
            ferry: None,
            observations: None,
            imagegen: None,
        };
        (service, pool, actor)
    }
    #[tokio::test]
    async fn place_lookup_admission_is_shared_token_fenced_and_crash_recoverable() {
        let (service, pool, actor) = fixture().await;
        let first = service.claim_place_lookup(&actor).await.unwrap().unwrap();
        let second_service = service.clone();
        assert!(
            second_service
                .claim_place_lookup(&actor)
                .await
                .unwrap()
                .is_none()
        );
        service
            .release_place_lookup(&actor, "wrong-token")
            .await
            .unwrap();
        assert!(
            second_service
                .claim_place_lookup(&actor)
                .await
                .unwrap()
                .is_none()
        );
        service.release_place_lookup(&actor, &first).await.unwrap();
        assert!(
            second_service
                .claim_place_lookup(&actor)
                .await
                .unwrap()
                .is_none(),
            "cooldown survives release"
        );
        sqlx::query("update place_lookup_admissions set leased_until=0,next_allowed=0")
            .execute(&pool)
            .await
            .unwrap();
        let next = second_service
            .claim_place_lookup(&actor)
            .await
            .unwrap()
            .unwrap();
        assert_ne!(first, next);
        service.release_place_lookup(&actor, &first).await.unwrap();
        assert!(
            second_service
                .claim_place_lookup(&actor)
                .await
                .unwrap()
                .is_none(),
            "stale release cannot clear fresh lease"
        );
        sqlx::query("update place_lookup_admissions set leased_until=0,next_allowed=0,calls=10")
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            service.claim_place_lookup(&actor).await.unwrap().is_none(),
            "minute quota survives lease expiry"
        );
        sqlx::query("update place_lookup_admissions set window_started=0")
            .execute(&pool)
            .await
            .unwrap();
        assert!(service.claim_place_lookup(&actor).await.unwrap().is_some());
    }
}
