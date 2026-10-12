-- Additive independent capability. Existing agents and jobs remain unchanged.
alter table circle_chat_agents add column sea_temperature text;
alter table circle_chat_agent_jobs add column sea_temperature_snapshot text;
alter table circle_chat_agent_jobs add column sea_temperature_valid_until bigint;
