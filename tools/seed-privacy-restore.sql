-- Synthetic only. Seed the otherwise disposable source before pg_dump.
begin;
insert into users(id,kind,display_name) values
    ('01900000-0000-7000-8000-000000004151','human','Synthetic erase'),
    ('01900000-0000-7000-8000-000000004152','human','Synthetic revoke'),
    ('01900000-0000-7000-8000-000000004153','human','Synthetic unaffected'),
    ('01900000-0000-7000-8000-000000004154','agent','Synthetic agent');
insert into personal_statuses(user_id,text,emoji,save_count,last_used_at) values
    ('01900000-0000-7000-8000-000000004151','Synthetic private erase status','',1,now()),
    ('01900000-0000-7000-8000-000000004153','Synthetic unaffected status','',1,now());
insert into circles(id,slug,name,created_by) values
    ('01900000-0000-7000-8000-000000004155','synthetic-restore','Synthetic shared circle','01900000-0000-7000-8000-000000004153');
insert into circle_memberships(circle_id,user_id,role) values
    ('01900000-0000-7000-8000-000000004155','01900000-0000-7000-8000-000000004151','member'),
    ('01900000-0000-7000-8000-000000004155','01900000-0000-7000-8000-000000004153','owner');
insert into channels(id,slug,name,kind,created_by,circle_id) values
    ('01900000-0000-7000-8000-000000004156','synthetic-restore-channel','Synthetic shared channel','private',
    '01900000-0000-7000-8000-000000004153','01900000-0000-7000-8000-000000004155');
insert into channel_memberships(channel_id,user_id,role) values
    ('01900000-0000-7000-8000-000000004156','01900000-0000-7000-8000-000000004151','member'),
    ('01900000-0000-7000-8000-000000004156','01900000-0000-7000-8000-000000004153','owner');
insert into messages(id,channel_id,sender_id,sender_display_name,sequence,body) values
    ('01900000-0000-7000-8000-000000004157','01900000-0000-7000-8000-000000004156','01900000-0000-7000-8000-000000004153','Synthetic unaffected',1,'Synthetic preserved history');
insert into agent_profiles(agent_id,owner_id,invited_by,provider,service_identity,purpose,rate_limit_per_minute,created_at)
values ('01900000-0000-7000-8000-000000004154','01900000-0000-7000-8000-000000004153','01900000-0000-7000-8000-000000004153','synthetic','synthetic','Synthetic restore',10,now());
insert into circle_chat_agents(agent_id,circle_id,trigger_words,response_phrases,created_by,updated_by,created_at,updated_at)
values ('01900000-0000-7000-8000-000000004154','01900000-0000-7000-8000-000000004155','[]','[]','01900000-0000-7000-8000-000000004153','01900000-0000-7000-8000-000000004153',1,1);
insert into agent_memory_profiles(id,circle_id,agent_id,user_id,created_at,updated_at)
values ('01900000-0000-7000-8000-000000004159','01900000-0000-7000-8000-000000004155','01900000-0000-7000-8000-000000004154','01900000-0000-7000-8000-000000004153',1,1);
insert into agent_location_shares(user_id,agent_id,channel_id,share_id,latitude,longitude,accuracy_m,observed_at,expires_at)
values ('01900000-0000-7000-8000-000000004153','01900000-0000-7000-8000-000000004154','01900000-0000-7000-8000-000000004156','01900000-0000-7000-8000-000000004158',1,1,1,now(),now()+interval '10 minutes');
insert into native_families(family,user_id,epoch,generation,phase,access_hash,refresh_hash,revoke_hash,expires_at,secret,lease_id,lease_expires) values
    ('synthetic-erased','01900000-0000-7000-8000-000000004151','1','1','active','a1','r1','v1','9999999999','synthetic','','0'),
    ('synthetic-revoked','01900000-0000-7000-8000-000000004152','1','1','active','a2','r2','v2','9999999999','synthetic','','0'),
    ('synthetic-unaffected','01900000-0000-7000-8000-000000004153','1','1','active','a3','r3','v3','9999999999','synthetic','','0');
commit;
