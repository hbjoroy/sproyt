// Both backends write the existing portable format from one transaction. Keep
// nested arrays on separate cursor steps because SQLx permits one active query
// per transaction. Flat arrays can stream one row at a time.
macro_rules! export_rows {
    ($writer:ident, $tx:ident, $query:expr, $convert:expr) => {{
        use futures_util::TryStreamExt;
        let mut rows = $query.fetch(&mut *$tx);
        let mut prefix: &'static [u8] = b"";
        while let Some(row) = rows.try_next().await.map_err(super::sql_error)? {
            $writer.value(prefix, &($convert)(row)?).await?;
            prefix = b",";
        }
    }};
}
pub(crate) use export_rows;

pub(crate) fn bounded_export_text(column: &str, bytes: usize, pg: bool) -> String {
    let length = if pg {
        format!("octet_length({column})")
    } else {
        format!("length(cast({column} as blob))")
    };
    // Required fields decode as non-null. An over-limit stored row fails the
    // export explicitly before the database sends the oversized value.
    format!("case when {length}<={bytes} then {column} end as {column}")
}

pub(crate) fn string_record<R>(
    row: R,
    keys: &[&str],
) -> Result<serde_json::Value, crate::domain::RepositoryError>
where
    R: sqlx::Row,
    for<'a> &'a str: sqlx::ColumnIndex<R>,
    for<'a> Option<String>: sqlx::Decode<'a, R::Database> + sqlx::Type<R::Database>,
{
    let mut record = serde_json::Map::new();
    for key in keys {
        let value: Option<String> = row.try_get(*key).map_err(super::sql_error)?;
        record.insert(
            (*key).into(),
            value
                .map(serde_json::Value::String)
                .unwrap_or(serde_json::Value::Null),
        );
    }
    Ok(serde_json::Value::Object(record))
}

