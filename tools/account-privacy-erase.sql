-- Operator only. Apply after all shared writers, connections and workers have drained.
-- account_id and confirmed_account_id come from independent checks in the private case.
\set ON_ERROR_STOP on
BEGIN;
-- An explicit schema is required when both supported namespaces contain a user table.
-- Existing single-schema callers can omit app_schema.
\if :{?app_schema}
\else
SELECT CASE WHEN count(*)=1 THEN min(n.nspname::text) ELSE '' END AS app_schema
FROM pg_catalog.pg_namespace n WHERE n.nspname IN ('public','sproyt')
AND pg_catalog.to_regclass(pg_catalog.format('%I.users',n.nspname)) IS NOT NULL
\gset
\endif
-- Keep builtins first and temporary relations last. Pin relation resolution to the
-- operator-confirmed schema, even when the database role has a broader search_path.
SELECT pg_catalog.set_config('search_path', pg_catalog.format('pg_catalog,%I,pg_temp', :'app_schema'),true) AS private_path
WHERE :'app_schema' IN ('public','sproyt')
AND pg_catalog.to_regclass(pg_catalog.format('%I.users', :'app_schema')) IS NOT NULL
\gset
SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '120s';
CREATE TEMP TABLE erase_subject(id uuid PRIMARY KEY, confirmed uuid NOT NULL) ON COMMIT DROP;
CREATE TEMP TABLE erase_image_jobs(id text PRIMARY KEY) ON COMMIT DROP;
INSERT INTO pg_temp.erase_subject VALUES (:'account_id'::uuid, :'confirmed_account_id'::uuid);
DO $$
DECLARE
    target uuid := (SELECT id FROM pg_temp.erase_subject);
