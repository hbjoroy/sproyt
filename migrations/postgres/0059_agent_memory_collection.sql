-- M3 privacy fences apply to every writer, independent of worker flags.
alter table agent_memory_scopes add column repair_sequence bigint check(repair_sequence>=start_sequence);

create function agent_memory_message_edit() returns trigger language plpgsql as $$
begin
update agent_memory_profiles set memory_epoch=memory_epoch+1 where id in(select profile_id from agent_memory_scopes where channel_id=OLD.channel_id and (start_sequence<OLD.sequence or profile_id in(select n.profile_id from agent_memory_notes n join agent_memory_note_sources ns on ns.note_id=n.id where ns.message_id=OLD.id)));
update agent_memory_scopes set repair_sequence=greatest(start_sequence,least(coalesce(repair_sequence,OLD.sequence-1),OLD.sequence-1)),source_generation=source_generation+1,lease_token=null,leased_until=null,attempts=0 where channel_id=OLD.channel_id and (start_sequence<OLD.sequence or profile_id in(select n.profile_id from agent_memory_notes n join agent_memory_note_sources ns on ns.note_id=n.id where ns.message_id=OLD.id));
delete from agent_memory_notes where id in(select note_id from agent_memory_note_sources where message_id=OLD.id);
return null;
end $$;
create trigger agent_memory_message_edit after update of body,edited_at,deleted_at,parent_message_id,sender_id on messages for each row execute function agent_memory_message_edit();

create function agent_memory_message_delete() returns trigger language plpgsql as $$
begin
update agent_memory_profiles set memory_epoch=memory_epoch+1 where id in(select profile_id from agent_memory_scopes where channel_id=OLD.channel_id and (start_sequence<OLD.sequence or profile_id in(select n.profile_id from agent_memory_notes n join agent_memory_note_sources ns on ns.note_id=n.id where ns.message_id=OLD.id)));
update agent_memory_scopes set repair_sequence=greatest(start_sequence,least(coalesce(repair_sequence,OLD.sequence-1),OLD.sequence-1)),source_generation=source_generation+1,lease_token=null,leased_until=null,attempts=0 where channel_id=OLD.channel_id and (start_sequence<OLD.sequence or profile_id in(select n.profile_id from agent_memory_notes n join agent_memory_note_sources ns on ns.note_id=n.id where ns.message_id=OLD.id));
delete from agent_memory_notes where id in(select note_id from agent_memory_note_sources where message_id=OLD.id);
return null;
end $$;
create trigger agent_memory_message_delete after delete on messages for each row execute function agent_memory_message_delete();

create function agent_memory_channel_leave() returns trigger language plpgsql as $$
begin
update agent_memory_profiles set memory_epoch=memory_epoch+1 where id in(select profile_id from agent_memory_scopes where channel_id=OLD.channel_id);
delete from agent_memory_notes where profile_id in(select id from agent_memory_profiles where circle_id=(select circle_id from channels where id=OLD.channel_id)) and channel_id=OLD.channel_id and (profile_id in(select id from agent_memory_profiles where user_id=OLD.user_id) or id in(select note_id from agent_memory_note_participants where user_id=OLD.user_id) or id in(select ns.note_id from agent_memory_note_sources ns join messages m on m.id=ns.message_id where m.sender_id=OLD.user_id));
update agent_memory_scopes set start_sequence=greatest(start_sequence,dirty_sequence,coalesce((select max(sequence) from messages where channel_id=agent_memory_scopes.channel_id),0)),processed_sequence=greatest(start_sequence,dirty_sequence,coalesce((select max(sequence) from messages where channel_id=agent_memory_scopes.channel_id),0)),dirty_sequence=greatest(start_sequence,dirty_sequence,coalesce((select max(sequence) from messages where channel_id=agent_memory_scopes.channel_id),0)),repair_sequence=null,source_generation=source_generation+1,lease_token=null,leased_until=null,attempts=0 where channel_id=OLD.channel_id;
return null;
end $$;
create trigger agent_memory_channel_leave after delete on channel_memberships for each row execute function agent_memory_channel_leave();

