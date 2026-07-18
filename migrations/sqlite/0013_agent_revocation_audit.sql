create trigger audit_agent_revoked after update of revoked_at on agent_profiles
when old.revoked_at is null and new.revoked_at is not null begin
  insert into audit_events(actor_id, action, target_kind, target_id, payload)
  values (new.owner_id, 'agent.revoked', 'agent', new.agent_id,
          json_object('provider', new.provider, 'service_identity', new.service_identity));
end;
