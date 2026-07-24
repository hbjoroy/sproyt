create table direct_conversations (
  channel_id text primary key not null references channels(id) on delete cascade,
  user_a_id text not null references users(id) on delete cascade,
  user_b_id text not null references users(id) on delete cascade,
  created_at text not null default current_timestamp,
  check (user_a_id < user_b_id),
  unique (user_a_id, user_b_id)
);

create index direct_conversations_user_a_idx on direct_conversations(user_a_id);
create index direct_conversations_user_b_idx on direct_conversations(user_b_id);
