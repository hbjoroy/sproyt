-- Enable the new admission path per route once every worker understands it.
alter table feedback_routes add column intake_enabled boolean not null default false;
alter table feedback_jobs add column intake_version integer not null default 0 check (intake_version in (0,1));

alter table feedback_jobs add column intake_clarification_needed boolean not null default false;

create table feedback_intakes (
  source_message_id uuid primary key references feedback_jobs(source_message_id) on delete cascade,
  instance_id uuid unique,
  definition_id uuid,
  stage text not null default 'starting',
  task_id uuid unique,
  reason text,
  candidates text not null default '[]',
  reviewed_at bigint,
  due_at bigint not null default 0,
  lease_token uuid
);
create index feedback_intake_due on feedback_intakes(due_at,source_message_id)
  where stage not in ('ignored','linked','resolved','rejected','failed');
create table feedback_intake_results (
  source_message_id uuid not null references feedback_intakes(source_message_id) on delete cascade,
  node_id text not null,
  task_id uuid primary key,
  recorded_at timestamp not null default current_timestamp,
  owner_note text,
  result_json text not null
);
create index feedback_intake_history on feedback_intake_results(source_message_id,node_id);
