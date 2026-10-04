-- Requester-authored additions never advance the Heart process or overwrite its source.
create table work_item_supplements (
  id uuid primary key,
  work_item_id uuid not null references work_items(id),
  actor_id uuid not null references users(id),
  request_id uuid not null,
  expected_revision bigint not null check (expected_revision > 0),
  body text not null check (octet_length(body) between 1 and 8000),
  created_at timestamptz not null default now(),
  unique(actor_id, request_id),
  unique(work_item_id, expected_revision)
);
create index work_item_supplements_item_idx on work_item_supplements(work_item_id, expected_revision);
alter table work_item_tasks add column decision_supplement_id uuid references work_item_supplements(id);
