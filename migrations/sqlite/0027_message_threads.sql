alter table messages
  add column parent_message_id text null references messages(id) on delete restrict;

create index messages_parent_sequence_idx
  on messages(parent_message_id, sequence)
  where parent_message_id is not null;
