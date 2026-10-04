//! Public, requester-authored additions to a case. Heart still owns every task.
use super::*;

#[derive(Clone, Deserialize)]
pub(crate) struct SupplementCommand {
    pub source_message_id: Uuid,
    pub request_id: Uuid,
    pub expected_revision: i64,
    pub body: String,
}

#[cfg(test)]
pub(super) async fn exercise_supplements(
    service: &WorkItems,
    owner: Uuid,
    reviewer: Uuid,
    channel: Uuid,
    source: Uuid,
    task: &TaskView,
) -> SupplementCommand {
    let read = service
        .source_items(UserId::from_uuid(owner), channel, source)
        .await
        .unwrap();
    let view = read
        .iter()
        .find(|item| item.id == task.work_item_id)
        .unwrap();
    assert!(view.can_supplement);
    let command = SupplementCommand {
        source_message_id: source,
        request_id: Uuid::now_v7(),
        expected_revision: view.revision,
        body: "The original report also happens after reconnect: <b>literal text</b>".into(),
    };
    let gated = WorkItems {
        supplements_enabled: false,
        ..service.clone()
    };
    assert!(
        !gated
            .source_items(UserId::from_uuid(owner), channel, source)
            .await
            .unwrap()
            .into_iter()
            .find(|value| value.id == task.work_item_id)
            .unwrap()
            .can_supplement
    );
    assert!(matches!(
        gated
            .add_supplement(UserId::from_uuid(owner), task.work_item_id, command.clone())
            .await,
        Err(RepositoryError::Conflict)
    ));
    for bad in [
        SupplementCommand {
            body: " ".into(),
            ..command.clone()
        },
        SupplementCommand {
            body: "ø".repeat(4001),
            ..command.clone()
        },
        SupplementCommand {
            source_message_id: Uuid::now_v7(),
            ..command.clone()
        },
    ] {
        assert!(
            service
                .add_supplement(UserId::from_uuid(owner), task.work_item_id, bad)
                .await
                .is_err()
        );
    }
    assert!(matches!(
        service
            .add_supplement(
                UserId::from_uuid(reviewer),
                task.work_item_id,
                command.clone()
            )
            .await,
        Err(RepositoryError::PermissionDenied)
    ));
    assert!(matches!(
        service
            .source_items(UserId::from_uuid(Uuid::now_v7()), channel, source)
            .await,
        Err(RepositoryError::NotFound)
    ));
    assert!(matches!(
        service
            .source_items(UserId::from_uuid(owner), Uuid::now_v7(), source)
            .await,
        Err(RepositoryError::NotFound)
    ));
    macro_rules! role {
        ($pool:expr,$pg:expr,$role:expr) => {{
            sqlx::query(&sql(
                "update channel_memberships set role=? where channel_id=?uuid and user_id=?uuid",
                $pg,
            ))
            .bind($role)
            .bind(channel.to_string())
            .bind(owner.to_string())
            .execute($pool)
            .await
            .unwrap();
        }};
    }
    match &service.store {
        Store::Pg(p) => role!(p, true, "observer"),
        Store::Sqlite(p) => role!(p, false, "observer"),
    };
    assert!(matches!(
        service
            .add_supplement(UserId::from_uuid(owner), task.work_item_id, command.clone())
            .await,
        Err(RepositoryError::PermissionDenied)
    ));
    assert!(
        !service
            .source_items(UserId::from_uuid(owner), channel, source)
            .await
            .unwrap()
            .iter()
            .find(|v| v.id == task.work_item_id)
            .unwrap()
            .can_supplement
    );
    match &service.store {
        Store::Pg(p) => role!(p, true, "owner"),
        Store::Sqlite(p) => role!(p, false, "owner"),
    };
    let accepted = service
        .add_supplement(UserId::from_uuid(owner), task.work_item_id, command.clone())
        .await
        .unwrap();
    assert_eq!(accepted.revision, command.expected_revision + 1);
    assert_eq!(accepted.supplements.len(), 1);
    assert_eq!(accepted.supplements[0].body, command.body);
    assert_eq!(
        accepted.description, task.description,
        "Original description is immutable"
    );
    let again = service
        .clone()
        .add_supplement(UserId::from_uuid(owner), task.work_item_id, command.clone())
        .await
        .unwrap();
    assert_eq!(again.supplements[0].id, accepted.supplements[0].id);
    assert_eq!(again.revision, accepted.revision);
    // An earlier authorized view cannot grant access to a later data read.
    macro_rules! revoke {
        ($pool:expr,$pg:expr) => {{
            sqlx::query(&sql(
                "delete from channel_memberships where channel_id=?uuid and user_id=?uuid",
                $pg,
            ))
            .bind(channel.to_string())
            .bind(owner.to_string())
            .execute($pool)
            .await
            .unwrap();
        }};
    }
    match &service.store {
        Store::Pg(p) => revoke!(p, true),
        Store::Sqlite(p) => revoke!(p, false),
    };
    assert!(
        service
            .supplements(
                &owner.to_string(),
                task.work_item_id,
                SupplementScope::Source {
                    channel,
                    message: source
                }
            )
            .await
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        service
            .add_supplement(UserId::from_uuid(owner), task.work_item_id, command.clone())
            .await,
        Err(RepositoryError::PermissionDenied)
    ));
    macro_rules! restore {($pool:expr,$pg:expr)=>{{
        sqlx::query(&sql("insert into channel_memberships(channel_id,user_id,role) values(?uuid,?uuid,'owner')",$pg)).bind(channel.to_string()).bind(owner.to_string()).execute($pool).await.unwrap();
    }};}
    match &service.store {
        Store::Pg(p) => restore!(p, true),
        Store::Sqlite(p) => restore!(p, false),
    };
    assert!(
        service
            .supplements(
                &owner.to_string(),
                task.work_item_id,
                SupplementScope::Task {
                    id: task.id,
                    message: Uuid::now_v7()
                }
            )
            .await
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        service
            .add_supplement(
                UserId::from_uuid(owner),
                task.work_item_id,
                SupplementCommand {
                    body: "Different payload".into(),
                    ..command.clone()
                }
            )
            .await,
        Err(RepositoryError::Conflict)
    ));
    assert!(matches!(
        service
            .add_supplement(
                UserId::from_uuid(owner),
                task.work_item_id,
                SupplementCommand {
                    request_id: Uuid::now_v7(),
                    ..command.clone()
                }
            )
            .await,
        Err(RepositoryError::Conflict)
    ));
    let exposed = serde_json::to_value(&again).unwrap();
    for internal in [
        "reviewer_id",
        "task_channel_id",
        "category",
        "priority",
        "internal_note",
        "heart_instance_id",
        "decision_status",
        "information_request",
    ] {
        assert!(exposed.get(internal).is_none());
    }
    let handler = service
        .task(UserId::from_uuid(reviewer), task.id, task.message_id)
        .await
        .unwrap();
    assert_eq!(handler.supplements[0].body, command.body);
    assert_eq!(handler.node_id, task.node_id);
    assert_eq!(handler.status, "pending");
    assert_eq!(handler.delivery_status, "ready");
    assert!(
        matches!(
            service
                .decide(
                    UserId::from_uuid(reviewer),
                    task.id,
                    Decision {
                        message_id: task.message_id,
                        request_id: Uuid::now_v7(),
                        expected_revision: handler.revision,
                        expected_supplement_id: None,
                        category: "bug".into(),
                        priority: "normal".into(),
                        status: "planned".into(),
                        note: String::new(),
                    }
                )
                .await,
            Err(RepositoryError::Conflict)
        ),
        "An old browser may poll the latest revision without understanding supplements"
    );
    assert_eq!(
        gated
            .add_supplement(UserId::from_uuid(owner), task.work_item_id, command.clone())
            .await
            .unwrap()
            .supplements[0]
            .id,
        accepted.supplements[0].id,
        "Disabling new additions must retain existing receipts"
    );
    assert!(matches!(
        service
            .decide(
                UserId::from_uuid(reviewer),
                task.id,
                Decision {
                    expected_supplement_id: None,
                    message_id: task.message_id,
                    request_id: Uuid::now_v7(),
                    expected_revision: task.revision,
                    category: "bug".into(),
                    priority: "normal".into(),
                    status: "planned".into(),
                    note: String::new()
                }
            )
            .await,
        Err(RepositoryError::Conflict)
    ));
    command
}

