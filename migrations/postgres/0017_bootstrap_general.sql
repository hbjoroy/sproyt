insert into users (id, kind, display_name, created_at)
values ('00000000-0000-0000-0000-000000000001', 'agent', 'Sprøyt', now())
on conflict (id) do nothing;

insert into channels (id, slug, name, kind, created_by, created_at)
values (
  '00000000-0000-0000-0000-000000000001',
  'general',
  'General',
  'public',
  '00000000-0000-0000-0000-000000000001',
  now()
)
on conflict (slug) do nothing;

insert into channel_sequences (channel_id, next_sequence)
select id, 1 from channels where slug = 'general' and circle_id is null
on conflict(channel_id) do nothing;

insert into channel_memberships (channel_id, user_id, role)
select channels.id,
       users.id,
       case when channels.created_by = users.id then 'owner' else 'member' end
from channels
cross join users
where channels.slug = 'general' and channels.circle_id is null
on conflict(channel_id, user_id) do nothing;
