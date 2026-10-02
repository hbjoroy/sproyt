-- GitHub credentials are external Secrets. This policy is operator-approved,
-- never inferred from the GitHub App's broader installation permissions.
create table work_github_bindings (
 application_id uuid primary key references work_applications(id),
 installation_id bigint not null check (installation_id > 0),
 repository_id bigint not null check (repository_id > 0),
 repository_name text not null check (length(repository_name) between 3 and 200),
 bot_login text not null check (length(bot_login) between 1 and 120),
 enabled boolean not null default false,
 export_tasks_enabled boolean not null default false,
 revision bigint not null default 1 check (revision > 0),
 updated_by uuid not null references users(id)
);
-- Preserve the immutable reviewed instance. The export task is a linked,
-- independently durable Heart continuation, including for historical cases.
create table work_item_export_processes (
 work_item_id uuid primary key references work_items(id),
 channel_id uuid not null references channels(id),
 assignee_id uuid not null references users(id),
 heart_instance_id uuid unique,
 status text not null default 'pending' check (status in ('pending','waiting','completed','cancelled','failed')),
 lease_until bigint not null default 0,
 lease_token uuid,
 created_at bigint not null
);
create table work_item_github_exports (
 work_item_id uuid primary key references work_items(id),
 task_id uuid not null unique references work_item_tasks(id),
 actor_id uuid not null references users(id),
 request_id uuid not null,
 expected_revision bigint not null,
 disposition text not null check (disposition in ('publish','skip')),
 title text not null check (length(title) <= 160),
 body text not null check (length(body) <= 8000),
 installation_id bigint,
 repository_id bigint,
 repository_name text,
 bot_login text,
 binding_revision bigint,
 marker text not null unique,
 status text not null check (status in ('pending','sending','uncertain','sent','skipped','blocked')),
 issue_id bigint,
 issue_number bigint,
 issue_url text,
 lease_until bigint not null default 0,
 lease_token uuid,
 attempted_at bigint,
 created_at bigint not null,
 unique(actor_id,request_id),
 check (disposition <> 'publish' or (length(title) > 0 and length(body) > 0 and installation_id is not null and repository_id is not null and repository_name is not null and binding_revision is not null)),
 check (status <> 'sent' or (issue_id > 0 and issue_number > 0 and issue_url is not null))
);
create index work_item_export_ready_idx on work_item_github_exports(status,lease_until);