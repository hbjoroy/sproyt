use axum::{
    Json,
    extract::State,
    http::{
        HeaderMap,
        header::{ACCEPT, AUTHORIZATION, ORIGIN},
    },
    response::IntoResponse,
};
use serde::Deserialize;

use crate::{
    agent::{AgentPrincipal, AgentScope},
    chat::ChatError,
    domain::{ChannelId, ChannelSequence, MessageBody, MessageLimit},
    process::{EnqueueCorrelation, EnqueueInspection, EnqueueProcessStart, ProcessLinkId},
    server::AppState,
};

#[derive(Deserialize)]
pub(crate) struct McpRequest {
    #[serde(default = "mcp_jsonrpc")]
    pub(crate) jsonrpc: String,
    #[serde(default)]
    pub(crate) id: serde_json::Value,
    pub(crate) method: String,
    #[serde(default)]
    pub(crate) params: serde_json::Value,
}

fn mcp_jsonrpc() -> String {
    "2.0".to_owned()
}
pub(crate) const MCP_PROTOCOL_VERSION: &str = "2025-11-25";

fn is_supported_mcp_version(version: &str) -> bool {
    matches!(version, "2025-03-26" | "2025-06-18" | MCP_PROTOCOL_VERSION)
}

pub(crate) async fn mcp_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<McpRequest>,
) -> axum::response::Response {
    if let Some(origin) = headers.get(ORIGIN).and_then(|value| value.to_str().ok()) {
        let allowed = std::env::var("SPROYT_MCP_ALLOWED_ORIGINS").unwrap_or_default();
        if !allowed
            .split(',')
            .map(str::trim)
            .any(|candidate| !candidate.is_empty() && candidate == origin)
        {
            return axum::http::StatusCode::FORBIDDEN.into_response();
        }
    }
    let accepts = headers
        .get(ACCEPT)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if !(accepts.contains("application/json") && accepts.contains("text/event-stream")) {
        return axum::http::StatusCode::NOT_ACCEPTABLE.into_response();
    }
    if request.jsonrpc != "2.0" {
        return mcp_json_response(
            axum::http::StatusCode::BAD_REQUEST,
            serde_json::json!({"jsonrpc":"2.0","id":request.id,"error":{"code":-32600,"message":"invalid JSON-RPC version"}}),
        );
    }
    if request.method != "initialize" {
        let version = headers
            .get("mcp-protocol-version")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("2025-03-26");
        if !is_supported_mcp_version(version) {
            return axum::http::StatusCode::BAD_REQUEST.into_response();
        }
    }
    let credential = match headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
    {
        Some(value) => value,
        None => return axum::http::StatusCode::UNAUTHORIZED.into_response(),
    };
    let principal = match state.agents.authenticate(credential).await {
        Ok(value) => value,
        Err(crate::domain::RepositoryError::Conflict) => {
            return axum::http::StatusCode::TOO_MANY_REQUESTS.into_response();
        }
        Err(_) => return axum::http::StatusCode::UNAUTHORIZED.into_response(),
    };
    let notification = request.id.is_null() || request.method.starts_with("notifications/");
    let response = match request.method.as_str() {
        "initialize" => {
            let protocol_version = request
                .params
                .get("protocolVersion")
                .and_then(|version| version.as_str())
                .filter(|version| is_supported_mcp_version(version))
                .unwrap_or(MCP_PROTOCOL_VERSION);
            Ok(
                serde_json::json!({"protocolVersion":protocol_version,"capabilities":{"tools":{}},"serverInfo":{"name":"sproyt","version":env!("CARGO_PKG_VERSION")}}),
            )
        }
        "notifications/initialized" => Ok(serde_json::Value::Null),
        "tools/list" => Ok(serde_json::json!({"tools": mcp_tools()})),
        "tools/call" => mcp_call(&state, &principal, request.params).await,
        _ => Err((-32601, "method not found".to_owned())),
    };
    if notification {
        return axum::http::StatusCode::ACCEPTED.into_response();
    }
    mcp_json_response(
        axum::http::StatusCode::OK,
        match response {
            Ok(result) => serde_json::json!({"jsonrpc":"2.0","id":request.id,"result":result}),
            Err((code, message)) => {
                serde_json::json!({"jsonrpc":"2.0","id":request.id,"error":{"code":code,"message":message}})
            }
        },
    )
}

fn mcp_json_response(
    status: axum::http::StatusCode,
    value: serde_json::Value,
) -> axum::response::Response {
    (status, Json(value)).into_response()
}

