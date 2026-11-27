create table profile_avatars (
    owner_id uuid primary key not null references users(id) on delete cascade,
    job_id text not null,
    jpeg bytea not null check (length(jpeg) between 1 and 262144)
);
