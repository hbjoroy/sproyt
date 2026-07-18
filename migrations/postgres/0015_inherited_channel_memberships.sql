insert into channel_memberships (channel_id, user_id, role)
select channels.id,
       circle_memberships.user_id,
       case circle_memberships.role when 'owner' then 'owner' else 'member' end
from channels
join circle_memberships on circle_memberships.circle_id = channels.circle_id
where channels.circle_id is not null
on conflict(channel_id, user_id) do nothing;

insert into channel_memberships (channel_id, user_id, role)
select channels.id,
       users.id,
       case when channels.created_by = users.id then 'owner' else 'member' end
from channels
cross join users
where channels.slug = 'general' and channels.circle_id is null
on conflict(channel_id, user_id) do nothing;
