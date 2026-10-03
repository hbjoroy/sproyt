-- Personal Unicode choices; removing a choice never touches chat history.
create table personal_emojis (
  user_id text not null references users(id) on delete cascade,
  slot integer not null check (slot between 1 and 50),
  emoji text not null check (length(cast(emoji as blob)) between 1 and 32),
  primary key (user_id, slot),
  unique (user_id, emoji)
);
