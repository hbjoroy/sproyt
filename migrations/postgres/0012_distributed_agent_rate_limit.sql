create table agent_rate_limits (
  agent_id uuid primary key references agent_profiles(agent_id) on delete cascade,
  window_started_at timestamptz not null,
  request_count integer not null check (request_count >= 1)
);
