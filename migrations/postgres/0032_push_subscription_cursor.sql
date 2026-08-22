-- Message UUIDv7 values provide a chronological high-water mark without the
-- second-level precision gap in SQLite. A newly registered device starts
-- after the database snapshot; existing outbox rows are deliberately left alone.
alter table push_subscriptions
  add column notification_after_message_id uuid;

update push_subscriptions
  set notification_after_message_id = coalesce(
    (select id from messages order by id desc limit 1),
    '00000000-0000-7000-8000-000000000000'::uuid
  )
  where notification_after_message_id is null;

alter table push_subscriptions
  alter column notification_after_message_id set not null;
