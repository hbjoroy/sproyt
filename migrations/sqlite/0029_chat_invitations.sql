create table chat_invitations (
  id text primary key not null,
  token_hash blob not null unique,
  target_type text not null check (target_type in ('circle', 'channel')),
  circle_id text not null references circles(id) on delete cascade,
  channel_id text null references channels(id) on delete cascade,
  invited_by text not null references users(id),
  expires_at text not null,
  created_at text not null default current_timestamp,
  check ((target_type = 'circle' and channel_id is null) or (target_type = 'channel' and channel_id is not null))
);
create table chat_invitation_responses (
  invitation_id text not null references chat_invitations(id) on delete cascade,
  user_id text not null references users(id) on delete cascade,
  response text not null check (response in ('declined', 'accepted')),
  responded_at text not null default current_timestamp,
  primary key (invitation_id, user_id)
);
