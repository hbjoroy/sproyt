create table agent_profiles (
  agent_id text primary key references users(id) on delete cascade,
  owner_id text not null references users(id),
  invited_by text not null references users(id),
  provider text not null,
  service_identity text not null,
  purpose text not null,
  rate_limit_per_minute integer not null check (rate_limit_per_minute between 1 and 600),
  expires_at text null,
  revoked_at text null,
  created_at text not null,
  unique(provider, service_identity)
);

create table agent_grants (
  id text primary key,
  agent_id text not null references agent_profiles(agent_id) on delete cascade,
  circle_id text null references circles(id) on delete cascade,
  channel_id text null references channels(id) on delete cascade,
  scope text not null check (scope in ('read_history','send_messages','start_processes','complete_process_work')),
  granted_by text not null references users(id),
  expires_at text null,
  revoked_at text null,
  revoked_by text null references users(id),
  created_at text not null,
  check (circle_id is not null or channel_id is not null)
);

create index agent_grants_effective_idx on agent_grants(agent_id, scope, revoked_at, expires_at);
create unique index agent_grants_identity_idx on agent_grants(agent_id, coalesce(circle_id,''), coalesce(channel_id,''), scope);

create table agent_credentials (
  id text primary key,
  agent_id text not null references agent_profiles(agent_id) on delete cascade,
  token_hash blob not null unique,
  expires_at text not null,
  revoked_at text null,
  last_used_at text null,
  created_at text not null
);

create trigger audit_agent_created after insert on agent_profiles begin
  insert into audit_events(actor_id,action,target_kind,target_id,payload)
  values(new.invited_by,'agent.created','agent',new.agent_id,json_object('owner_id',new.owner_id,'provider',new.provider,'purpose',new.purpose));
end;
create trigger audit_agent_granted after insert on agent_grants begin
  insert into audit_events(actor_id,action,target_kind,target_id,payload)
  values(new.granted_by,'agent.grant_created','agent',new.agent_id,json_object('grant_id',new.id,'scope',new.scope,'circle_id',new.circle_id,'channel_id',new.channel_id));
end;
create trigger audit_agent_grant_revoked after update of revoked_at on agent_grants when old.revoked_at is null and new.revoked_at is not null begin
  insert into audit_events(actor_id,action,target_kind,target_id,payload)
  values(new.revoked_by,'agent.grant_revoked','agent',new.agent_id,json_object('grant_id',new.id,'scope',new.scope));
end;
