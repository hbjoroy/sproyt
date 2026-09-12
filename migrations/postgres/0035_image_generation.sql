CREATE TABLE image_generation_jobs (
    id TEXT PRIMARY KEY,
    owner_id TEXT NOT NULL,
    channel_id TEXT NOT NULL,
    state TEXT NOT NULL,
    slot INTEGER NOT NULL,
    updated_at BIGINT NOT NULL,
    revision BIGINT NOT NULL DEFAULT 0,
    data TEXT NOT NULL
);
CREATE UNIQUE INDEX image_generation_owner_pending ON image_generation_jobs(owner_id)
    WHERE state IN ('queued','submitting','running','ready','accepting');
CREATE UNIQUE INDEX image_generation_queue_slot ON image_generation_jobs(slot)
    WHERE state IN ('queued','submitting','running');
CREATE TABLE image_generation_worker (id INTEGER PRIMARY KEY, lease_until BIGINT NOT NULL);
INSERT INTO image_generation_worker VALUES (1, 0);