macro_rules! write_account_export {
    ($tx:ident, $pg:expr, $actor:ident, $writer:ident) => {{
        use super::account_export::export_rows;
        use crate::chatbot::sql;
        use crate::domain::{ExportedCircle, PORTABLE_USER_EXPORT_FORMAT};

        let actor_id = $actor.to_string();
        let row = sqlx::query(&sql("select cast(id as text) as id,kind,display_name,handle,external_provider,external_subject,created_at from users where id=?uuid", $pg))
            .bind(&actor_id).fetch_optional(&mut *$tx).await.map_err(sql_error)?
            .ok_or(RepositoryError::NotFound)?;
        let kind: String = row.try_get("kind").map_err(storage)?;
        let user = User {
            id: UserId::new(row.try_get::<String, _>("id").map_err(storage)?).map_err(storage)?,
            kind: crate::domain::PrincipalKind::parse(&kind).ok_or_else(|| storage("invalid principal kind"))?,
            display_name: DisplayName::new(row.try_get::<String, _>("display_name").map_err(storage)?).map_err(storage)?,
            handle: row.try_get::<Option<String>, _>("handle").map_err(storage)?.map(Handle::new).transpose().map_err(storage)?,
            external_provider: row.try_get("external_provider").map_err(storage)?,
            external_subject: row.try_get("external_subject").map_err(storage)?,
            created_at: row.try_get("created_at").map_err(storage)?,
        };
        let signup_ordinal = sqlx::query_scalar::<_, i64>(&sql("select ordinal from signup_ordinals where user_id=?uuid", $pg))
            .bind(&actor_id).fetch_optional(&mut *$tx).await.map_err(sql_error)?
            .map(|ordinal| u64::try_from(ordinal).map_err(|_| storage("invalid signup ordinal"))).transpose()?;

        $writer.value(b"{\"format\":", &PORTABLE_USER_EXPORT_FORMAT).await?;
        $writer.value(b",\"exported_at\":", &Utc::now()).await?;
        $writer.value(b",\"user\":", &user).await?;
        $writer.value(b",\"signup_ordinal\":", &signup_ordinal).await?;

        $writer.token(b",\"saved_emojis\":[").await?;
        let query = sql("select emoji from personal_emojis where user_id=?uuid order by slot", $pg);
        export_rows!($writer, $tx, sqlx::query_scalar::<_, String>(&query).bind(&actor_id), Ok::<_, RepositoryError>);
        $writer.token(b"],\"saved_statuses\":[").await?;
        let status_order = if $pg { r#"text collate "C",emoji collate "C""# } else { "text,emoji" };
        let query = sql(&format!("select text,emoji,save_count,last_used_at from personal_statuses where user_id=?uuid order by save_count desc,last_used_at desc,{status_order}"), $pg);
        export_rows!($writer, $tx, sqlx::query(&query).bind(&actor_id), saved_status_from_row);
        $writer.token(b"],\"favorite_circles\":[").await?;
        let query = sql("select cast(circle_id as text) from circle_memberships where user_id=?uuid and is_favorite", $pg);
        let query = format!("{query} order by circle_id");
        export_rows!($writer, $tx, sqlx::query_scalar::<_, String>(&query).bind(&actor_id), |id: String| {
            Uuid::parse_str(&id).map(CircleId::from_uuid).map_err(storage)
        });
        $writer.token(b"]").await?;
        let markdown = super::account_export::bounded_export_text("note_markdown", 64 * 1024, $pg);
        let note = sqlx::query_as(&sql(&format!("select {markdown},note_revision from personal_spaces where owner_id=?uuid"), $pg))
            .bind(&actor_id).fetch_optional(&mut *$tx).await.map_err(sql_error)?
            .map(|(markdown, revision)| crate::domain::PersonalNote { markdown, revision });
        if let Some(note) = note {
            $writer.value(b",\"personal_note\":", &note).await?;
        }

        $writer.token(b",\"circles\":[").await?;
        let query = sql("select c.id,c.slug,c.name,c.created_by,c.created_at,m.role from circles c join circle_memberships m on m.circle_id=c.id where m.user_id=?uuid order by c.slug", $pg);
        export_rows!($writer, $tx, sqlx::query(&query).bind(&actor_id), |row| {
            circle_with_role(row).map(|(circle, role)| ExportedCircle { circle, role })
        });

        $writer.token(b"],\"channels\":[").await?;
        let channel_query = sql("select c.id,c.slug,c.name,c.kind,c.circle_id,c.description,
            (select case when d.user_a_id=?uuid then d.user_b_id else d.user_a_id end from direct_conversations d where d.channel_id=c.id) as direct_user_id,
            (exists(select 1 from direct_conversations d where d.channel_id=c.id) or exists(select 1 from direct_group_conversations g where g.channel_id=c.id)) as is_direct,
            exists(select 1 from personal_spaces p where p.channel_id=c.id) as is_personal, c.hidden,coalesce(c.events_enabled,exists(select 1 from channel_events ce where ce.channel_id=c.id)) as events_enabled,
            m.role,m.last_read_sequence,coalesce((select max(sequence) from messages where channel_id=c.id),0) as latest_sequence
            from channels c join channel_memberships m on m.channel_id=c.id
            where m.user_id=?uuid and (cast(? as text) is null or c.slug>?) order by c.slug limit 1", $pg);
        let body = super::account_export::bounded_export_text("body", 64 * 1024, $pg);
        let sender = super::account_export::bounded_export_text("sender_display_name", 1024, $pg);
        let message_query = sql(&format!("select id,channel_id,parent_message_id,sender_id,{sender},sequence,{body},created_at,edited_at,deleted_at
            from messages where channel_id=?uuid and sequence>?int order by sequence limit 16"), $pg);
        let mut slug: Option<String> = None;
        let mut prefix: &'static [u8] = b"{\"channel\":";
        while let Some(row) = sqlx::query(&channel_query).bind(&actor_id).bind(&actor_id)
            .bind(slug.as_deref()).bind(slug.as_deref())
            .fetch_optional(&mut *$tx).await.map_err(sql_error)?
        {
            slug = Some(row.try_get("slug").map_err(storage)?);
            let channel = channel_summary(row)?;
            let channel_id = channel.id.to_string();
            $writer.value(prefix, &channel).await?;
            $writer.token(b",\"messages\":[").await?;
            let mut sequence = 0_i64;
            let mut message_prefix: &'static [u8] = b"";
            loop {
                let page = sqlx::query(&message_query).bind(&channel_id).bind(sequence)
                    .fetch_all(&mut *$tx).await.map_err(sql_error)?;
                if page.is_empty() {
                    break;
                }
                let last_page = page.len() < 16;
                for row in page {
                    sequence = row.try_get("sequence").map_err(storage)?;
                    $writer.value(message_prefix, &chat_message(row)?).await?;
                    message_prefix = b",";
                }
                if last_page {
                    break;
                }
            }
            $writer.token(b"]}").await?;
            prefix = b",{\"channel\":";
        }

        $writer.token(b"],\"events_created\":[").await?;
        let description=super::account_export::bounded_export_text("description",20000,$pg);
        let query=sql(&format!("select id,cast(channel_id as text) as channel_id,code,title,starts_at,ends_at,timezone,location,{description},cast(cancelled as text) as cancelled,cast(revision as text) as revision,updated_at,cast(cover_media_id as text) as cover_media_id from channel_events where creator_id=?uuid order by id"),$pg);
        export_rows!($writer,$tx,sqlx::query(&query).bind(&actor_id),|row| super::account_export::string_record(row,&["id","channel_id","code","title","starts_at","ends_at","timezone","location","description","cancelled","revision","updated_at","cover_media_id"]));
        $writer.token(b"],\"event_responses\":[").await?;
        let query=sql("select event_id,status,updated_at,cast(seen_revision as text) as seen_revision from channel_event_responses where user_id=?uuid order by event_id",$pg);
        export_rows!($writer,$tx,sqlx::query(&query).bind(&actor_id),|row| super::account_export::string_record(row,&["event_id","status","updated_at","seen_revision"]));
        $writer.token(b"],\"event_image_contributions\":[").await?;
        let query=sql("select event_id,cast(media_id as text) as media_id from channel_event_images where contributor_id=?uuid order by event_id,media_id",$pg);
        export_rows!($writer,$tx,sqlx::query(&query).bind(&actor_id),|row| super::account_export::string_record(row,&["event_id","media_id"]));
        $writer.token(b"],\"agent_memories\":[").await?;
        let mut prefix: &'static [u8] = b"";
        crate::chatbot::memory::repository::for_each_export_memory!($tx, $pg, $actor, view, {
            $writer.value(prefix, &view).await?;
            prefix = b",";
        });
        $writer.token(b"]").await?;
        $tx.commit().await.map_err(sql_error)?;
        Ok(())
    }};
}
pub(crate) use write_account_export;
