-- Channel notifications are explicit, per-user opt-ins.  Both cursors start at
-- the current UUIDv7 high-water mark so enabling this feature never replays
-- historical messages during a rolling deployment.
alter table push_subscriptions
  add column thread_notification_after_message_id text not null
  default '00000000-0000-7000-8000-000000000000';

update push_subscriptions
  set thread_notification_after_message_id = coalesce(
    (select id from messages order by id desc limit 1),
    '00000000-0000-7000-8000-000000000000'
  );

create table channel_notification_subscriptions (
  user_id text not null references users(id) on delete cascade,
  channel_id text not null references channels(id) on delete cascade,
  notification_after_message_id text not null,
  created_at text not null default current_timestamp,
  primary key(user_id, channel_id)
);

create table notification_outbox_next (
  subscription_id text not null references push_subscriptions(id) on delete cascade,
  recipient_id text not null references users(id) on delete cascade,
  message_id text not null references messages(id) on delete cascade,
  kind text not null check (kind in ('direct_message', 'mention', 'thread_reply', 'channel_message')),
  available_at text not null default current_timestamp,
  leased_until text null,
  attempts integer not null default 0,
  delivered_at text null,
  last_error text null,
  created_at text not null default current_timestamp,
  primary key(subscription_id, message_id)
);

insert into notification_outbox_next
select subscription_id,recipient_id,message_id,kind,available_at,leased_until,attempts,delivered_at,last_error,created_at
from notification_outbox;

drop table notification_outbox;
alter table notification_outbox_next rename to notification_outbox;
create index notification_outbox_pending_idx on notification_outbox(available_at, created_at);
