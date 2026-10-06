-- M1 only: no collection/model worker is enabled by this schema.
alter table circle_chat_agents add column memory_enabled integer not null default false;
create unique index circle_chat_agents_memory_owner_idx on circle_chat_agents(agent_id,circle_id);
create unique index channels_memory_scope_idx on channels(id,circle_id);

create table agent_memory_profiles (
  id text primary key,
  circle_id text not null,
  agent_id text not null,
  user_id text not null references users(id) on delete cascade,
  enabled integer not null default false,
  collection_started_at integer,
  revision integer not null default 0 check(revision>=0),
  memory_epoch integer not null default 1 check(memory_epoch>0),
  history_compactions integer not null default 0 check(history_compactions>=0),
  created_at integer not null,
  updated_at integer not null,
  unique(circle_id,agent_id,user_id),
  unique(id,circle_id),
  foreign key(agent_id,circle_id) references circle_chat_agents(agent_id,circle_id) on delete cascade,
  foreign key(circle_id,user_id) references circle_memberships(circle_id,user_id) on delete cascade
);
create index agent_memory_profiles_user_idx on agent_memory_profiles(user_id,circle_id,agent_id);

create table agent_memory_scopes (
  profile_id text not null,
  circle_id text not null,
  channel_id text not null,
  start_sequence integer not null check(start_sequence>=0),
  processed_sequence integer not null check(processed_sequence>=start_sequence),
  dirty_sequence integer not null check(dirty_sequence>=processed_sequence),
  source_generation integer not null default 1 check(source_generation>0),
  attempts integer not null default 0 check(attempts>=0),
  lease_token text,
  available_at integer not null,
  leased_until integer,
  primary key(profile_id,channel_id),
  foreign key(profile_id,circle_id) references agent_memory_profiles(id,circle_id) on delete cascade,
  foreign key(channel_id,circle_id) references channels(id,circle_id) on delete cascade
);
create index agent_memory_scopes_ready_idx on agent_memory_scopes(available_at,leased_until) where dirty_sequence>processed_sequence;

create table agent_memory_notes (
  id text primary key,
  profile_id text not null,
  channel_id text not null,
  kind text not null check(kind in ('preference','temporary_context','interaction')),
  content text not null check(json_valid(content) and json_type(content)='object' and json_type(content,'$.text')='text' and length(cast(json_extract(content,'$.text') as blob)) between 1 and 1024),
  origin text not null check(origin in ('automatic','user')),
  evidence text not null check(evidence in ('user_stated','conversation_event','user_confirmed')),
  revision integer not null default 1 check(revision>0),
  created_at integer not null,
  updated_at integer not null,
  expires_at integer,
  foreign key(profile_id,channel_id) references agent_memory_scopes(profile_id,channel_id) on delete cascade
);
create index agent_memory_notes_owner_idx on agent_memory_notes(profile_id,channel_id);

-- Keep missing/deleted message IDs as dependencies: absence must invalidate,
-- not silently remove one dependency and bless the remaining sources.
create table agent_memory_note_sources (
  note_id text not null references agent_memory_notes(id) on delete cascade,
  message_id text not null,
  source_version text not null check(length(source_version)=64 and source_version not glob '*[^0-9a-f]*'),
  primary key(note_id,message_id)
);
create index agent_memory_note_sources_message_idx on agent_memory_note_sources(message_id,note_id);
create table agent_memory_note_participants (
  note_id text not null references agent_memory_notes(id) on delete cascade,
  user_id text not null references users(id) on delete cascade,
  primary key(note_id,user_id)
);
create index agent_memory_note_participants_user_idx on agent_memory_note_participants(user_id,note_id);
create table agent_memory_exclusions (
  profile_id text not null references agent_memory_profiles(id) on delete cascade,
  message_id text not null,
  primary key(profile_id,message_id)
);
