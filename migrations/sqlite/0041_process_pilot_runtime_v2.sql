-- Rebuild only the two delivery tables, preserving IDs, receipts and message links.
-- No outside table references them. Copy before dropping; recreate audit triggers
-- after copying so migration never replays user actions.
create temporary table saved_process_pilot_tasks as select * from process_pilot_tasks;
create table process_pilot_runs_v2 (
 id text primary key,
 channel_id text not null references process_pilot_channels(channel_id) on delete cascade,
 actor_id text not null references users(id), assignee_id text not null references users(id),
 request_id text not null, instance_id text,
 status text not null default 'starting' check(status in ('starting','waiting','completed','cancelled','failed')),
 lease_until integer not null default 0, lease_token text not null default '',
 runtime_model text not null default 'v1' check(runtime_model in ('v1','v2')),
 engine_url text not null default '',
 definition_name text not null default 'sproyt-user-task-pilot',
 definition_version text not null default '1.0.0',
 unique(actor_id,request_id)
);
insert into process_pilot_runs_v2(id,channel_id,actor_id,assignee_id,request_id,instance_id,status,lease_until,lease_token)
select id,channel_id,actor_id,assignee_id,request_id,instance_id,status,lease_until,lease_token from process_pilot_runs;
drop table process_pilot_tasks;
drop table process_pilot_runs;
alter table process_pilot_runs_v2 rename to process_pilot_runs;
create index process_pilot_runs_work on process_pilot_runs(status,lease_until);
create table process_pilot_tasks (
 id text primary key,
 run_id text not null references process_pilot_runs(id) on delete cascade,
 message_id text not null unique references messages(id), node_id text not null,
 status text not null check(status in ('pending','completed','cancelled')),
 command_id text not null default '',
 delivery_status text not null default 'ready' check(delivery_status in ('ready','pending')),
 heart_task_id text not null default '',
 unique(run_id,heart_task_id)
);
insert into process_pilot_tasks(id,run_id,message_id,node_id,status,command_id,delivery_status,heart_task_id)
select id,run_id,message_id,node_id,status,command_id,delivery_status,id from saved_process_pilot_tasks;
drop table saved_process_pilot_tasks;
-- Preserve inserts issued by the previous adapter during a rolling update.
create trigger process_pilot_activation_compat after insert on process_pilot_tasks when new.heart_task_id='' begin
 update process_pilot_tasks set heart_task_id=new.id where id=new.id;
end;
create trigger audit_process_pilot_started after insert on process_pilot_runs begin
 insert into audit_events(actor_id,action,target_kind,target_id) values(new.actor_id,'process.pilot_started','pilot_run',new.id);
end;
create trigger audit_process_user_task_completion after update of command_id on process_pilot_tasks when old.command_id='' and new.command_id<>'' begin
 insert into audit_events(actor_id,action,target_kind,target_id,payload)
 select assignee_id,'process.user_task_completion_requested','user_task',new.id,json_object('request_id',new.command_id) from process_pilot_runs where id=new.run_id;
end;
