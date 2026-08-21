use axum::{
    extract::Path,
    http::{
        HeaderName, HeaderValue,
        header::{CACHE_CONTROL, CONTENT_TYPE},
    },
    response::{Html, IntoResponse},
};
use sha2::{Digest, Sha256};

pub(crate) const BUILD_REVISION: &str = match option_env!("SPROYT_BUILD_REVISION") {
    Some(revision) => revision,
    None => "unknown",
};
pub(crate) const PWA_MANIFEST: &str = include_str!("../../assets/manifest.webmanifest");
pub(crate) const APP_BUNDLE: &str = include_str!(concat!(env!("OUT_DIR"), "/app.js"));
#[cfg(test)]
pub(crate) const APP_SOURCE: &str = include_str!("../../frontend/src/app.ts");
#[cfg(test)]
pub(crate) const CONNECTION_SOURCE: &str = include_str!("../../frontend/src/connection.ts");
#[cfg(test)]
pub(crate) const SESSION_SOURCE: &str = include_str!("../../frontend/src/session.ts");
#[cfg(test)]
pub(crate) const NAVIGATION_SOURCE: &str = include_str!("../../frontend/src/navigation.ts");
// Compatibility endpoint for already-open pages during the app bundle rollout.
pub(crate) const CLIENT_STORE: &str = include_str!(concat!(env!("OUT_DIR"), "/client-store.js"));
pub(crate) const SERVICE_WORKER: &str = include_str!("../../assets/service-worker.js");
pub(crate) const OFFLINE_HTML: &str = include_str!("../../assets/offline.html");
pub(crate) const INDEX_HTML: &str = include_str!("../../assets/index.html");
const WAVE_LOGO_SVG: &str = include_str!("../../assets/sproyt-wave.svg");
pub(crate) const WAVE_LOGO_192: &[u8] = include_bytes!("../../assets/sproyt-wave-192.png");
pub(crate) const WAVE_LOGO_512: &[u8] = include_bytes!("../../assets/sproyt-wave-512.png");

pub(crate) async fn pwa_manifest() -> axum::response::Response {
    (
        [
            (CONTENT_TYPE, "application/manifest+json"),
            (CACHE_CONTROL, "public, max-age=3600"),
        ],
        PWA_MANIFEST,
    )
        .into_response()
}

pub(crate) fn asset_fingerprint(build_revision: &str, asset: &[u8]) -> String {
    let revision_is_safe = (7..=64).contains(&build_revision.len())
        && build_revision
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'));
    if revision_is_safe {
        return build_revision.to_owned();
    }
    format!("{:x}", Sha256::digest(asset))
}

pub(crate) fn client_store_fingerprint(build_revision: &str, client_store: &[u8]) -> String {
    asset_fingerprint(build_revision, client_store)
}

pub(crate) fn app_bundle_fingerprint(build_revision: &str, app_bundle: &[u8]) -> String {
    asset_fingerprint(build_revision, app_bundle)
}

pub(crate) async fn client_store_legacy() -> axum::response::Response {
    (
        [
            (CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (CACHE_CONTROL, "no-cache"),
        ],
        CLIENT_STORE,
    )
        .into_response()
}

pub(crate) async fn client_store(Path(fingerprint): Path<String>) -> axum::response::Response {
    if fingerprint != client_store_fingerprint(BUILD_REVISION, CLIENT_STORE.as_bytes()) {
        return axum::http::StatusCode::NOT_FOUND.into_response();
    }
    (
        [
            (CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (CACHE_CONTROL, "public, max-age=31536000, immutable"),
        ],
        CLIENT_STORE,
    )
        .into_response()
}

pub(crate) async fn app_bundle(Path(fingerprint): Path<String>) -> axum::response::Response {
    if fingerprint != app_bundle_fingerprint(BUILD_REVISION, APP_BUNDLE.as_bytes()) {
        return axum::http::StatusCode::NOT_FOUND.into_response();
    }
    (
        [
            (CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (CACHE_CONTROL, "public, max-age=31536000, immutable"),
        ],
        APP_BUNDLE,
    )
        .into_response()
}

pub(crate) async fn service_worker() -> axum::response::Response {
    (
        [
            (CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (CACHE_CONTROL, "no-cache"),
            (HeaderName::from_static("service-worker-allowed"), "/"),
            (
                HeaderName::from_static("content-security-policy"),
                "default-src 'none'; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'",
            ),
        ],
        SERVICE_WORKER,
    )
        .into_response()
}

pub(crate) async fn offline_page() -> axum::response::Response {
    let mut response = Html(OFFLINE_HTML).into_response();
    response.headers_mut().insert(
        HeaderName::from_static("content-security-policy"),
        HeaderValue::from_static("default-src 'self'; img-src 'self'; style-src 'unsafe-inline'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'"),
    );
    response
}

pub(crate) async fn wave_logo_svg() -> axum::response::Response {
    static_asset("image/svg+xml", WAVE_LOGO_SVG.as_bytes())
}

pub(crate) async fn wave_logo_192() -> axum::response::Response {
    static_asset("image/png", WAVE_LOGO_192)
}

pub(crate) async fn wave_logo_512() -> axum::response::Response {
    static_asset("image/png", WAVE_LOGO_512)
}

fn static_asset(content_type: &'static str, content: &'static [u8]) -> axum::response::Response {
    (
        [
            (CONTENT_TYPE, content_type),
            (CACHE_CONTROL, "public, max-age=604800, immutable"),
        ],
        content,
    )
        .into_response()
}
