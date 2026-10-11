-- Additive storage. No existing originals or attachments are rewritten.
create table media_upload_budget (
    id integer primary key check (id = 1),
    revision bigint not null default 0
);
insert into media_upload_budget(id) values(1);

create table media_uploads (
    id uuid primary key,
    owner_id uuid not null references users(id) on delete cascade,
    channel_id uuid not null references channels(id) on delete cascade,
    request_id uuid not null,
    original_filename text not null check(length(original_filename) between 1 and 255),
    declared_type text not null check(length(declared_type) between 1 and 127),
    expected_bytes bigint not null check(expected_bytes between 1 and 1073741824),
    received_bytes bigint not null default 0 check(received_bytes >= 0 and received_bytes <= expected_bytes),
    next_chunk integer not null default 0 check(next_chunk >= 0),
    state text not null default 'open' check(state in ('open','sealed','complete')),
    created_at bigint not null,
    expires_at bigint not null,
    media_id uuid unique references media_objects(id) on delete cascade,
    unique(owner_id,request_id),
    check(expires_at > created_at),
    check((state = 'complete') = (media_id is not null))
);
create index media_uploads_expiry_idx on media_uploads(state,expires_at);
create index media_uploads_owner_idx on media_uploads(owner_id);

create table media_upload_chunks (
    upload_id uuid not null references media_uploads(id) on delete cascade,
    chunk_index integer not null check(chunk_index >= 0),
    content bytea not null check(octet_length(content) between 1 and 1048576),
    sha256 text not null check(length(sha256) = 64),
    primary key(upload_id,chunk_index)
);

create table media_preview_jobs (
    media_id uuid primary key references media_objects(id) on delete cascade,
    state text not null default 'pending' check(state in ('pending','running','ready','failed')),
    attempts integer not null default 0 check(attempts >= 0),
    available_at bigint not null,
    lease_token uuid,
    lease_until bigint,
    last_error text not null default '' check(length(last_error) <= 1000),
    derivative_version integer not null default 1 check(derivative_version > 0),
    check((state = 'running') = (lease_token is not null and lease_until is not null)),
    check((lease_token is null) = (lease_until is null))
);
create index media_preview_jobs_available_idx on media_preview_jobs(state,available_at);
