mod agent;
mod auth;
mod chat;
mod config;
mod db;
mod domain;
mod notification;
mod operations;
mod process;
mod protocol;
mod server;
mod web;
mod ws;

#[cfg(test)]
use server::{AppState, build_router};
#[cfg(test)]
use web::media::{MediaPreparationError, detected_media_type, prepare_uploaded_media};

#[cfg(test)]
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, HeaderName, HeaderValue},
    response::IntoResponse,
};
#[cfg(test)]
use axum::{Router, routing::post};
#[cfg(test)]
use std::time::Duration;

#[cfg(test)]
use crate::{
    agent::{AgentScope, CreateAgent, GrantAgent},
    auth::AuthService,
    chat::ChatEngine,
    domain::{ChannelId, UserId},
    notification::NotificationService,
    operations::OperationalState,
    process::{HeartGateway, SetCircleFeature, SharedProcessGateway},
    web::browser::is_safe_invitation_token,
    web::mcp::{MCP_PROTOCOL_VERSION, McpRequest, mcp_handler},
};
#[cfg(test)]
use axum::http::header::{ACCEPT, AUTHORIZATION, ORIGIN};

#[cfg(test)]
use web::assets::{
    APP_BUNDLE, APP_SOURCE, BUILD_REVISION, CLIENT_STORE, INDEX_HTML, app_bundle_fingerprint,
    client_store_fingerprint,
};
#[cfg(test)]
use web::assets::{PWA_MANIFEST, SERVICE_WORKER, WAVE_LOGO_192, WAVE_LOGO_512};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    server::run().await
}

#[cfg(test)]
#[path = "main_tests/mcp.rs"]
mod mcp_tests;

#[cfg(test)]
#[path = "main_tests/protocol_capacity.rs"]
mod protocol_capacity_tests;
