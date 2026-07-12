create table agent_profiles (
  agent_id uuid primary key references users(id) on delete cascade,
  owner_id uuid not null references users(id),
  invited_by uuid not null references users(id),
  provider text not null,
  service_identity text not null,
  purpose text not null,
  rate_limit_per_minute integer not null check (rate_limit_per_minute between 1 and 600),
  expires_at timestamptz null,
  revoked_at timestamptz null,
  created_at timestamptz not null,
  unique(provider, service_identity)
);

create table agent_grants (
  id uuid primary key,
  agent_id uuid not null references agent_profiles(agent_id) on delete cascade,
  circle_id uuid null references circles(id) on delete cascade,
  channel_id uuid null references channels(id) on delete cascade,
  scope text not null check (scope in ('read_history','send_messages','start_processes','complete_process_work')),
  granted_by uuid not null references users(id),
  expires_at timestamptz null,
  revoked_at timestamptz null,
  revoked_by uuid null references users(id),
  created_at timestamptz not null,
  check (circle_id is not null or channel_id is not null),
  unique nulls not distinct(agent_id, circle_id, channel_id, scope)
);

create index agent_grants_effective_idx on agent_grants(agent_id, scope, revoked_at, expires_at);

create table agent_credentials (
  id uuid primary key,
  agent_id uuid not null references agent_profiles(agent_id) on delete cascade,
  token_hash bytea not null unique,
  expires_at timestamptz not null,
  revoked_at timestamptz null,
  last_used_at timestamptz null,
  created_at timestamptz not null
);

create function sproyt_audit_agent_mutation() returns trigger language plpgsql as $$
begin
  if TG_ARGV[0]='agent.created' then
    insert into audit_events(actor_id,action,target_kind,target_id,payload) values(new.invited_by,TG_ARGV[0],'agent',new.agent_id::text,jsonb_build_object('owner_id',new.owner_id,'provider',new.provider,'purpose',new.purpose));
  elsif TG_ARGV[0]='agent.grant_created' then
    insert into audit_events(actor_id,action,target_kind,target_id,payload) values(new.granted_by,TG_ARGV[0],'agent',new.agent_id::text,jsonb_build_object('grant_id',new.id,'scope',new.scope,'circle_id',new.circle_id,'channel_id',new.channel_id));
  elsif TG_ARGV[0]='agent.grant_revoked' and old.revoked_at is null and new.revoked_at is not null then
    insert into audit_events(actor_id,action,target_kind,target_id,payload) values(new.revoked_by,TG_ARGV[0],'agent',new.agent_id::text,jsonb_build_object('grant_id',new.id,'scope',new.scope));
  end if;
  return new;
end; $$;
create trigger audit_agent_created after insert on agent_profiles for each row execute function sproyt_audit_agent_mutation('agent.created');
create trigger audit_agent_granted after insert on agent_grants for each row execute function sproyt_audit_agent_mutation('agent.grant_created');
create trigger audit_agent_grant_revoked after update of revoked_at on agent_grants for each row execute function sproyt_audit_agent_mutation('agent.grant_revoked');
