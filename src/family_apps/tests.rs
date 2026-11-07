use super::*;

fn manifest(agent: &str) -> Manifest {
    Manifest {
        app_id: "open-drinks".into(),
        name: "Open Drinks".into(),
        version: 1,
        agent_id: agent.into(),
        sharing_notice: "Your answer and name are shared".into(),
        notice_version: 1,
        attributes: BTreeMap::from([(
            "participant_ref".into(),
            AttributeField::String { max_length: 64 },
        )]),
        scopes: vec![AppScope::ResponsesRead, AppScope::AttributesWrite],
    }
}
struct Fixture {
    apps: FamilyApps,
    owner: UserId,
    member: UserId,
    channel: String,
    circle: String,
    principal: AgentPrincipal,
}
impl Fixture {
    async fn new(pg: Option<PgPool>) -> Self {
        let agent = UserId::named(opaque());
        let owner = UserId::named(opaque());
        let member = UserId::named(opaque());
        let config = DatabaseConfig::new(if pg.is_some() {
            "postgres://test/test"
        } else {
            "sqlite::memory:"
        })
        .unwrap();
        let apps = FamilyApps::connect(&config, pg.as_ref(), vec![manifest(&agent.to_string())])
            .await
            .unwrap();
        match &apps.db {
            Db::Sqlite(pool) => sqlx::migrate!("./migrations/sqlite")
                .run(pool)
                .await
                .unwrap(),
            Db::Pg(pool) => sqlx::migrate!("./migrations/postgres")
                .run(pool)
                .await
                .unwrap(),
        };
        let channel = opaque();
        let circle = opaque();
        let credential = Uuid::now_v7();
        let mut tx = apps.db.begin().await.unwrap();
        for (id, kind, name) in [
            (&owner, "human", "Owner"),
            (&member, "human", "Member"),
            (&agent, "agent", "Open Drinks"),
        ] {
            tx.exec(
                "insert into users(id,kind,display_name) values($1::id,$2,$3)",
                &[&id.to_string(), kind, name],
            )
            .await
            .unwrap();
        }
        tx.exec(
            "insert into circles(id,slug,name,created_by) values($1::id,$2,'Circle',$3::id)",
            &[&circle, &opaque(), &owner.to_string()],
        )
        .await
        .unwrap();
        tx.exec(
            "insert into circle_memberships(circle_id,user_id,role) values($1::id,$2::id,'owner')",
            &[&circle, &owner.to_string()],
        )
        .await
        .unwrap();
        tx.exec(
            "insert into circle_memberships(circle_id,user_id,role) values($1::id,$2::id,'member')",
            &[&circle, &member.to_string()],
        )
        .await
        .unwrap();
        tx.exec("insert into channels(id,slug,name,kind,created_by,circle_id) values($1::id,$2,'Test','private',$3::id,$4::id)",&[&channel,&opaque(),&owner.to_string(),&circle]).await.unwrap();
        tx.exec("insert into channel_memberships(channel_id,user_id,role) values($1::id,$2::id,'owner')",&[&channel,&owner.to_string()]).await.unwrap();
        tx.exec("insert into channel_memberships(channel_id,user_id,role) values($1::id,$2::id,'member')",&[&channel,&member.to_string()]).await.unwrap();
        tx.exec("insert into agent_profiles(agent_id,owner_id,invited_by,provider,service_identity,purpose,rate_limit_per_minute,created_at) values($1::id,$2::id,$2::id,'open-drinks',$3,'Test',60,$4::time)",&[&agent.to_string(),&owner.to_string(),&opaque(),&Utc::now().to_rfc3339()]).await.unwrap();
        let token = opaque().replace('-', "");
        let byte_sql = match apps.db {
            Db::Pg(_) => format!("decode('{token}','hex')"),
            _ => format!("X'{token}'"),
        };
        let sql = format!(
            "insert into agent_credentials(id,agent_id,token_hash,expires_at,created_at) values($1::id,$2::id,{byte_sql},$3::time,$4::time)"
        );
        tx.exec(
            &sql,
            &[
                &credential.to_string(),
                &agent.to_string(),
                &(Utc::now() + chrono::Duration::days(1)).to_rfc3339(),
                &Utc::now().to_rfc3339(),
            ],
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        Self {
            apps,
            owner: owner.clone(),
            member,
            channel,
            circle,
            principal: AgentPrincipal {
                agent_id: agent,
                credential_id: credential,
                owner_id: owner,
                provider: "open-drinks".into(),
                purpose: "Test".into(),
                rate_limit_per_minute: 60,
            },
        }
    }
    async fn binding(&self, scope: ScopeRef) -> Binding {
        let auth = self
            .apps
            .prepare_authorization(&self.owner, "open-drinks", scope, "session-1")
            .await
            .unwrap();
        let mut authorization = proof();
        authorization.app_user_id = self.owner.to_string();
        self.apps
            .create_binding(
                &self.owner,
                &auth.state,
                authorization,
                Some("owner@example.test"),
            )
            .await
            .unwrap()
    }
}
fn proof() -> AuthorizationProof {
    AuthorizationProof {
        authorization_id: opaque(),
        app_user_id: "od-owner".into(),
        session_id: "session-1".into(),
        session_name: "Evening".into(),
        verified_email: Some("OWNER@example.test".into()),
        expires_at: now() + 120,
    }
}

#[tokio::test]
async fn manifest_limits_reject_reserved_fields() {
    let config = DatabaseConfig::new("sqlite::memory:").unwrap();
    let mut m = manifest(&opaque());
    m.attributes
        .insert("participation".into(), AttributeField::Boolean);
    assert!(matches!(
        FamilyApps::connect(&config, None, vec![m]).await,
        Err(FamilyError::Configuration)
    ));
}
async fn verify_contract(f: Fixture) {
    let b = f.binding(ScopeRef::Channel(f.channel.clone())).await;
    assert!(
        f.apps
            .snapshot(&f.principal, &b.binding_id)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    assert_eq!(
        f.apps
            .respond(&f.member, &b.binding_id, Participation::Yes, 2)
            .await
            .unwrap_err(),
        FamilyError::Invalid
    );
    f.apps
        .respond(&f.owner, &b.binding_id, Participation::Yes, 1)
        .await
        .unwrap();
    f.apps
        .respond(&f.member, &b.binding_id, Participation::No, 1)
        .await
        .unwrap();
    let snapshot = f.apps.snapshot(&f.principal, &b.binding_id).await.unwrap();
    assert_eq!(snapshot.items.len(), 2);
    assert_eq!(snapshot.changes_cursor, 2);
    let member = snapshot
        .items
        .iter()
        .find(|m| m.display_name == "Member")
        .unwrap();
    assert_eq!(member.response, json!({"participation":"no"}));
    assert_ne!(member.subject_id, f.member.to_string());
    let attrs = json!({"participant_ref":"od-123"});
    let changed = f
        .apps
        .replace_attributes(
            &f.principal,
            &b.binding_id,
            &member.subject_id,
            member.revision,
            "create-123",
            attrs.clone(),
        )
        .await
        .unwrap();
    assert_eq!(changed.revision, 2);
    assert_eq!(
        f.apps
            .replace_attributes(
                &f.principal,
                &b.binding_id,
                &member.subject_id,
                member.revision,
                "create-123",
                attrs
            )
            .await
            .unwrap()
            .revision,
        2
    );
    assert_eq!(
        f.apps
            .replace_attributes(
                &f.principal,
                &b.binding_id,
                &member.subject_id,
                member.revision,
                "create-123",
                json!({})
            )
            .await
            .unwrap_err(),
        FamilyError::Conflict
    );
    assert_eq!(
        f.apps
            .replace_attributes(
                &f.principal,
                &b.binding_id,
                &member.subject_id,
                member.revision,
                "stale",
                json!({})
            )
            .await
            .unwrap_err(),
        FamilyError::RevisionConflict
    );
    assert_eq!(
        f.apps
            .replace_attributes(
                &f.principal,
                &b.binding_id,
                &member.subject_id,
                2,
                "bad",
                json!({"participation":"yes"})
            )
            .await
            .unwrap_err(),
        FamilyError::Invalid
    );
    let events = f
        .apps
        .changes(&f.principal, &b.binding_id, 2, 100)
        .await
        .unwrap();
    assert_eq!(events[0].kind, "attributes.updated");
    f.apps.revoke(&f.member, &b.binding_id).await.unwrap();
    let events = f
        .apps
        .changes(&f.principal, &b.binding_id, 0, 100)
        .await
        .unwrap();
    assert!(
        events
            .iter()
            .filter(|e| e.subject_id == member.subject_id)
            .all(|e| e.kind == "response.revoked" && e.member.is_none())
    );
    assert_eq!(
        f.apps
            .replace_attributes(
                &f.principal,
                &b.binding_id,
                &member.subject_id,
                2,
                "withdrawn",
                json!({})
            )
            .await
            .unwrap_err(),
        FamilyError::Forbidden
    );
    f.apps
        .respond(&f.member, &b.binding_id, Participation::Yes, 1)
        .await
        .unwrap();
    let epoch = {
        let mut tx = f.apps.db.begin().await.unwrap();
        let epoch = f
            .apps
            .epoch(&mut tx, &b, &f.member.to_string())
            .await
            .unwrap();
        tx.exec(
            "delete from channel_memberships where channel_id=$1::id and user_id=$2::id",
            &[&f.channel, &f.member.to_string()],
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        epoch
    };
    assert_eq!(
        f.apps
            .snapshot(&f.principal, &b.binding_id)
            .await
            .unwrap()
            .items
            .len(),
        1
    );
    {
        let mut tx = f.apps.db.begin().await.unwrap();
        tx.exec("insert into channel_memberships(channel_id,user_id,role) values($1::id,$2::id,'member')",&[&f.channel,&f.member.to_string()]).await.unwrap();
        assert_ne!(
            epoch,
            f.apps
                .epoch(&mut tx, &b, &f.member.to_string())
                .await
                .unwrap()
        );
        tx.commit().await.unwrap();
    }
    assert_eq!(
        f.apps
            .snapshot(&f.principal, &b.binding_id)
            .await
            .unwrap()
            .items
            .len(),
        1
    );
    let rejoined = f
        .apps
        .respond(&f.member, &b.binding_id, Participation::No, 1)
        .await
        .unwrap();
    assert_eq!(rejoined.app_attributes, json!({}));
    let mut other = f.principal.clone();
    other.agent_id = UserId::named("different-app");
    assert_eq!(
        f.apps.snapshot(&other, &b.binding_id).await.unwrap_err(),
        FamilyError::NotFound
    );
    let circle_binding = f.binding(ScopeRef::Circle(f.circle.clone())).await;
    assert!(
        f.apps
            .snapshot(&f.principal, &circle_binding.binding_id)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    f.apps
        .respond(&f.member, &circle_binding.binding_id, Participation::Yes, 1)
        .await
        .unwrap();
    assert_eq!(
        f.apps
            .snapshot(&f.principal, &circle_binding.binding_id)
            .await
            .unwrap()
            .items[0]
            .subject_id,
        member.subject_id
    );
    f.apps.disable(&f.owner, &b.binding_id).await.unwrap();
    assert_eq!(
        f.apps
            .snapshot(&f.principal, &b.binding_id)
            .await
            .unwrap_err(),
        FamilyError::Disabled
    );
}
#[tokio::test]
async fn sqlite_contract() {
    verify_contract(Fixture::new(None).await).await;
}
#[tokio::test]
async fn postgres_upgrade_preserves_memberships_without_rewriting_tables() {
    let Ok(url) = std::env::var("SPROYT_POSTGRES_TEST_URL") else {
        return;
    };
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    let mut tx = pool.begin().await.unwrap();
    let schema = format!("family_upgrade_{}", Uuid::now_v7().simple());
    sqlx::raw_sql(&format!(
        "create schema {schema}; set local search_path = {schema}"
    ))
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::raw_sql(
        "create table users (id uuid primary key, display_name text);
         create table channels (id uuid primary key);
         create table circles (id uuid primary key);
         create table channel_memberships (channel_id uuid, user_id uuid, role text);
         create table circle_memberships (circle_id uuid, user_id uuid, role text);
         insert into channel_memberships (channel_id,user_id,role)
           values ('01900000-0000-7000-8000-000000000001','01900000-0000-7000-8000-000000000002','owner');
         insert into circle_memberships (circle_id,user_id,role)
           values ('01900000-0000-7000-8000-000000000003','01900000-0000-7000-8000-000000000002','member');",
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    let files = "select pg_relation_filenode('channel_memberships'::regclass)::bigint,
                        pg_relation_filenode('circle_memberships'::regclass)::bigint";
    let before: (i64, i64) = sqlx::query_as(files).fetch_one(&mut *tx).await.unwrap();

    sqlx::raw_sql(include_str!(
        "../../migrations/postgres/0071_family_apps.sql"
    ))
    .execute(&mut *tx)
    .await
    .unwrap();

    let after: (i64, i64) = sqlx::query_as(files).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(before, after);
    for (table, scope_column, role) in [
        ("channel_memberships", "channel_id", "owner"),
        ("circle_memberships", "circle_id", "member"),
    ] {
        let query = format!("select {scope_column},user_id,role,app_membership_epoch from {table}");
        let (scope, user, persisted_role, epoch): (Uuid, Uuid, String, String) =
            sqlx::query_as(&query).fetch_one(&mut *tx).await.unwrap();
        assert_eq!(persisted_role, role);
        assert_eq!(epoch, "legacy");

        let mut previous = epoch;
        for _ in 0..2 {
            sqlx::query(&format!(
                "delete from {table} where {scope_column}=$1 and user_id=$2"
            ))
            .bind(scope)
            .bind(user)
            .execute(&mut *tx)
            .await
            .unwrap();
            let epoch: String = sqlx::query_scalar(&format!(
                "insert into {table} ({scope_column},user_id,role) values ($1,$2,$3) returning app_membership_epoch"
            ))
            .bind(scope)
            .bind(user)
            .bind(role)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
            assert_ne!(epoch, previous);
            assert_ne!(epoch, "legacy");
            previous = epoch;
        }
    }
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn postgres_contract() {
    let Ok(url) = std::env::var("SPROYT_POSTGRES_TEST_URL") else {
        eprintln!("skipping family app PostgreSQL contract: SPROYT_POSTGRES_TEST_URL unset");
        return;
    };
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .unwrap();
    verify_contract(Fixture::new(Some(pool)).await).await;
}
#[tokio::test]
async fn authorization_is_single_use_bound_to_actor_and_verified_identity() {
    let f = Fixture::new(None).await;
    assert_eq!(
        f.apps
            .prepare_authorization(
                &f.member,
                "open-drinks",
                ScopeRef::Channel(f.channel.clone()),
                "session-1"
            )
            .await
            .err(),
        Some(FamilyError::Forbidden)
    );
    let auth = f
        .apps
        .prepare_authorization(
            &f.owner,
            "open-drinks",
            ScopeRef::Channel(f.channel.clone()),
            "session-1",
        )
        .await
        .unwrap();
    assert_eq!(
        f.apps
            .create_binding(&f.member, &auth.state, proof(), Some("owner@example.test"))
            .await
            .unwrap_err(),
        FamilyError::Forbidden
    );
    assert_eq!(
        f.apps
            .create_binding(&f.owner, &auth.state, proof(), None)
            .await
            .unwrap_err(),
        FamilyError::Forbidden
    );
    f.apps
        .create_binding(&f.owner, &auth.state, proof(), Some("owner@example.test"))
        .await
        .unwrap();
    assert_eq!(
        f.apps
            .create_binding(&f.owner, &auth.state, proof(), Some("owner@example.test"))
            .await
            .unwrap_err(),
        FamilyError::Forbidden
    );
}

async fn verify_concurrent_replicas(f: Fixture) {
    let b = f.binding(ScopeRef::Channel(f.channel.clone())).await;
    let replica = FamilyApps {
        db: f.apps.db.clone(),
        manifests: f.apps.manifests.clone(),
        environment: "canary".into(),
    };
    for _ in 0..6 {
        let (left, right) = tokio::join!(
            f.apps
                .respond(&f.owner, &b.binding_id, Participation::Yes, 1),
            replica.respond(&f.member, &b.binding_id, Participation::No, 1)
        );
        left.unwrap();
        right.unwrap();
    }
    let snapshot = f.apps.snapshot(&f.principal, &b.binding_id).await.unwrap();
    assert_eq!(snapshot.changes_cursor, 12);
    let events = f
        .apps
        .changes(&f.principal, &b.binding_id, 0, 100)
        .await
        .unwrap();
    assert_eq!(
        events.iter().map(|e| e.sequence).collect::<Vec<_>>(),
        (1..=12).collect::<Vec<_>>()
    );
    // Name changes advance export revisions without storing historical name/response payloads.
    {
        let mut tx = f.apps.db.begin().await.unwrap();
        tx.exec(
            "update users set display_name='Renamed member' where id=$1::id",
            &[&f.member.to_string()],
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }
    let events = f
        .apps
        .changes(&f.principal, &b.binding_id, 12, 100)
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].member.as_ref().unwrap().display_name,
        "Renamed member"
    );
    assert_eq!(
        f.apps
            .require_scope(&f.principal, &b.binding_id, AppScope::MessagesWrite)
            .await
            .unwrap_err(),
        FamilyError::Forbidden
    );
    // A cached principal cannot survive service credential revocation.
    {
        let mut tx = f.apps.db.begin().await.unwrap();
        tx.exec(
            "update agent_credentials set revoked_at=$2::time where id=$1::id",
            &[
                &f.principal.credential_id.to_string(),
                &Utc::now().to_rfc3339(),
            ],
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }
    assert_eq!(
        f.apps
            .snapshot(&f.principal, &b.binding_id)
            .await
            .unwrap_err(),
        FamilyError::Unauthorized
    );
    assert_eq!(
        f.apps.bindings(&f.principal).await.unwrap_err(),
        FamilyError::Unauthorized
    );
}
#[tokio::test]
async fn sqlite_serialized_revisions_and_credential_revocation() {
    verify_concurrent_replicas(Fixture::new(None).await).await
}
#[tokio::test]
async fn postgres_serialized_revisions_and_credential_revocation() {
    let Ok(url) = std::env::var("SPROYT_POSTGRES_TEST_URL") else {
        eprintln!("skipping family app PostgreSQL concurrency: test URL unset");
        return;
    };
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .unwrap();
    verify_concurrent_replicas(Fixture::new(Some(pool)).await).await
}
#[tokio::test]
async fn moderator_must_still_have_private_channel_access_at_confirmation() {
    let f = Fixture::new(None).await;
    {
        let mut tx = f.apps.db.begin().await.unwrap();
        tx.exec("update circle_memberships set role='moderator' where circle_id=$1::id and user_id=$2::id",&[&f.circle,&f.member.to_string()]).await.unwrap();
        tx.commit().await.unwrap();
    }
    let auth = f
        .apps
        .prepare_authorization(
            &f.member,
            "open-drinks",
            ScopeRef::Channel(f.channel.clone()),
            "session-1",
        )
        .await
        .unwrap();
    {
        let mut tx = f.apps.db.begin().await.unwrap();
        tx.exec(
            "delete from channel_memberships where channel_id=$1::id and user_id=$2::id",
            &[&f.channel, &f.member.to_string()],
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }
    assert_eq!(
        f.apps
            .create_binding(&f.member, &auth.state, proof(), Some("owner@example.test"))
            .await
            .unwrap_err(),
        FamilyError::Forbidden
    );
    assert_eq!(
        f.apps
            .pending_authorization(&f.member, &auth.state)
            .await
            .err(),
        Some(FamilyError::Forbidden)
    );
}
