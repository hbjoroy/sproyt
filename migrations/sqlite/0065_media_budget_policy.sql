alter table media_upload_budget add column max_file_bytes integer;
alter table media_upload_budget add column owner_bytes integer;
alter table media_upload_budget add column total_bytes integer;
create trigger media_budget_policy_insert before insert on media_upload_budget
when not coalesce((NEW.max_file_bytes is null and NEW.owner_bytes is null and NEW.total_bytes is null) or
 (NEW.max_file_bytes between 1 and 1073741824 and NEW.owner_bytes>=NEW.max_file_bytes and NEW.total_bytes>=NEW.owner_bytes),0)
begin select raise(abort,'SPROYT_MEDIA_INVALID_POLICY'); end;
create trigger media_budget_policy_update before update on media_upload_budget
when not coalesce((NEW.max_file_bytes is null and NEW.owner_bytes is null and NEW.total_bytes is null) or
 (NEW.max_file_bytes between 1 and 1073741824 and NEW.owner_bytes>=NEW.max_file_bytes and NEW.total_bytes>=NEW.owner_bytes),0)
begin select raise(abort,'SPROYT_MEDIA_INVALID_POLICY'); end;

create trigger media_object_immutable before update on media_objects
when NEW.owner_id is not OLD.owner_id or NEW.size_bytes is not OLD.size_bytes or NEW.storage_key is not OLD.storage_key
begin select raise(abort,'SPROYT_MEDIA_IMMUTABLE'); end;

create trigger media_object_budget before insert on media_objects
when (select total_bytes from media_upload_budget where id=1) is not null
begin
 select case when NEW.size_bytes>(select max_file_bytes from media_upload_budget where id=1)
 or (select coalesce(sum(max(size_bytes,1048576)),0) from media_objects)
 + (select coalesce(sum(max(expected_bytes,1048576)),0) from media_uploads where state!='complete')
 + max(NEW.size_bytes,1048576)
 - (select coalesce(sum(max(expected_bytes,1048576)),0) from media_uploads where NEW.storage_key='chunks:v1:'||id and owner_id=NEW.owner_id and channel_id=NEW.channel_id and expected_bytes=NEW.size_bytes and received_bytes=expected_bytes and state='sealed' and media_id is null)
 > (select total_bytes from media_upload_budget where id=1)
 or (select coalesce(sum(max(size_bytes,1048576)),0) from media_objects where owner_id=NEW.owner_id)
 + (select coalesce(sum(max(expected_bytes,1048576)),0) from media_uploads where owner_id=NEW.owner_id and state!='complete')
 + max(NEW.size_bytes,1048576)
 - (select coalesce(sum(max(expected_bytes,1048576)),0) from media_uploads where NEW.storage_key='chunks:v1:'||id and owner_id=NEW.owner_id and channel_id=NEW.channel_id and expected_bytes=NEW.size_bytes and received_bytes=expected_bytes and state='sealed' and media_id is null)
 > (select owner_bytes from media_upload_budget where id=1)
 then raise(abort,'SPROYT_MEDIA_QUOTA') end;
end;
