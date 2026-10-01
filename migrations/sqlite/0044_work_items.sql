create table work_items (
  id text primary key,
  source_channel_id text not null references channels(id),
  source_message_id text not null references messages(id),
  source_body text not null,
  source_edited_at text,
  application_id text not null references work_applications(id),
  binding_id text not null references channel_process_bindings(id),
  binding_revision integer not null,
  title text not null check (length(title) between 1 and 160),
  description text not null check (length(description) between 1 and 8000),
  revision integer not null default 1,
  status text not null default 'new' check (status in ('new','reviewing','needs_information','planned','in_development','resolved','rejected','duplicate')),
  requested_by text not null references users(id),
  request_id text not null,
  reviewer_id text not null references users(id),
  task_channel_id text not null references channels(id),
  heart_instance_id text unique,
  start_status text not null default 'pending' check (start_status in ('pending','leased','started','failed')),
  start_attempts integer not null default 0,
  lease_until integer,
  available_at integer not null default (unixepoch()),
  last_error text,
  created_at integer not null default (unixepoch()),
  updated_at integer not null default (unixepoch()),
  unique(requested_by, request_id)
);
create index work_items_start_ready_idx on work_items(start_status, available_at, lease_until);
create index work_items_channel_created_idx on work_items(source_channel_id, created_at desc);
