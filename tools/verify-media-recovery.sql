-- Fail the drill if any original, reservation, attachment or policy changed.
do $$
declare original bytea; expected_hash text;
begin
    select content into original from media_blobs where media_id='01900000-0000-7000-8000-000000000010';
    if original is distinct from 'legacy-original!'::bytea then raise exception 'legacy original lost'; end if;
    select string_agg(content,''::bytea order by chunk_index) into original from media_upload_chunks where upload_id='01900000-0000-7000-8000-000000000012';
    select sha256 into expected_hash from media_objects where id='01900000-0000-7000-8000-000000000011';
    if octet_length(original) is distinct from 1048593 or encode(sha256(original),'hex') is distinct from expected_hash then raise exception 'chunk original changed'; end if;
    if exists(select 1 from media_upload_chunks where sha256 is distinct from encode(sha256(content),'hex')) then raise exception 'chunk hash changed'; end if;
    if not exists(select 1 from media_uploads where id='01900000-0000-7000-8000-000000000012' and state='complete' and media_id='01900000-0000-7000-8000-000000000011' and received_bytes=1048593 and next_chunk=2) then raise exception 'completed upload mapping lost'; end if;
    if not exists(select 1 from media_uploads where id='01900000-0000-7000-8000-000000000014' and state='open' and received_bytes=1048576 and expected_bytes=2097152 and next_chunk=1 and media_id is null) then raise exception 'reservation lost'; end if;
    if not exists(select 1 from media_upload_chunks where upload_id='01900000-0000-7000-8000-000000000014' and chunk_index=0 and content=repeat('z',1048576)::bytea) then raise exception 'partial upload content lost'; end if;
    if not exists(select 1 from media_preview_jobs where media_id='01900000-0000-7000-8000-000000000011' and state='running' and attempts=1 and lease_until=2 and lease_token='01900000-0000-7000-8000-000000000016') then raise exception 'worker lease lost'; end if;
    if (select count(*) from message_attachments where message_id='01900000-0000-7000-8000-000000000003')!=2 then raise exception 'attachments lost'; end if;
    if not exists(select 1 from media_upload_budget where id=1 and max_file_bytes=4194304 and owner_bytes=16777216 and total_bytes=33554432) then raise exception 'quota policy lost'; end if;
end $$;
