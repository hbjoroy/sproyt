create table thread_read_markers (
  root_message_id text not null references messages(id) on delete cascade,
  user_id text not null references users(id) on delete cascade,
  last_read_sequence integer not null default 0 check (last_read_sequence >= 0),
  updated_at text not null default current_timestamp,
  primary key(root_message_id, user_id)
);
