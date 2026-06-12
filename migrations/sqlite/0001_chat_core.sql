create table users (
  id text primary key not null,
  kind text not null check (kind in ('human', 'agent')),
  display_name text not null check (length(display_name) between 1 and 120),
  external_provider text null,
  external_subject text null,
  created_at text not null default current_timestamp,
  unique (external_provider, external_subject)
);

create table channels (
  id text primary key not null,
  slug text not null unique,
  name text not null check (length(name) between 1 and 120),
  kind text not null check (kind in ('public', 'local', 'private')),
  created_by text not null references users(id),
  created_at text not null default current_timestamp
);

create table channel_memberships (
  channel_id text not null references channels(id) on delete cascade,
  user_id text not null references users(id) on delete cascade,
  role text not null check (role in ('owner', 'moderator', 'member', 'observer')),
  joined_at text not null default current_timestamp,
  last_read_sequence integer not null default 0 check (last_read_sequence >= 0),
  primary key (channel_id, user_id)
);

create table channel_sequences (
  channel_id text primary key not null references channels(id) on delete cascade,
  next_sequence integer not null default 1 check (next_sequence >= 1)
);

create table messages (
  id text primary key not null,
  channel_id text not null references channels(id) on delete cascade,
  sender_id text not null references users(id),
  sequence integer not null check (sequence >= 1),
  body text not null check (length(body) > 0),
  created_at text not null default current_timestamp,
  unique (channel_id, sequence)
);

create index channel_memberships_user_idx
  on channel_memberships(user_id);

create index messages_channel_sequence_idx
  on messages(channel_id, sequence desc);
