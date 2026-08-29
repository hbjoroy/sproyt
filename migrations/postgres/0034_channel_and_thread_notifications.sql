-- Channel notifications are explicit, per-user opt-ins.  Both cursors start at
-- the current UUIDv7 high-water mark so enabling this feature never replays
-- historical messages during a rolling deployment.
alter table push_subscriptions
  add column thread_notification_after_message_id uuid;

update push_subscriptions
  set thread_notification_after_message_id = coalesce(
    (select id from messages order by id desc limit 1),
    '00000000-0000-7000-8000-000000000000'::uuid
  )
  where thread_notification_after_message_id is null;

alter table push_subscriptions
  alter column thread_notification_after_message_id set not null;

alter table push_subscriptions
  alter column thread_notification_after_message_id
  set default '00000000-0000-7000-8000-000000000000'::uuid;

create table channel_notification_subscriptions (
  user_id uuid not null references users(id) on delete cascade,
  channel_id uuid not null references channels(id) on delete cascade,
  notification_after_message_id uuid not null,
  created_at timestamptz not null default now(),
  primary key(user_id, channel_id)
);

alter table notification_outbox
  drop constraint notification_outbox_kind_check;

alter table notification_outbox
  add constraint notification_outbox_kind_check
  check (kind in ('direct_message', 'mention', 'thread_reply', 'channel_message'));
