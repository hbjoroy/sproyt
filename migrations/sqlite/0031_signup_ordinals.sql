-- A durable, private historical ledger. `on delete set null` retains a slot
-- without retaining a deleted account identity.
create table signup_ordinals (
  user_id text unique references users(id) on delete set null,
  ordinal integer primary key check (ordinal > 0)
);
create table signup_ordinal_counter (
  singleton integer primary key check (singleton = 1),
  next_ordinal integer not null check (next_ordinal > 0)
);
insert into signup_ordinals(user_id, ordinal)
select id, row_number() over (order by created_at, id) from users where kind = 'human';
insert into signup_ordinal_counter(singleton, next_ordinal)
select 1, coalesce(max(ordinal), 0) + 1 from signup_ordinals;
