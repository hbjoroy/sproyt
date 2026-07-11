create table process_command_receipts (
  actor_id text not null references users(id),
  request_id text not null,
  process_link_id text not null references process_links(id) on delete cascade,
  outbox_id text not null references process_outbox(id) on delete cascade,
  command_type text not null,
  created_at text not null,
  primary key(actor_id, request_id)
);

create index process_command_receipts_link_idx on process_command_receipts(process_link_id, created_at);
