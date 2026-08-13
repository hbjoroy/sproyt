use crate::{server::AppState, web::assets::BUILD_REVISION};
use axum::{
    Json,
    extract::{Request, State},
    http::{HeaderName, HeaderValue},
    middleware,
    response::IntoResponse,
};
use serde::Serialize;
use std::time::Duration;
use tracing::warn;

#[derive(Serialize)]
pub(crate) struct VersionInfo {
    service: &'static str,
    version: &'static str,
    revision: &'static str,
}

pub(crate) async fn versionz() -> Json<VersionInfo> {
    Json(VersionInfo {
        service: "sproyt",
        version: env!("CARGO_PKG_VERSION"),
        revision: BUILD_REVISION,
    })
}

pub(crate) async fn add_security_headers(
    request: Request,
    next: middleware::Next,
) -> axum::response::Response {
    let mut response = next.run(request).await;
    // A WebSocket 101 is not a document response. Extra document policy
    // headers are unnecessary there and some proxies treat decorated upgrade
    // responses inconsistently.
    if response.status() == axum::http::StatusCode::SWITCHING_PROTOCOLS {
        return response;
    }
    let headers = response.headers_mut();
    let defaults = [
        (
            "content-security-policy",
            "default-src 'none'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'",
        ),
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        ("referrer-policy", "no-referrer"),
        (
            "permissions-policy",
            "camera=(), microphone=(), geolocation=()",
        ),
        ("cross-origin-opener-policy", "same-origin"),
        ("cache-control", "no-store"),
    ];
    for (name, value) in defaults {
        if !headers.contains_key(name) {
            headers.insert(
                HeaderName::from_static(name),
                HeaderValue::from_static(value),
            );
        }
    }
    response
}

pub(crate) async fn app_readyz(State(state): State<AppState>) -> axum::response::Response {
    if !state.operations.is_ready() {
        return (axum::http::StatusCode::SERVICE_UNAVAILABLE, "not ready\n").into_response();
    }
    // Readiness protects traffic that needs this replica's durable store. OIDC
    // discovery is verified at startup and refreshed during authentication;
    // putting an internet request here makes kubelet probes remove otherwise
    // healthy replicas whenever the identity provider is merely slow.
    let database = async {
        state
            .chat
            .health_check()
            .await
            .map_err(|error| error.kind())
    };
    match tokio::time::timeout(Duration::from_secs(2), database).await {
        Ok(Ok(())) => (axum::http::StatusCode::OK, "ready\n").into_response(),
        Ok(Err(error_kind)) => {
            warn!(error_kind, "readiness dependency probe failed");
            (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "dependency unavailable\n",
            )
                .into_response()
        }
        Err(_) => {
            warn!("readiness dependency probe timed out");
            (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "dependency timeout\n",
            )
                .into_response()
        }
    }
}
