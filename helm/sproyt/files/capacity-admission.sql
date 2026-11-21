-- Use only counts visible to an ordinary login. Never select query text or addresses.
WITH clients AS (
  SELECT count(*) AS total,
         count(*) FILTER (WHERE datname = 'sproyt') AS web_media,
         count(*) FILTER (WHERE datname = 'heart_sproyt_canary') AS heart,
         count(*) FILTER (WHERE datname = 'authentik') AS identity
  FROM pg_stat_activity
  -- Ordinary roles see NULL backend_type on other owners' sessions. Count all
  -- database-attached processes and ambiguous role-backed rows conservatively.
  WHERE pid <> pg_backend_pid()
    AND (datid IS NOT NULL OR backend_type = 'client backend'
         OR (usesysid IS NOT NULL AND backend_type IS NULL))
), demand AS (
  SELECT least(132, current_setting('max_connections')::int
                   - current_setting('reserved_connections')::int
                   - current_setting('superuser_reserved_connections')::int)
         - total - greatest(0, 15-web_media) - greatest(0, 6-heart)
         - greatest(0, 14-identity) AS available
  FROM clients
)
SELECT available, (available >= :'extra'::integer + 2)::integer FROM demand;
