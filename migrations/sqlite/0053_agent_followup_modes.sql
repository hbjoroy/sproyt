ALTER TABLE circle_chat_agent_jobs ADD COLUMN followup_mode TEXT NOT NULL DEFAULT 'weather'
    CHECK (followup_mode IN ('weather', 'explicit', 'implicit'));
