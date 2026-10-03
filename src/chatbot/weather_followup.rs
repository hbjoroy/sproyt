//! Followups are anchored to committed replies, not worker settlement.

pub(super) fn query(pg: bool) -> String {
    let fresh = if pg {
        "anchor.created_at >= clock_timestamp() - interval '20 minutes'"
    } else {
        "julianday(anchor.created_at) >= julianday('now','-20 minutes')"
    };
    format!(
        "select cast(anchor.id as text) from circle_chat_agent_jobs previous \
        join command_receipts receipt on receipt.principal_id=previous.agent_id and receipt.request_id='circle-chat-agent:' || cast(previous.id as text) \
        join messages anchor on anchor.id=receipt.message_id \
        join messages original on original.id=previous.source_message_id \
        where previous.agent_id=?uuid and previous.channel_id=?uuid and original.sender_id=?uuid \
        and previous.config_revision=?int and previous.access_revision=?int \
        and anchor.sender_id=previous.agent_id and anchor.channel_id=previous.channel_id \
        and original.channel_id=previous.channel_id and original.deleted_at is null and original.edited_at is null \
        and anchor.deleted_at is null and anchor.edited_at is null \
        and coalesce(cast(anchor.parent_message_id as text),'')=? \
        and coalesce(cast(original.parent_message_id as text),'')=coalesce(cast(anchor.parent_message_id as text),'') \
        and anchor.sequence<?int and {fresh} order by anchor.sequence desc limit 1"
    )
}

pub(super) fn publication_clause(pg: bool) -> String {
    let fresh = if pg {
        "anchor.created_at >= clock_timestamp() - interval '20 minutes'"
    } else {
        "julianday(anchor.created_at) >= julianday('now','-20 minutes')"
    };
    let weather_fresh = if pg {
        "j.weather_valid_until > extract(epoch from clock_timestamp())"
    } else {
        "j.weather_valid_until > cast(strftime('%s','now') as integer)"
    };
    format!(
        " and (a.weather is null or {weather_fresh}) \
        and (j.followup_anchor_message_id is null or (a.weather is not null and exists( \
        select 1 from circle_chat_agent_jobs previous \
        join command_receipts receipt on receipt.principal_id=previous.agent_id and receipt.request_id='circle-chat-agent:' || cast(previous.id as text) \
        join messages anchor on anchor.id=receipt.message_id \
        join messages original on original.id=previous.source_message_id \
        join messages target on target.id=j.source_message_id \
        where anchor.id=j.followup_anchor_message_id and previous.agent_id=j.agent_id \
        and previous.channel_id=j.channel_id and previous.config_revision=j.config_revision \
        and previous.access_revision=j.access_revision and original.sender_id=target.sender_id \
        and original.channel_id=j.channel_id and anchor.channel_id=j.channel_id and anchor.sender_id=j.agent_id \
        and original.deleted_at is null and original.edited_at is null \
        and anchor.deleted_at is null and anchor.edited_at is null \
        and coalesce(cast(anchor.parent_message_id as text),'')=coalesce(cast(target.parent_message_id as text),'') \
        and coalesce(cast(original.parent_message_id as text),'')=coalesce(cast(target.parent_message_id as text),'') \
        and anchor.sequence<target.sequence and {fresh})))"
    )
}

/// The snapshot does not exist yet during source/context lookup.
pub(super) fn anchor_clause(pg: bool) -> String {
    let clause = publication_clause(pg);
    clause[clause
        .find(" and (j.followup_anchor")
        .expect("anchor clause")..]
        .to_owned()
}
