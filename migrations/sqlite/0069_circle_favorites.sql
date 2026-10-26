alter table circle_memberships add column is_favorite integer not null default 0 check (is_favorite in (0, 1));
create index circle_memberships_favorites_user_idx on circle_memberships(user_id, circle_id) where is_favorite = 1;
