alter table circle_chat_agents add column image_generation text;
create table agent_image_publications (
 id text primary key,
 agent_id text not null references circle_chat_agents(agent_id) on delete cascade,
 channel_id text not null references channels(id) on delete cascade,
 source_message_id text not null references messages(id) on delete cascade,
 text_job_id text not null references circle_chat_agent_jobs(id) on delete cascade,
 source_sha256 text not null,
 config_revision integer not null,
 access_revision integer not null,
 identity_id text not null,
 identity_sha256 text not null,
 image_job_id text unique references image_generation_jobs(id),
 image_revision bigint,
 state text not null default 'pending' check(state in ('pending','admitting','queued','publishing','published','skipped','failed')),
 mode text check(mode in ('explicit','occasional')),
 lease_token text,
 leased_until integer,
 attempts integer not null default 0,
 reserved_at integer,
 created_at integer not null,
 updated_at integer not null,
 expires_at integer not null,
 media_id text references media_objects(id),
 reply_message_id text references messages(id),
 error_code text,
 unique(agent_id,source_message_id)
);
create index agent_image_work_queue on agent_image_publications(state,updated_at);
create index agent_image_daily_quota on agent_image_publications(agent_id,reserved_at) where reserved_at is not null;
