-- `sub` remains the authentication identity.  A handle is only a stable,
-- public address and is deliberately nullable for non-human service agents.
alter table users add column handle text null;
alter table users add column handle_source text null check (handle_source in ('legacy', 'oidc', 'agent'));

with bases as (
  select id,
    coalesce(nullif(regexp_replace(lower(display_name), '[^a-z0-9_-]', '', 'g'), ''), 'user') as base
  from users where kind = 'human'
)
update users u set handle = left(bases.base, 47) || '-' || replace(u.id::text, '-', ''),
  handle_source = 'legacy'
from bases where u.id = bases.id;

update users set handle_source = 'agent' where kind = 'agent';

create unique index users_handle_case_insensitive_unique
  on users (lower(handle)) where handle is not null;
