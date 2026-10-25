mod provider;
mod store;

use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use openidconnect::{
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointMaybeSet, EndpointNotSet,
    EndpointSet, IssuerUrl, Nonce, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, TokenResponse,
    core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata},
};
use serde::Deserialize;
use serde_json::json;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

type Oidc = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;
pub struct App {
    instance_id: String,
    interactive: bool,
    address: String,
    issuer: String,
    oidc_callback: String,
    callback_uri: String,
    store: Mutex<store::Store>,
    provider: provider::Provider,
    client: tokio::sync::OnceCell<Oidc>,
    http: reqwest::Client,
}

pub struct ProbeError;
impl IntoResponse for ProbeError {
    fn into_response(self) -> Response {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"probe_rejected"})),
        )
            .into_response()
    }
}
impl From<()> for ProbeError {
    fn from(_: ()) -> Self {
        Self
    }
}

pub fn redirect(url: &str) -> std::result::Result<Response, ProbeError> {
    let mut response = StatusCode::FOUND.into_response();
    response.headers_mut().insert(
        header::LOCATION,
        HeaderValue::from_str(url).map_err(|_| ProbeError)?,
    );
    Ok(response)
}

struct CallbackClaim {
    app: Arc<App>,
    id: String,
}
impl Drop for CallbackClaim {
    fn drop(&mut self) {
        // Release only our in-flight phase on failure/cancellation. Successful
        // callbacks have already moved to ready, so the conditional UPDATE is
        // a no-op. Another callback cannot claim this attempt in the meantime.
        if let Ok(mut store) = self.app.store.lock() {
            let _ = store.release_callback(&self.id);
        }
    }
}

#[tokio::main]
async fn main() {
    if run().await.is_err() {
        eprintln!("FAIL isolated identity server");
        std::process::exit(1);
    }
}

async fn run() -> store::Result<()> {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some("--database") {
        return Err(());
    }
    let database = PathBuf::from(args.next().ok_or(())?);
    if args.next().as_deref() != Some("--callback-uri") {
        return Err(());
    }
    let callback_uri = args.next().ok_or(())?;
    let interactive = match args.next().as_deref() {
        None => false,
        Some("--interactive") => true,
        _ => return Err(()),
    };
    if args.next().is_some() {
        return Err(());
    }
    let callback = reqwest::Url::parse(&callback_uri).map_err(|_| ())?;
    if callback.scheme() != "http"
        || callback.host_str() != Some("127.0.0.1")
        || callback.port().is_none_or(|v| v == 0)
        || callback.path() != "/return"
        || !callback.username().is_empty()
        || callback.password().is_some()
        || callback.query().is_some()
        || callback.fragment().is_some()
        || callback.as_str() != callback_uri
    {
        return Err(());
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|_| ())?;
    let address = format!("http://{}", listener.local_addr().map_err(|_| ())?);
    let issuer = format!("{address}/provider");
    let http = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|_| ())?;
    let app = Arc::new(App {
        instance_id: store::random()?,
        interactive,
        oidc_callback: format!("{address}/probe/callback"),
        address: address.clone(),
        issuer: issuer.clone(),
        callback_uri,
        store: Mutex::new(store::Store::create(&database, interactive)?),
        provider: provider::Provider::new()?,
        client: tokio::sync::OnceCell::new(),
        http,
    });
    let routes = Router::new()
        .route("/provider/choose", post(provider::choose))
        .route(
            "/provider/.well-known/openid-configuration",
            get(provider::metadata),
        )
        .route("/provider/jwks", get(provider::jwks))
        .route("/provider/authorize", get(provider::authorize))
        .route("/provider/token", post(provider::token))
        .route("/probe/start", post(start))
        .route("/probe/browser/{id}", get(browser))
        .route("/probe/callback", get(callback_handler))
        .route("/probe/exchange", post(exchange))
        .route("/probe/status", get(status))
        .route("/fixture/expire", post(expire))
        .fallback(|| async { ProbeError })
        .layer(DefaultBodyLimit::max(8192))
        .layer(axum::middleware::map_response(private_response))
        .with_state(app.clone());
    let server = tokio::spawn(async move { axum::serve(listener, routes).await });
    let metadata = CoreProviderMetadata::discover_async(
        IssuerUrl::new(issuer.clone()).map_err(|_| ())?,
        &app.http,
    )
    .await
    .map_err(|_| ())?;
    let client = CoreClient::from_provider_metadata(
        metadata,
        ClientId::new("n01-broker".into()),
        Some(ClientSecret::new(provider::SECRET.into())),
    )
    .set_redirect_uri(RedirectUrl::new(app.oidc_callback.clone()).map_err(|_| ())?);
    app.client.set(client).map_err(|_| ())?;
    println!(
        "N01_READY {}",
        json!({"address":address,"issuer":issuer,"instance_id":app.instance_id})
    );
    server.await.map_err(|_| ())?.map_err(|_| ())
}

async fn private_response(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .entry("referrer-policy")
        .or_insert(HeaderValue::from_static("no-referrer"));
    response
}

