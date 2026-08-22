-- A group direct conversation is distinct from its source conversation: it
-- starts empty so adding a person never reveals the source's private history.
create table direct_group_conversations (
  channel_id uuid primary key not null references channels(id) on delete cascade,
  source_channel_id uuid not null references channels(id) on delete restrict,
  created_at timestamptz not null default now()
);

create table direct_group_expansions (
  source_channel_id uuid not null references channels(id) on delete cascade,
  added_user_id uuid not null references users(id) on delete cascade,
  channel_id uuid not null unique references channels(id) on delete cascade,
  created_at timestamptz not null default now(),
  primary key (source_channel_id, added_user_id)
);
