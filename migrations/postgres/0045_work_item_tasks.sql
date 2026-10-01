alter table work_items add column category text check (category in ('bug','change','question'));
alter table work_items add column priority text check (priority in ('untriaged','low','normal','high','critical'));
alter table work_items add column process_status text not null default 'starting'
  check (process_status in ('starting','waiting','completed','cancelled','failed'));
alter table work_items add column sync_lease_until bigint;
alter table work_items add column sync_lease_token uuid;

create table work_item_tasks (
  id uuid primary key,
  work_item_id uuid not null references work_items(id) on delete cascade,
  message_id uuid not null unique references messages(id),
  channel_id uuid not null references channels(id),
  assignee_id uuid not null references users(id),
  node_id text not null,
  status text not null check (status in ('pending','completed','cancelled')),
  decision_request_id uuid,
  decision_category text check (decision_category in ('bug','change','question')),
  decision_priority text check (decision_priority in ('untriaged','low','normal','high','critical')),
  decision_status text check (decision_status in ('reviewing','needs_information','planned','resolved','rejected')),
  delivery_status text not null default 'ready' check (delivery_status in ('ready','pending','failed')),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  unique(work_item_id,node_id)
);
create index work_item_tasks_channel_idx on work_item_tasks(channel_id,created_at desc);
create index work_items_sync_idx on work_items(start_status,process_status,sync_lease_until);
