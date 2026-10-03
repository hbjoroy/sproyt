-- A channel choice is independent of the circle agent's global switch.
create table channel_chat_agent_settings (
  channel_id text not null references channels(id) on delete cascade,
  agent_id text not null references circle_chat_agents(agent_id) on delete cascade,
  enabled integer not null check(enabled in (0,1)),
  updated_by text not null references users(id),
  updated_at integer not null,
  primary key(channel_id, agent_id)
);
alter table channels add column chat_agent_access_revision integer not null default 1 check(chat_agent_access_revision > 0);
alter table circle_chat_agent_jobs add column access_revision integer not null default 1 check(access_revision > 0);
