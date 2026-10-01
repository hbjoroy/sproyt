-- Accepted work items are durable before Heart is contacted. All rows are
-- private to the configured source channel and application.
create table work_items (
  id uuid primary key,
  source_channel_id uuid not null references channels(id),
  source_message_id uuid not null references messages(id),
  source_body text not null,
  source_edited_at text,
  application_id uuid not null references work_applications(id),
  binding_id uuid not null references channel_process_bindings(id),
  binding_revision bigint not null,
  title text not null check (length(title) between 1 and 160),
  description text not null check (length(description) between 1 and 8000),
  revision bigint not null default 1,
  status text not null default 'new' check (status in ('new','reviewing','needs_information','planned','in_development','resolved','rejected','duplicate')),
  requested_by uuid not null references users(id),
  request_id uuid not null,
  reviewer_id uuid not null references users(id),
  task_channel_id uuid not null references channels(id),
  heart_instance_id uuid unique,
  start_status text not null default 'pending' check (start_status in ('pending','leased','started','failed')),
  start_attempts integer not null default 0,
  lease_until bigint,
  available_at bigint not null default (extract(epoch from now())::bigint),
  last_error text,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  unique(requested_by, request_id)
);
create index work_items_start_ready_idx on work_items(start_status, available_at, lease_until);
create index work_items_channel_created_idx on work_items(source_channel_id, created_at desc);
