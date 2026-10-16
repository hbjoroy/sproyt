-- Dedicated admission queue, invisible to ordinary workers.
create table agent_handoff_configs (
 agent_id text primary key references circle_chat_agents(agent_id) on delete cascade,
 revision integer not null default 1 check(revision>0),
 target_agent_ids text not null default '[]', guidance text not null default ''
);
create table agent_handoff_jobs (
 id text primary key,
 agent_id text not null references circle_chat_agents(agent_id) on delete cascade,
 source_message_id text not null references messages(id) on delete cascade,
 channel_id text not null references channels(id) on delete cascade,
 config_revision integer not null, access_revision integer not null,
 routing_revision integer not null,
 status text not null default 'pending' check(status in ('pending','leased','completed','skipped','failed')),
 available_at integer not null default 0,
 attempts integer not null default 0, lease_token text, leased_until integer,
 created_at integer not null, expires_at integer not null,
 recipient_agent_id text, unique(agent_id,source_message_id)
);
create index agent_handoff_jobs_ready on agent_handoff_jobs(status,leased_until,expires_at);
create trigger agent_handoff_ordinary_fence before insert on circle_chat_agent_jobs
when exists(select 1 from agent_handoff_jobs h where h.agent_id=new.agent_id and h.source_message_id=new.source_message_id and (h.status!='completed' or h.recipient_agent_id!=new.agent_id))
begin select raise(ignore); end;
