create function sproyt_audit_process_command_requested() returns trigger language plpgsql as $$
begin
  if new.command_type in ('correlate', 'inspect') then
    insert into audit_events(actor_id, action, target_kind, target_id, payload)
    values (new.actor_id,
            case new.command_type
              when 'correlate' then 'process.correlation_requested'
              else 'process.inspection_requested'
            end,
            'process_link', new.process_link_id::text,
            jsonb_build_object('outbox_id', new.outbox_id));
  end if;
  return new;
end;
$$;

create trigger audit_process_command_requested after insert on process_command_receipts
for each row execute function sproyt_audit_process_command_requested();

create function sproyt_audit_agent_grant_changed() returns trigger language plpgsql as $$
begin
  if new.revoked_at is null and
     (old.revoked_at is not null or old.expires_at is distinct from new.expires_at or
      old.granted_by is distinct from new.granted_by) then
    insert into audit_events(actor_id, action, target_kind, target_id, payload)
    values (new.granted_by, 'agent.grant_changed', 'agent', new.agent_id::text,
            jsonb_build_object('grant_id', new.id, 'scope', new.scope,
                               'circle_id', new.circle_id, 'channel_id', new.channel_id,
                               'reactivated', old.revoked_at is not null));
  end if;
  return new;
end;
$$;

create trigger audit_agent_grant_changed after update on agent_grants
for each row execute function sproyt_audit_agent_grant_changed();
