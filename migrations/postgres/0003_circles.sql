create table circles (
  id uuid primary key,
  slug text not null unique,
  name text not null check (length(name) between 1 and 120),
  created_by uuid not null references users(id),
  created_at timestamptz not null default now()
);

create table circle_memberships (
  circle_id uuid not null references circles(id) on delete cascade,
  user_id uuid not null references users(id) on delete cascade,
  role text not null check (role in ('owner', 'member')),
  joined_at timestamptz not null default now(),
  primary key (circle_id, user_id)
);

create table circle_invitations (
  id uuid primary key,
  circle_id uuid not null references circles(id) on delete cascade,
  invited_by uuid not null references users(id),
  token_hash bytea not null unique,
  expires_at timestamptz not null,
  accepted_by uuid null references users(id),
  accepted_at timestamptz null,
  created_at timestamptz not null default now()
);

alter table channels add column circle_id uuid null references circles(id) on delete cascade;

create index circle_memberships_user_idx on circle_memberships(user_id);
create index channels_circle_idx on channels(circle_id);
