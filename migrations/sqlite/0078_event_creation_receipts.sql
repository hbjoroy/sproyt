-- Keep retries of the same create request tied to one event, including after edits.
alter table channel_events add column creation_request_id text;
alter table channel_events add column creation_fingerprint text;
create unique index channel_event_creation_request
    on channel_events(channel_id,creator_id,creation_request_id)
    where creation_request_id is not null;
