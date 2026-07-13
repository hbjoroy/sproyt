create trigger audit_channel_membership_left after delete on channel_memberships begin
  insert into audit_events(actor_id, action, target_kind, target_id, payload)
  values (old.user_id, 'channel.membership_left', 'channel', old.channel_id,
          json_object('role', old.role, 'last_read_sequence', old.last_read_sequence));
end;
