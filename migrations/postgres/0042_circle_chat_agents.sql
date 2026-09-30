create table circle_chat_agents (
  agent_id uuid primary key references agent_profiles(agent_id),
  circle_id uuid not null references circles(id) on delete cascade,
  trigger_words text not null,
  response_phrases text not null,
  enabled boolean not null default false,
  revision bigint not null default 1 check (revision > 0),
  created_by uuid not null references users(id),
  updated_by uuid not null references users(id),
  created_at bigint not null,
  updated_at bigint not null
);
create index circle_chat_agents_circle_idx on circle_chat_agents(circle_id, enabled);

create table circle_chat_agent_jobs (
  id uuid primary key,
  agent_id uuid not null references circle_chat_agents(agent_id) on delete cascade,
  source_message_id uuid not null references messages(id) on delete cascade,
  channel_id uuid not null references channels(id) on delete cascade,
  config_revision bigint not null,
  status text not null check (status in ('pending','leased','completed','skipped','failed')),
  attempts integer not null default 0 check (attempts >= 0),
  available_at bigint not null,
  lease_token uuid,
  leased_until bigint,
  reply_body text,
  reply_message_id uuid references messages(id),
  error_code text,
  created_at bigint not null,
  finished_at bigint,
  unique(agent_id, source_message_id)
);
create index circle_chat_agent_jobs_ready_idx
  on circle_chat_agent_jobs(status, available_at, leased_until);
