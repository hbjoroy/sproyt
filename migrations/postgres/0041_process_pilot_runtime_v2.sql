-- Preserve existing v1 runs/message references; pin new runtime and task activation.
alter table process_pilot_runs add column runtime_model text not null default 'v1' check (runtime_model in ('v1','v2'));
alter table process_pilot_runs add column engine_url text not null default '';
alter table process_pilot_runs add column definition_name text not null default 'sproyt-user-task-pilot';
alter table process_pilot_runs add column definition_version text not null default '1.0.0';
alter table process_pilot_runs drop constraint process_pilot_runs_status_check;
alter table process_pilot_runs add constraint process_pilot_runs_status_check check(status in ('starting','waiting','completed','cancelled','failed'));
alter table process_pilot_tasks add column heart_task_id text;
update process_pilot_tasks set heart_task_id=id;
alter table process_pilot_tasks alter column heart_task_id set not null;
alter table process_pilot_tasks drop constraint process_pilot_tasks_run_id_node_id_key;
alter table process_pilot_tasks add constraint process_pilot_task_activation unique(run_id,heart_task_id);
alter table process_pilot_tasks drop constraint process_pilot_tasks_status_check;
alter table process_pilot_tasks add constraint process_pilot_tasks_status_check check(status in ('pending','completed','cancelled'));
