create table circles (
  id text primary key not null,
  slug text not null unique,
  name text not null check (length(name) between 1 and 120),
  created_by text not null references users(id),
  created_at text not null default current_timestamp
);

create table circle_memberships (
  circle_id text not null references circles(id) on delete cascade,
  user_id text not null references users(id) on delete cascade,
  role text not null check (role in ('owner', 'member')),
  joined_at text not null default current_timestamp,
  primary key (circle_id, user_id)
);

create table circle_invitations (
  id text primary key not null,
  circle_id text not null references circles(id) on delete cascade,
  invited_by text not null references users(id),
  token_hash blob not null unique,
  expires_at text not null,
  accepted_by text null references users(id),
  accepted_at text null,
  created_at text not null default current_timestamp
);

alter table channels add column circle_id text null references circles(id) on delete cascade;

create index circle_memberships_user_idx on circle_memberships(user_id);
create index channels_circle_idx on channels(circle_id);
