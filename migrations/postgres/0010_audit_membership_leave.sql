create function sproyt_audit_channel_membership_left() returns trigger language plpgsql as $$
begin
  insert into audit_events(actor_id, action, target_kind, target_id, payload)
  values (old.user_id, 'channel.membership_left', 'channel', old.channel_id::text,
          jsonb_build_object('role', old.role, 'last_read_sequence', old.last_read_sequence));
  return old;
end;
$$;

create trigger audit_channel_membership_left after delete on channel_memberships
for each row execute function sproyt_audit_channel_membership_left();
