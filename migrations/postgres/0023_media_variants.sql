create table media_variants (
    media_id uuid not null references media_objects(id) on delete cascade,
    variant text not null,
    content_type text not null,
    size_bytes bigint not null check (size_bytes >= 0),
    width integer not null check (width > 0),
    height integer not null check (height > 0),
    content bytea not null,
    created_at timestamptz not null,
    primary key (media_id, variant),
    check (variant in ('preview'))
);
