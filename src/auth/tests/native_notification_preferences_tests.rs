//! HTTP admission against the real native-session validator and shared SQLite
//! notification store, using the parent module's synthetic OIDC provider.
use super::*;
use crate::{
    config::DatabaseConfig,
    native_sessions::{ExchangeRequest, Grant, LoginRequest, NativeSessions},
    notification::{NotificationMode, NotificationPreferences, NotificationService},
};
use axum::http::StatusCode;
use serde_json::{Value, json};

const PATH: &str = "/api/v1/native/me/notifications/preferences";

struct Harness {
    origin: String,
    state: crate::server::AppState,
    native_pool: sqlx::SqlitePool,
    grant: Grant,
    browser_cookie: String,
    other: crate::domain::UserId,
    server: tokio::task::JoinHandle<()>,
    provider_server: tokio::task::JoinHandle<()>,
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.server.abort();
        self.provider_server.abort();
    }
}

impl Harness {
    async fn start() -> Self {
        let (config, provider, provider_server) = test_oidc_provider().await;
        let auth = AuthService::oidc(&config).await.unwrap();
        let login = auth.login(None).unwrap();
        *provider.nonce.lock().unwrap() =
            authorization_parameter(&login.authorization_url, "nonce");
        let browser = auth
            .callback(
                "valid-code".into(),
                authorization_parameter(&login.authorization_url, "state"),
                Some(&login.set_cookie),
            )
            .await
            .unwrap();
        let native_pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let native =
            NativeSessions::for_test(native_pool.clone(), config.issuer(), config.session_key())
                .await
                .unwrap();
        let verifier = URL_SAFE_NO_PAD.encode([42_u8; 32]);
        let started = native
            .login(
                &auth,
                LoginRequest {
                    challenge: URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())),
                    destination_id: "windows-loopback".into(),
                    callback_port: 19193,
                    expected_user_id: None,
                },
            )
            .await
            .unwrap();
        let (upstream, cookie) = native.browser(&started.transaction_id).await.unwrap();
        *provider.nonce.lock().unwrap() = authorization_parameter(&upstream, "nonce");
        let confirmation = native
            .callback(
                &auth,
                "native-code".into(),
                authorization_parameter(&upstream, "state"),
                Some(&cookie),
            )
            .await
            .unwrap();
        let target = native
            .confirm(&started.transaction_id, &confirmation.nonce, Some(&cookie))
            .await
            .unwrap();
        let code = reqwest::Url::parse(&target)
            .unwrap()
            .query_pairs()
            .find(|(name, _)| name == "code")
            .unwrap()
            .1
            .into_owned();
        let grant = native
            .exchange(ExchangeRequest {
                transaction_id: started.transaction_id,
                code,
                verifier,
                redemption_id: "d".repeat(32),
                environment: "canary".into(),
                destination_id: "windows-loopback".into(),
            })
            .await
            .unwrap()
            .native_session;

        // Isolated synthetic database retained in the build's fixture directory.
        // A real migrated store makes GET/PUT/browser read-back test the same
        // persisted rows rather than mocking the authorization or domain layer.
        let fixtures = std::env::var_os("CARGO_TARGET_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("native-http-fixtures");
        std::fs::create_dir_all(&fixtures).unwrap();
        let database = fixtures.join(format!("{}.sqlite", uuid::Uuid::now_v7()));
        let url = format!("sqlite://{}", database.to_string_lossy().replace('\\', "/"));
        let repository = Arc::new(
            crate::db::SqliteChatRepository::connect(&url)
                .await
                .unwrap(),
        );
        repository.migrate().await.unwrap();
        let notifications =
            NotificationService::connect(&DatabaseConfig::new(url.clone()).unwrap(), None)
                .await
                .unwrap();
        let mut state = crate::web::native::test_state(auth, Some(native)).await;
        state.chat = crate::chat::ChatEngine::start(repository);
        state.notifications = notifications;
        // Do not pre-materialize the native/browser account. Its first action
        // is a preference PUT before any chat/WS connection has created users.
        let other = principal(config.issuer(), "other-user", "Other User", None, None).unwrap();
        state.chat.ensure_user(other.user.clone()).await.unwrap();
        state
            .notifications
            .save_preferences(
                other.user.id.clone(),
                NotificationPreferences {
                    mode: NotificationMode::Muted,
                    direct_messages: false,
                    mentions: false,
                    weekly_weekday: 7,
                },
            )
            .await
            .unwrap();
        let check = sqlx::SqlitePool::connect(&url).await.unwrap();
        let materialized: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE id=$1")
            .bind(&grant.user_id)
            .fetch_one(&check)
            .await
            .unwrap();
        assert_eq!(
            materialized, 0,
            "the first native PUT must precede chat setup"
        );
        check.close().await;
        let app = crate::server::build_router(state.clone(), state.operations.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            origin,
            state,
            native_pool,
            grant,
            browser_cookie: browser.set_cookie,
            other: other.user.id,
            server,
            provider_server,
        }
    }

    fn request(&self, method: reqwest::Method) -> reqwest::RequestBuilder {
        reqwest::Client::new()
            .request(method, format!("{}{PATH}", self.origin))
            .bearer_auth(&self.grant.access_token)
            .header("x-sproyt-account", &self.grant.user_id)
    }
}

