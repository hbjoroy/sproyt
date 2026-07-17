create table process_command_receipts (
  actor_id uuid not null references users(id),
  request_id text not null,
  process_link_id uuid not null references process_links(id) on delete cascade,
  outbox_id uuid not null references process_outbox(id) on delete cascade,
  command_type text not null,
  created_at timestamptz not null,
  primary key(actor_id, request_id)
);

create index process_command_receipts_link_idx on process_command_receipts(process_link_id, created_at);