fn mcp_tools() -> serde_json::Value {
    serde_json::json!([
      {"name":"list_channels","description":"List channels granted to this agent","inputSchema":{"type":"object","properties":{},"additionalProperties":false}},
      {"name":"read_messages","description":"Read channel history","inputSchema":{"type":"object","required":["channel_id"],"properties":{"channel_id":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":200},"after_sequence":{"type":"integer","minimum":0}},"additionalProperties":false}},
      {"name":"send_message","description":"Send an agent-authored message","inputSchema":{"type":"object","required":["channel_id","body","request_id"],"properties":{"channel_id":{"type":"string"},"body":{"type":"string"},"request_id":{"type":"string"},"provenance":{"type":"string","enum":["generated","delegated"],"default":"generated"}},"additionalProperties":false}},
      {"name":"mark_read","description":"Advance the agent read marker","inputSchema":{"type":"object","required":["channel_id","sequence"],"properties":{"channel_id":{"type":"string"},"sequence":{"type":"integer","minimum":0}},"additionalProperties":false}},
      {"name":"start_process","description":"Start an enabled Heart process from a channel","inputSchema":{"type":"object","required":["channel_id","request_id","namespace","definition_name"],"properties":{"channel_id":{"type":"string"},"request_id":{"type":"string"},"namespace":{"type":"string"},"definition_name":{"type":"string"},"definition_version":{"type":"string"},"metadata":{"type":"object"}},"additionalProperties":false}},
      {"name":"get_process","description":"Read a process linked to an authorized channel","inputSchema":{"type":"object","required":["process_link_id"],"properties":{"process_link_id":{"type":"string"}},"additionalProperties":false}},
      {"name":"inspect_process","description":"Request a fresh Heart process inspection","inputSchema":{"type":"object","required":["process_link_id","request_id"],"properties":{"process_link_id":{"type":"string"},"request_id":{"type":"string"}},"additionalProperties":false}},
      {"name":"complete_process_work","description":"Correlate an authorized response to a Heart receive node","inputSchema":{"type":"object","required":["process_link_id","request_id","payload"],"properties":{"process_link_id":{"type":"string"},"request_id":{"type":"string"},"payload":{}},"additionalProperties":false}}
    ])
}

async fn mcp_call(
    state: &AppState,
    principal: &AgentPrincipal,
    params: serde_json::Value,
) -> Result<serde_json::Value, (i64, String)> {
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or((-32602, "missing tool name".to_owned()))?;
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let result = match name {
        "list_channels" => {
            let channels = state
                .chat
                .list_channels(principal.agent_id.clone())
                .await
                .map_err(mcp_chat_error)?;
            let mut granted = Vec::with_capacity(channels.len());
            for channel in channels {
                if state
                    .agents
                    .has_any_scope(
                        principal,
                        channel.circle_id.clone(),
                        Some(channel.id.clone()),
                    )
                    .await
                    .map_err(mcp_repository_error)?
                {
                    granted.push(channel);
                }
            }
            serde_json::to_value(granted).map_err(|e| (-32603, e.to_string()))?
        }
        "read_messages" => {
            let channel = mcp_channel(&args)?;
            state
                .agents
                .require_scope(
                    principal,
                    None,
                    Some(channel.clone()),
                    AgentScope::ReadHistory,
                )
                .await
                .map_err(mcp_repository_error)?;
            let limit = args
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(50)
                .min(200) as u16;
            let after = args
                .get("after_sequence")
                .and_then(|v| v.as_u64())
                .map(ChannelSequence::new);
            serde_json::to_value(
                state
                    .chat
                    .load_messages(
                        principal.agent_id.clone(),
                        channel,
                        MessageLimit::new(limit),
                        after,
                        None,
                    )
                    .await
                    .map_err(mcp_chat_error)?,
            )
            .map_err(|e| (-32603, e.to_string()))?
        }
        "send_message" => {
            let channel = mcp_channel(&args)?;
            state
                .agents
                .require_scope(
                    principal,
                    None,
                    Some(channel.clone()),
                    AgentScope::SendMessages,
                )
                .await
                .map_err(mcp_repository_error)?;
            let body = MessageBody::new(
                args.get("body")
                    .and_then(|v| v.as_str())
                    .ok_or((-32602, "missing body".to_owned()))?,
            )
            .map_err(|e| (-32602, e.to_string()))?;
            let request_id = args
                .get("request_id")
                .and_then(|v| v.as_str())
                .ok_or((-32602, "missing request_id".to_owned()))?
                .to_owned();
            let delegated =
                args.get("provenance").and_then(|value| value.as_str()) == Some("delegated");
            let agent_id = principal.agent_id.clone();
            let message = state
                .chat
                .send_message_idempotent(channel, agent_id.clone(), body, request_id)
                .await
                .map_err(mcp_chat_error)?;
            if delegated {
                state
                    .agents
                    .mark_delegated(agent_id, message.id)
                    .await
                    .map_err(mcp_repository_error)?;
            }
            let provenance = state
                .agents
                .message_provenance(message.id)
                .await
                .map_err(mcp_repository_error)?;
            serde_json::json!({"message":message,"provenance":provenance})
        }
        "mark_read" => {
            let channel = mcp_channel(&args)?;
            state
                .agents
                .require_scope(
                    principal,
                    None,
                    Some(channel.clone()),
                    AgentScope::ReadHistory,
                )
                .await
                .map_err(mcp_repository_error)?;
            let sequence = args
                .get("sequence")
                .and_then(|v| v.as_u64())
                .ok_or((-32602, "missing sequence".to_owned()))?;
            serde_json::to_value(
                state
                    .chat
                    .mark_read(
                        principal.agent_id.clone(),
                        channel,
                        ChannelSequence::new(sequence),
                    )
                    .await
                    .map_err(mcp_chat_error)?,
            )
            .map_err(|e| (-32603, e.to_string()))?
        }
        "start_process" => {
            let channel = mcp_channel(&args)?;
            state
                .agents
                .require_scope(
                    principal,
                    None,
                    Some(channel.clone()),
                    AgentScope::StartProcesses,
                )
                .await
                .map_err(mcp_repository_error)?;
            let request_id = mcp_required_string(&args, "request_id")?;
            let namespace = mcp_required_string(&args, "namespace")?;
            let definition_name = mcp_required_string(&args, "definition_name")?;
            let definition_version = args
                .get("definition_version")
                .and_then(|value| value.as_str())
                .map(str::to_owned);
            let metadata = args
                .get("metadata")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            serde_json::to_value(
                state
                    .processes
                    .enqueue_start(EnqueueProcessStart {
                        channel_id: channel,
                        actor: principal.agent_id.clone(),
                        request_id,
                        namespace,
                        definition_name,
                        definition_version,
                        metadata,
                    })
                    .await
                    .map_err(mcp_repository_error)?,
            )
            .map_err(|_| (-32603, "failed to encode tool result".to_owned()))?
        }
        "get_process" => {
            let (_, view) =
                mcp_process_with_scope(state, principal, &args, AgentScope::CompleteProcessWork)
                    .await?;
            serde_json::to_value(view)
                .map_err(|_| (-32603, "failed to encode tool result".to_owned()))?
        }
        "inspect_process" => {
            let (process_link_id, _) =
                mcp_process_with_scope(state, principal, &args, AgentScope::CompleteProcessWork)
                    .await?;
            let request_id = mcp_required_string(&args, "request_id")?;
            let outbox_id = state
                .processes
                .enqueue_inspection(EnqueueInspection {
                    process_link_id,
                    actor: principal.agent_id.clone(),
                    request_id,
                })
                .await
                .map_err(mcp_repository_error)?;
            serde_json::json!({"outbox_id":outbox_id.as_uuid()})
        }
        "complete_process_work" => {
            let (process_link_id, _) =
                mcp_process_with_scope(state, principal, &args, AgentScope::CompleteProcessWork)
                    .await?;
            let request_id = mcp_required_string(&args, "request_id")?;
            let payload = args
                .get("payload")
                .cloned()
                .ok_or((-32602, "missing payload".to_owned()))?;
            let outbox_id = state
                .processes
                .enqueue_correlation(EnqueueCorrelation {
                    process_link_id,
                    actor: principal.agent_id.clone(),
                    request_id,
                    payload,
                })
                .await
                .map_err(mcp_repository_error)?;
            serde_json::json!({"outbox_id":outbox_id.as_uuid()})
        }
        _ => return Err((-32602, "unknown tool".to_owned())),
    };
    Ok(
        serde_json::json!({"content":[{"type":"text","text":serde_json::to_string(&result).map_err(|e|(-32603,e.to_string()))?}],"structuredContent":result}),
    )
}

fn mcp_channel(args: &serde_json::Value) -> Result<ChannelId, (i64, String)> {
    ChannelId::new(
        args.get("channel_id")
            .and_then(|v| v.as_str())
            .ok_or((-32602, "missing channel_id".to_owned()))?,
    )
    .map_err(|e| (-32602, e.to_string()))
}

fn mcp_required_string(
    args: &serde_json::Value,
    name: &'static str,
) -> Result<String, (i64, String)> {
    args.get(name)
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .ok_or((-32602, format!("missing {name}")))
}

async fn mcp_process_with_scope(
    state: &AppState,
    principal: &AgentPrincipal,
    args: &serde_json::Value,
    scope: AgentScope,
) -> Result<(ProcessLinkId, crate::process::ProcessView), (i64, String)> {
    let process_link_id = args
        .get("process_link_id")
        .and_then(|value| value.as_str())
        .ok_or((-32602, "missing process_link_id".to_owned()))
        .and_then(|value| {
            ProcessLinkId::parse(value).map_err(|_| (-32602, "invalid process_link_id".to_owned()))
        })?;
    let view = state
        .processes
        .get_process(principal.agent_id.clone(), process_link_id)
        .await
        .map_err(mcp_repository_error)?;
    state
        .agents
        .require_scope(
            principal,
            None,
            Some(view.process.channel_id.clone()),
            scope,
        )
        .await
        .map_err(mcp_repository_error)?;
    Ok((process_link_id, view))
}
fn mcp_repository_error(error: crate::domain::RepositoryError) -> (i64, String) {
    (-32003, error.public_message().to_owned())
}
fn mcp_chat_error(error: ChatError) -> (i64, String) {
    (-32004, error.public_message())
}
