create table channel_topic_settings (
    channel_id uuid primary key references channels(id) on delete cascade,
    enabled boolean not null,
    backfill_after bigint,
    backfill_picked_at bigint not null default 0
);

create table channel_topic_jobs (
    message_id uuid primary key references messages(id) on delete cascade,
    channel_id uuid not null references channels(id) on delete cascade,
    due_at bigint,
    lease_token text,
    attempts integer not null default 0,
    is_backfill boolean not null default false,
    analyzer_version integer,
    source_revision bigint
);
create index channel_topic_jobs_due_idx on channel_topic_jobs(is_backfill,due_at,message_id) where due_at is not null;
create index channel_topic_jobs_channel_idx on channel_topic_jobs(channel_id);

create table channel_topic_evidence (
    message_id uuid not null references channel_topic_jobs(message_id) on delete cascade,
    channel_id uuid not null references channels(id) on delete cascade,
    topic_key text not null,
    label text not null,
    origin_id text not null,
    source_revision bigint not null,
    analyzer_version integer not null,
    kind text not null check (kind in ('text','image')),
    sequence bigint not null,
    primary key (message_id,topic_key,kind,origin_id)
);
create index channel_topic_evidence_recent_idx on channel_topic_evidence(channel_id,sequence desc);
create index channel_topic_evidence_match_idx on channel_topic_evidence(channel_id,topic_key,sequence desc);

create index personal_saved_links_topic_source_idx on personal_saved_links(source_message_id,log_message_id);
create index personal_saved_links_topic_channel_idx on personal_saved_links(owner_id,source_channel_id);

create table channel_topic_source_revisions (
    message_id uuid primary key references messages(id) on delete cascade,
    revision bigint not null
);

-- No foreign key on source_message_id: hard deletion also needs to clear
-- the derived evidence from saved log entries.
create table channel_topic_dirty_sources (
    source_message_id uuid primary key,
    after_log_message_id uuid,
    picked_at bigint not null default 0
);

-- A preview publisher can race an uncommitted attachment insert.
create table channel_topic_dirty_media (
    media_id uuid primary key references media_objects(id) on delete cascade
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

create function channel_topics_personal_default() returns trigger language plpgsql as $$
begin
    insert into channel_topic_settings(channel_id,enabled,backfill_after)
    values(new.channel_id,true,0) on conflict(channel_id) do nothing;
    return new;
end;
$$;
create trigger channel_topics_personal_default after insert on personal_spaces
for each row execute function channel_topics_personal_default();

create function channel_topics_refresh(source_id uuid) returns void language sql as $$
    -- Saving a source holds a shared lock on the message. Serialize against
    -- it so a newly committed bookmark cannot miss a concurrent refresh.
    select id from messages where id=source_id for no key update;
    insert into channel_topic_source_revisions(message_id,revision)
    select id,1 from messages where id=source_id
    on conflict(message_id) do update set revision=channel_topic_source_revisions.revision+1;

    -- Lock/update the job before removing evidence so an in-flight worker
    -- cannot commit stale evidence after the invalidation.
    insert into channel_topic_jobs(message_id,channel_id,due_at)
    select m.id,m.channel_id,0 from messages m
    left join channel_topic_settings s on s.channel_id=m.channel_id
    where m.id=source_id and m.deleted_at is null
      and (s.enabled or exists(select 1 from channel_topic_jobs j where j.message_id=m.id))
    on conflict(message_id) do update set due_at=excluded.due_at,lease_token=null,attempts=0,is_backfill=false,analyzer_version=null,source_revision=null;
    delete from channel_topic_evidence where message_id=source_id;
    insert into channel_topic_dirty_sources(source_message_id)
    select source_id where exists(select 1 from personal_saved_links where source_message_id=source_id)
    on conflict(source_message_id) do update set after_log_message_id=null;
$$;

create function channel_topics_message_changed() returns trigger language plpgsql as $$
begin
    if tg_op='INSERT' then
        insert into channel_topic_jobs(message_id,channel_id,due_at)
        select new.id,new.channel_id,0 from channel_topic_settings s
        where s.channel_id=new.channel_id and s.enabled and new.deleted_at is null
        on conflict(message_id) do nothing;
    elsif tg_op='DELETE' then
        perform channel_topics_refresh(old.id);
    elsif old.body is distinct from new.body or old.deleted_at is distinct from new.deleted_at then
        if new.deleted_at is not null then
            delete from channel_topic_jobs where message_id=new.id;
        end if;
        perform channel_topics_refresh(new.id);
    end if;
    return null;
end;
$$;
create trigger channel_topics_message_changed
after insert or update of body,deleted_at or delete on messages
for each row execute function channel_topics_message_changed();

create function channel_topics_link_changed() returns trigger language plpgsql as $$
begin
    if new.log_message_id is not null then
        perform channel_topics_refresh(new.log_message_id);
    end if;
    return null;
end;
$$;
create trigger channel_topics_link_changed after insert or update of log_message_id on personal_saved_links
for each row execute function channel_topics_link_changed();

create function channel_topics_media_changed() returns trigger language plpgsql as $$
declare linked_message uuid;
begin
    if tg_table_name='message_attachments' then
        if tg_op='DELETE' then linked_message := old.message_id;
        else linked_message := new.message_id;
        end if;
    elsif (tg_op='DELETE' and old.variant='preview') or
          (tg_op<>'DELETE' and new.variant='preview') then
        select message_id into linked_message from message_attachments
        where media_id=case when tg_op='DELETE' then old.media_id else new.media_id end;
        if linked_message is null and tg_op<>'DELETE' then
            insert into channel_topic_dirty_media(media_id)
            values(new.media_id) on conflict(media_id) do nothing;
        end if;
    end if;
    if linked_message is not null then
        perform channel_topics_refresh(linked_message);
    end if;
    return null;
end;
$$;
create trigger channel_topics_attachment_changed after insert or delete on message_attachments
for each row execute function channel_topics_media_changed();
create trigger channel_topics_preview_changed after insert or update or delete on media_variants
for each row execute function channel_topics_media_changed();

-- Revisit bounded batches after rejoining a channel with saved links. Evidence
-- already indexed becomes visible immediately under the live membership check.
create function channel_topics_source_rejoined() returns trigger language plpgsql as $$
begin
    update channel_topic_settings set backfill_after=0
    where enabled and channel_id in (select channel_id from personal_spaces where owner_id=new.user_id)
      and exists(select 1 from personal_saved_links where owner_id=new.user_id and source_channel_id=new.channel_id);
    return null;
end;
$$;
create trigger channel_topics_source_rejoined after insert on channel_memberships
for each row execute function channel_topics_source_rejoined();
