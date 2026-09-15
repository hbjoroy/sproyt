-- SQLite has no built-in regexp replace.  Build the old display-name handle
-- one character at a time, then suffix colliding bases deterministically.
alter table users add column handle text null;
alter table users add column handle_source text null check (handle_source in ('legacy', 'oidc', 'agent'));

with recursive characters(id, source, position, base) as (
  select id, lower(display_name), 1, '' from users where kind = 'human'
  union all
  select id, source, position + 1,
    base || case
      when substr(source, position, 1) glob '[a-z0-9_-]' then substr(source, position, 1)
      else '' end
  from characters where position <= length(source)
), bases as (
  select id, coalesce(nullif(base, ''), 'user') as base
  from characters where position > length(source)
)
update users set handle = (
  select substr(base, 1, 47) || '-' || replace(users.id, '-', '')
  from bases where bases.id = users.id
), handle_source = 'legacy' where kind = 'human';

update users set handle_source = 'agent' where kind = 'agent';

create unique index users_handle_case_insensitive_unique
  on users (handle collate nocase) where handle is not null;
