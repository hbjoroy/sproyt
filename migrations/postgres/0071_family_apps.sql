-- Bound waits for locks on core tables in the shared database.
set local lock_timeout = '2s';

create table app_bindings (
 id text primary key, app_id text not null, channel_id uuid references channels(id) on delete cascade,
 circle_id uuid references circles(id) on delete cascade,
 resource_id text not null, resource_name text not null, created_at text not null default current_timestamp,
 manifest_version bigint not null check(manifest_version>0),notice_version bigint not null check(notice_version>0),
 manifest_json jsonb not null,
 status text not null check(status in ('active','disabled')),revision bigint not null default 0 check(revision>=0),
 created_by uuid not null references users(id),environment text not null check(environment in ('canary','production')),
 check((channel_id is null) <> (circle_id is null))
);
create unique index app_binding_channel_active on app_bindings(app_id,channel_id,environment) where status='active' and channel_id is not null;
create unique index app_binding_circle_active on app_bindings(app_id,circle_id,environment) where status='active' and circle_id is not null;
create table app_subjects(app_id text not null,user_id uuid not null references users(id) on delete cascade,subject_id text not null unique,primary key(app_id,user_id));
create table app_identity_links(app_id text not null,user_id uuid not null references users(id) on delete cascade,external_user_id text not null,primary key(app_id,user_id),unique(app_id,external_user_id));
create table app_authorizations(state_hash text primary key,binding_id text not null unique,app_id text not null,actor_id uuid not null references users(id) on delete cascade,scope_kind text not null check(scope_kind in ('channel','circle')),scope_id text not null,resource_id text not null,expires_at bigint not null);
create table app_consumed_approvals(app_id text not null,authorization_id text not null,expires_at bigint not null,primary key(app_id,authorization_id));
create table app_member_responses(
 binding_id text not null references app_bindings(id) on delete cascade,user_id uuid not null references users(id) on delete cascade,
 membership_epoch text not null,participation text not null check(participation in ('yes','no')),
 sharing bigint not null check(sharing in (0,1)),notice_version bigint not null,revision bigint not null check(revision>0),updated_at text not null,
 attributes jsonb not null default '{}' check(octet_length(attributes::text)<=16384),writer_id uuid references users(id),
 event_kind text not null default 'response.upserted' check(event_kind in ('response.upserted','attributes.updated')),
 primary key(binding_id,user_id)
);
create table app_binding_events(binding_id text not null references app_bindings(id) on delete cascade,sequence bigint not null,event_id text not null unique,kind text not null check(kind in ('response.upserted','response.revoked','attributes.updated')),subject_id text not null,revision bigint not null,created_at text not null default current_timestamp,primary key(binding_id,sequence));
create table app_command_receipts(binding_id text not null references app_bindings(id) on delete cascade,operation text not null,idempotency_key text not null,request_hash text not null,expires_at bigint not null,primary key(binding_id,operation,idempotency_key));
create function app_response_changed() returns trigger language plpgsql as $$
begin
 -- Updating the binding row serializes its sequence and commit order across replicas.
 update app_bindings set revision=revision+1 where id=new.binding_id;
 insert into app_binding_events(binding_id,sequence,event_id,kind,subject_id,revision)
 select b.id,b.revision,md5(random()::text||clock_timestamp()::text),case when new.sharing=0 then 'response.revoked' else new.event_kind end,s.subject_id,new.revision from app_bindings b join app_subjects s on s.app_id=b.app_id and s.user_id=new.user_id where b.id=new.binding_id;
 return new;
end $$;
create trigger app_response_insert after insert or update on app_member_responses for each row execute function app_response_changed();
create function app_membership_left() returns trigger language plpgsql as $$
begin
 if TG_TABLE_NAME='channel_memberships' then
  update app_member_responses set sharing=0,revision=revision+1,updated_at=current_timestamp::text where user_id=old.user_id and sharing=1 and binding_id in(select id from app_bindings where channel_id=old.channel_id);
 else
  update app_member_responses set sharing=0,revision=revision+1,updated_at=current_timestamp::text where user_id=old.user_id and sharing=1 and binding_id in(select id from app_bindings where circle_id=old.circle_id);
 end if;
 return old;
end $$;
create function app_display_name_changed() returns trigger language plpgsql as $$
begin
 update app_member_responses set revision=revision+1,event_kind='response.upserted',updated_at=current_timestamp::text where user_id=new.id and sharing=1;
 return new;
end $$;

-- Existing memberships predate any app response. Give them a constant epoch
-- without rewriting their tables; each subsequent insert gets a fresh epoch.
alter table channel_memberships add column app_membership_epoch text not null default 'legacy';
alter table channel_memberships alter column app_membership_epoch set default md5(random()::text||clock_timestamp()::text);
alter table circle_memberships add column app_membership_epoch text not null default 'legacy';
alter table circle_memberships alter column app_membership_epoch set default md5(random()::text||clock_timestamp()::text);

create trigger app_channel_leave after delete on channel_memberships for each row execute function app_membership_left();
create trigger app_circle_leave after delete on circle_memberships for each row execute function app_membership_left();
create trigger app_display_name after update of display_name on users for each row when(old.display_name is distinct from new.display_name) execute function app_display_name_changed();
