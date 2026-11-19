-- Scope a private operator case. Use psql -X -v account_id=<Sprøyt UUID> -f <this file>.
-- Output contains row counts only. A zero is specific to the named local table.
\set ON_ERROR_STOP on
BEGIN READ ONLY;
SET LOCAL statement_timeout = '5s';
SET LOCAL lock_timeout = '1s';
WITH subject AS (SELECT :'account_id'::uuid AS id)
SELECT scope, row_count
FROM subject CROSS JOIN LATERAL (VALUES
    ('profile', (SELECT count(*) FROM public.users WHERE id = subject.id)),
    ('circle_memberships', (SELECT count(*) FROM public.circle_memberships WHERE user_id = subject.id)),
    ('channel_memberships', (SELECT count(*) FROM public.channel_memberships WHERE user_id = subject.id)),
    ('messages_authored', (SELECT count(*) FROM public.messages WHERE sender_id = subject.id)),
    ('owned_media', (SELECT count(*) FROM public.media_objects WHERE owner_id = subject.id)),
    ('uploads', (SELECT count(*) FROM public.media_uploads WHERE owner_id = subject.id)),
    ('native_session_families', (SELECT count(*) FROM public.native_families WHERE user_id = subject.id::text)),
    ('owned_agent_profiles', (SELECT count(*) FROM public.agent_profiles WHERE owner_id = subject.id)),
    ('agent_memory_profiles', (SELECT count(*) FROM public.agent_memory_profiles WHERE user_id = subject.id)),
    ('granted_positions', (SELECT count(*) FROM public.agent_location_shares WHERE user_id = subject.id)),
    ('push_subscriptions', (SELECT count(*) FROM public.push_subscriptions WHERE user_id = subject.id)),
    ('undelivered_push_jobs', (SELECT count(*) FROM public.notification_outbox WHERE recipient_id = subject.id AND delivered_at IS NULL)),
    ('image_jobs', (SELECT count(*) FROM public.image_generation_jobs WHERE owner_id = subject.id::text)),
    ('events_created', (SELECT count(*) FROM public.channel_events WHERE creator_id = subject.id)),
    ('event_responses', (SELECT count(*) FROM public.channel_event_responses WHERE user_id = subject.id)),
    ('event_image_contributions', (SELECT count(*) FROM public.channel_event_images WHERE contributor_id = subject.id)),
    ('open_agent_jobs_from_authored_messages', (
        SELECT count(*) FROM public.circle_chat_agent_jobs j
        JOIN public.messages m ON m.id = j.source_message_id
        WHERE m.sender_id = subject.id AND j.status IN ('pending', 'leased')
    ))
) AS counts(scope, row_count)
ORDER BY scope;
COMMIT;
