-- Additive event domain; old channel membership writers remain authoritative.
create table channel_events (
 id text primary key, channel_id text not null references channels(id) on delete cascade,
 creator_id text not null references users(id), code text not null,
 title text not null, starts_at text not null, ends_at text, timezone text not null,
 location text, description text not null, cancelled integer not null default 0,
 revision integer not null default 1, updated_at text not null, cover_media_id text,
 unique(channel_id,code),
 foreign key(id,cover_media_id) references channel_event_images(event_id,media_id) deferrable initially deferred
);
create index channel_events_recent on channel_events(channel_id,starts_at desc,id);
create table channel_event_codes (
 channel_id text not null references channels(id) on delete cascade,
 code text not null, event_id text not null references channel_events(id) on delete cascade,
 primary key(channel_id,code)
);
create table channel_event_responses (
 event_id text not null references channel_events(id) on delete cascade,
 user_id text not null references users(id) on delete cascade,
 status text not null check(status in ('attending','declined','following')),
 updated_at text not null, seen_revision integer not null, primary key(event_id,user_id)
);
create index channel_event_responses_user on channel_event_responses(user_id,event_id);
create table channel_event_images (
 event_id text not null references channel_events(id) on delete cascade,
 media_id text not null references media_objects(id) on delete cascade,
 contributor_id text not null references users(id), primary key(event_id,media_id)
);
create table channel_event_initialized (
 channel_id text primary key references channels(id) on delete cascade
);

create trigger channel_event_code_insert after insert on channel_events begin
 insert into channel_event_codes values(new.channel_id,new.code,new.id);
 insert into channel_event_initialized values(new.channel_id) on conflict do nothing;
 insert into channel_topic_settings(channel_id,enabled,backfill_after)
 select new.channel_id,1,0 where changes()=1
 on conflict(channel_id) do update set enabled=1,backfill_after=0;
end;
create trigger channel_event_code_update after update of code on channel_events when old.code<>new.code begin
 select case when (select count(*) from channel_event_codes where event_id=new.id)>=200 and not exists(select 1 from channel_event_codes where event_id=new.id and code=new.code) then raise(abort,'SPROYT_EVENT_CODE') end;
 insert into channel_event_codes values(new.channel_id,new.code,new.id) on conflict(channel_id,code) do update set event_id=excluded.event_id where channel_event_codes.event_id=excluded.event_id;
 select case when exists(select 1 from channel_event_codes where channel_id=new.channel_id and code=new.code and event_id<>new.id) then raise(abort,'SPROYT_EVENT_CODE') end;
end;
create trigger channel_event_membership_removed after delete on channel_memberships begin
 delete from channel_event_responses where user_id=old.user_id and event_id in
 (select id from channel_events where channel_id=old.channel_id and coalesce(ends_at,starts_at)>strftime('%Y-%m-%dT%H:%M:%fZ','now'));
end;
create trigger channel_event_circle_removed after delete on circle_memberships begin
 delete from channel_event_responses where user_id=old.user_id and event_id in
 (select e.id from channel_events e join channels c on c.id=e.channel_id where c.circle_id=old.circle_id and coalesce(e.ends_at,e.starts_at)>strftime('%Y-%m-%dT%H:%M:%fZ','now'));
end;
create trigger channel_event_response_guard before insert on channel_event_responses begin
 select case when not exists(select 1 from channel_events e join channel_memberships m on m.channel_id=e.channel_id join channels c on c.id=e.channel_id where e.id=new.event_id and m.user_id=new.user_id and (c.circle_id is null or exists(select 1 from circle_memberships cm where cm.circle_id=c.circle_id and cm.user_id=new.user_id))) then raise(abort,'SPROYT_EVENT_MEMBERSHIP') end;
end;
create trigger channel_event_cover_clear after delete on channel_event_images begin
 update channel_events set cover_media_id=null where id=old.event_id and cover_media_id=old.media_id;
end;

