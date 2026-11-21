-- Index open, unsubmitted process tasks by assignee for the personal queue.
create index work_item_tasks_personal_queue
    on work_item_tasks(assignee_id, message_id)
    where status='pending' and delivery_status='ready' and decision_request_id is null;

create index work_item_status_changes_personal_queue
    on work_item_status_changes(actor_id, message_id)
    where status='waiting' and decision_result is null and task_id is not null and message_id is not null;

create index process_pilot_runs_personal_queue
    on process_pilot_runs(assignee_id, channel_id, id)
    where status in ('starting','waiting');
