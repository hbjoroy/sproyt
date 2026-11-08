-- NULL preserves rolling compatibility: existing/old-writer-created events
-- remain available, while an explicit false is never reset by event writes.
alter table channels add column events_enabled integer check(events_enabled in (0,1));
