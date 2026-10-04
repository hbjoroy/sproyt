alter table circle_chat_agents add column ferry_port text check (ferry_port is null or ferry_port = 'paros');
alter table circle_chat_agent_jobs add column ferry_snapshot text;
alter table circle_chat_agent_jobs add column ferry_valid_until integer;
