alter table messages add column sender_display_name text;

update messages
set sender_display_name = users.display_name
from users
where users.id = messages.sender_id;

alter table messages
  alter column sender_display_name set not null,
  add constraint messages_sender_display_name_length
    check (length(sender_display_name) between 1 and 120);
