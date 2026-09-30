create table circle_chat_agents (
  agent_id text primary key references agent_profiles(agent_id),
  circle_id text not null references circles(id) on delete cascade,
  trigger_words text not null,
  response_phrases text not null,
  enabled integer not null default 0 check (enabled in (0,1)),
  revision integer not null default 1 check (revision > 0),
  created_by text not null references users(id),
  updated_by text not null references users(id),
  created_at integer not null,
  updated_at integer not null
);
create index circle_chat_agents_circle_idx on circle_chat_agents(circle_id, enabled);

create table circle_chat_agent_jobs (
  id text primary key,
  agent_id text not null references circle_chat_agents(agent_id) on delete cascade,
  source_message_id text not null references messages(id) on delete cascade,
  channel_id text not null references channels(id) on delete cascade,
  config_revision integer not null,
  status text not null check (status in ('pending','leased','completed','skipped','failed')),
  attempts integer not null default 0 check (attempts >= 0),
  available_at integer not null,
  lease_token text,
  leased_until integer,
  reply_body text,
  reply_message_id text references messages(id),
  error_code text,
  created_at integer not null,
  finished_at integer,
  unique(agent_id, source_message_id)
);
create index circle_chat_agent_jobs_ready_idx
  on circle_chat_agent_jobs(status, available_at, leased_until);
