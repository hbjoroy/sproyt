ALTER TABLE circle_chat_agents ADD COLUMN vision_enabled boolean NOT NULL DEFAULT false;
ALTER TABLE circle_chat_agent_jobs ADD COLUMN vision_snapshot text;
ALTER TABLE circle_chat_agent_jobs ADD COLUMN observation_snapshot text;
ALTER TABLE circle_chat_agent_jobs ADD COLUMN observation_valid_until bigint;
