create table media_objects (
    id uuid primary key,
    owner_id uuid not null references users(id) on delete cascade,
    channel_id uuid not null references channels(id) on delete cascade,
    storage_key text not null unique,
    original_filename text not null,
    content_type text not null,
    size_bytes bigint not null check (size_bytes >= 0),
    sha256 text not null,
    width integer,
    height integer,
    duration_ms bigint,
    alt_text text not null default '',
    analysis_status text not null default 'pending' check (analysis_status in ('pending', 'ready', 'failed', 'disabled')),
    analysis_metadata jsonb not null default '{}'::jsonb,
    created_at timestamptz not null,
    check (length(original_filename) between 1 and 255),
    check (length(content_type) between 1 and 127),
    check (length(alt_text) <= 1000)
);

create table media_blobs (
    media_id uuid primary key references media_objects(id) on delete cascade,
    content bytea not null
);

create table message_attachments (
    message_id uuid not null references messages(id) on delete cascade,
    media_id uuid not null unique references media_objects(id) on delete cascade,
    position smallint not null check (position >= 0),
    primary key (message_id, position)
);

create index media_objects_channel_created_idx on media_objects(channel_id, created_at desc, id);
create index media_objects_owner_created_idx on media_objects(owner_id, created_at desc, id);
create index media_objects_analysis_idx on media_objects(analysis_status, created_at, id);
