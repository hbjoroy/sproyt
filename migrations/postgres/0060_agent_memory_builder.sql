-- Shared admission state; no model call is made while holding a transaction.
create table agent_memory_model_quota (
  id integer primary key check(id=1),
  lease_token uuid,
  leased_until bigint not null default 0,
  memory_window_started bigint not null default 0,
  memory_calls integer not null default 0 check(memory_calls between 0 and 2)
);
insert into agent_memory_model_quota(id) values(1);
alter table agent_memory_scopes add column last_error text;
alter table agent_memory_scopes add column last_completed_at bigint;
