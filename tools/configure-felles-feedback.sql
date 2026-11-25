-- PostgreSQL 17+, psql. Run on the Sprøyt database after migrations 0080/0081.
-- Preview: psql -X -f tools/configure-felles-feedback.sql
-- Start:   psql -X -v apply=on -f tools/configure-felles-feedback.sql
-- Stop:    psql -X -v stop=on -v apply=on -f tools/configure-felles-feedback.sql
-- schema defaults to sproyt. Optionally pass reviewer=<existing product handler UUID>.
\set ON_ERROR_STOP on
\if :{?apply}
\else
\set apply off
\endif
\if :{?stop}
\else
\set stop off
\endif
\if :{?schema}
\else
\set schema sproyt
\endif
\if :{?reviewer}
\else
\set reviewer ''
\endif

begin;
set local search_path to pg_catalog, :"schema";
set local lock_timeout = '5s';
select set_config('sproyt.feedback_stop', :'stop'::boolean::text, true),
       set_config('sproyt.feedback_reviewer', :'reviewer', true);

do $configure$
declare
    target channels%rowtype;
    app work_applications%rowtype;
    binding work_github_bindings%rowtype;
    route feedback_routes%rowtype;
    handler users%rowtype;
    bot uuid;
    boundary bigint;
    epoch bigint := extract(epoch from clock_timestamp())::bigint;
    identity_key text;
begin
    select * into strict target from channels
    where name='Tilbakemeldingar Sprøyt' and circle_id is null and kind='public'
    for update;
    select * into route from feedback_routes where channel_id=target.id for update;
    if current_setting('sproyt.feedback_stop')::boolean then
        if route.channel_id is null then
            raise notice 'No active route in channel %',target.id;
            return;
        end if;
        update feedback_routes set enabled=false where channel_id=target.id;
        raise notice 'Stop channel %; preserve agent %, admitted jobs, receipts and Heart instances',target.id,route.agent_id;
        return;
    end if;
    if target.hidden then
        raise exception 'Feedback channel % is hidden',target.id;
    end if;
    select * into strict app from work_applications where key='sproyt' and enabled for share;
    select * into strict binding from work_github_bindings
    where application_id=app.id and enabled and repository_name='hbjoroy/sproyt' and repository_id=1272543078
    for share;
    select u.* into strict handler from users u
    join application_processors p on p.user_id=u.id and p.application_id=app.id and p.can_review and p.can_export
    join application_process_roles role on role.user_id=u.id and role.application_id=app.id and role.process_role='product-handler'
    join channel_memberships cm on cm.user_id=u.id and cm.channel_id=target.id and cm.role<>'observer'
    where u.kind='human' and (nullif(current_setting('sproyt.feedback_reviewer'),'') is null
        or u.id=nullif(current_setting('sproyt.feedback_reviewer'),'')::uuid)
    for share of u,p,role,cm;

    -- Standard circle-agent identity, dedicated to this channel even if the route is stopped.
    identity_key := 'feedback-channel:' || target.id;
    select p.agent_id into bot from agent_profiles p where p.provider='sproyt-circle-chat' and p.service_identity=identity_key;
    if route.channel_id is not null and (route.agent_id is distinct from bot or
        route.application_id<>app.id or route.reviewer_id<>handler.id) then
        raise exception 'Existing feedback binding differs; cannot replace it with this command';
    end if;
    if bot is null then
        perform 1 from circle_memberships where circle_id=app.owner_circle_id and user_id=handler.id and role in ('owner','moderator') for share;
        if not found then raise exception 'Creating the agent requires an owner or moderator of its home circle'; end if;
        if (select count(*) from circle_chat_agents where circle_id=app.owner_circle_id)>=10 then
            raise exception 'Home circle has reached its agent limit';
        end if;
        bot := gen_random_uuid();
        insert into users(id,kind,display_name,external_provider,external_subject)
        values(bot,'agent','Sprøyt tilbakemeldingar','sproyt-circle-chat',bot::text);
        insert into agent_profiles(agent_id,owner_id,invited_by,provider,service_identity,purpose,rate_limit_per_minute,created_at)
        values(bot,handler.id,handler.id,'sproyt-circle-chat',identity_key,'Felles feedback agent',30,now());
        insert into circle_chat_agents(agent_id,circle_id,trigger_words,response_phrases,enabled,memory_enabled,
            created_by,updated_by,created_at,updated_at)
        values(bot,app.owner_circle_id,'[]','[]',true,false,handler.id,handler.id,epoch,epoch);
    else
        perform 1 from agent_profiles p join circle_chat_agents a on a.agent_id=p.agent_id
        where p.agent_id=bot and p.purpose='Felles feedback agent' and p.revoked_at is null
            and (p.expires_at is null or p.expires_at>now()) and a.enabled and a.circle_id=app.owner_circle_id for share;
        if not found then raise exception 'Existing agent is revoked, expired, disabled or incompatible'; end if;
    end if;
    insert into channel_chat_agent_settings(channel_id,agent_id,enabled,updated_by,updated_at)
    values(target.id,bot,true,handler.id,epoch) on conflict(channel_id,agent_id) do nothing;
    if found then
        update channels set chat_agent_access_revision=chat_agent_access_revision+1 where id=target.id;
    elsif not exists(select 1 from channel_chat_agent_settings where channel_id=target.id and agent_id=bot and enabled) then
        raise exception 'Channel grant was revoked; restore it explicitly before enabling the route';
    end if;

    -- Channel sequence allocation and this activation share a transaction boundary.
    -- Repeating start while running keeps the original fence. Resuming skips the stopped interval.
    select next_sequence-1 into strict boundary from channel_sequences where channel_id=target.id for update;
    insert into feedback_routes(channel_id,agent_id,reviewer_id,application_id,enabled,after_sequence)
    values(target.id,bot,handler.id,app.id,true,boundary)
    on conflict(channel_id) do update set enabled=true,after_sequence=excluded.after_sequence where not feedback_routes.enabled;
    select * into strict route from feedback_routes where channel_id=target.id;
    raise notice 'Channel % (%) | agent % | reviewer % (%) | repository % (%) | first eligible sequence %',
        target.name,target.id,bot,handler.display_name,handler.id,binding.repository_name,binding.repository_id,route.after_sequence+1;
end
$configure$;
\if :apply
commit;
\else
rollback;
\echo Dry run complete. Use -v apply=on to persist the change.
\endif
