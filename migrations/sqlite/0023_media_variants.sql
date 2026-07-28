create table media_variants (
    media_id text not null references media_objects(id) on delete cascade,
    variant text not null,
    content_type text not null,
    size_bytes integer not null check (size_bytes >= 0),
    width integer not null check (width > 0),
    height integer not null check (height > 0),
    content blob not null,
    created_at text not null,
    primary key (media_id, variant),
    check (variant in ('preview'))
);
