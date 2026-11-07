-- Additive event domain; old channel membership writers remain authoritative.
create table channel_events (
 id text primary key, channel_id uuid not null references channels(id) on delete cascade,
 creator_id uuid not null references users(id), code text collate "C" not null,
 title text not null, starts_at text not null, ends_at text, timezone text not null,
 location text, description text not null, cancelled integer not null default 0,
 revision integer not null default 1, updated_at text not null, cover_media_id uuid,
 unique(channel_id,code)
);
create index channel_events_recent on channel_events(channel_id,starts_at desc,id);
create table channel_event_codes (
 channel_id uuid not null references channels(id) on delete cascade,
 code text collate "C" not null, event_id text not null references channel_events(id) on delete cascade,
 primary key(channel_id,code)
);
create table channel_event_responses (
 event_id text not null references channel_events(id) on delete cascade,
 user_id uuid not null references users(id) on delete cascade,
 status text not null check(status in ('attending','declined','following')),
 updated_at text not null, seen_revision integer not null, primary key(event_id,user_id)
);
create index channel_event_responses_user on channel_event_responses(user_id,event_id);
create table channel_event_images (
 event_id text not null references channel_events(id) on delete cascade,
 media_id uuid not null references media_objects(id) on delete cascade,
 contributor_id uuid not null references users(id), primary key(event_id,media_id)
);
alter table channel_events add constraint channel_event_cover_library foreign key(id,cover_media_id) references channel_event_images(event_id,media_id) deferrable initially deferred;
create table channel_event_initialized (
 channel_id uuid primary key references channels(id) on delete cascade
);

create function channel_event_created() returns trigger language plpgsql as $$ begin
 insert into channel_event_codes values(new.channel_id,new.code,new.id);
 insert into channel_event_initialized values(new.channel_id) on conflict do nothing;
 if found then
 insert into channel_topic_settings(channel_id,enabled,backfill_after) values(new.channel_id,true,0) on conflict(channel_id) do update set enabled=true,backfill_after=0;
 end if; return new; end $$;
create trigger channel_event_code_insert after insert on channel_events for each row execute function channel_event_created();
create function channel_event_code_changed() returns trigger language plpgsql as $$ begin
 if old.code<>new.code then
 if (select count(*) from channel_event_codes where event_id=new.id)>=200 and not exists(select 1 from channel_event_codes where event_id=new.id and code=new.code) then raise exception 'SPROYT_EVENT_CODE'; end if;
 insert into channel_event_codes values(new.channel_id,new.code,new.id) on conflict(channel_id,code) do update set event_id=excluded.event_id where channel_event_codes.event_id=excluded.event_id;
 if exists(select 1 from channel_event_codes where channel_id=new.channel_id and code=new.code and event_id<>new.id) then raise exception 'SPROYT_EVENT_CODE'; end if;
 end if; return new; end $$;
create trigger channel_event_code_update after update of code on channel_events for each row execute function channel_event_code_changed();
create function channel_event_membership_cleanup() returns trigger language plpgsql as $$ begin
 delete from channel_event_responses where user_id=old.user_id and event_id in (select id from channel_events where channel_id=old.channel_id and coalesce(ends_at,starts_at)>to_char(now() at time zone 'UTC','YYYY-MM-DD"T"HH24:MI:SS.MS"Z"'));
 return old; end $$;
create trigger channel_event_membership_removed after delete on channel_memberships for each row execute function channel_event_membership_cleanup();
create function channel_event_circle_cleanup() returns trigger language plpgsql as $$ begin
 delete from channel_event_responses where user_id=old.user_id and event_id in (select e.id from channel_events e join channels c on c.id=e.channel_id where c.circle_id=old.circle_id and coalesce(e.ends_at,e.starts_at)>to_char(now() at time zone 'UTC','YYYY-MM-DD"T"HH24:MI:SS.MS"Z"'));
 return old; end $$;
create trigger channel_event_circle_removed after delete on circle_memberships for each row execute function channel_event_circle_cleanup();
create function channel_event_response_check() returns trigger language plpgsql as $$ begin
 perform m.user_id from channel_events e join channel_memberships m on m.channel_id=e.channel_id join channels c on c.id=e.channel_id where e.id=new.event_id and m.user_id=new.user_id and (c.circle_id is null or exists(select 1 from circle_memberships cm where cm.circle_id=c.circle_id and cm.user_id=new.user_id for share)) for share of m;
 if not found then raise exception 'SPROYT_EVENT_MEMBERSHIP'; end if;
 return new; end $$;
create trigger channel_event_response_guard before insert or update on channel_event_responses for each row execute function channel_event_response_check();
create function channel_event_clear_cover() returns trigger language plpgsql as $$ begin
 update channel_events set cover_media_id=null where id=old.event_id and cover_media_id=old.media_id; return old; end $$;
create trigger channel_event_cover_clear after delete on channel_event_images for each row execute function channel_event_clear_cover();

