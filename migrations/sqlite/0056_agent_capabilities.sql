ALTER TABLE circle_chat_agents ADD COLUMN vision_enabled integer NOT NULL DEFAULT 0 CHECK(vision_enabled IN (0,1));
ALTER TABLE circle_chat_agent_jobs ADD COLUMN vision_snapshot text;
ALTER TABLE circle_chat_agent_jobs ADD COLUMN observation_snapshot text;
ALTER TABLE circle_chat_agent_jobs ADD COLUMN observation_valid_until integer;