create function agent_memory_circle_leave() returns trigger language plpgsql as $$
begin
update agent_memory_profiles set memory_epoch=memory_epoch+1 where id in(select profile_id from agent_memory_scopes where circle_id=OLD.circle_id);
delete from agent_memory_notes where profile_id in(select id from agent_memory_profiles where circle_id=OLD.circle_id) and (profile_id in(select id from agent_memory_profiles where user_id=OLD.user_id) or id in(select note_id from agent_memory_note_participants where user_id=OLD.user_id) or id in(select ns.note_id from agent_memory_note_sources ns join messages m on m.id=ns.message_id where m.sender_id=OLD.user_id));
update agent_memory_scopes set start_sequence=greatest(start_sequence,dirty_sequence,coalesce((select max(sequence) from messages where channel_id=agent_memory_scopes.channel_id),0)),processed_sequence=greatest(start_sequence,dirty_sequence,coalesce((select max(sequence) from messages where channel_id=agent_memory_scopes.channel_id),0)),dirty_sequence=greatest(start_sequence,dirty_sequence,coalesce((select max(sequence) from messages where channel_id=agent_memory_scopes.channel_id),0)),repair_sequence=null,source_generation=source_generation+1,lease_token=null,leased_until=null,attempts=0 where circle_id=OLD.circle_id;
return null;
end $$;
create trigger agent_memory_circle_leave after delete on circle_memberships for each row execute function agent_memory_circle_leave();

create function agent_memory_channel_access() returns trigger language plpgsql as $$
begin
update agent_memory_profiles set memory_epoch=memory_epoch+1 where id in(select profile_id from agent_memory_scopes where channel_id=OLD.id);
delete from agent_memory_notes where exists(select 1 from agent_memory_scopes s where s.profile_id=agent_memory_notes.profile_id and s.channel_id=agent_memory_notes.channel_id and (s.channel_id=OLD.id));
update agent_memory_scopes set start_sequence=greatest(start_sequence,dirty_sequence,coalesce((select max(sequence) from messages where channel_id=agent_memory_scopes.channel_id),0)),processed_sequence=greatest(start_sequence,dirty_sequence,coalesce((select max(sequence) from messages where channel_id=agent_memory_scopes.channel_id),0)),dirty_sequence=greatest(start_sequence,dirty_sequence,coalesce((select max(sequence) from messages where channel_id=agent_memory_scopes.channel_id),0)),repair_sequence=null,source_generation=source_generation+1,lease_token=null,leased_until=null,attempts=0 where channel_id=OLD.id;
return null;
end $$;
create trigger agent_memory_channel_access after update of kind,circle_id on channels for each row execute function agent_memory_channel_access();

create function agent_memory_agent_settings() returns trigger language plpgsql as $$
begin
update agent_memory_profiles set memory_epoch=memory_epoch+1 where id in(select profile_id from agent_memory_scopes where profile_id in(select id from agent_memory_profiles where agent_id=OLD.agent_id));
update agent_memory_scopes set start_sequence=greatest(start_sequence,dirty_sequence,coalesce((select max(sequence) from messages where channel_id=agent_memory_scopes.channel_id),0)),processed_sequence=greatest(start_sequence,dirty_sequence,coalesce((select max(sequence) from messages where channel_id=agent_memory_scopes.channel_id),0)),dirty_sequence=greatest(start_sequence,dirty_sequence,coalesce((select max(sequence) from messages where channel_id=agent_memory_scopes.channel_id),0)),repair_sequence=null,source_generation=source_generation+1,lease_token=null,leased_until=null,attempts=0 where profile_id in(select id from agent_memory_profiles where agent_id=OLD.agent_id);
return null;
end $$;
create trigger agent_memory_agent_settings after update of enabled,memory_enabled on circle_chat_agents for each row when (OLD.enabled<>NEW.enabled or OLD.memory_enabled<>NEW.memory_enabled) execute function agent_memory_agent_settings();

