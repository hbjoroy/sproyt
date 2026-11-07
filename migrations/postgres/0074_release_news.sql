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
create table release_news_channel (
    singleton boolean primary key default true check (singleton),
    channel_id uuid not null references channels(id) on delete cascade
);

insert into release_news_channel (channel_id)
select id from channels where circle_id is null and kind='public' and name='Nyhende i Sprøyt'
    and (select count(*) from channels where circle_id is null and kind='public' and name='Nyhende i Sprøyt')=1;

insert into channel_memberships (channel_id, user_id, role)
select c.id, '00000000-0000-0000-0000-000000000001',
       case when c.created_by='00000000-0000-0000-0000-000000000001' then 'owner' else 'member' end
from release_news_channel destination join channels c on c.id=destination.channel_id
on conflict(channel_id, user_id) do nothing;

-- Retain only the publication receipt if a message is later hard-deleted.
create table release_news_publications (
    revision text primary key,
    summary_digest text not null,
    channel_id uuid not null,
    message_id uuid not null,
    sequence bigint not null
);

create function record_release_news_publication() returns trigger language plpgsql as $$
begin
    if new.principal_id='00000000-0000-0000-0000-000000000001' and new.message_id is not null
       and new.request_id ~ '^release-news:[0-9a-f]{40}:[0-9a-f]{64}$' then
        insert into release_news_publications(revision,summary_digest,channel_id,message_id,sequence)
        select split_part(new.request_id, ':', 2),split_part(new.request_id, ':', 3),m.channel_id,m.id,m.sequence
        from messages m join release_news_channel c on c.channel_id=m.channel_id
        where m.id=new.message_id and m.sender_id=new.principal_id and m.parent_message_id is null;
        if not found then
            raise exception 'invalid release news channel or message';
        end if;
    end if;
    return null;
end;
$$;

create trigger record_release_news_publication after update of message_id on command_receipts
for each row execute function record_release_news_publication();
