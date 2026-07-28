create table notification_preferences (
  user_id text primary key references users(id) on delete cascade,
  mode text not null default 'instant' check (mode in ('instant', 'weekly', 'muted')),
  direct_messages integer not null default 1,
  mentions integer not null default 1,
  weekly_weekday integer not null default 1 check (weekly_weekday between 1 and 7),
  updated_at text not null default current_timestamp
);

create table push_subscriptions (
  id text primary key,
  user_id text not null references users(id) on delete cascade,
  endpoint text not null unique,
  p256dh text not null,
  auth text not null,
  user_agent text null,
  created_at text not null default current_timestamp,
  last_success_at text null,
  failure_count integer not null default 0
);
create index push_subscriptions_user_idx on push_subscriptions(user_id);

create table notification_outbox (
  subscription_id text not null references push_subscriptions(id) on delete cascade,
  recipient_id text not null references users(id) on delete cascade,
  message_id text not null references messages(id) on delete cascade,
  kind text not null check (kind in ('direct_message', 'mention')),
  available_at text not null default current_timestamp,
  leased_until text null,
  attempts integer not null default 0,
  delivered_at text null,
  last_error text null,
  created_at text not null default current_timestamp,
  primary key(subscription_id, message_id)
);
create index notification_outbox_pending_idx on notification_outbox(available_at, created_at);
