-- Only a model-selected request plan. Never store Google results here.
ALTER TABLE circle_chat_agent_jobs ADD COLUMN place_lookup_plan TEXT;

-- One admission record per existing user; no request or provider content.
CREATE TABLE place_lookup_admission_gate (id INTEGER PRIMARY KEY CHECK(id=1));
INSERT INTO place_lookup_admission_gate(id) VALUES(1);
CREATE TABLE place_lookup_admissions (
 user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
 token TEXT NOT NULL,
 leased_until BIGINT NOT NULL,
 next_allowed BIGINT NOT NULL,
 window_started BIGINT NOT NULL,
 calls BIGINT NOT NULL
);

-- Geoapify consent/resume is isolated from the original neutral reply receipt.
CREATE TABLE agent_place_rolls (
 id TEXT PRIMARY KEY,
 source_job_id TEXT NOT NULL UNIQUE REFERENCES circle_chat_agent_jobs(id) ON DELETE CASCADE,
 original_reply_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
 author_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 descriptor_snapshot TEXT NOT NULL,
 plan_snapshot TEXT NOT NULL,
 state TEXT NOT NULL CHECK(state IN ('geocoding','choose_origin','pending','leased','ready','published','failed','cancelled')),
 origin_query TEXT,
 origin_json TEXT,
 geocodes TEXT,
 expires_at BIGINT NOT NULL,
 attempts BIGINT NOT NULL DEFAULT 0,
 lease_token TEXT,
 leased_until BIGINT NOT NULL DEFAULT 0,
 outcome TEXT,
 reply_body TEXT,
 result_message_id TEXT REFERENCES messages(id),
 error_code TEXT
);
CREATE INDEX agent_place_rolls_ready ON agent_place_rolls(state, leased_until, expires_at);
