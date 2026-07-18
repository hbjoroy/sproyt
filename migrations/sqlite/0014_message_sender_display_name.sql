alter table messages
  add column sender_display_name text not null default 'Unknown'
  check (length(sender_display_name) between 1 and 120);

update messages
set sender_display_name = (
  select users.display_name from users where users.id = messages.sender_id
);
