create index messages_person_activity on messages(sender_id, created_at desc, id desc)
where deleted_at is null;
