-- Dedicated admission queue, invisible to ordinary workers.
create table agent_handoff_configs (
 agent_id uuid primary key references circle_chat_agents(agent_id) on delete cascade,
 revision bigint not null default 1 check(revision>0),
 target_agent_ids text not null default '[]', guidance text not null default ''
);
create table agent_handoff_jobs (
 id uuid primary key,
 agent_id uuid not null references circle_chat_agents(agent_id) on delete cascade,
 source_message_id uuid not null references messages(id) on delete cascade,
 channel_id uuid not null references channels(id) on delete cascade,
 config_revision bigint not null, access_revision bigint not null,
 routing_revision bigint not null,
 status text not null default 'pending' check(status in ('pending','leased','completed','skipped','failed')),
 available_at bigint not null default 0,
 attempts integer not null default 0, lease_token uuid, leased_until bigint,
 created_at bigint not null, expires_at bigint not null,
 recipient_agent_id uuid, unique(agent_id,source_message_id)
);
create index agent_handoff_jobs_ready on agent_handoff_jobs(status,leased_until,expires_at);
create function agent_handoff_ordinary_fence() returns trigger language plpgsql as $$
begin
 perform 1 from messages where id=new.source_message_id for update;
 if exists(select 1 from agent_handoff_jobs h where h.agent_id=new.agent_id and h.source_message_id=new.source_message_id and (h.status!='completed' or h.recipient_agent_id!=new.agent_id)) then return null; end if;
 return new;
end $$;
create trigger agent_handoff_ordinary_fence before insert on circle_chat_agent_jobs for each row execute function agent_handoff_ordinary_fence();
