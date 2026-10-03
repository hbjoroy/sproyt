alter table circle_memberships drop constraint circle_memberships_role_check;
alter table circle_memberships add constraint circle_memberships_role_check check (role in ('owner','moderator','member'));
