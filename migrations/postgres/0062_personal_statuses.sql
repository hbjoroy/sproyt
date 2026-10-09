create table personal_statuses (
    user_id uuid not null references users(id) on delete cascade,
    text text not null check (length(text) <= 100),
    emoji text not null check (length(emoji) <= 16),
    save_count bigint not null check (save_count >= 1),
    last_used_at timestamptz not null,
    primary key (user_id, text, emoji),
    check (text <> '' or emoji <> '')
);
create index personal_statuses_order on personal_statuses(user_id, save_count desc, last_used_at desc, text, emoji);
