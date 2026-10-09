-- REQUIRED after restoring an older database, before ANY reply or memory
-- worker (including canary) is restarted. The deletion history is not backed
-- by an independent journal in this MVP; restored memory must not be revived.
-- Run with psql --set ON_ERROR_STOP=1 --file ... against the restored database.
begin;
-- Temporary position consent must never be resurrected from a backup.
update circle_chat_agent_jobs
set status=case when status in ('pending','leased') then 'skipped' else status end,
    error_code=case when status in ('pending','leased') then 'location_restore_reset' else error_code end,
    lease_token=null,leased_until=null,reply_body=null,weather_snapshot=null
where location_share_id is not null and reply_message_id is null;
delete from agent_location_shares;
update circle_chat_agent_jobs
set status=case when status in ('pending','leased') then 'skipped' else status end,
    error_code=case when status in ('pending','leased') then 'memory_restore_reset' else error_code end,
    lease_token=null,leased_until=null,reply_body=null,memory_dependencies=null
where memory_dependencies is not null;
-- Cascades all notes (also user-confirmed), dependencies, exclusions and work.
-- A missing profile means consent is off and fresh opt-in starts at a new floor.
delete from agent_memory_profiles;
-- Preserve a fresh quarantine unless the remote model service was drained.
update agent_memory_model_quota set lease_token=null,
    leased_until=extract(epoch from clock_timestamp())::bigint+180,
    memory_window_started=0,memory_last_started=0,memory_calls=0 where id=1;
commit;
-- Original messages and already published replies are intentionally untouched.
