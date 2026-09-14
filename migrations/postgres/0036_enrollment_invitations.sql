create table enrollment_invitations (
  id uuid primary key,
  circle_id uuid not null references circles(id) on delete cascade,
  invited_by uuid not null references users(id),
  token_hash bytea not null unique,
  expected_email_hash bytea not null,
  expires_at timestamptz not null,
  state text not null default 'inactive' check (state in ('inactive', 'active', 'consumed')),
  authentik_invitation_id uuid null unique,
  activated_at timestamptz null,
  consumed_by uuid null references users(id),
  consumed_at timestamptz null,
  created_at timestamptz not null default now(),
  check (
    (state = 'inactive' and authentik_invitation_id is null and activated_at is null and consumed_by is null and consumed_at is null)
    or (state = 'active' and authentik_invitation_id is not null and activated_at is not null and consumed_by is null and consumed_at is null)
    or (state = 'consumed' and authentik_invitation_id is not null and activated_at is not null and consumed_by is not null and consumed_at is not null)
  )
);

create index enrollment_invitations_circle_idx on enrollment_invitations(circle_id);