#[derive(Serialize)]
pub(crate) struct Supplement {
    pub id: Uuid,
    pub actor_name: String,
    pub body: String,
    pub created_at: String,
}

// Deliberately separate from TaskView: no reviewer, internal notes or task identity.
#[derive(Serialize)]
pub(crate) struct SourceWorkItem {
    pub id: Uuid,
    pub source_message_id: Uuid,
    pub revision: i64,
    pub title: String,
    pub description: String,
    pub application_name: String,
    pub status: String,
    pub can_supplement: bool,
    pub supplements: Vec<Supplement>,
}

const OPEN_REVIEW: &str = "w.process_status='waiting' and w.status in ('new','reviewing') and exists(select 1 from work_item_tasks t where t.work_item_id=w.id and t.node_id in ('review','followup-review') and t.status='pending' and t.delivery_status='ready' and t.decision_request_id is null) and (select count(*) from work_item_supplements s where s.work_item_id=w.id)<100";

pub(super) enum SupplementScope {
    Source { channel: Uuid, message: Uuid },
    Task { id: Uuid, message: Uuid },
    StatusTask { id: Uuid, message: Uuid },
}

impl WorkItems {
    pub(super) async fn supplements(
        &self,
        actor: &str,
        item: Uuid,
        scope: SupplementScope,
    ) -> Result<Vec<Supplement>> {
        // Revalidate access in the data query, including additions made after the
        // outer view was read. Revoking membership must not reveal newer text.
        let (scope_id, message, access) = match scope {
            SupplementScope::Source { channel, message } => (
                channel,
                message,
                "exists(select 1 from work_items w join channel_memberships cm on cm.channel_id=w.source_channel_id where w.id=s.work_item_id and w.source_channel_id=?uuid and w.source_message_id=?uuid and cm.user_id=?uuid)",
            ),
            SupplementScope::Task { id, message } => (
                id,
                message,
                "exists(select 1 from work_item_tasks t join channel_memberships cm on cm.channel_id=t.channel_id where t.work_item_id=s.work_item_id and t.id=?uuid and t.message_id=?uuid and cm.user_id=?uuid)",
            ),
            SupplementScope::StatusTask { id, message } => (
                id,
                message,
                "exists(select 1 from work_item_status_changes t join channel_memberships cm on cm.channel_id=t.channel_id where t.work_item_id=s.work_item_id and t.task_id=?uuid and t.message_id=?uuid and cm.user_id=?uuid)",
            ),
        };
        macro_rules! read { ($pool:expr,$pg:expr) => {{
            let timestamp=if $pg { "to_char(s.created_at at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')" } else { "s.created_at" };
            let query=sql(&format!("select cast(s.id as text) as id,u.display_name as actor_name,s.body,{timestamp} as created_at from work_item_supplements s join users u on u.id=s.actor_id where s.work_item_id=?uuid and {access} order by s.expected_revision limit 100"),$pg);
            sqlx::query(&query).bind(item.to_string()).bind(scope_id.to_string()).bind(message.to_string()).bind(actor).fetch_all($pool).await.map_err(storage)?.into_iter().map(|r| Ok(Supplement {
                id: Uuid::parse_str(&r.try_get::<String,_>("id").map_err(storage)?).map_err(storage)?,
                actor_name:r.try_get("actor_name").map_err(storage)?,body:r.try_get("body").map_err(storage)?,created_at:r.try_get("created_at").map_err(storage)?
            })).collect()
        }}; }
        match &self.store {
            Store::Pg(p) => read!(p, true),
            Store::Sqlite(p) => read!(p, false),
        }
    }

