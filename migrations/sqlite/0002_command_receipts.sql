create table command_receipts (
  principal_id text not null references users(id) on delete cascade,
  request_id text not null check (length(request_id) between 1 and 128),
  message_id text null unique references messages(id) on delete cascade,
  created_at text not null default current_timestamp,
  primary key (principal_id, request_id)
);