BEGIN
    IF NOT EXISTS (SELECT FROM pg_temp.erase_subject WHERE id=confirmed)
       OR NOT EXISTS (SELECT FROM users WHERE id=target AND kind='human' FOR UPDATE) THEN
        RAISE EXCEPTION 'verified human account required';
    END IF;
    IF EXISTS (SELECT FROM circle_memberships m WHERE m.user_id=target AND m.role='owner'
        AND NOT EXISTS (SELECT FROM circle_memberships n WHERE n.circle_id=m.circle_id
                        AND n.user_id<>target AND n.role='owner')) THEN
        RAISE EXCEPTION 'transfer sole circle ownership or resolve circle disposition first';
    END IF;

    -- All native grants and handoff receipts for this principal, including incomplete attempts.
    DELETE FROM native_refresh_receipts WHERE family IN
        (SELECT family FROM native_families WHERE user_id=target::text);
    DELETE FROM native_attempts WHERE user_id=target::text OR expected_user_id=target::text;
    DELETE FROM native_families WHERE user_id=target::text;
    DELETE FROM native_epochs WHERE user_id=target::text;

    -- Remove current authority before releasing leases. External sends already in flight
    -- cannot be recalled. The operator must drain both environments before running this.
    UPDATE agent_credentials SET revoked_at=now() WHERE revoked_at IS NULL
        AND agent_id IN (SELECT agent_id FROM agent_profiles WHERE owner_id=target);
    UPDATE agent_grants SET revoked_at=now(), revoked_by=target WHERE revoked_at IS NULL
        AND agent_id IN (SELECT agent_id FROM agent_profiles WHERE owner_id=target);
    UPDATE agent_profiles SET revoked_at=now() WHERE owner_id=target AND revoked_at IS NULL;
    UPDATE circle_chat_agents SET enabled=false, revision=revision+1 WHERE enabled
        AND agent_id IN (SELECT agent_id FROM agent_profiles WHERE owner_id=target);
    -- Preserve published results shared with other people. Remove ownership of any
    -- generation payload being deleted below without removing the publication itself.
    INSERT INTO pg_temp.erase_image_jobs
        SELECT id FROM image_generation_jobs WHERE owner_id=target::text
        UNION
        SELECT image_job_id FROM agent_image_publications
        WHERE image_job_id IS NOT NULL AND state IN ('pending','admitting','queued','publishing')
          AND (source_message_id IN (SELECT id FROM messages WHERE sender_id=target)
               OR agent_id IN (SELECT agent_id FROM agent_profiles WHERE owner_id=target));
    UPDATE agent_image_publications SET image_job_id=NULL
        WHERE image_job_id IN (SELECT id FROM pg_temp.erase_image_jobs);
    UPDATE agent_image_publications SET state='skipped',lease_token=NULL,leased_until=NULL
        WHERE state IN ('pending','admitting','queued','publishing') AND (source_message_id IN
        (SELECT id FROM messages WHERE sender_id=target) OR agent_id IN
        (SELECT agent_id FROM agent_profiles WHERE owner_id=target));
    UPDATE agent_handoff_jobs SET status='skipped',lease_token=NULL,leased_until=NULL
        WHERE status IN ('pending','leased') AND (source_message_id IN
        (SELECT id FROM messages WHERE sender_id=target) OR agent_id IN
        (SELECT agent_id FROM agent_profiles WHERE owner_id=target));
    UPDATE circle_chat_agent_jobs SET status='skipped',lease_token=NULL,leased_until=NULL,reply_body=NULL
        WHERE status IN ('pending','leased') AND (source_message_id IN
        (SELECT id FROM messages WHERE sender_id=target) OR agent_id IN
        (SELECT agent_id FROM agent_profiles WHERE owner_id=target));
    DELETE FROM image_generation_jobs WHERE id IN (SELECT id FROM pg_temp.erase_image_jobs);
    DELETE FROM agent_location_shares WHERE user_id=target;
    DELETE FROM agent_memory_notes WHERE id IN
        (SELECT note_id FROM agent_memory_note_participants WHERE user_id=target)
        OR id IN (SELECT s.note_id FROM agent_memory_note_sources s JOIN messages m
                  ON m.id=s.message_id WHERE m.sender_id=target);
    DELETE FROM agent_memory_profiles WHERE user_id=target;
    DELETE FROM notification_outbox WHERE recipient_id=target OR (delivered_at IS NULL AND
        message_id IN (SELECT id FROM messages WHERE sender_id=target));
    DELETE FROM push_subscriptions WHERE user_id=target;
    DELETE FROM notification_preferences WHERE user_id=target;
    DELETE FROM channel_notification_subscriptions WHERE user_id=target;

    -- Personal log channels have one owner. Stop rather than cascade into another participant.
    IF EXISTS (SELECT FROM personal_spaces p JOIN channel_memberships m ON m.channel_id=p.channel_id
               WHERE p.owner_id=target AND m.user_id<>target) THEN
        RAISE EXCEPTION 'review unexpected shared personal space';
    END IF;
    DELETE FROM message_attachments WHERE message_id IN
        (SELECT m.id FROM messages m JOIN personal_spaces p ON p.channel_id=m.channel_id
         WHERE p.owner_id=target AND m.sender_id=target);
    UPDATE messages SET body='[sletta]', deleted_at=coalesce(deleted_at,now())
        WHERE sender_id=target AND channel_id IN (SELECT channel_id FROM personal_spaces WHERE owner_id=target)
          AND (deleted_at IS NULL OR body<>'[sletta]');
    DELETE FROM personal_spaces WHERE owner_id=target;
    DELETE FROM personal_statuses WHERE user_id=target;
    DELETE FROM personal_emojis WHERE user_id=target;
    DELETE FROM media_uploads WHERE owner_id=target AND media_id IS NULL;
    DELETE FROM media_objects o WHERE o.owner_id=target
        AND NOT EXISTS (SELECT FROM message_attachments a WHERE a.media_id=o.id)
        AND NOT EXISTS (SELECT FROM channel_event_images e WHERE e.media_id=o.id)
        AND NOT EXISTS (SELECT FROM agent_image_publications i WHERE i.media_id=o.id);
    DELETE FROM channel_event_responses WHERE user_id=target;
    DELETE FROM message_reactions WHERE user_id=target;
    DELETE FROM chat_invitation_responses WHERE user_id=target;
    UPDATE circle_invitations SET expires_at=least(expires_at,now()) WHERE invited_by=target AND accepted_at IS NULL;
    UPDATE chat_invitations SET expires_at=least(expires_at,now()) WHERE invited_by=target;
    UPDATE enrollment_invitations SET expires_at=least(expires_at,now()) WHERE invited_by=target AND state<>'consumed';
    DELETE FROM channel_memberships WHERE user_id=target;
    DELETE FROM circle_memberships WHERE user_id=target;
    UPDATE signup_ordinals SET user_id=NULL WHERE user_id=target;
    UPDATE messages SET sender_display_name='Sletta brukar' WHERE sender_id=target
        AND sender_display_name<>'Sletta brukar';
    UPDATE users SET display_name='Sletta brukar', handle=NULL, handle_source=NULL,
        external_provider=NULL, external_subject=NULL, status_text='', status_emoji='', status_expires_at=NULL
        WHERE id=target;
END $$;
COMMIT;
