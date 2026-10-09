create table agent_location_shares (
  user_id uuid not null references users(id) on delete cascade,
  agent_id uuid not null references circle_chat_agents(agent_id) on delete cascade,
  channel_id uuid not null references channels(id) on delete cascade,
  share_id uuid not null unique,
  latitude double precision not null check (latitude between -90 and 90),
  longitude double precision not null check (longitude between -180 and 180),
  accuracy_m double precision not null check (accuracy_m between 0 and 100000),
  observed_at timestamptz not null,
  expires_at timestamptz not null,
  primary key (user_id, agent_id, channel_id)
);
create index agent_location_shares_expiry_idx on agent_location_shares(expires_at, share_id);

-- Deliberately no foreign key: replacing or revoking a share must invalidate
-- already-bound jobs instead of silently attaching them to a newer location.
alter table circle_chat_agent_jobs add column location_share_id uuid;
create index circle_chat_agent_jobs_location_idx on circle_chat_agent_jobs(location_share_id)
  where location_share_id is not null;