    pub async fn source_items(
        &self,
        actor: UserId,
        channel: Uuid,
        message: Uuid,
    ) -> Result<Vec<SourceWorkItem>> {
        self.read_source_items(actor, channel, message, None).await
    }

    async fn read_source_items(
        &self,
        actor: UserId,
        channel: Uuid,
        message: Uuid,
        item: Option<Uuid>,
    ) -> Result<Vec<SourceWorkItem>> {
        macro_rules! read { ($pool:expr,$pg:expr) => {{
            let access=sql("select 1 from messages m join channel_memberships cm on cm.channel_id=m.channel_id where cm.user_id=?uuid and m.channel_id=?uuid and m.id=?uuid",$pg);
            if sqlx::query_scalar::<_,i32>(&access).bind(actor.to_string()).bind(channel.to_string()).bind(message.to_string()).fetch_optional($pool).await.map_err(storage)?.is_none() { return Err(RepositoryError::NotFound); }
            let query=sql(&format!("select cast(w.id as text) as id,w.revision,w.title,w.description,a.name as application_name,w.status,case when w.requested_by=cm.user_id and cm.role<>'observer' and {OPEN_REVIEW} then 1 else 0 end as allowed from work_items w join work_applications a on a.id=w.application_id join channel_memberships cm on cm.channel_id=w.source_channel_id and cm.user_id=?uuid where w.source_channel_id=?uuid and w.source_message_id=?uuid and (?uuid is null or w.id=?uuid) order by w.created_at desc,w.id limit 100"),$pg);
            let rows=sqlx::query(&query).bind(actor.to_string()).bind(channel.to_string()).bind(message.to_string()).bind(item.map(|id|id.to_string())).bind(item.map(|id|id.to_string())).fetch_all($pool).await.map_err(storage)?;
            let mut result=Vec::with_capacity(rows.len());
            for row in rows {
                let id=Uuid::parse_str(&row.try_get::<String,_>("id").map_err(storage)?).map_err(storage)?;
                result.push(SourceWorkItem {id,source_message_id:message,revision:row.try_get("revision").map_err(storage)?,title:row.try_get("title").map_err(storage)?,description:row.try_get("description").map_err(storage)?,application_name:row.try_get("application_name").map_err(storage)?,status:row.try_get("status").map_err(storage)?,can_supplement:self.supplements_enabled && row.try_get::<i32,_>("allowed").map_err(storage)?==1,supplements:self.supplements(&actor.to_string(),id,SupplementScope::Source{channel,message}).await?});
            }
            Ok(result)
        }}; }
        match &self.store {
            Store::Pg(p) => read!(p, true),
            Store::Sqlite(p) => read!(p, false),
        }
    }

