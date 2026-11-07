use super::*;
use std::collections::HashSet;
use uuid::Uuid;

macro_rules! contract {
    ($name:ident,$connect:expr,$migrations:literal,$list:ident) => {
        #[tokio::test]
        async fn $name() {
            let Some(pool)=($connect) else{return};
            sqlx::migrate!($migrations).run(&pool).await.unwrap();
            let [owner,moderator,member,agent,circle,other,channel,hidden,cross,application,binding,message,work,export_route,change,legacy,pilot,task,change_task,foreign_task]=std::array::from_fn::<_,20,_>(|_|Uuid::now_v7());
            // All interpolation below is generated UUIDs/static fixture text.
            let seed=format!(r#"
insert into users(id,kind,display_name) values('{owner}','human','Owner'),('{moderator}','human','Moderator'),('{member}','human','Member'),('{agent}','agent','Agent');
insert into circles(id,slug,name,created_by) values('{circle}','overview-{circle}','Overview','{owner}'),('{other}','overview-{other}','Other','{owner}');
insert into circle_memberships(circle_id,user_id,role) values('{circle}','{owner}','owner'),('{circle}','{moderator}','moderator'),('{circle}','{member}','member'),('{circle}','{agent}','moderator'),('{other}','{owner}','owner');
insert into channels(id,slug,name,kind,created_by,circle_id) values('{channel}','overview-{channel}','Visible','public','{owner}','{circle}'),('{hidden}','overview-{hidden}','Hidden','private','{owner}','{circle}'),('{cross}','overview-{cross}','Other','public','{owner}','{other}');
insert into channel_memberships(channel_id,user_id,role) values('{channel}','{owner}','owner'),('{channel}','{moderator}','member'),('{channel}','{member}','moderator'),('{channel}','{agent}','moderator'),('{cross}','{owner}','owner');
insert into messages(id,channel_id,sender_id,sequence,body) values('{message}','{channel}','{owner}',1,'SECRET-SOURCE');
insert into work_applications(id,key,name,owner_circle_id,updated_by,created_at,updated_at) values('{application}','a-{application}','Application','{circle}','{owner}',current_timestamp,current_timestamp);
insert into channel_process_bindings(id,channel_id,process_key,namespace,definition_name,definition_version,updated_at) values('{binding}','{channel}','review','sproyt','work-item-review','1.1.0',current_timestamp);
insert into channel_task_routes(id,binding_id,channel_id,task_key,process_role) values('{export_route}','{binding}','{channel}','status','reviewer');
insert into work_items(id,source_channel_id,source_message_id,source_body,application_id,binding_id,binding_revision,title,description,requested_by,request_id,reviewer_id,task_channel_id,start_status,process_status) values('{work}','{channel}','{message}','SECRET-SOURCE','{application}','{binding}',1,'A useful case','SECRET-DESCRIPTION','{owner}','{work}','{owner}','{channel}','started','waiting');
insert into work_item_tasks(id,work_item_id,message_id,channel_id,assignee_id,node_id,status,decision_note) values('{task}','{work}','{message}','{channel}','{owner}','review','pending','SECRET-NOTE');
insert into work_item_export_processes(work_item_id,channel_id,assignee_id,status,created_at) values('{work}','{channel}','{owner}','failed',100);
insert into work_item_status_changes(id,work_item_id,actor_id,channel_id,route_id,source_task_id,source_message_id,request_id,start_revision,status,internal_note,created_at) values('{change}','{work}','{owner}','{channel}','{export_route}','{task}','{message}','{change}',1,'waiting','SECRET-NOTE',100);
insert into process_links(id,channel_id,namespace,definition_name,initiated_by,status,request_id,created_at,updated_at) values('{legacy}','{channel}','sproyt','event-planning','{owner}','active','{legacy}','2026-01-01 00:00:00+00:00','2026-01-01 00:00:00+00:00');
insert into process_pilot_channels(channel_id,assignee_id) values('{channel}','{owner}');
insert into process_pilot_runs(id,channel_id,actor_id,assignee_id,request_id,status,runtime_model,engine_url) values('{pilot}','{channel}','{owner}','{owner}','{pilot}','waiting','v2','SECRET-ENGINE');
insert into process_pilot_tasks(id,run_id,message_id,node_id,status) values('{change_task}','{pilot}','{message}','product-review','pending');
"#);
            sqlx::raw_sql(&seed).execute(&pool).await.unwrap();
            let event=Uuid::now_v7();
            let inspection=format!(r#"insert into process_events(id,process_link_id,event_key,event_type,payload,actor_id,occurred_at) values('{event}','{legacy}','inspect','process.inspected','{{"status":"completed","current_node":"review","secret":"SECRET-PAYLOAD"}}','{owner}','2026-01-02 00:00:00+00:00');"#);
            sqlx::raw_sql(&inspection).execute(&pool).await.unwrap();
            for index in 0..25 {
                let event=Uuid::now_v7();
                sqlx::raw_sql(&format!("insert into process_events(id,process_link_id,event_key,event_type,payload,actor_id,occurred_at) values('{event}','{legacy}','bounded-{index}','process.correlated','{{}}','{owner}','2026-01-02 00:00:00+00:00');")).execute(&pool).await.unwrap();
            }
            let query=CircleProcessQuery{circle:CircleId::from_uuid(circle),actor:UserId::from_uuid(owner),limit:2,all:true,after:None};
            let mut next=query.clone();
            let mut ids=HashSet::new();
            let mut rows=Vec::new();
            loop {
                let page=$list(&pool,next.clone()).await.unwrap();
                assert!(page.processes.len()<=2);
                for row in page.processes {assert!(ids.insert(row.id.clone()),"no repeated cursor rows");rows.push(row);}
                let Some(cursor)=page.next_cursor else{break};
                next.after=Some(cursor);
            }
            assert_eq!(rows.len(),5,"all five real process families are covered");
            let history=&rows.iter().find(|r|r.kind=="process").unwrap();
            assert!(history.history_truncated);assert_eq!(history.history.len(),20);
            assert_eq!(history.steps.len(),1,"actual latest Heart node, not transport events");
            assert_eq!(rows.iter().find(|r|r.kind=="process").unwrap().status,"completed");
            assert_eq!(rows.iter().find(|r|r.kind=="work_export").unwrap().status,"failed");
            assert!(rows.iter().all(|r|r.remaining_steps.is_none()&&r.projection));
            assert!(rows.iter().filter(|r|r.kind!="process").all(|r|r.history.is_empty()),"current tasks are not invented event history");
            let json=serde_json::to_string(&rows).unwrap();
            assert!(!json.contains("SECRET"));
            let mut open=query.clone();open.all=false;open.limit=50;
            assert_eq!($list(&pool,open.clone()).await.unwrap().processes.len(),4);
            // A newer terminal failure takes precedence over an old completed inspection.
            sqlx::raw_sql(&format!("update process_links set status='failed',updated_at='2026-01-03 00:00:00+00:00' where id='{legacy}';")).execute(&pool).await.unwrap();
            let page=$list(&pool,open.clone()).await.unwrap();
            assert_eq!(page.processes.len(),5);
            let legacy_row=page.processes.iter().find(|r|r.kind=="process").unwrap();
            assert_eq!(legacy_row.status,"failed");assert!(legacy_row.steps.is_empty());
            for actor in [member,agent] {
                let mut denied=query.clone();denied.actor=UserId::from_uuid(actor);
                assert!(matches!($list(&pool,denied).await,Err(RepositoryError::PermissionDenied)));
            }
            let mut mod_query=query.clone();mod_query.actor=UserId::from_uuid(moderator);mod_query.limit=50;
            assert_eq!($list(&pool,mod_query.clone()).await.unwrap().processes.len(),5);
            sqlx::raw_sql(&format!("delete from circle_memberships where circle_id='{circle}' and user_id='{moderator}';")).execute(&pool).await.unwrap();
            assert!(matches!($list(&pool,mod_query).await,Err(RepositoryError::PermissionDenied)));
            // Both private and open channels need actual channel membership.
            for target in [hidden,cross] {
                let id=Uuid::now_v7();
                sqlx::raw_sql(&format!("insert into process_links(id,channel_id,namespace,definition_name,initiated_by,status,request_id,created_at,updated_at) values('{id}','{target}','sproyt','Hidden','{owner}','active','{id}','2026-01-01 00:00:00+00:00','2026-01-01 00:00:00+00:00');")).execute(&pool).await.unwrap();
            }
            assert_eq!($list(&pool,open.clone()).await.unwrap().processes.len(),5);
            // Cross-channel work hides the whole row and its associated processes.
            sqlx::raw_sql(&format!("update work_items set task_channel_id='{hidden}' where id='{work}';")).execute(&pool).await.unwrap();
            assert_eq!($list(&pool,open.clone()).await.unwrap().processes.len(),2);
            sqlx::raw_sql(&format!("update work_items set task_channel_id='{channel}' where id='{work}'; update work_item_tasks set channel_id='{hidden}' where id='{task}';")).execute(&pool).await.unwrap();
            assert_eq!($list(&pool,open.clone()).await.unwrap().processes.len(),2);
            // Real status history exposes only status/time, never internal notes.
            let receipt=Uuid::now_v7();
            sqlx::raw_sql(&format!("update work_item_tasks set channel_id='{channel}' where id='{task}'; update work_item_status_changes set status='completed' where id='{change}'; insert into work_item_status_history(id,change_id,work_item_id,revision,actor_id,from_status,to_status,internal_note,public_feedback,created_at) values('{receipt}','{change}','{work}',2,'{owner}','reviewing','planned','SECRET-HISTORY','SECRET-FEEDBACK',1794050000);")).execute(&pool).await.unwrap();
            let mut all=query.clone();all.limit=50;
            let page=$list(&pool,all).await.unwrap();
            let row=page.processes.iter().find(|r|r.kind=="work_status").unwrap();
            assert_eq!(row.history[0].status,"planned");
            assert!(chrono::DateTime::parse_from_rfc3339(row.history[0].occurred_at.as_ref().unwrap()).is_ok());
            assert!(!serde_json::to_string(&page).unwrap().contains("SECRET"));
            assert!($list(&pool,open.clone()).await.unwrap().processes.iter().all(|r|r.kind!="work_status"));
            // Cursor scope/mode/size and limits fail closed.
            let mut wrong=query.clone();wrong.limit=1;
            let cursor=$list(&pool,wrong.clone()).await.unwrap().next_cursor.unwrap();
            wrong.after=Some(cursor);wrong.circle=CircleId::from_uuid(other);
            assert!(wrong.validate().is_err());wrong=query.clone();wrong.limit=51;assert!(wrong.validate().is_err());
            let _=foreign_task;
            pool.close().await;
        }
    }
}

contract!(
    sqlite_circle_processes_contract,
    Some(
        sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap()
    ),
    "./migrations/sqlite",
    list_sqlite
);
contract!(
    postgres_circle_processes_contract,
    {
        match std::env::var("SPROYT_POSTGRES_TEST_URL") {
            Ok(url) => Some(
                sqlx::postgres::PgPoolOptions::new()
                    .max_connections(2)
                    .connect(&url)
                    .await
                    .unwrap(),
            ),
            Err(_) => None,
        }
    },
    "./migrations/postgres",
    list_postgres
);
