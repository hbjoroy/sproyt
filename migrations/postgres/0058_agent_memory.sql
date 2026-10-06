-- M1 only: no collection/model worker is enabled by this schema.
alter table circle_chat_agents add column memory_enabled boolean not null default false;
create unique index circle_chat_agents_memory_owner_idx on circle_chat_agents(agent_id,circle_id);
create unique index channels_memory_scope_idx on channels(id,circle_id);

create table agent_memory_profiles (
  id uuid primary key,
  circle_id uuid not null,
  agent_id uuid not null,
  user_id uuid not null references users(id) on delete cascade,
  enabled boolean not null default false,
  collection_started_at bigint,
  revision bigint not null default 0 check(revision>=0),
  memory_epoch bigint not null default 1 check(memory_epoch>0),
  history_compactions bigint not null default 0 check(history_compactions>=0),
  created_at bigint not null,
  updated_at bigint not null,
  unique(circle_id,agent_id,user_id),
  unique(id,circle_id),
  foreign key(agent_id,circle_id) references circle_chat_agents(agent_id,circle_id) on delete cascade,
  foreign key(circle_id,user_id) references circle_memberships(circle_id,user_id) on delete cascade
);
create index agent_memory_profiles_user_idx on agent_memory_profiles(user_id,circle_id,agent_id);

create table agent_memory_scopes (
  profile_id uuid not null,
  circle_id uuid not null,
  channel_id uuid not null,
  start_sequence bigint not null check(start_sequence>=0),
  processed_sequence bigint not null check(processed_sequence>=start_sequence),
  dirty_sequence bigint not null check(dirty_sequence>=processed_sequence),
  source_generation bigint not null default 1 check(source_generation>0),
  attempts integer not null default 0 check(attempts>=0),
  lease_token uuid,
  available_at bigint not null,
  leased_until bigint,
  primary key(profile_id,channel_id),
  foreign key(profile_id,circle_id) references agent_memory_profiles(id,circle_id) on delete cascade,
  foreign key(channel_id,circle_id) references channels(id,circle_id) on delete cascade
);
create index agent_memory_scopes_ready_idx on agent_memory_scopes(available_at,leased_until) where dirty_sequence>processed_sequence;

create table agent_memory_notes (
  id uuid primary key,
  profile_id uuid not null,
  channel_id uuid not null,
  kind text not null check(kind in ('preference','temporary_context','interaction')),
  content jsonb not null check(jsonb_typeof(content)='object' and jsonb_typeof(content->'text')='string' and octet_length(content->>'text') between 1 and 1024),
  origin text not null check(origin in ('automatic','user')),
  evidence text not null check(evidence in ('user_stated','conversation_event','user_confirmed')),
  revision bigint not null default 1 check(revision>0),
  created_at bigint not null,
  updated_at bigint not null,
  expires_at bigint,
  foreign key(profile_id,channel_id) references agent_memory_scopes(profile_id,channel_id) on delete cascade
);
create index agent_memory_notes_owner_idx on agent_memory_notes(profile_id,channel_id);

-- Keep missing/deleted message IDs as dependencies: absence must invalidate,
-- not silently remove one dependency and bless the remaining sources.
create table agent_memory_note_sources (
  note_id uuid not null references agent_memory_notes(id) on delete cascade,
  message_id uuid not null,
  source_version text not null check(source_version ~ '^[0-9a-f]{64}$'),
  primary key(note_id,message_id)
);
create index agent_memory_note_sources_message_idx on agent_memory_note_sources(message_id,note_id);
create table agent_memory_note_participants (
  note_id uuid not null references agent_memory_notes(id) on delete cascade,
  user_id uuid not null references users(id) on delete cascade,
  primary key(note_id,user_id)
);
create index agent_memory_note_participants_user_idx on agent_memory_note_participants(user_id,note_id);
create table agent_memory_exclusions (
  profile_id uuid not null references agent_memory_profiles(id) on delete cascade,
  message_id uuid not null,
  primary key(profile_id,message_id)
);
