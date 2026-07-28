create table message_reactions (
    message_id text not null references messages(id) on delete cascade,
    user_id text not null references users(id) on delete cascade,
    emoji text not null,
    created_at text not null,
    primary key (message_id, user_id, emoji)
);

create index message_reactions_message_idx on message_reactions(message_id, emoji);
