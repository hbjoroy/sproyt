create table work_item_supplements (
  id text primary key,
  work_item_id text not null references work_items(id),
  actor_id text not null references users(id),
  request_id text not null,
  expected_revision integer not null check (expected_revision > 0),
  body text not null check (length(cast(body as blob)) between 1 and 8000),
  created_at text not null default (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  unique(actor_id, request_id),
  unique(work_item_id, expected_revision)
);
create index work_item_supplements_item_idx on work_item_supplements(work_item_id, expected_revision);
alter table work_item_tasks add column decision_supplement_id text references work_item_supplements(id);
