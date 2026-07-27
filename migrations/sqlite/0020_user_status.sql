alter table users add column status_text text not null default '' check (length(status_text) <= 100);
alter table users add column status_emoji text not null default '' check (length(status_emoji) <= 32);
alter table users add column status_expires_at text null;
