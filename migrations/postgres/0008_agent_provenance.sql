create table message_provenance (
  message_id uuid primary key references messages(id) on delete cascade,
  provenance text not null check (provenance in ('human','generated','delegated','human_approved')),
  agent_id uuid null references agent_profiles(agent_id),
  owner_id uuid null references users(id),
  delegated_by uuid null references users(id),
  approved_by uuid null references users(id),
  approved_at timestamptz null,
  created_at timestamptz not null
);

insert into message_provenance(message_id,provenance,agent_id,owner_id,created_at)
select m.id,case when u.kind='agent' then 'generated' else 'human' end,
       case when u.kind='agent' then m.sender_id end,p.owner_id,m.created_at
from messages m join users u on u.id=m.sender_id left join agent_profiles p on p.agent_id=u.id;

create function sproyt_classify_message_provenance() returns trigger language plpgsql as $$
declare sender_kind text; agent_owner uuid;
begin
  select kind into sender_kind from users where id=new.sender_id;
  if sender_kind='agent' then select owner_id into agent_owner from agent_profiles where agent_id=new.sender_id; end if;
  insert into message_provenance(message_id,provenance,agent_id,owner_id,created_at)
  values(new.id,case when sender_kind='agent' then 'generated' else 'human' end,
         case when sender_kind='agent' then new.sender_id end,agent_owner,new.created_at);
  return new;
end; $$;
create trigger classify_message_provenance after insert on messages for each row execute function sproyt_classify_message_provenance();

create function sproyt_audit_agent_provenance() returns trigger language plpgsql as $$
begin
  if TG_OP='INSERT' and new.agent_id is not null then
    insert into audit_events(actor_id,action,target_kind,target_id,payload) values(new.agent_id,'agent.message_created','message',new.message_id::text,jsonb_build_object('provenance',new.provenance,'owner_id',new.owner_id));
  elsif TG_OP='UPDATE' and old.provenance is distinct from new.provenance then
    insert into audit_events(actor_id,action,target_kind,target_id,payload) values(coalesce(new.approved_by,new.delegated_by,new.agent_id),'agent.message_provenance_changed','message',new.message_id::text,jsonb_build_object('from',old.provenance,'to',new.provenance,'owner_id',new.owner_id));
  end if;
  return new;
end; $$;
create trigger audit_agent_message_provenance after insert or update on message_provenance for each row execute function sproyt_audit_agent_provenance();
