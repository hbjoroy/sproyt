create table channel_topic_settings (
    channel_id text primary key references channels(id) on delete cascade,
    enabled integer not null check (enabled in (0,1)),
    backfill_after integer,
    backfill_picked_at integer not null default 0
);

create table channel_topic_jobs (
    message_id text primary key references messages(id) on delete cascade,
    channel_id text not null references channels(id) on delete cascade,
    due_at integer,
    lease_token text,
    attempts integer not null default 0,
    is_backfill integer not null default false,
    analyzer_version integer,
    source_revision integer
);
create index channel_topic_jobs_due_idx on channel_topic_jobs(is_backfill,due_at,message_id) where due_at is not null;
create index channel_topic_jobs_channel_idx on channel_topic_jobs(channel_id);

create table channel_topic_evidence (
    message_id text not null references channel_topic_jobs(message_id) on delete cascade,
    channel_id text not null references channels(id) on delete cascade,
    topic_key text not null,
    label text not null,
    origin_id text not null,
    source_revision integer not null,
    analyzer_version integer not null,
    kind text not null check (kind in ('text','image')),
    sequence integer not null,
    primary key (message_id,topic_key,kind,origin_id)
);
create index channel_topic_evidence_recent_idx on channel_topic_evidence(channel_id,sequence desc);
create index channel_topic_evidence_match_idx on channel_topic_evidence(channel_id,topic_key,sequence desc);

create index personal_saved_links_topic_source_idx on personal_saved_links(source_message_id,log_message_id);
create index personal_saved_links_topic_channel_idx on personal_saved_links(owner_id,source_channel_id);

create table channel_topic_source_revisions (
    message_id text primary key references messages(id) on delete cascade,
    revision integer not null
);

-- No foreign key on source_message_id: hard deletion also needs to clear
-- the derived evidence from saved log entries.
create table channel_topic_dirty_sources (
    source_message_id text primary key,
    after_log_message_id text,
    picked_at integer not null default 0
);

-- A saved log entry gets evidence from its live source, with the saved-link
-- owner's current membership. A link to another log entry carries no content.
create view channel_topic_sources as
select target.id as message_id,target.channel_id,
       source.id as source_message_id,source.channel_id as source_channel_id,
       coalesce(link.owner_id,target.sender_id) as actor_id,link.owner_id as saved_owner_id,
       source.body,source.sender_display_name,source.created_at,
       coalesce(revision.revision,0) as source_revision
from messages target
left join personal_saved_links link on link.log_message_id=target.id
join messages source on source.id=coalesce(link.source_message_id,target.id)
left join channel_topic_source_revisions revision on revision.message_id=source.id
where target.deleted_at is null and source.deleted_at is null
  and (link.owner_id is null or (
    source.channel_id=link.source_channel_id
    and exists(select 1 from channel_memberships cm where cm.channel_id=source.channel_id and cm.user_id=link.owner_id)
    and not exists(select 1 from personal_saved_links nested where nested.log_message_id=source.id)
  ));

insert into channel_topic_settings(channel_id,enabled,backfill_after)
select channel_id,true,0 from personal_spaces;

create trigger channel_topics_personal_default after insert on personal_spaces
begin
    insert into channel_topic_settings(channel_id,enabled,backfill_after)
    values(new.channel_id,1,0) on conflict(channel_id) do nothing;
end;

create trigger channel_topics_message_insert after insert on messages
when new.deleted_at is null
begin
    insert into channel_topic_jobs(message_id,channel_id,due_at)
    select new.id,new.channel_id,0 from channel_topic_settings s
    where s.channel_id=new.channel_id and s.enabled
    on conflict(message_id) do nothing;
end;

-- Reuse one refresh implementation across SQLite trigger entry points.
create view channel_topic_refresh_requests as
select message_id from channel_topic_source_revisions where false;

create trigger channel_topics_refresh instead of insert on channel_topic_refresh_requests
begin
    insert into channel_topic_source_revisions(message_id,revision)
    select id,1 from messages where id=new.message_id
    on conflict(message_id) do update set revision=channel_topic_source_revisions.revision+1;

    insert into channel_topic_jobs(message_id,channel_id,due_at)
    select m.id,m.channel_id,0 from messages m
    left join channel_topic_settings s on s.channel_id=m.channel_id
    where m.id=new.message_id and m.deleted_at is null
      and (s.enabled or exists(select 1 from channel_topic_jobs j where j.message_id=m.id))
    on conflict(message_id) do update set due_at=excluded.due_at,lease_token=null,attempts=0,is_backfill=false,analyzer_version=null,source_revision=null;
    delete from channel_topic_evidence where message_id=new.message_id;
    insert into channel_topic_dirty_sources(source_message_id)
    select new.message_id where exists(select 1 from personal_saved_links where source_message_id=new.message_id)
    on conflict(source_message_id) do update set after_log_message_id=null;
end;

create trigger channel_topics_message_edit after update of body,deleted_at on messages
when old.body is not new.body or old.deleted_at is not new.deleted_at
begin
    delete from channel_topic_jobs where message_id=new.id and new.deleted_at is not null;
    insert into channel_topic_refresh_requests values(new.id);
end;

create trigger channel_topics_message_delete after delete on messages
begin
    insert into channel_topic_refresh_requests values(old.id);
end;

create trigger channel_topics_link_insert after insert on personal_saved_links
when new.log_message_id is not null
begin
    insert into channel_topic_refresh_requests values(new.log_message_id);
end;

create trigger channel_topics_link_update after update of log_message_id on personal_saved_links
when new.log_message_id is not null and old.log_message_id is not new.log_message_id
begin
    insert into channel_topic_refresh_requests values(new.log_message_id);
end;

create trigger channel_topics_attachment_insert after insert on message_attachments
begin
    insert into channel_topic_refresh_requests values(new.message_id);
end;

create trigger channel_topics_attachment_delete after delete on message_attachments
begin
    insert into channel_topic_refresh_requests values(old.message_id);
end;

create trigger channel_topics_preview_insert after insert on media_variants
when new.variant='preview'
begin
    insert into channel_topic_refresh_requests
    select message_id from message_attachments where media_id=new.media_id;
end;

create trigger channel_topics_preview_update after update on media_variants
when new.variant='preview'
begin
    insert into channel_topic_refresh_requests
    select message_id from message_attachments where media_id=new.media_id;
end;

create trigger channel_topics_preview_delete after delete on media_variants
when old.variant='preview'
begin
    insert into channel_topic_refresh_requests
    select message_id from message_attachments where media_id=old.media_id;
end;

create trigger channel_topics_source_rejoined after insert on channel_memberships
begin
    update channel_topic_settings set backfill_after=0
    where enabled and channel_id in (select channel_id from personal_spaces where owner_id=new.user_id)
      and exists(select 1 from personal_saved_links where owner_id=new.user_id and source_channel_id=new.channel_id);
end;
