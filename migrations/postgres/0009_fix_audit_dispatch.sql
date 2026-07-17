create or replace function sproyt_audit_mutation() returns trigger language plpgsql as $$
begin
  if TG_ARGV[0] = 'circle.created' then
    insert into audit_events(actor_id, action, target_kind, target_id)
    values (new.created_by, TG_ARGV[0], 'circle', new.id::text);
  elsif TG_ARGV[0] = 'circle.invitation_created' then
    insert into audit_events(actor_id, action, target_kind, target_id, payload)
    values (new.invited_by, TG_ARGV[0], 'circle', new.circle_id::text,
            jsonb_build_object('invitation_id', new.id));
  elsif TG_ARGV[0] = 'circle.invitation_accepted' then
    if old.accepted_at is null and new.accepted_at is not null then
      insert into audit_events(actor_id, action, target_kind, target_id, payload)
      values (new.accepted_by, TG_ARGV[0], 'circle', new.circle_id::text,
              jsonb_build_object('invitation_id', new.id));
    end if;
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
