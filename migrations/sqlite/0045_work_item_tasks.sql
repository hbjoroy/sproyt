alter table work_items add column category text check (category in ('bug','change','question'));
alter table work_items add column priority text check (priority in ('untriaged','low','normal','high','critical'));
alter table work_items add column process_status text not null default 'starting'
  check (process_status in ('starting','waiting','completed','cancelled','failed'));
alter table work_items add column sync_lease_until integer;
alter table work_items add column sync_lease_token text;

create table work_item_tasks (
  id text primary key,
  work_item_id text not null references work_items(id) on delete cascade,
  message_id text not null unique references messages(id),
  channel_id text not null references channels(id),
  assignee_id text not null references users(id),
  node_id text not null,
  status text not null check (status in ('pending','completed','cancelled')),
  decision_request_id text,
  decision_category text check (decision_category in ('bug','change','question')),
  decision_priority text check (decision_priority in ('untriaged','low','normal','high','critical')),
  decision_status text check (decision_status in ('reviewing','needs_information','planned','resolved','rejected')),
  delivery_status text not null default 'ready' check (delivery_status in ('ready','pending','failed')),
  created_at integer not null default (unixepoch()),
  updated_at integer not null default (unixepoch()),
  unique(work_item_id,node_id)
);
create index work_item_tasks_channel_idx on work_item_tasks(channel_id,created_at desc);
create index work_items_sync_idx on work_items(start_status,process_status,sync_lease_until);