fn changed_preferences() -> Value {
    json!({"mode":"weekly","direct_messages":false,"mentions":true,"weekly_weekday":3})
}

#[tokio::test]
async fn native_notification_preferences_share_browser_data_and_reject_mixed_authority() {
    let harness = Harness::start().await;
    let client = reqwest::Client::new();
    let initial = harness.request(reqwest::Method::GET).send().await.unwrap();
    assert_eq!(initial.status(), StatusCode::OK);
    assert_eq!(initial.headers()["cache-control"], "private, no-store");
    assert_eq!(initial.headers()["x-sproyt-account"], harness.grant.user_id);
    assert!(!initial.headers().contains_key("set-cookie"));
    let initial: Value = initial.json().await.unwrap();
    assert_eq!(initial["mode"], "instant");
    assert!(initial.get("public_key").is_none());
    let saved = harness
        .request(reqwest::Method::PUT)
        .json(&changed_preferences())
        .send()
        .await
        .unwrap();
    assert_eq!(saved.status(), StatusCode::OK);
    assert_eq!(saved.json::<Value>().await.unwrap(), changed_preferences());
    let browser = client
        .get(format!("{}/api/v1/me/notifications", harness.origin))
        .header("cookie", &harness.browser_cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(browser.status(), StatusCode::OK);
    assert_eq!(
        browser.json::<Value>().await.unwrap()["preferences"],
        changed_preferences()
    );

    // The existing browser route retains cookie authority. A native bearer
    // alone does not acquire admission to this or other browser REST handlers.
    let denied_browser = client
        .put(format!("{}/api/v1/me/notifications", harness.origin))
        .bearer_auth(&harness.grant.access_token)
        .json(&changed_preferences())
        .send()
        .await
        .unwrap();
    assert_eq!(denied_browser.status(), StatusCode::UNAUTHORIZED);
    let browser_saved = client
        .put(format!("{}/api/v1/me/notifications", harness.origin))
        .header("cookie", &harness.browser_cookie)
        .json(&json!({"mode":"instant","direct_messages":true,"mentions":false,"weekly_weekday":6}))
        .send()
        .await
        .unwrap();
    assert_eq!(browser_saved.status(), StatusCode::OK);
    let browser_preferences: Value = browser_saved.json().await.unwrap();
    assert_eq!(
        harness
            .request(reqwest::Method::GET)
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap(),
        browser_preferences
    );

    // Both reads and writes fail closed for mixed or wrong token authority.
    for method in [reqwest::Method::GET, reqwest::Method::PUT] {
        for token in [
            "",
            "Bearer sproyt-refresh.invalid",
            "Bearer agent.invalid",
            "Basic invalid",
        ] {
            let mut request = client
                .request(method.clone(), format!("{}{PATH}", harness.origin))
                .header("x-sproyt-account", &harness.grant.user_id);
            if !token.is_empty() {
                request = request.header("authorization", token);
            }
            assert_eq!(
                request
                    .json(&changed_preferences())
                    .send()
                    .await
                    .unwrap()
                    .status(),
                StatusCode::UNAUTHORIZED
            );
        }
        for token in [&harness.grant.refresh_token, &harness.grant.revoke_token] {
            let response = client
                .request(method.clone(), format!("{}{PATH}", harness.origin))
                .bearer_auth(token)
                .header("x-sproyt-account", &harness.grant.user_id)
                .json(&changed_preferences())
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
        let mixed = harness
            .request(method.clone())
            .header("cookie", &harness.browser_cookie)
            .json(&changed_preferences())
            .send()
            .await
            .unwrap();
        assert_eq!(mixed.status(), StatusCode::UNAUTHORIZED);
        let duplicate = harness
            .request(method.clone())
            .header("authorization", "Bearer second")
            .json(&changed_preferences())
            .send()
            .await
            .unwrap();
        assert_eq!(duplicate.status(), StatusCode::UNAUTHORIZED);
        let cookie_only = client
            .request(method.clone(), format!("{}{PATH}", harness.origin))
            .header("cookie", &harness.browser_cookie)
            .header("x-sproyt-account", &harness.grant.user_id)
            .json(&changed_preferences())
            .send()
            .await
            .unwrap();
        assert_eq!(cookie_only.status(), StatusCode::UNAUTHORIZED);
        for query in [
            "?participant=other-user",
            "?participant=",
            "?user_id=other-user",
            "?unrecognised=1",
        ] {
            let response = client
                .request(method.clone(), format!("{}{PATH}{query}", harness.origin))
                .bearer_auth(&harness.grant.access_token)
                .header("x-sproyt-account", &harness.grant.user_id)
                .json(&changed_preferences())
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
        for header in [None, Some(harness.other.to_string())] {
            let mut request = client
                .request(method.clone(), format!("{}{PATH}", harness.origin))
                .bearer_auth(&harness.grant.access_token);
            if let Some(header) = header {
                request = request.header("x-sproyt-account", header);
            }
            assert_eq!(
                request
                    .json(&changed_preferences())
                    .send()
                    .await
                    .unwrap()
                    .status(),
                StatusCode::CONFLICT
            );
        }
        let duplicate = harness
            .request(method.clone())
            .header("x-sproyt-account", &harness.grant.user_id)
            .json(&changed_preferences())
            .send()
            .await
            .unwrap();
        assert_eq!(duplicate.status(), StatusCode::CONFLICT);
        for (header, value) in [
            ("origin", "https://untrusted.example"),
            ("sec-fetch-site", "cross-site"),
        ] {
            let denied = harness
                .request(method.clone())
                .header(header, value)
                .json(&changed_preferences())
                .send()
                .await
                .unwrap();
            assert_eq!(denied.status(), StatusCode::FORBIDDEN);
        }
    }
    for input in [
        json!({"mode":"unknown","direct_messages":true,"mentions":true,"weekly_weekday":3}),
        json!({"mode":"weekly","direct_messages":true,"mentions":true,"weekly_weekday":3,"user_id":harness.other.to_string()}),
        json!({"mode":"weekly"}),
    ] {
        assert_eq!(
            harness
                .request(reqwest::Method::PUT)
                .json(&input)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        harness
            .request(reqwest::Method::PUT)
            .json(
                &json!({"mode":"weekly","direct_messages":true,"mentions":true,"weekly_weekday":0})
            )
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        harness
            .request(reqwest::Method::PUT)
            .body("x".repeat(8193))
            .header("content-type", "application/json")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let unchanged: Value = harness
        .request(reqwest::Method::GET)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(unchanged, browser_preferences);
    let other = harness
        .state
        .notifications
        .settings(harness.other.clone())
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(other.preferences).unwrap(),
        json!({"mode":"muted","direct_messages":false,"mentions":false,"weekly_weekday":7})
    );

    // Expiry is enforced on every HTTP operation, not just session bootstrap.
    sqlx::query("UPDATE native_families SET expires_at='0' WHERE family=$1")
        .bind(&harness.grant.family)
        .execute(&harness.native_pool)
        .await
        .unwrap();
    for method in [reqwest::Method::GET, reqwest::Method::PUT] {
        assert_eq!(
            harness
                .request(method)
                .json(&changed_preferences())
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
}

#[tokio::test]
async fn native_notification_preferences_revoke_and_disabled_routes_fail_closed() {
    let harness = Harness::start().await;
    harness
        .state
        .native_sessions
        .as_ref()
        .unwrap()
        .revoke(&harness.grant.revoke_token)
        .await
        .unwrap();
    for method in [reqwest::Method::GET, reqwest::Method::PUT] {
        assert_eq!(
            harness
                .request(method.clone())
                .json(&changed_preferences())
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    // Revoking the dedicated native family must not revoke the browser grant.
    let browser = reqwest::Client::new()
        .get(format!("{}/api/v1/me/notifications", harness.origin))
        .header("cookie", &harness.browser_cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(browser.status(), StatusCode::OK);

    let state = crate::web::native::test_state(AuthService::development(), None).await;
    let app = crate::web::native::routes().with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    for method in [reqwest::Method::GET, reqwest::Method::PUT] {
        let response = reqwest::Client::new()
            .request(method, format!("http://{address}{PATH}"))
            .bearer_auth(&harness.grant.access_token)
            .header("x-sproyt-account", &harness.grant.user_id)
            .json(&changed_preferences())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    server.abort();
}
