do $exercise$
begin
    if (select count(*) from users where id='01900000-0000-7000-8000-000000004151' and display_name='Sletta brukar') <> 1
       or exists (select 1 from personal_statuses where user_id='01900000-0000-7000-8000-000000004151')
       or exists (select 1 from channel_memberships where user_id='01900000-0000-7000-8000-000000004151')
       or exists (select 1 from native_families where family='synthetic-erased') then
        raise exception 'private data or access returned after erasure';
    end if;
    if (select count(*) from native_families where family='synthetic-revoked' and phase='revoked' and secret='') <> 1
       or (select count(*) from users where id='01900000-0000-7000-8000-000000004152' and display_name='Synthetic revoke') <> 1 then
        raise exception 'targeted revocation failed or erased the account';
    end if;
    if (select count(*) from native_families where family='synthetic-unaffected' and phase='active' and secret='synthetic') <> 1
       or (select count(*) from channel_memberships where user_id='01900000-0000-7000-8000-000000004153' and role='owner') <> 1
       or (select count(*) from messages where body='Synthetic preserved history') <> 1
       or (select count(*) from personal_statuses where text='Synthetic unaffected status') <> 1 then
        raise exception 'unaffected user lost legitimate access or data';
    end if;
    if exists (select 1 from agent_location_shares) or exists (select 1 from agent_memory_profiles) then
        raise exception 'restored consent or memory survived reset';
    end if;
end
$exercise$;
