-- Shared, persisted quota policy. NULL policy keeps existing deployments unchanged.
alter table media_upload_budget add column max_file_bytes bigint;
alter table media_upload_budget add column owner_bytes bigint;
alter table media_upload_budget add column total_bytes bigint;
alter table media_upload_budget add constraint media_budget_policy_valid check (
    num_nonnulls(max_file_bytes,owner_bytes,total_bytes)=0 or
    (num_nonnulls(max_file_bytes,owner_bytes,total_bytes)=3 and max_file_bytes between 1 and 1073741824 and owner_bytes>=max_file_bytes and total_bytes>=owner_bytes)
);

create function enforce_media_object_budget() returns trigger language plpgsql volatile as $$
declare policy media_upload_budget%rowtype; credit bigint:=0; total_used numeric; owner_used numeric; charge bigint;
begin
    if TG_OP='UPDATE' then
        if NEW.owner_id is distinct from OLD.owner_id or NEW.size_bytes is distinct from OLD.size_bytes or NEW.storage_key is distinct from OLD.storage_key then
            raise exception using errcode='P0001',message='SPROYT_MEDIA_IMMUTABLE';
        end if;
        return NEW;
    end if;
    select * into policy from media_upload_budget where id=1 for update;
    if policy.total_bytes is null then return NEW; end if;
    charge:=greatest(NEW.size_bytes,1048576);
    -- Credit ONLY this sealed upload's matching reservation. All other active
    -- sessions (including expired ones awaiting cleanup) continue to count.
    select greatest(expected_bytes,1048576) into credit from media_uploads
      where NEW.storage_key='chunks:v1:'||id::text and owner_id=NEW.owner_id and channel_id=NEW.channel_id
      and expected_bytes=NEW.size_bytes and received_bytes=expected_bytes and state='sealed' and media_id is null;
    credit:=coalesce(credit,0);
    select coalesce(sum(greatest(size_bytes,1048576)),0) into total_used from media_objects;
    select total_used+coalesce(sum(greatest(expected_bytes,1048576)),0) into total_used from media_uploads where state!='complete';
    select coalesce(sum(greatest(size_bytes,1048576)),0) into owner_used from media_objects where owner_id=NEW.owner_id;
    select owner_used+coalesce(sum(greatest(expected_bytes,1048576)),0) into owner_used from media_uploads where owner_id=NEW.owner_id and state!='complete';
    if NEW.size_bytes>policy.max_file_bytes or total_used+charge-credit>policy.total_bytes or owner_used+charge-credit>policy.owner_bytes then
        raise exception using errcode='P0001',message='SPROYT_MEDIA_QUOTA';
    end if;
    return NEW;
end $$;
create trigger media_object_budget before insert or update on media_objects for each row execute function enforce_media_object_budget();
