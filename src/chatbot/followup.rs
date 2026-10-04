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
    let ferry_fresh = if pg {
        "j.ferry_valid_until > extract(epoch from clock_timestamp())"
    } else {
        "j.ferry_valid_until > cast(strftime('%s','now') as integer)"
    };
    format!(
        " and (a.weather is null or {weather_fresh}) \
        and (a.ferry_port is null or {ferry_fresh}) \
        and (j.followup_anchor_message_id is null or exists( \
        select 1 from circle_chat_agent_jobs previous \
        join command_receipts receipt on receipt.principal_id=previous.agent_id and receipt.request_id='circle-chat-agent:' || cast(previous.id as text) \
        join messages anchor on anchor.id=receipt.message_id \
        join messages original on original.id=previous.source_message_id \
        join messages target on target.id=j.source_message_id \
        where anchor.id=j.followup_anchor_message_id and previous.agent_id=j.agent_id \
        and previous.channel_id=j.channel_id and previous.config_revision=j.config_revision \
        and previous.access_revision=j.access_revision \
        and original.channel_id=j.channel_id and anchor.channel_id=j.channel_id and anchor.sender_id=j.agent_id \
        and original.deleted_at is null and original.edited_at is null \
        and anchor.deleted_at is null and anchor.edited_at is null \
        and coalesce(cast(original.parent_message_id as text),'')=coalesce(cast(anchor.parent_message_id as text),'') \
        and anchor.sequence<target.sequence and {fresh} \
        and ((j.followup_mode='weather' and a.weather is not null and original.sender_id=target.sender_id \
          and coalesce(cast(anchor.parent_message_id as text),'')=coalesce(cast(target.parent_message_id as text),'')) \
        or (j.followup_mode='explicit' and anchor.parent_message_id is null and target.parent_message_id=anchor.id) \
        or (j.followup_mode='implicit' and original.sender_id=target.sender_id and {implicit_fresh} \
          and coalesce(cast(anchor.parent_message_id as text),'')=coalesce(cast(target.parent_message_id as text),'') \
          and not exists(select 1 from messages between_reply where between_reply.channel_id=j.channel_id \
            and between_reply.deleted_at is null and between_reply.sequence>anchor.sequence and between_reply.sequence<target.sequence \
            and coalesce(cast(between_reply.parent_message_id as text),'')=coalesce(cast(target.parent_message_id as text),''))))) )",
        implicit_fresh = if pg {
            "anchor.created_at >= clock_timestamp() - interval '3 minutes'"
        } else {
            "julianday(anchor.created_at) >= julianday('now','-3 minutes')"
        }
    )
}

/// Candidates are bounded by server-owned receipts and conversation order.
/// The model may decline these candidates; it cannot enlarge their scope.
pub(super) fn conversation_query(pg: bool) -> String {
    let object = if pg {
        "cast(json_build_object('anchor',cast(anchor.id as text),'mode',case when incoming.parent_id=cast(anchor.id as text) then 'explicit' else 'implicit' end) as text)"
    } else {
        "json_object('anchor',anchor.id,'mode',case when incoming.parent_id=anchor.id then 'explicit' else 'implicit' end)"
    };
    let fresh = |minutes| {
        if pg {
            format!("anchor.created_at >= clock_timestamp() - interval '{minutes} minutes'")
        } else {
            format!("julianday(anchor.created_at) >= julianday('now','-{minutes} minutes')")
        }
    };
    format!(
        "with incoming as (select ?uuid agent_id,?uuid channel_id,?uuid actor_id,?int config_revision,?int access_revision,? parent_id,?int sequence) \
        select {object} from incoming join circle_chat_agent_jobs previous on previous.agent_id=incoming.agent_id and previous.channel_id=incoming.channel_id \
        join command_receipts receipt on receipt.principal_id=previous.agent_id and receipt.request_id='circle-chat-agent:' || cast(previous.id as text) \
        join messages anchor on anchor.id=receipt.message_id join messages original on original.id=previous.source_message_id \
        where previous.config_revision=incoming.config_revision and previous.access_revision=incoming.access_revision \
        and anchor.sender_id=previous.agent_id and anchor.channel_id=incoming.channel_id and original.channel_id=incoming.channel_id \
        and anchor.deleted_at is null and anchor.edited_at is null and original.deleted_at is null and original.edited_at is null \
        and coalesce(cast(original.parent_message_id as text),'')=coalesce(cast(anchor.parent_message_id as text),'') and anchor.sequence<incoming.sequence \
        and ((anchor.parent_message_id is null and incoming.parent_id=cast(anchor.id as text) and {explicit_fresh}) \
        or (original.sender_id=incoming.actor_id and coalesce(cast(anchor.parent_message_id as text),'')=incoming.parent_id and {implicit_fresh} \
          and not exists(select 1 from messages between_reply where between_reply.channel_id=incoming.channel_id \
            and between_reply.deleted_at is null and between_reply.sequence>anchor.sequence and between_reply.sequence<incoming.sequence \
            and coalesce(cast(between_reply.parent_message_id as text),'')=incoming.parent_id))) \
        order by case when incoming.parent_id=cast(anchor.id as text) then 0 else 1 end,anchor.sequence desc limit 1",
        explicit_fresh = fresh(20),
        implicit_fresh = fresh(3)
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
