create table agent_rate_limits (
  agent_id text primary key references agent_profiles(agent_id) on delete cascade,
  window_started_at text not null,
  request_count integer not null check (request_count >= 1)
);
