create table chat_invitations (
  id uuid primary key,
  token_hash bytea not null unique,
  target_type text not null check (target_type in ('circle', 'channel')),
  circle_id uuid not null references circles(id) on delete cascade,
  channel_id uuid null references channels(id) on delete cascade,
  invited_by uuid not null references users(id),
  expires_at timestamptz not null,
  created_at timestamptz not null default now(),
  check ((target_type = 'circle' and channel_id is null) or (target_type = 'channel' and channel_id is not null))
);
create table chat_invitation_responses (
  invitation_id uuid not null references chat_invitations(id) on delete cascade,
  user_id uuid not null references users(id) on delete cascade,
  response text not null check (response in ('declined', 'accepted')),
  responded_at timestamptz not null default now(),
  primary key (invitation_id, user_id)
);