async fn start(
    State(app): State<Arc<App>>,
    input: std::result::Result<Json<store::Start>, axum::extract::rejection::JsonRejection>,
) -> std::result::Result<Json<serde_json::Value>, ProbeError> {
    let Json(input) = input.map_err(|_| ProbeError)?;
    let id = app.store.lock().map_err(|_| ProbeError)?.start(&input)?;
    Ok(Json(
        json!({"transaction_id":id,"authorization_url":format!("{}/probe/browser/{id}",app.address)}),
    ))
}

async fn browser(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> std::result::Result<Response, ProbeError> {
    let attempt = app
        .store
        .lock()
        .map_err(|_| ProbeError)?
        .begin_browser(&id)?;
    let client = app.client.get().ok_or(ProbeError)?;
    let verifier = PkceCodeVerifier::new(attempt.verifier);
    let (url, _, _) = client
        .authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            || CsrfToken::new(attempt.state),
            || Nonce::new(attempt.nonce),
        )
        .set_pkce_challenge(PkceCodeChallenge::from_code_verifier_sha256(&verifier))
        .url();
    let mut response = redirect(url.as_str())?;
    // HTTP and non-Secure cookies are exclusively for this loopback fixture.
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&format!(
            "probe_{}={}; Path=/probe/callback; HttpOnly; SameSite=Lax; Max-Age={}",
            attempt.id,
            attempt.cookie,
            if app.interactive { 600 } else { 120 }
        ))
        .map_err(|_| ProbeError)?,
    );
    Ok(response)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Callback {
    code: String,
    state: String,
}

async fn callback_handler(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    input: std::result::Result<Query<Callback>, axum::extract::rejection::QueryRejection>,
) -> std::result::Result<Response, ProbeError> {
    let Query(input) = input.map_err(|_| ProbeError)?;
    if !store::valid_challenge(&input.code) {
        return Err(ProbeError);
    }
    let cookie = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .ok_or(ProbeError)?;
    let attempt = app
        .store
        .lock()
        .map_err(|_| ProbeError)?
        .claim_callback(&input.state, cookie)?;
    let _claim = CallbackClaim {
        app: app.clone(),
        id: attempt.id.clone(),
    };
    let client = app.client.get().ok_or(ProbeError)?;
    let token = client
        .exchange_code(AuthorizationCode::new(input.code))
        .map_err(|_| ProbeError)?
        .set_pkce_verifier(PkceCodeVerifier::new(attempt.verifier))
        .request_async(&app.http)
        .await
        .map_err(|_| ProbeError)?;
    let assertion = token.id_token().ok_or(ProbeError)?;
    let nonce = Nonce::new(attempt.nonce);
    let verifier = client.id_token_verifier();
    let claims = assertion
        .claims(&verifier, &nonce)
        .map_err(|_| ProbeError)?;
    let user =
        sproyt_protocol::UserId::from_oidc(&app.issuer, claims.subject().as_str()).to_string();
    let code = app
        .store
        .lock()
        .map_err(|_| ProbeError)?
        .complete_callback(&attempt.id, &user)?;
    let mut target = reqwest::Url::parse(&app.callback_uri).map_err(|_| ProbeError)?;
    target
        .query_pairs_mut()
        .append_pair("transaction_id", &attempt.id)
        .append_pair("code", &code);
    let mut response = redirect(target.as_str())?;
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&format!(
            "probe_{}=; Path=/probe/callback; HttpOnly; SameSite=Lax; Max-Age=0",
            attempt.id
        ))
        .map_err(|_| ProbeError)?,
    );
    Ok(response)
}

async fn exchange(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    input: std::result::Result<Json<store::Exchange>, axum::extract::rejection::JsonRejection>,
) -> std::result::Result<Response, ProbeError> {
    let Json(input) = input.map_err(|_| ProbeError)?;
    let receipt = app.store.lock().map_err(|_| ProbeError)?.exchange(&input)?;
    if headers
        .get("x-probe-drop-ack")
        .and_then(|v| v.to_str().ok())
        == Some("1")
    {
        // The SQLite commit above happened. Fail body delivery, never rollback
        // or create a replacement family when the client retries the same ID.
        let body = Body::from_stream(futures_failure());
        return Ok((
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            body,
        )
            .into_response());
    }
    Ok(Json(receipt).into_response())
}

fn futures_failure()
-> impl futures_util::Stream<Item = std::result::Result<axum::body::Bytes, std::io::Error>> {
    futures_util::stream::once(async {
        Err(std::io::Error::other(
            "synthetic acknowledgement delivery failure",
        ))
    })
}

async fn status(
    State(app): State<Arc<App>>,
) -> std::result::Result<Json<serde_json::Value>, ProbeError> {
    let (attempts, session_families) = app.store.lock().map_err(|_| ProbeError)?.status()?;
    Ok(Json(
        json!({"attempts":attempts,"session_families":session_families,"instance_id":app.instance_id}),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Expire {
    transaction_id: String,
    scope: String,
}

async fn expire(
    State(app): State<Arc<App>>,
    input: std::result::Result<Json<Expire>, axum::extract::rejection::JsonRejection>,
) -> std::result::Result<Json<serde_json::Value>, ProbeError> {
    let Json(input) = input.map_err(|_| ProbeError)?;
    app.store
        .lock()
        .map_err(|_| ProbeError)?
        .expire(&input.transaction_id, &input.scope)?;
    Ok(Json(json!({"ok":true})))
}
