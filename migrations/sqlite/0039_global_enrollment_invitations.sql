drop index if exists enrollment_invitations_circle_idx;

create table enrollment_invitations_global_scope (
  id text primary key not null,
  circle_id text null references circles(id) on delete cascade,
  invited_by text not null references users(id),
  token_hash blob not null unique,
  expected_email_hash blob not null,
  expires_at text not null,
  state text not null default 'inactive' check (state in ('inactive', 'active', 'consumed')),
  authentik_invitation_id text null unique,
  activated_at text null,
  consumed_by text null references users(id),
  consumed_at text null,
  created_at text not null default current_timestamp,
  check (
    (state = 'inactive' and authentik_invitation_id is null and activated_at is null and consumed_by is null and consumed_at is null)
    or (state = 'active' and authentik_invitation_id is not null and activated_at is not null and consumed_by is null and consumed_at is null)
    or (state = 'consumed' and authentik_invitation_id is not null and activated_at is not null and consumed_by is not null and consumed_at is not null)
  )
);

insert into enrollment_invitations_global_scope (
  id, circle_id, invited_by, token_hash, expected_email_hash, expires_at, state,
  authentik_invitation_id, activated_at, consumed_by, consumed_at, created_at
)
select id, circle_id, invited_by, token_hash, expected_email_hash, expires_at, state,
  authentik_invitation_id, activated_at, consumed_by, consumed_at, created_at
from enrollment_invitations;

drop table enrollment_invitations;
alter table enrollment_invitations_global_scope rename to enrollment_invitations;
create index enrollment_invitations_circle_idx on enrollment_invitations(circle_id);
