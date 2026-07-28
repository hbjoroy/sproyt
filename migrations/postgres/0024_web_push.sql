create table notification_preferences (
  user_id uuid primary key references users(id) on delete cascade,
  mode text not null default 'instant' check (mode in ('instant', 'weekly', 'muted')),
  direct_messages boolean not null default true,
  mentions boolean not null default true,
  weekly_weekday smallint not null default 1 check (weekly_weekday between 1 and 7),
  updated_at timestamptz not null default now()
);

create table push_subscriptions (
  id uuid primary key,
  user_id uuid not null references users(id) on delete cascade,
  endpoint text not null unique,
  p256dh text not null,
  auth text not null,
  user_agent text null,
  created_at timestamptz not null default now(),
  last_success_at timestamptz null,
  failure_count integer not null default 0
);
create index push_subscriptions_user_idx on push_subscriptions(user_id);

create table notification_outbox (
  subscription_id uuid not null references push_subscriptions(id) on delete cascade,
  recipient_id uuid not null references users(id) on delete cascade,
  message_id uuid not null references messages(id) on delete cascade,
  kind text not null check (kind in ('direct_message', 'mention')),
  available_at timestamptz not null default now(),
  leased_until timestamptz null,
  attempts integer not null default 0,
  delivered_at timestamptz null,
  last_error text null,
  created_at timestamptz not null default now(),
  primary key(subscription_id, message_id)
);
create index notification_outbox_pending_idx
  on notification_outbox(available_at, created_at)
  where delivered_at is null;
