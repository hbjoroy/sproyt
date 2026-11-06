-- The bootstrap agent predates agent provenance. Give its messages a profile
-- without enabling credential-based access or granting it any agent scopes.
insert into agent_profiles (agent_id, owner_id, invited_by, provider, service_identity, purpose,
                            rate_limit_per_minute, revoked_at, created_at)
select id, id, id, 'sproyt-system', 'release-news', 'Production release announcements',
       1, current_timestamp, current_timestamp
from users where id='00000000-0000-0000-0000-000000000001' and kind='agent'
on conflict do nothing;

insert into channels (id, slug, name, kind, created_by)
select '00000000-0000-0000-0000-000000000002', 'nyhende-i-sproyt', 'Nyhende i Sprøyt', 'public',
       '00000000-0000-0000-0000-000000000001'
where not exists (
    select 1 from channels where circle_id is null and kind='public' and name='Nyhende i Sprøyt'
)
on conflict do nothing;

insert into channel_sequences (channel_id, next_sequence)
select id, 1 from channels where id='00000000-0000-0000-0000-000000000002'
    and circle_id is null and kind='public' and name='Nyhende i Sprøyt'
on conflict(channel_id) do nothing;

-- Keep any existing owner. The bootstrap principal may publish as a member.
insert into channel_memberships (channel_id, user_id, role)
select id, '00000000-0000-0000-0000-000000000001',
       case when created_by='00000000-0000-0000-0000-000000000001' then 'owner' else 'member' end
from channels where circle_id is null and kind='public' and name='Nyhende i Sprøyt'
    and (select count(*) from channels where circle_id is null and kind='public' and name='Nyhende i Sprøyt')=1
on conflict(channel_id, user_id) do nothing;
