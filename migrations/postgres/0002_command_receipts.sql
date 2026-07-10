create table command_receipts (
  principal_id uuid not null references users(id) on delete cascade,
  request_id text not null check (length(request_id) between 1 and 128),
  message_id uuid null unique references messages(id) on delete cascade,
  created_at timestamptz not null default now(),
  primary key (principal_id, request_id)
);
