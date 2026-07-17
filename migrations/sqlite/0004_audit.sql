create table audit_events (
  sequence integer primary key autoincrement,
  actor_id text null references users(id),
  action text not null,
  target_kind text not null,
  target_id text not null,
  payload text not null default '{}',
  occurred_at text not null default current_timestamp
);

create index audit_events_actor_sequence_idx on audit_events(actor_id, sequence desc);
create index audit_events_target_sequence_idx on audit_events(target_kind, target_id, sequence desc);

create trigger audit_circle_created after insert on circles begin
  insert into audit_events(actor_id, action, target_kind, target_id)
  values (new.created_by, 'circle.created', 'circle', new.id);
end;

create trigger audit_circle_invitation_created after insert on circle_invitations begin
  insert into audit_events(actor_id, action, target_kind, target_id, payload)
  values (new.invited_by, 'circle.invitation_created', 'circle', new.circle_id,
          json_object('invitation_id', new.id));
end;

create trigger audit_circle_invitation_accepted after update of accepted_at on circle_invitations
when old.accepted_at is null and new.accepted_at is not null begin
  insert into audit_events(actor_id, action, target_kind, target_id, payload)
  values (new.accepted_by, 'circle.invitation_accepted', 'circle', new.circle_id,
          json_object('invitation_id', new.id));
end;

create trigger audit_circle_membership_joined after insert on circle_memberships begin
  insert into audit_events(actor_id, action, target_kind, target_id, payload)
  values (new.user_id, 'circle.membership_joined', 'circle', new.circle_id,
          json_object('role', new.role));
end;

create trigger audit_channel_membership_joined after insert on channel_memberships begin
  insert into audit_events(actor_id, action, target_kind, target_id, payload)
  values (new.user_id, 'channel.membership_joined', 'channel', new.channel_id,
          json_object('role', new.role));
end;
