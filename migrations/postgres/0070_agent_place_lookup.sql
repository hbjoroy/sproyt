-- Only a model-selected request plan. Never store Google results here.
ALTER TABLE circle_chat_agent_jobs ADD COLUMN place_lookup_plan TEXT;

-- One admission record per existing user; no request or provider content.
CREATE TABLE place_lookup_admission_gate (id INTEGER PRIMARY KEY CHECK(id=1));
INSERT INTO place_lookup_admission_gate(id) VALUES(1);
CREATE TABLE place_lookup_admissions (
 user_id UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
 token TEXT NOT NULL,
 leased_until BIGINT NOT NULL,
 next_allowed BIGINT NOT NULL,
 window_started BIGINT NOT NULL,
 calls BIGINT NOT NULL
);
