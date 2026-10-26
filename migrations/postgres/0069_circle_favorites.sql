alter table circle_memberships add column is_favorite boolean not null default false;
create index circle_memberships_favorites_user_idx on circle_memberships(user_id, circle_id) where is_favorite;
