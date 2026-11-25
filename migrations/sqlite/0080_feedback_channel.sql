-- Explicit operator binding. after_sequence fixes the activation boundary.
create table feedback_routes (
    channel_id text primary key references channels(id),
    agent_id text not null references circle_chat_agents(agent_id),
    reviewer_id text not null references users(id),
    application_id text not null references work_github_bindings(application_id),
    enabled boolean not null default false,
    after_sequence bigint not null check (after_sequence >= 0)
);

create table feedback_jobs (
    source_message_id text primary key references messages(id) on delete cascade,
    channel_id text not null references channels(id),
    source_sequence bigint not null,
    source_body text not null,
    agent_id text not null references users(id),
    reviewer_id text not null references users(id),
    application_id text not null references work_applications(id),
    binding_revision bigint not null,
    repository_id bigint not null,
    installation_id bigint not null,
    repository_name text not null,
    bot_login text not null,
    status text not null default 'pending' check (status in
        ('pending','ignored','needs_review','ready','sending','uncertain','published','done')),
    publication_kind text check (publication_kind in ('create','comment','duplicate')),
    published_title text,
    published_body text,
    issue_number bigint,
    issue_id bigint,
    issue_url text,
    github_state text,
    github_status_name text,
    github_state_reason text,
    github_proposed boolean,
    github_checked_at bigint not null default 0,
    attempts bigint not null default 0,
    ack_message_id text references messages(id),
    lease_token text,
    available_at bigint not null default 0,
    check (status not in ('published','done') or
        (issue_id is not null and issue_number is not null and issue_url is not null))
);
create index feedback_ready on feedback_jobs(status,available_at);
create index feedback_issue on feedback_jobs(repository_id,issue_id);
create index feedback_channel_sequence on feedback_jobs(channel_id,source_sequence);
create index feedback_queue on feedback_jobs(repository_id,source_sequence);