    pub async fn add_supplement(
        &self,
        actor: UserId,
        item: Uuid,
        command: SupplementCommand,
    ) -> Result<SourceWorkItem> {
        if command.expected_revision < 1
            || command.body.trim().is_empty()
            || command.body.len() > 8000
        {
            return Err(RepositoryError::Conflict);
        }
        macro_rules! save { ($pool:expr,$pg:expr) => {{
            let mut tx=$pool.begin().await.map_err(storage)?;
            let mut rights=sql("select cast(w.source_channel_id as text) as channel from work_items w join channel_memberships cm on cm.channel_id=w.source_channel_id and cm.user_id=w.requested_by and cm.role<>'observer' where w.id=?uuid and w.source_message_id=?uuid and w.requested_by=?uuid",$pg);
            // A concurrent membership revocation cannot overtake this authorized write.
            if $pg { rights.push_str(" for update of w for share of cm"); }
            let channel=sqlx::query_scalar::<_,String>(&rights).bind(item.to_string()).bind(command.source_message_id.to_string()).bind(actor.to_string()).fetch_optional(&mut *tx).await.map_err(storage)?.ok_or(RepositoryError::PermissionDenied)?;
            let existing=sqlx::query(&sql("select cast(work_item_id as text) as item,expected_revision,body from work_item_supplements where actor_id=?uuid and request_id=?uuid",$pg)).bind(actor.to_string()).bind(command.request_id.to_string()).fetch_optional(&mut *tx).await.map_err(storage)?;
            if let Some(row)=existing {
                if row.try_get::<String,_>("item").map_err(storage)?!=item.to_string() || row.try_get::<i64,_>("expected_revision").map_err(storage)?!=command.expected_revision || row.try_get::<String,_>("body").map_err(storage)?!=command.body { return Err(RepositoryError::Conflict); }
            } else {
                if !self.supplements_enabled { return Err(RepositoryError::Conflict); }
                // Same revision CAS as decide(): only an addition or the decision can win.
                let bump=sql(&format!("update work_items as w set revision=revision+1 where w.id=?uuid and w.revision=? and {OPEN_REVIEW}"),$pg);
                if sqlx::query(&bump).bind(item.to_string()).bind(command.expected_revision).execute(&mut *tx).await.map_err(storage)?.rows_affected()!=1 { return Err(RepositoryError::Conflict); }
                sqlx::query(&sql("insert into work_item_supplements(id,work_item_id,actor_id,request_id,expected_revision,body) values(?uuid,?uuid,?uuid,?uuid,?,?)",$pg)).bind(Uuid::now_v7().to_string()).bind(item.to_string()).bind(actor.to_string()).bind(command.request_id.to_string()).bind(command.expected_revision).bind(&command.body).execute(&mut *tx).await.map_err(storage)?;
            }
            tx.commit().await.map_err(storage)?;
            self.read_source_items(actor,Uuid::parse_str(&channel).map_err(storage)?,command.source_message_id,Some(item)).await?.into_iter().next().ok_or(RepositoryError::NotFound)
        }}; }
        match &self.store {
            Store::Pg(p) => save!(p, true),
            Store::Sqlite(p) => save!(p, false),
        }
    }
}
