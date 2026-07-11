create table process_links (
  id uuid primary key,
  channel_id uuid not null references channels(id) on delete cascade,
  heart_instance_id uuid null unique,
  namespace text not null,
  definition_name text not null,
  definition_version text null,
  initiated_by uuid not null references users(id),
  visibility text not null default 'channel' check (visibility in ('channel', 'circle')),
  status text not null default 'starting',
  request_id text not null,
  created_at timestamptz not null,
  updated_at timestamptz not null,
  unique(initiated_by, request_id)
);

create index process_links_channel_created_idx on process_links(channel_id, created_at);

create table process_outbox (
  id uuid primary key,
  process_link_id uuid not null references process_links(id) on delete cascade,
  operation text not null check (operation in ('start', 'correlate', 'inspect')),
  payload jsonb not null,
  status text not null default 'pending' check (status in ('pending', 'leased', 'completed', 'failed')),
  attempts integer not null default 0,
  available_at timestamptz not null,
  lease_until timestamptz null,
  last_error text null,
  created_at timestamptz not null,
  completed_at timestamptz null
);

create index process_outbox_ready_idx on process_outbox(status, available_at, lease_until);

create table process_events (
  id uuid primary key,
  process_link_id uuid not null references process_links(id) on delete cascade,
  event_key text not null,
  event_type text not null,
  payload jsonb not null default '{}'::jsonb,
  actor_id uuid null references users(id),
  occurred_at timestamptz not null,
  unique(process_link_id, event_key)
);

create index process_events_link_time_idx on process_events(process_link_id, occurred_at);

create table circle_features (
  circle_id uuid not null references circles(id) on delete cascade,
  feature text not null,
  enabled boolean not null,
  updated_by uuid not null references users(id),
  updated_at timestamptz not null,
  primary key(circle_id, feature)
);

create function sproyt_audit_process_mutation() returns trigger language plpgsql as $$
begin
  if TG_ARGV[0] = 'process.started' then
    insert into audit_events(actor_id, action, target_kind, target_id, payload)
    values (new.initiated_by, TG_ARGV[0], 'process_link', new.id::text,
            jsonb_build_object('channel_id', new.channel_id, 'definition', new.definition_name));
  elsif TG_ARGV[0] = 'circle.feature_changed' then
    insert into audit_events(actor_id, action, target_kind, target_id, payload)
    values (new.updated_by, TG_ARGV[0], 'circle', new.circle_id::text,
            jsonb_build_object('feature', new.feature, 'enabled', new.enabled));
  end if;
  return new;
end;
$$;

create trigger audit_process_link_created after insert on process_links
for each row execute function sproyt_audit_process_mutation('process.started');
create trigger audit_circle_feature_changed after insert or update on circle_features
for each row execute function sproyt_audit_process_mutation('circle.feature_changed');
