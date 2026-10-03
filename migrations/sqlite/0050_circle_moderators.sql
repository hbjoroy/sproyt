-- Copy before restoring the insert trigger: migration must not fabricate joins.
create table circle_memberships_new (
 circle_id text not null references circles(id) on delete cascade,
 user_id text not null references users(id) on delete cascade,
 role text not null check(role in ('owner','moderator','member')),
 joined_at text not null default current_timestamp,
 primary key(circle_id,user_id)
);
insert into circle_memberships_new select circle_id,user_id,role,joined_at from circle_memberships;
drop table circle_memberships;
alter table circle_memberships_new rename to circle_memberships;
create index circle_memberships_user_idx on circle_memberships(user_id);
create trigger audit_circle_membership_joined after insert on circle_memberships begin
 insert into audit_events(actor_id,action,target_kind,target_id,payload)
 values(new.user_id,'circle.membership_joined','circle',new.circle_id,json_object('role',new.role));
end;
