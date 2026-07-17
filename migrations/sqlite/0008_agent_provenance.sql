create table message_provenance (
  message_id text primary key references messages(id) on delete cascade,
  provenance text not null check (provenance in ('human','generated','delegated','human_approved')),
  agent_id text null references agent_profiles(agent_id),
  owner_id text null references users(id),
  delegated_by text null references users(id),
  approved_by text null references users(id),
  approved_at text null,
  created_at text not null
);

insert into message_provenance(message_id,provenance,agent_id,owner_id,created_at)
select m.id,case when u.kind='agent' then 'generated' else 'human' end,
       case when u.kind='agent' then m.sender_id end,p.owner_id,m.created_at
from messages m join users u on u.id=m.sender_id left join agent_profiles p on p.agent_id=u.id;

create trigger classify_message_provenance after insert on messages begin
  insert into message_provenance(message_id,provenance,agent_id,owner_id,created_at)
  select new.id,case when u.kind='agent' then 'generated' else 'human' end,
         case when u.kind='agent' then new.sender_id end,p.owner_id,new.created_at
  from users u left join agent_profiles p on p.agent_id=u.id where u.id=new.sender_id;
end;

create trigger audit_agent_message_created after insert on message_provenance
when new.agent_id is not null begin
  insert into audit_events(actor_id,action,target_kind,target_id,payload)
  values(new.agent_id,'agent.message_created','message',new.message_id,
         json_object('provenance',new.provenance,'owner_id',new.owner_id));
end;

create trigger audit_agent_message_provenance after update of provenance on message_provenance begin
  insert into audit_events(actor_id,action,target_kind,target_id,payload)
  values(coalesce(new.approved_by,new.delegated_by,new.agent_id),'agent.message_provenance_changed','message',new.message_id,
         json_object('from',old.provenance,'to',new.provenance,'owner_id',new.owner_id));
end;
