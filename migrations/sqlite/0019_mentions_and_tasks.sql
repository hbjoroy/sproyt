create table message_mentions (
  message_id text not null references messages(id) on delete cascade,
  mentioned_user_id text not null references users(id) on delete cascade,
  read_at text null,
  created_at text not null default current_timestamp,
  primary key (message_id, mentioned_user_id)
);

create index message_mentions_user_unread_idx
  on message_mentions(mentioned_user_id, read_at, created_at desc);

create table user_tasks (
  id text primary key not null,
  source_message_id text not null references messages(id) on delete cascade,
  assignee_id text not null references users(id) on delete cascade,
  created_by text not null references users(id),
  process_link_id text null references process_links(id) on delete set null,
  title text not null check (length(title) between 1 and 240),
  status text not null default 'open' check (status in ('open', 'done')),
  created_at text not null default current_timestamp,
  completed_at text null,
  unique (source_message_id, assignee_id)
);

create index user_tasks_assignee_status_idx
  on user_tasks(assignee_id, status, created_at desc);