create function agent_memory_agent_revoke() returns trigger language plpgsql as $$
begin
update agent_memory_profiles set memory_epoch=memory_epoch+1 where id in(select profile_id from agent_memory_scopes where profile_id in(select id from agent_memory_profiles where agent_id=OLD.agent_id));
delete from agent_memory_notes where exists(select 1 from agent_memory_scopes s where s.profile_id=agent_memory_notes.profile_id and s.channel_id=agent_memory_notes.channel_id and (profile_id in(select id from agent_memory_profiles where agent_id=OLD.agent_id)));
update agent_memory_scopes set start_sequence=greatest(start_sequence,dirty_sequence,coalesce((select max(sequence) from messages where channel_id=agent_memory_scopes.channel_id),0)),processed_sequence=greatest(start_sequence,dirty_sequence,coalesce((select max(sequence) from messages where channel_id=agent_memory_scopes.channel_id),0)),dirty_sequence=greatest(start_sequence,dirty_sequence,coalesce((select max(sequence) from messages where channel_id=agent_memory_scopes.channel_id),0)),repair_sequence=null,source_generation=source_generation+1,lease_token=null,leased_until=null,attempts=0 where profile_id in(select id from agent_memory_profiles where agent_id=OLD.agent_id);
return null;
end $$;
create trigger agent_memory_agent_revoke after update of revoked_at,expires_at on agent_profiles for each row execute function agent_memory_agent_revoke();

create function agent_memory_profile_choice() returns trigger language plpgsql as $$
begin
update agent_memory_profiles set memory_epoch=memory_epoch+1 where id in(select profile_id from agent_memory_scopes where profile_id=OLD.id);
update agent_memory_scopes set start_sequence=greatest(start_sequence,dirty_sequence,coalesce((select max(sequence) from messages where channel_id=agent_memory_scopes.channel_id),0)),processed_sequence=greatest(start_sequence,dirty_sequence,coalesce((select max(sequence) from messages where channel_id=agent_memory_scopes.channel_id),0)),dirty_sequence=greatest(start_sequence,dirty_sequence,coalesce((select max(sequence) from messages where channel_id=agent_memory_scopes.channel_id),0)),repair_sequence=null,source_generation=source_generation+1,lease_token=null,leased_until=null,attempts=0 where profile_id=OLD.id;
return null;
end $$;
create trigger agent_memory_profile_choice after update of enabled on agent_memory_profiles for each row when (OLD.enabled<>NEW.enabled) execute function agent_memory_profile_choice();

create function agent_memory_channel_agent_insert() returns trigger language plpgsql as $$
begin
update agent_memory_profiles set memory_epoch=memory_epoch+1 where id in(select profile_id from agent_memory_scopes where channel_id=NEW.channel_id and profile_id in(select id from agent_memory_profiles where agent_id=NEW.agent_id));
delete from agent_memory_notes where exists(select 1 from agent_memory_scopes s where s.profile_id=agent_memory_notes.profile_id and s.channel_id=agent_memory_notes.channel_id and (s.channel_id=NEW.channel_id and profile_id in(select id from agent_memory_profiles where agent_id=NEW.agent_id)));
update agent_memory_scopes set start_sequence=greatest(start_sequence,dirty_sequence,coalesce((select max(sequence) from messages where channel_id=agent_memory_scopes.channel_id),0)),processed_sequence=greatest(start_sequence,dirty_sequence,coalesce((select max(sequence) from messages where channel_id=agent_memory_scopes.channel_id),0)),dirty_sequence=greatest(start_sequence,dirty_sequence,coalesce((select max(sequence) from messages where channel_id=agent_memory_scopes.channel_id),0)),repair_sequence=null,source_generation=source_generation+1,lease_token=null,leased_until=null,attempts=0 where channel_id=NEW.channel_id and profile_id in(select id from agent_memory_profiles where agent_id=NEW.agent_id);
return null;
end $$;
create trigger agent_memory_channel_agent_insert after insert on channel_chat_agent_settings for each row execute function agent_memory_channel_agent_insert();

