create function sproyt_audit_agent_revoked() returns trigger language plpgsql as $$
begin
  if old.revoked_at is null and new.revoked_at is not null then
    insert into audit_events(actor_id, action, target_kind, target_id, payload)
    values (new.owner_id, 'agent.revoked', 'agent', new.agent_id::text,
            jsonb_build_object('provider', new.provider, 'service_identity', new.service_identity));
  end if;
  return new;
end;
$$;

create trigger audit_agent_revoked after update of revoked_at on agent_profiles
for each row execute function sproyt_audit_agent_revoked();
