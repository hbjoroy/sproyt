-- Run against the shared PostgreSQL primary with pg_read_all_stats visibility.
-- Count all client backends, including superusers. An existing superuser client
-- consumes a slot even when no reserved slot was needed at its admission.
BEGIN READ ONLY;
WITH limits AS (
    SELECT current_setting('max_connections')::int AS total,
           current_setting('reserved_connections')::int AS reserved,
           current_setting('superuser_reserved_connections')::int AS super_reserved
), observed AS (
    SELECT count(*) AS clients,
           count(*) FILTER (WHERE state = 'active') AS active
    FROM pg_stat_activity
    WHERE backend_type = 'client backend' AND pid <> pg_backend_pid()
)
SELECT clock_timestamp() AS sampled_at, total, reserved, super_reserved,
       clients, active,
       greatest(0, total - reserved - super_reserved - clients) AS ordinary_headroom
FROM limits CROSS JOIN observed;

SELECT CASE WHEN datname = 'sproyt' THEN 'sproyt'
            WHEN datname = 'heart_sproyt_canary' THEN 'sproyt-heart'
            WHEN datname = 'authentik' THEN 'identity'
            WHEN datname = 'grafana' THEN 'monitoring'
            ELSE 'other' END AS client_group,
       count(*) AS clients
FROM pg_stat_activity
WHERE backend_type = 'client backend' AND pid <> pg_backend_pid()
GROUP BY 1 ORDER BY 1;

SELECT count(*) AS ordinary_login_roles,
       count(*) FILTER (WHERE rolconnlimit = -1) AS without_role_cap
FROM pg_roles WHERE rolcanlogin AND NOT rolsuper;
COMMIT;
