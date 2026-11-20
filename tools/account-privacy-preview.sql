-- Scope a private operator case. Use psql -X -v account_id=<Sprøyt UUID> -f <this file>.
-- Output contains row counts only. A zero is specific to the named local table.
\set ON_ERROR_STOP on
BEGIN READ ONLY;
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
SET LOCAL statement_timeout = '5s';
SET LOCAL lock_timeout = '1s';
WITH subject AS (SELECT :'account_id'::uuid AS id)
SELECT scope, row_count
FROM subject CROSS JOIN LATERAL (VALUES
    ('profile', (SELECT count(*) FROM users WHERE id = subject.id)),
    ('circle_memberships', (SELECT count(*) FROM circle_memberships WHERE user_id = subject.id)),
    ('channel_memberships', (SELECT count(*) FROM channel_memberships WHERE user_id = subject.id)),
    ('messages_authored', (SELECT count(*) FROM messages WHERE sender_id = subject.id)),
    ('owned_media', (SELECT count(*) FROM media_objects WHERE owner_id = subject.id)),
    ('uploads', (SELECT count(*) FROM media_uploads WHERE owner_id = subject.id)),
    ('native_session_families', (SELECT count(*) FROM native_families WHERE user_id = subject.id::text)),
    ('owned_agent_profiles', (SELECT count(*) FROM agent_profiles WHERE owner_id = subject.id)),
    ('agent_memory_profiles', (SELECT count(*) FROM agent_memory_profiles WHERE user_id = subject.id)),
    ('granted_positions', (SELECT count(*) FROM agent_location_shares WHERE user_id = subject.id)),
    ('push_subscriptions', (SELECT count(*) FROM push_subscriptions WHERE user_id = subject.id)),
    ('undelivered_push_jobs', (SELECT count(*) FROM notification_outbox WHERE recipient_id = subject.id AND delivered_at IS NULL)),
    ('image_jobs', (SELECT count(*) FROM image_generation_jobs WHERE owner_id = subject.id::text)),
    ('events_created', (SELECT count(*) FROM channel_events WHERE creator_id = subject.id)),
    ('event_responses', (SELECT count(*) FROM channel_event_responses WHERE user_id = subject.id)),
    ('event_image_contributions', (SELECT count(*) FROM channel_event_images WHERE contributor_id = subject.id)),
    ('open_agent_jobs_from_authored_messages', (
        SELECT count(*) FROM circle_chat_agent_jobs j
        JOIN messages m ON m.id = j.source_message_id
        WHERE m.sender_id = subject.id AND j.status IN ('pending', 'leased')
    ))
) AS counts(scope, row_count)
ORDER BY scope;
COMMIT;
