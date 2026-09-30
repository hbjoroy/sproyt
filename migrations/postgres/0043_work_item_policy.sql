-- Configuration for the work-item flow. No channel is enabled by default.
create table work_applications (
  id uuid primary key,
  owner_circle_id uuid not null references circles(id) on delete cascade,
  key text not null unique check (key ~ '^[a-z][a-z0-9-]{1,63}$'),
  name text not null check (length(name) between 1 and 120),
  enabled boolean not null default false,
  github_repository_id text,
  updated_by uuid not null references users(id),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);

create table channel_process_bindings (
  id uuid primary key,
  channel_id uuid not null references channels(id) on delete cascade,
  process_key text not null check (length(process_key) between 1 and 120),
  namespace text not null,
  definition_name text not null,
  definition_version text not null,
  enabled boolean not null default false,
  revision bigint not null default 1 check (revision > 0),
  updated_by uuid references users(id),
  updated_at timestamptz not null default now(),
  unique(channel_id, process_key),
  unique(channel_id, namespace, definition_name, definition_version)
);

create table channel_process_applications (
  binding_id uuid not null references channel_process_bindings(id) on delete cascade,
  application_id uuid not null references work_applications(id) on delete cascade,
  primary key(binding_id, application_id)
);

create table application_processors (
  application_id uuid not null references work_applications(id) on delete cascade,
  user_id uuid not null references users(id) on delete cascade,
  can_review boolean not null default false,
  can_export boolean not null default false,
  can_start_development boolean not null default false,
  check (can_review or (not can_export and not can_start_development)),
  primary key(application_id, user_id)
);

create table channel_task_routes (
  id uuid primary key,
  binding_id uuid not null references channel_process_bindings(id) on delete cascade,
  channel_id uuid not null references channels(id) on delete cascade,
  task_key text not null,
  process_role text not null,
  enabled boolean not null default false,
  unique(binding_id, task_key, process_role)
);

create table application_process_roles (
  application_id uuid not null references work_applications(id) on delete cascade,
  user_id uuid not null references users(id) on delete cascade,
  process_role text not null,
  primary key(application_id, user_id, process_role)
);

create index channel_process_applications_application_idx on channel_process_applications(application_id);
create index work_applications_circle_idx on work_applications(owner_circle_id);
create index channel_task_routes_channel_idx on channel_task_routes(channel_id, enabled);

-- Preserve only definitions already started through the legacy generic API.
-- New definitions require an explicit administrative binding.
insert into channel_process_bindings
  (id, channel_id, process_key, namespace, definition_name, definition_version, enabled)
select distinct on (p.channel_id, p.namespace, p.definition_name, coalesce(p.definition_version, ''))
  p.id, p.channel_id, 'legacy-' || p.id::text, p.namespace, p.definition_name,
  coalesce(p.definition_version, ''), true
from process_links p
order by p.channel_id, p.namespace, p.definition_name, coalesce(p.definition_version, ''), p.created_at, p.id;

create function sproyt_audit_work_policy() returns trigger language plpgsql as $$
begin
  insert into audit_events(actor_id, action, target_kind, target_id, payload)
  values (new.updated_by, 'process.binding_changed', 'channel', new.channel_id::text,
          jsonb_build_object('process_key', new.process_key, 'enabled', new.enabled,
                             'revision', new.revision));
  return new;
end;
$$;
create trigger audit_channel_process_binding after insert or update on channel_process_bindings
for each row execute function sproyt_audit_work_policy();

create function sproyt_audit_work_application() returns trigger language plpgsql as $$
begin
  insert into audit_events(actor_id, action, target_kind, target_id, payload)
  values (new.updated_by, 'work.application_changed', 'work_application', new.id::text,
          jsonb_build_object('key', new.key, 'enabled', new.enabled));
  return new;
end;
$$;
create trigger audit_work_application after insert or update on work_applications
for each row execute function sproyt_audit_work_application();
