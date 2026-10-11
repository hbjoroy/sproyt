-- Isolated CI database only. Exercise both readers, reservations and worker leases.
begin;
insert into media_objects(id,owner_id,channel_id,storage_key,original_filename,content_type,size_bytes,sha256,created_at)
values ('01900000-0000-7000-8000-000000000010','01900000-0000-7000-8000-000000000001','01900000-0000-7000-8000-000000000002','recovery:legacy','legacy.bin','application/octet-stream',16,encode(sha256('legacy-original!'::bytea),'hex'),now()),
('01900000-0000-7000-8000-000000000011','01900000-0000-7000-8000-000000000001','01900000-0000-7000-8000-000000000002','chunks:v1:01900000-0000-7000-8000-000000000012','queued.jpg','image/jpeg',1048593,encode(sha256((repeat('x',1048576)||repeat('y',17))::bytea),'hex'),now());
insert into media_blobs(media_id,content) values ('01900000-0000-7000-8000-000000000010','legacy-original!'::bytea);
insert into media_uploads(id,owner_id,channel_id,request_id,original_filename,declared_type,expected_bytes,received_bytes,next_chunk,state,created_at,expires_at,media_id)
values ('01900000-0000-7000-8000-000000000012','01900000-0000-7000-8000-000000000001','01900000-0000-7000-8000-000000000002','01900000-0000-7000-8000-000000000013','queued.jpg','image/jpeg',1048593,1048593,2,'complete',1,86400001,'01900000-0000-7000-8000-000000000011'),
('01900000-0000-7000-8000-000000000014','01900000-0000-7000-8000-000000000001','01900000-0000-7000-8000-000000000002','01900000-0000-7000-8000-000000000015','partial.mp4','video/mp4',2097152,1048576,1,'open',1,86400001,null);
insert into media_upload_chunks(upload_id,chunk_index,content,sha256)
values ('01900000-0000-7000-8000-000000000012',0,repeat('x',1048576)::bytea,encode(sha256(repeat('x',1048576)::bytea),'hex')),
('01900000-0000-7000-8000-000000000012',1,repeat('y',17)::bytea,encode(sha256(repeat('y',17)::bytea),'hex')),
('01900000-0000-7000-8000-000000000014',0,repeat('z',1048576)::bytea,encode(sha256(repeat('z',1048576)::bytea),'hex'));
insert into media_preview_jobs(media_id,state,attempts,available_at,lease_token,lease_until)
values ('01900000-0000-7000-8000-000000000011','running',1,1,'01900000-0000-7000-8000-000000000016',2);
insert into message_attachments(message_id,media_id,position)
values ('01900000-0000-7000-8000-000000000003','01900000-0000-7000-8000-000000000010',0),
('01900000-0000-7000-8000-000000000003','01900000-0000-7000-8000-000000000011',1);
update media_upload_budget set max_file_bytes=4194304,owner_bytes=16777216,total_bytes=33554432 where id=1;
commit;
