create table direct_conversations (
  channel_id uuid primary key not null references channels(id) on delete cascade,
  user_a_id uuid not null references users(id) on delete cascade,
  user_b_id uuid not null references users(id) on delete cascade,
  created_at timestamptz not null default now(),
  check (user_a_id < user_b_id),
  unique (user_a_id, user_b_id)
);

create index direct_conversations_user_a_idx on direct_conversations(user_a_id);
create index direct_conversations_user_b_idx on direct_conversations(user_b_id);
