create table personal_spaces (
  owner_id uuid primary key references users(id) on delete cascade,
  channel_id uuid not null unique references channels(id) on delete cascade,
  note_markdown text not null default '',
  note_revision bigint not null default 0 check (note_revision >= 0),
  -- Preserve the owner membership when an older server handles a leave request.
  foreign key (channel_id, owner_id) references channel_memberships (channel_id, user_id)
    deferrable initially deferred
);

create table personal_saved_links (
  id uuid primary key,
  owner_id uuid not null references personal_spaces(owner_id) on delete cascade,
  source_channel_id uuid not null,
  source_message_id uuid not null,
  log_message_id uuid unique references messages(id) on delete set null,
  saved_at timestamptz not null,
  unique (owner_id, source_message_id)
);
