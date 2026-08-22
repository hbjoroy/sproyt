-- A group direct conversation is distinct from its source conversation: it
-- starts empty so adding a person never reveals the source's private history.
create table direct_group_conversations (
  channel_id text primary key not null references channels(id) on delete cascade,
  source_channel_id text not null references channels(id) on delete restrict,
  created_at text not null default current_timestamp
);

create table direct_group_expansions (
  source_channel_id text not null references channels(id) on delete cascade,
  added_user_id text not null references users(id) on delete cascade,
  channel_id text not null unique references channels(id) on delete cascade,
  created_at text not null default current_timestamp,
  primary key (source_channel_id, added_user_id)
);
