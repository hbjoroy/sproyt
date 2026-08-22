-- A durable, private historical ledger. `on delete set null` retains a slot
-- without retaining a deleted account identity.
create table signup_ordinals (
  user_id uuid unique references users(id) on delete set null,
  ordinal bigint primary key check (ordinal > 0)
);
create table signup_ordinal_counter (
  singleton boolean primary key check (singleton),
  next_ordinal bigint not null check (next_ordinal > 0)
);
insert into signup_ordinals(user_id, ordinal)
select id, row_number() over (order by created_at, id) from users where kind = 'human';
insert into signup_ordinal_counter(singleton, next_ordinal)
select true, coalesce(max(ordinal), 0) + 1 from signup_ordinals;
