create table presence_leases (
  channel_id uuid not null references channels(id) on delete cascade,
  user_id uuid not null references users(id) on delete cascade,
  connection_id uuid primary key,
  expires_at timestamptz not null
);

create index presence_leases_channel_user_expires
  on presence_leases(channel_id, user_id, expires_at);
