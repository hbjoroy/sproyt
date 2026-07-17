create table audit_events (
  sequence bigint generated always as identity primary key,
  actor_id uuid null references users(id),
  action text not null,
  target_kind text not null,
  target_id text not null,
  payload jsonb not null default '{}'::jsonb,
  occurred_at timestamptz not null default now()
);

create index audit_events_actor_sequence_idx on audit_events(actor_id, sequence desc);
create index audit_events_target_sequence_idx on audit_events(target_kind, target_id, sequence desc);

create function sproyt_audit_mutation() returns trigger language plpgsql as $$
begin
  if TG_ARGV[0] = 'circle.created' then
    insert into audit_events(actor_id, action, target_kind, target_id)
    values (new.created_by, TG_ARGV[0], 'circle', new.id::text);
  elsif TG_ARGV[0] = 'circle.invitation_created' then
    insert into audit_events(actor_id, action, target_kind, target_id, payload)
    values (new.invited_by, TG_ARGV[0], 'circle', new.circle_id::text,
            jsonb_build_object('invitation_id', new.id));
  elsif TG_ARGV[0] = 'circle.invitation_accepted' and old.accepted_at is null and new.accepted_at is not null then
    insert into audit_events(actor_id, action, target_kind, target_id, payload)
    values (new.accepted_by, TG_ARGV[0], 'circle', new.circle_id::text,
            jsonb_build_object('invitation_id', new.id));
  elsif TG_ARGV[0] = 'circle.membership_joined' then
    insert into audit_events(actor_id, action, target_kind, target_id, payload)
    values (new.user_id, TG_ARGV[0], 'circle', new.circle_id::text,
            jsonb_build_object('role', new.role));
  elsif TG_ARGV[0] = 'channel.membership_joined' then
    insert into audit_events(actor_id, action, target_kind, target_id, payload)
    values (new.user_id, TG_ARGV[0], 'channel', new.channel_id::text,
            jsonb_build_object('role', new.role));
  end if;
  return new;
end;
$$;

create trigger audit_circle_created after insert on circles
for each row execute function sproyt_audit_mutation('circle.created');
create trigger audit_circle_invitation_created after insert on circle_invitations
for each row execute function sproyt_audit_mutation('circle.invitation_created');
create trigger audit_circle_invitation_accepted after update of accepted_at on circle_invitations
for each row execute function sproyt_audit_mutation('circle.invitation_accepted');
create trigger audit_circle_membership_joined after insert on circle_memberships
for each row execute function sproyt_audit_mutation('circle.membership_joined');
create trigger audit_channel_membership_joined after insert on channel_memberships
for each row execute function sproyt_audit_mutation('channel.membership_joined');
