alter table circle_chat_agents add column weather text;
alter table circle_chat_agent_jobs add column followup_anchor_message_id text references messages(id);
alter table circle_chat_agent_jobs add column weather_snapshot text;
alter table circle_chat_agent_jobs add column weather_valid_until integer;
