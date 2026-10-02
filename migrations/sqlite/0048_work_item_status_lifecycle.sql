-- Each status change has its own real Heart activation and immutable receipt.
create table work_item_status_changes (
 id text primary key,
 work_item_id text not null references work_items(id),
 actor_id text not null references users(id),
 channel_id text not null references channels(id),
 route_id text not null references channel_task_routes(id),
 source_task_id text not null,
 source_message_id text not null references messages(id),
 request_id text not null,
 start_revision integer not null,
 heart_instance_id text unique,
 task_id text unique,
 message_id text unique references messages(id),
 status text not null default 'pending' check(status in ('pending','waiting','completed','cancelled','failed')),
 lease_until integer not null default 0,
 lease_token text,
 decision_request_id text,
 decision_revision integer,
 decision_status text,
 internal_note text check(length(internal_note)<=2000),
 public_feedback text check(length(public_feedback)<=2000),
 no_change integer,
 decision_result text,
 created_at integer not null,
 unique(actor_id,request_id)
);
create unique index work_item_status_one_active on work_item_status_changes(work_item_id) where status in ('pending','waiting');
create index work_item_status_ready on work_item_status_changes(status,lease_until);
create table work_item_status_history (
 id text primary key,
 change_id text not null unique references work_item_status_changes(id),
 work_item_id text not null references work_items(id),
 revision integer not null,
 actor_id text not null references users(id),
 from_status text not null,
 to_status text not null,
 internal_note text not null,
 public_feedback text not null,
 created_at integer not null,
 unique(work_item_id,revision)
);
create table work_item_status_publications (
 work_item_id text primary key references work_items(id),
 message_id text not null unique references messages(id),
 channel_id text not null references channels(id)
);
