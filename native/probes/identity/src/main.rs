mod authconfig;
mod provider;
mod sessions;
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
    external: bool,
    session_lab: bool,
    upstream: authconfig::Sessions,
    control: tokio::sync::OnceCell<authconfig::Control>,
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
    let mut interactive = false;
    let mut external_config = None;
    let mut session_lab = false;
    let mut listen_port = None;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--interactive" if !interactive => interactive = true,
            "--session-lab" if !session_lab => session_lab = true,
            "--listen-port" if listen_port.is_none() => {
                let port = args.next().ok_or(())?.parse::<u16>().map_err(|_| ())?;
                if port == 0 {
                    return Err(());
                }
                listen_port = Some(port);
            }
            "--authentik-config" if external_config.is_none() => {
                external_config = Some(authconfig::Config::read(&PathBuf::from(
                    args.next().ok_or(())?,
                ))?);
            }
            _ => return Err(()),
        }
    }
    if (session_lab && external_config.is_some()) || (listen_port.is_some() && !session_lab) {
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
    let listener = tokio::net::TcpListener::bind((
        std::net::Ipv4Addr::LOCALHOST,
        external_config
            .as_ref()
            .map_or(listen_port.unwrap_or(0), |config| config.listen_port),
    ))
    .await
    .map_err(|_| ())?;
    let address = format!("http://{}", listener.local_addr().map_err(|_| ())?);
    let issuer = external_config.as_ref().map_or_else(
        || format!("{address}/provider"),
        |config| config.issuer.clone(),
    );
    let http = reqwest::Client::builder()
        .resolve(
            "localhost",
            std::net::SocketAddr::from(([127, 0, 0, 1], 19090)),
        )
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|_| ())?;
    let store = if session_lab {
        store::Store::open_session_lab(&database, interactive, &address, &issuer)?
    } else {
        store::Store::create(&database, interactive)?
    };
    let instance_id = store
        .instance_id()
        .map(str::to_owned)
        .unwrap_or(store::random()?);
    let app = Arc::new(App {
        instance_id,
        interactive,
        external: external_config.is_some(),
        session_lab,
        upstream: tokio::sync::Mutex::new(std::collections::HashMap::new()),
        control: tokio::sync::OnceCell::new(),
        oidc_callback: format!("{address}/probe/callback"),
        address: address.clone(),
        issuer: issuer.clone(),
        callback_uri,
        store: Mutex::new(store),
        provider: provider::Provider::new()?,
        client: tokio::sync::OnceCell::new(),
        http,
    });
    let mut routes = Router::new();
    if external_config.is_none() {
        routes = routes
            .route("/provider/choose", post(provider::choose))
            .route(
                "/provider/.well-known/openid-configuration",
                get(provider::metadata),
            )
            .route("/provider/jwks", get(provider::jwks))
            .route("/provider/authorize", get(provider::authorize))
            .route("/provider/token", post(provider::token))
            .route("/fixture/expire", post(expire));
    }
    if session_lab {
        routes = routes
            .route("/session/info", get(session_info))
            .route("/session/probe", get(session_probe))
            .route("/session/refresh", post(session_refresh))
            .route("/session/revoke", post(session_revoke))
            .route("/session/cancel-login", post(session_cancel_login))
            .route("/fixture/session/epoch", post(session_epoch))
            .route("/fixture/session/expire", post(session_expire));
    }
    let routes = routes
        .route("/probe/start", post(start))
        .route("/probe/browser/{id}", get(browser))
        .route("/probe/callback", get(callback_handler))
        .route("/probe/exchange", post(exchange))
        .route("/probe/status", get(status))
        .route("/probe/upstream-check", post(upstream_check))
        .fallback(|| async { ProbeError })
        .layer(DefaultBodyLimit::max(8192))
        .layer(axum::middleware::map_response(private_response))
        .with_state(app.clone());
    let server = tokio::spawn(async move { axum::serve(listener, routes).await });
    let (metadata, client_id, secret) = if let Some(config) = &external_config {
        (
            config.metadata(&app.http).await?,
            config.client_id.clone(),
            config.secret()?,
        )
    } else {
        (
            CoreProviderMetadata::discover_async(
                IssuerUrl::new(issuer.clone()).map_err(|_| ())?,
                &app.http,
            )
            .await
            .map_err(|_| ())?,
            "n01-broker".into(),
            provider::SECRET.into(),
        )
    };
    if let Some(config) = &external_config {
        app.control
            .set(authconfig::Control::new(config, &secret)?)
            .map_err(|_| ())?;
    }
    let client = CoreClient::from_provider_metadata(
        metadata,
        ClientId::new(client_id),
        Some(ClientSecret::new(secret)),
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
    let authorization = client
        .authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            || CsrfToken::new(attempt.state),
            || Nonce::new(attempt.nonce),
        )
        .set_pkce_challenge(PkceCodeChallenge::from_code_verifier_sha256(&verifier));
    let authorization = if app.external {
        authorization.add_scope(openidconnect::Scope::new("offline_access".into()))
    } else {
        authorization
    };
    let (url, _, _) = authorization.url();
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
    if if app.external {
        input.code.is_empty() || input.code.len() > 2048 || input.code.chars().any(char::is_control)
    } else {
        !store::valid_challenge(&input.code)
    } {
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
    if app.external {
        app.upstream
            .lock()
            .await
            .insert((attempt.id.clone(), user), authconfig::session(&token));
    }
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
        json!({"attempts":attempts,"session_families":session_families,"instance_id":app.instance_id,"issuer":app.issuer}),
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

async fn upstream_check(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    input: std::result::Result<Json<authconfig::Check>, axum::extract::rejection::JsonRejection>,
) -> std::result::Result<Json<serde_json::Value>, ProbeError> {
    if headers
        .get("x-probe-control")
        .and_then(|value| value.to_str().ok())
        != Some("1")
    {
        return Err(ProbeError);
    }
    let Json(input) = input.map_err(|_| ProbeError)?;
    Ok(Json(authconfig::check(&app, input).await?))
}

fn session_denied() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"error":"probe_rejected"})),
    )
        .into_response()
}
fn session_bearer(headers: &HeaderMap) -> store::Result<&str> {
    if headers.get_all(header::AUTHORIZATION).iter().count() != 1 {
        return Err(());
    }
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty() && !value.chars().any(char::is_whitespace))
        .ok_or(())
}
async fn session_info(State(app): State<Arc<App>>) -> Response {
    if !app.session_lab {
        return session_denied();
    }
    Json(json!({"scope":sessions::SCOPE,"environment":store::ENVIRONMENT,"instance_id":app.instance_id,"issuer":app.issuer})).into_response()
}
async fn session_probe(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let result = (|| {
        let token = session_bearer(&headers)?;
        app.store.lock().map_err(|_| ())?.session_probe(token)
    })();
    match result {
        Ok(claims) => Json(claims).into_response(),
        Err(()) => session_denied(),
    }
}
async fn session_refresh(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    input: std::result::Result<Json<sessions::Refresh>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let result = (|| {
        let Json(input) = input.map_err(|_| ())?;
        let token = session_bearer(&headers)?;
        app.store
            .lock()
            .map_err(|_| ())?
            .session_refresh(token, &input)
    })();
    match result {
        Ok(grant)
            if headers
                .get("x-probe-drop-ack")
                .and_then(|value| value.to_str().ok())
                == Some("1") =>
        {
            drop(grant); // The rotation is already durably committed: deliberately lose the result.
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error":"probe_rejected"})),
            )
                .into_response()
        }
        Ok(grant) => Json(grant).into_response(),
        Err(()) => session_denied(),
    }
}
async fn session_revoke(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let result = (|| {
        if !body.is_empty() {
            return Err(());
        }
        let token = session_bearer(&headers)?;
        app.store.lock().map_err(|_| ())?.session_revoke(token)
    })();
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(()) => session_denied(),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionEpoch {
    user_id: String,
}
async fn session_epoch(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    input: std::result::Result<Json<SessionEpoch>, axum::extract::rejection::JsonRejection>,
) -> std::result::Result<Json<serde_json::Value>, ProbeError> {
    if headers
        .get("x-probe-control")
        .and_then(|value| value.to_str().ok())
        != Some("1")
    {
        return Err(ProbeError);
    }
    let Json(input) = input.map_err(|_| ProbeError)?;
    let epoch = app
        .store
        .lock()
        .map_err(|_| ProbeError)?
        .session_epoch(&input.user_id)?;
    Ok(Json(json!({"epoch":epoch})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionExpire {
    family: String,
    scope: String,
}
async fn session_expire(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    input: std::result::Result<Json<SessionExpire>, axum::extract::rejection::JsonRejection>,
) -> std::result::Result<Json<serde_json::Value>, ProbeError> {
    if headers
        .get("x-probe-control")
        .and_then(|value| value.to_str().ok())
        != Some("1")
    {
        return Err(ProbeError);
    }
    let Json(input) = input.map_err(|_| ProbeError)?;
    app.store
        .lock()
        .map_err(|_| ProbeError)?
        .session_expire(&input.family, &input.scope)?;
    Ok(Json(json!({"ok":true})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CancelLogin {
    transaction_id: String,
    verifier: String,
    environment: String,
    destination_id: String,
}
async fn session_cancel_login(
    State(app): State<Arc<App>>,
    input: std::result::Result<Json<CancelLogin>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let result = (|| {
        let Json(input) = input.map_err(|_| ())?;
        app.store.lock().map_err(|_| ())?.session_cancel_login(
            &input.transaction_id,
            &input.verifier,
            &input.environment,
            &input.destination_id,
        )
    })();
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(()) => session_denied(),
    }
}
