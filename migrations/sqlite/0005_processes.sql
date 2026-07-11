create table process_links (
  id text primary key,
  channel_id text not null references channels(id) on delete cascade,
  heart_instance_id text null unique,
  namespace text not null,
  definition_name text not null,
  definition_version text null,
  initiated_by text not null references users(id),
  visibility text not null default 'channel' check (visibility in ('channel', 'circle')),
  status text not null default 'starting',
  request_id text not null,
  created_at text not null,
  updated_at text not null,
  unique(initiated_by, request_id)
);

create index process_links_channel_created_idx on process_links(channel_id, created_at);

create table process_outbox (
  id text primary key,
  process_link_id text not null references process_links(id) on delete cascade,
  operation text not null check (operation in ('start', 'correlate', 'inspect')),
  payload text not null,
  status text not null default 'pending' check (status in ('pending', 'leased', 'completed', 'failed')),
  attempts integer not null default 0,
  available_at text not null,
  lease_until text null,
  last_error text null,
  created_at text not null,
  completed_at text null
);

create index process_outbox_ready_idx on process_outbox(status, available_at, lease_until);

create table process_events (
  id text primary key,
  process_link_id text not null references process_links(id) on delete cascade,
  event_key text not null,
  event_type text not null,
  payload text not null default '{}',
  actor_id text null references users(id),
  occurred_at text not null,
  unique(process_link_id, event_key)
);

create index process_events_link_time_idx on process_events(process_link_id, occurred_at);

create table circle_features (
  circle_id text not null references circles(id) on delete cascade,
  feature text not null,
  enabled integer not null check (enabled in (0, 1)),
  updated_by text not null references users(id),
  updated_at text not null,
  primary key(circle_id, feature)
);

create trigger audit_process_link_created after insert on process_links begin
  insert into audit_events(actor_id, action, target_kind, target_id, payload)
  values (new.initiated_by, 'process.started', 'process_link', new.id,
          json_object('channel_id', new.channel_id, 'definition', new.definition_name));
end;

create trigger audit_circle_feature_changed after insert on circle_features begin
  insert into audit_events(actor_id, action, target_kind, target_id, payload)
  values (new.updated_by, 'circle.feature_changed', 'circle', new.circle_id,
          json_object('feature', new.feature, 'enabled', new.enabled));
end;
