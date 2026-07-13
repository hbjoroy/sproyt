create trigger audit_circle_feature_updated after update on circle_features
when old.enabled is not new.enabled or old.updated_by is not new.updated_by begin
  insert into audit_events(actor_id, action, target_kind, target_id, payload)
  values (new.updated_by, 'circle.feature_changed', 'circle', new.circle_id,
          json_object('feature', new.feature, 'enabled', new.enabled));
end;

create trigger audit_process_command_requested after insert on process_command_receipts
when new.command_type in ('correlate', 'inspect') begin
  insert into audit_events(actor_id, action, target_kind, target_id, payload)
  values (new.actor_id,
          case new.command_type
            when 'correlate' then 'process.correlation_requested'
            else 'process.inspection_requested'
          end,
          'process_link', new.process_link_id,
          json_object('outbox_id', new.outbox_id));
end;

create trigger audit_agent_grant_changed after update on agent_grants
when new.revoked_at is null and
     (old.revoked_at is not null or old.expires_at is not new.expires_at or
      old.granted_by is not new.granted_by) begin
  insert into audit_events(actor_id, action, target_kind, target_id, payload)
  values (new.granted_by, 'agent.grant_changed', 'agent', new.agent_id,
          json_object('grant_id', new.id, 'scope', new.scope,
                      'circle_id', new.circle_id, 'channel_id', new.channel_id,
                      'reactivated', old.revoked_at is not null));
end;