create function agent_memory_channel_agent_update() returns trigger language plpgsql as $$
begin
update agent_memory_profiles set memory_epoch=memory_epoch+1 where id in(select profile_id from agent_memory_scopes where channel_id=OLD.channel_id and profile_id in(select id from agent_memory_profiles where agent_id=OLD.agent_id));
delete from agent_memory_notes where exists(select 1 from agent_memory_scopes s where s.profile_id=agent_memory_notes.profile_id and s.channel_id=agent_memory_notes.channel_id and (s.channel_id=OLD.channel_id and profile_id in(select id from agent_memory_profiles where agent_id=OLD.agent_id)));
update agent_memory_scopes set start_sequence=greatest(start_sequence,dirty_sequence,coalesce((select max(sequence) from messages where channel_id=agent_memory_scopes.channel_id),0)),processed_sequence=greatest(start_sequence,dirty_sequence,coalesce((select max(sequence) from messages where channel_id=agent_memory_scopes.channel_id),0)),dirty_sequence=greatest(start_sequence,dirty_sequence,coalesce((select max(sequence) from messages where channel_id=agent_memory_scopes.channel_id),0)),repair_sequence=null,source_generation=source_generation+1,lease_token=null,leased_until=null,attempts=0 where channel_id=OLD.channel_id and profile_id in(select id from agent_memory_profiles where agent_id=OLD.agent_id);
return null;
end $$;
create trigger agent_memory_channel_agent_update after update of enabled on channel_chat_agent_settings for each row when (OLD.enabled<>NEW.enabled) execute function agent_memory_channel_agent_update();

create function agent_memory_channel_agent_delete() returns trigger language plpgsql as $$
begin
update agent_memory_profiles set memory_epoch=memory_epoch+1 where id in(select profile_id from agent_memory_scopes where channel_id=OLD.channel_id and profile_id in(select id from agent_memory_profiles where agent_id=OLD.agent_id));
delete from agent_memory_notes where exists(select 1 from agent_memory_scopes s where s.profile_id=agent_memory_notes.profile_id and s.channel_id=agent_memory_notes.channel_id and (s.channel_id=OLD.channel_id and profile_id in(select id from agent_memory_profiles where agent_id=OLD.agent_id)));
update agent_memory_scopes set start_sequence=greatest(start_sequence,dirty_sequence,coalesce((select max(sequence) from messages where channel_id=agent_memory_scopes.channel_id),0)),processed_sequence=greatest(start_sequence,dirty_sequence,coalesce((select max(sequence) from messages where channel_id=agent_memory_scopes.channel_id),0)),dirty_sequence=greatest(start_sequence,dirty_sequence,coalesce((select max(sequence) from messages where channel_id=agent_memory_scopes.channel_id),0)),repair_sequence=null,source_generation=source_generation+1,lease_token=null,leased_until=null,attempts=0 where channel_id=OLD.channel_id and profile_id in(select id from agent_memory_profiles where agent_id=OLD.agent_id);
return null;
end $$;
create trigger agent_memory_channel_agent_delete after delete on channel_chat_agent_settings for each row execute function agent_memory_channel_agent_delete();

create function agent_memory_provenance_change() returns trigger language plpgsql as $$
begin
update agent_memory_profiles set memory_epoch=memory_epoch+1 where id in(select profile_id from agent_memory_scopes where channel_id in(select channel_id from messages where id=OLD.message_id));
update agent_memory_scopes set source_generation=source_generation+1,lease_token=null,leased_until=null where channel_id in(select channel_id from messages where id=OLD.message_id);
delete from agent_memory_notes where id in(select note_id from agent_memory_note_sources where message_id=OLD.message_id);
return null;
end $$;
create trigger agent_memory_provenance_change after update of provenance on message_provenance for each row when (OLD.provenance<>NEW.provenance) execute function agent_memory_provenance_change();
