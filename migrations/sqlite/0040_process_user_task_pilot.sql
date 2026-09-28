-- Small, isolated pilot. Heart remains authoritative for user tasks.
create table process_pilot_channels (
 channel_id text primary key references channels(id) on delete cascade,
 assignee_id text not null references users(id),
 enabled integer not null default 1 check (enabled in (0,1))
);
create table process_pilot_runs (
 id text primary key,
 channel_id text not null references process_pilot_channels(channel_id) on delete cascade,
 actor_id text not null references users(id),
 assignee_id text not null references users(id),
 request_id text not null,
 instance_id text,
 status text not null default 'starting' check (status in ('starting','waiting','completed')),
 lease_until bigint not null default 0,
 lease_token text not null default '',
 unique(actor_id,request_id)
);
create table process_pilot_tasks (
 id text primary key,
 run_id text not null references process_pilot_runs(id) on delete cascade,
 message_id text not null unique references messages(id),
 node_id text not null,
 status text not null check (status in ('pending','completed')),
 command_id text not null default '',
 delivery_status text not null default 'ready' check (delivery_status in ('ready','pending')),
 unique(run_id,node_id)
);
create index process_pilot_runs_work on process_pilot_runs(status,lease_until);

create trigger audit_process_pilot_configured after insert on process_pilot_channels begin
 insert into audit_events(actor_id,action,target_kind,target_id) values(new.assignee_id,'process.pilot_configured','channel',new.channel_id);
end;
create trigger audit_process_pilot_enabled after update of enabled on process_pilot_channels when old.enabled<>new.enabled begin
 insert into audit_events(actor_id,action,target_kind,target_id,payload) values(new.assignee_id,'process.pilot_enabled','channel',new.channel_id,json_object('enabled',new.enabled));
end;
create trigger audit_process_pilot_started after insert on process_pilot_runs begin
 insert into audit_events(actor_id,action,target_kind,target_id) values(new.actor_id,'process.pilot_started','pilot_run',new.id);
end;
create trigger audit_process_user_task_completion after update of command_id on process_pilot_tasks when old.command_id='' and new.command_id<>'' begin
 insert into audit_events(actor_id,action,target_kind,target_id,payload)
 select assignee_id,'process.user_task_completion_requested','user_task',new.id,json_object('request_id',new.command_id) from process_pilot_runs where id=new.run_id;
end;
