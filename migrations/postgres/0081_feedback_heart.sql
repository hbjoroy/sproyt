-- One independent Heart process for the original publication of an issue.
create table feedback_heart (
  source_message_id uuid primary key references feedback_jobs(source_message_id) on delete cascade,
  instance_id uuid unique,
  stage text not null default 'starting' check (stage in
    ('starting','review','review_submitted','waiting_for_close','resolved','rejected','failed')),
  reviewed_at bigint,
  assessment text check (length(assessment) between 1 and 2000),
  review_request_id uuid unique,
  closure text check (closure in ('resolved','rejected')),
  due_at bigint not null default 0,
  lease_token uuid,
  check ((assessment is null) = (review_request_id is null)),
  check ((assessment is null) = (reviewed_at is null))
);
create index feedback_heart_ready on feedback_heart(due_at, source_message_id)
  where stage in ('starting','review','review_submitted','waiting_for_close');
