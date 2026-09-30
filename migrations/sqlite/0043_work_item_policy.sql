-- Configuration for the work-item flow. No channel is enabled by default.
create table work_applications (
  id text primary key,
  owner_circle_id text not null references circles(id) on delete cascade,
  key text not null unique check (length(key) between 2 and 64),
  name text not null check (length(name) between 1 and 120),
  enabled integer not null default 0 check (enabled in (0,1)),
  github_repository_id text,
  updated_by text not null references users(id),
  created_at integer not null,
  updated_at integer not null
);

create table channel_process_bindings (
  id text primary key,
  channel_id text not null references channels(id) on delete cascade,
  process_key text not null check (length(process_key) between 1 and 120),
  namespace text not null,
  definition_name text not null,
  definition_version text not null,
  enabled integer not null default 0 check (enabled in (0,1)),
  revision integer not null default 1 check (revision > 0),
  updated_by text references users(id),
  updated_at integer not null,
  unique(channel_id, process_key),
  unique(channel_id, namespace, definition_name, definition_version)
);

create table channel_process_applications (
  binding_id text not null references channel_process_bindings(id) on delete cascade,
  application_id text not null references work_applications(id) on delete cascade,
  primary key(binding_id, application_id)
);

create table application_processors (
  application_id text not null references work_applications(id) on delete cascade,
  user_id text not null references users(id) on delete cascade,
  can_review integer not null default 0 check (can_review in (0,1)),
  can_export integer not null default 0 check (can_export in (0,1)),
  can_start_development integer not null default 0 check (can_start_development in (0,1)),
  check (can_review=1 or (can_export=0 and can_start_development=0)),
  primary key(application_id, user_id)
);

create table channel_task_routes (
  id text primary key,
  binding_id text not null references channel_process_bindings(id) on delete cascade,
  channel_id text not null references channels(id) on delete cascade,
  task_key text not null,
  process_role text not null,
  enabled integer not null default 0 check (enabled in (0,1)),
  unique(binding_id, task_key, process_role)
);

create table application_process_roles (
  application_id text not null references work_applications(id) on delete cascade,
  user_id text not null references users(id) on delete cascade,
  process_role text not null,
  primary key(application_id, user_id, process_role)
);

create index channel_process_applications_application_idx on channel_process_applications(application_id);
create index work_applications_circle_idx on work_applications(owner_circle_id);
create index channel_task_routes_channel_idx on channel_task_routes(channel_id, enabled);

insert into channel_process_bindings
  (id, channel_id, process_key, namespace, definition_name, definition_version, enabled, updated_at)
select min(p.id), p.channel_id, 'legacy-' || min(p.id), p.namespace, p.definition_name,
  coalesce(p.definition_version, ''), 1, strftime('%s','now')
from process_links p
group by p.channel_id, p.namespace, p.definition_name, coalesce(p.definition_version, '');

create trigger audit_channel_process_binding_insert after insert on channel_process_bindings begin
  insert into audit_events(actor_id, action, target_kind, target_id, payload)
  values (new.updated_by, 'process.binding_changed', 'channel', new.channel_id,
          json_object('process_key', new.process_key, 'enabled', new.enabled, 'revision', new.revision));
end;
create trigger audit_channel_process_binding_update after update on channel_process_bindings begin
  insert into audit_events(actor_id, action, target_kind, target_id, payload)
  values (new.updated_by, 'process.binding_changed', 'channel', new.channel_id,
          json_object('process_key', new.process_key, 'enabled', new.enabled, 'revision', new.revision));
end;

create trigger audit_work_application_insert after insert on work_applications begin
  insert into audit_events(actor_id, action, target_kind, target_id, payload)
  values (new.updated_by, 'work.application_changed', 'work_application', new.id,
          json_object('key', new.key, 'enabled', new.enabled));
end;
create trigger audit_work_application_update after update on work_applications begin
  insert into audit_events(actor_id, action, target_kind, target_id, payload)
  values (new.updated_by, 'work.application_changed', 'work_application', new.id,
          json_object('key', new.key, 'enabled', new.enabled));
end;
