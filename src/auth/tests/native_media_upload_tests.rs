//! Real HTTP/native-grant admission and shared migrated SQLite upload contract.
use super::*;
use crate::{
    domain::{ChannelId, ChannelKind, ChannelSlug, DisplayName, UserId},
    media_store::{CHUNK_BYTES, NewUpload, UploadLimits},
    native_sessions::{ExchangeRequest, Grant, LoginRequest, NativeSessions},
};
use axum::http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;
const CONFIG: &str = "/api/v1/native/media-uploads/config";
struct Harness {
    origin: String,
    state: crate::server::AppState,
    native_pool: sqlx::SqlitePool,
    data_pool: sqlx::SqlitePool,
    grant: Grant,
    browser_cookie: String,
    channel: ChannelId,
    other_channel: ChannelId,
    other: UserId,
    server: tokio::task::JoinHandle<()>,
    provider_server: tokio::task::JoinHandle<()>,
}

static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn native_media_upload_lifecycle_shares_browser_storage_and_preserves_bounds() {
    let _serial = SERIAL.lock().await;
    let h = Harness::start().await;
    let config = h
        .request(reqwest::Method::GET, CONFIG)
        .send()
        .await
        .unwrap();
    assert_eq!(config.status(), StatusCode::OK);
    assert_eq!(config.headers()["x-sproyt-account"], h.grant.user_id);
    assert_eq!(config.headers()["cache-control"], "private, no-store");
    let config: Value = config.json().await.unwrap();
    assert_eq!(config["chunk_bytes"], CHUNK_BYTES);
    assert_eq!(config["enabled"], true);
    let create_path = format!("/api/v1/native/channels/{}/media-uploads", h.channel);
    let mut bytes = vec![7; CHUNK_BYTES + 13];
    bytes[..3].copy_from_slice(&[0xff, 0xd8, 0xff]);
    let input = json!({"request_id":Uuid::now_v7(),"filename":"original.jpg","content_type":"image/jpeg","size_bytes":bytes.len()});
    let first = h
        .request(reqwest::Method::POST, &create_path)
        .json(&input)
        .send()
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::CREATED);
    let upload: Value = first.json().await.unwrap();
    assert_eq!(
        h.request(reqwest::Method::POST, &create_path)
            .json(&input)
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap()["id"],
        upload["id"]
    );
    let mut conflict = input.clone();
    conflict["filename"] = json!("changed.jpg");
    assert_eq!(
        h.request(reqwest::Method::POST, &create_path)
            .json(&conflict)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        h.request(
            reqwest::Method::POST,
            &format!("/api/v1/native/channels/{}/media-uploads", h.other_channel)
        )
        .json(&input)
        .send()
        .await
        .unwrap()
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        h.chunk(&upload, 1, &bytes[CHUNK_BYTES..]).await.status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        h.chunk(&upload, 0, &bytes[..CHUNK_BYTES]).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        h.chunk(&upload, 0, &bytes[..CHUNK_BYTES]).await.status(),
        StatusCode::OK
    );
    let mut altered = bytes[..CHUNK_BYTES].to_vec();
    altered[4] ^= 1;
    assert_eq!(
        h.chunk(&upload, 0, &altered).await.status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        h.chunk(&upload, 1, &vec![0; CHUNK_BYTES + 1])
            .await
            .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let path = format!(
        "/api/v1/native/media-uploads/{}",
        upload["id"].as_str().unwrap()
    );
    assert_eq!(
        h.request(reqwest::Method::GET, &path)
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap()["next_chunk"],
        1
    );
    assert_eq!(
        h.chunk(&upload, 1, &bytes[CHUNK_BYTES..]).await.status(),
        StatusCode::OK
    );
    let response = h.complete(&upload).await;
    assert_eq!(response.status(), StatusCode::OK);
    let complete: Value = response.json().await.unwrap();
    assert_eq!(
        h.complete(&upload).await.json::<Value>().await.unwrap()["media"]["id"],
        complete["media"]["id"]
    );
    let media_path = complete["url"].as_str().unwrap();
    let browser = reqwest::Client::new()
        .get(format!("{}{media_path}", h.origin))
        .header("cookie", &h.browser_cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(browser.status(), StatusCode::OK);
    assert_eq!(browser.bytes().await.unwrap().as_ref(), bytes);
    let native = h
        .request(reqwest::Method::GET, media_path)
        .header("range", "bytes=2-8")
        .send()
        .await
        .unwrap();
    assert_eq!(native.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(native.bytes().await.unwrap().as_ref(), &bytes[2..9]);
    assert_eq!(
        h.request(reqwest::Method::DELETE, &path)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    let pending = h.create("cancel.txt", "text/plain", 3).await;
    let pending_path = format!(
        "/api/v1/native/media-uploads/{}",
        pending["id"].as_str().unwrap()
    );
    assert_eq!(
        h.request(reqwest::Method::DELETE, &pending_path)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        h.request(reqwest::Method::GET, &pending_path)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn native_media_upload_authority_account_owner_and_expiry_fail_closed_before_body() {
    let _serial = SERIAL.lock().await;
    let h = Harness::start().await;
    let client = reqwest::Client::new();
    for token in [
        "",
        "Bearer invalid",
        "Bearer sproyt-refresh.invalid",
        "Bearer agent.invalid",
        "Basic invalid",
    ] {
        let mut request = client
            .get(format!("{}{CONFIG}", h.origin))
            .header("x-sproyt-account", &h.grant.user_id);
        if !token.is_empty() {
            request = request.header("authorization", token);
        }
        assert_eq!(
            request.send().await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        h.request(reqwest::Method::GET, CONFIG)
            .header("cookie", &h.browser_cookie)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        h.request(
            reqwest::Method::GET,
            &format!("{CONFIG}?participant={}", h.other)
        )
        .send()
        .await
        .unwrap()
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        h.request(reqwest::Method::GET, CONFIG)
            .header("origin", "https://wrong.example")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        h.request(reqwest::Method::GET, CONFIG)
            .header("sec-fetch-site", "cross-site")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        client
            .get(format!("{}{CONFIG}", h.origin))
            .bearer_auth(&h.grant.access_token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        h.request(reqwest::Method::GET, CONFIG)
            .header("x-sproyt-account", h.other.to_string())
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        h.request(reqwest::Method::GET, CONFIG)
            .header("authorization", format!("Bearer {}", h.grant.access_token))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    // Native bearer authority remains absent from the existing browser edge.
    assert_eq!(
        h.request(reqwest::Method::GET, "/api/v1/media-uploads/config")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        client
            .get(format!("{}/api/v1/media-uploads/config", h.origin))
            .header("cookie", &h.browser_cookie)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let foreign = h
        .state
        .media_store
        .as_ref()
        .unwrap()
        .create(
            &h.other,
            *h.channel.as_uuid(),
            &NewUpload {
                request_id: Uuid::now_v7(),
                filename: "other.txt".into(),
                content_type: "text/plain".into(),
                size_bytes: 3,
            },
        )
        .await
        .unwrap();
    let foreign_path = format!("/api/v1/native/media-uploads/{}", foreign.id);
    assert_eq!(
        h.request(reqwest::Method::GET, &foreign_path)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        h.request(reqwest::Method::DELETE, &foreign_path)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    // Send headers with a large declared body but no body bytes. Wrong upload
    // ownership must produce its response without polling/awaiting that body.
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let address = h.origin.strip_prefix("http://").unwrap();
    let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
    let headers = format!(
        "PUT {foreign_path}/chunks/0 HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {}\r\nX-Sproyt-Account: {}\r\nX-Chunk-Sha256: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        h.grant.access_token,
        h.grant.user_id,
        "0".repeat(64),
        CHUNK_BYTES
    );
    socket.write_all(headers.as_bytes()).await.unwrap();
    let mut response = [0u8; 1024];
    let read = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        socket.read(&mut response),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(String::from_utf8_lossy(&response[..read]).starts_with("HTTP/1.1 404"));
    let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
    let headers = format!(
        "POST /api/v1/native/channels/{}/media-uploads HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer invalid\r\nX-Sproyt-Account: {}\r\nContent-Type: application/json\r\nContent-Length: 8192\r\nConnection: close\r\n\r\n",
        h.channel, h.grant.user_id
    );
    socket.write_all(headers.as_bytes()).await.unwrap();
    let read = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        socket.read(&mut response),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(String::from_utf8_lossy(&response[..read]).starts_with("HTTP/1.1 401"));
    let stage = h.create("expired.txt", "text/plain", 3).await;
    sqlx::query("update media_uploads set created_at=0, expires_at=1 where id=?")
        .bind(stage["id"].as_str().unwrap())
        .execute(&h.data_pool)
        .await
        .unwrap();
    assert_eq!(
        h.request(
            reqwest::Method::GET,
            &format!(
                "/api/v1/native/media-uploads/{}",
                stage["id"].as_str().unwrap()
            )
        )
        .send()
        .await
        .unwrap()
        .status(),
        StatusCode::NOT_FOUND
    );
    sqlx::query("UPDATE native_families SET expires_at='0' WHERE family=$1")
        .bind(&h.grant.family)
        .execute(&h.native_pool)
        .await
        .unwrap();
    assert_eq!(
        h.request(reqwest::Method::GET, CONFIG)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn native_media_upload_documents_validate_streaming_content_and_force_download() {
    let _serial = SERIAL.lock().await;
    let h = Harness::start().await;
    let mut zip = b"PK\x05\x06".to_vec();
    zip.resize(22, 0);
    for (name, mime, content, extension) in [
        (
            "report.exe",
            "application/pdf",
            b"%PDF-1.7\nbody\n%%EOF\n".to_vec(),
            "pdf",
        ),
        ("archive.html", "application/zip", zip, "zip"),
        (
            "notes.svg",
            "text/plain",
            "Notes €\n".as_bytes().to_vec(),
            "txt",
        ),
        ("table.exe", "text/csv", b"a,b\n1,2\n".to_vec(), "csv"),
    ] {
        let upload = h.create(name, mime, content.len()).await;
        assert_eq!(h.chunk(&upload, 0, &content).await.status(), StatusCode::OK);
        let completed = h.complete(&upload).await;
        assert_eq!(completed.status(), StatusCode::OK);
        let data: Value = completed.json().await.unwrap();
        let path = data["url"].as_str().unwrap();
        for suffix in ["", "/download"] {
            let response = h
                .request(reqwest::Method::GET, &format!("{path}{suffix}"))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()["content-type"], mime);
            assert_eq!(response.headers()["cache-control"], "private, no-store");
            assert_eq!(response.headers()["x-content-type-options"], "nosniff");
            let disposition = response.headers()["content-disposition"].to_str().unwrap();
            assert!(
                disposition.starts_with("attachment;")
                    && disposition.contains(&format!(".{extension}\""))
            );
            assert_eq!(response.bytes().await.unwrap().as_ref(), content);
        }
        assert_eq!(
            h.request(reqwest::Method::GET, &format!("{path}/preview"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE
        );
    }
    // A multibyte UTF-8 sequence split across exact one-MiB chunk boundaries.
    let mut text = vec![b'a'; CHUNK_BYTES - 1];
    text.extend_from_slice("€\n".as_bytes());
    let upload = h.create("split.txt", "text/plain", text.len()).await;
    assert_eq!(
        h.chunk(&upload, 0, &text[..CHUNK_BYTES]).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        h.chunk(&upload, 1, &text[CHUNK_BYTES..]).await.status(),
        StatusCode::OK
    );
    assert_eq!(h.complete(&upload).await.status(), StatusCode::OK);
    // Text that happens to resemble an image/video header stays download-only.
    for text in [
        b"GIF89a a textual example".as_slice(),
        b"1234ftyp a textual example".as_slice(),
    ] {
        let upload = h.create("header.txt", "text/plain", text.len()).await;
        assert_eq!(h.chunk(&upload, 0, text).await.status(), StatusCode::OK);
        let response = h.complete(&upload).await;
        assert_eq!(response.status(), StatusCode::OK);
        let complete: Value = response.json().await.unwrap();
        assert_eq!(complete["media"]["content_type"], "text/plain");
        let response = h
            .request(reqwest::Method::GET, complete["url"].as_str().unwrap())
            .send()
            .await
            .unwrap();
        assert!(
            response.headers()["content-disposition"]
                .to_str()
                .unwrap()
                .starts_with("attachment;")
        );
        assert_eq!(response.headers()["content-type"], "text/plain");
    }
    for (mime, bytes) in [
        ("application/pdf", b"%PDF-1.7\ntruncated".as_slice()),
        ("application/zip", b"PK\x03\x04fake".as_slice()),
        ("text/plain", b"MZ\0binary".as_slice()),
        ("text/csv", b"a,\xff".as_slice()),
        (
            "application/pdf",
            b"GIF89a wrong declared document".as_slice(),
        ),
    ] {
        let upload = h.create("spoof.dat", mime, bytes.len()).await;
        let before: i64 = sqlx::query_scalar("select count(*) from media_objects")
            .fetch_one(&h.data_pool)
            .await
            .unwrap();
        assert_eq!(h.chunk(&upload, 0, bytes).await.status(), StatusCode::OK);
        assert_eq!(
            h.complete(&upload).await.status(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE
        );
        let after: i64 = sqlx::query_scalar("select count(*) from media_objects")
            .fetch_one(&h.data_pool)
            .await
            .unwrap();
        assert_eq!(before, after);
        h.request(
            reqwest::Method::DELETE,
            &format!(
                "/api/v1/native/media-uploads/{}",
                upload["id"].as_str().unwrap()
            ),
        )
        .send()
        .await
        .unwrap();
    }
    for mime in ["text/html", "image/svg+xml", "application/x-msdownload"] {
        let response=h.request(reqwest::Method::POST,&format!("/api/v1/native/channels/{}/media-uploads",h.channel)).json(&json!({"request_id":Uuid::now_v7(),"filename":"blocked.exe","content_type":mime,"size_bytes":12})).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
}

#[tokio::test]
async fn native_media_upload_json_membership_revoke_and_disabled_contract() {
    let _serial = SERIAL.lock().await;
    let h = Harness::start().await;
    let create = format!("/api/v1/native/channels/{}/media-uploads", h.channel);
    assert_eq!(
        h.request(reqwest::Method::POST, &create)
            .header("content-type", "application/json")
            .body("x".repeat(8193))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(h.request(reqwest::Method::POST,&create).json(&json!({"request_id":Uuid::now_v7(),"filename":"bad.txt","content_type":"text/plain","size_bytes":1,"owner_id":h.grant.user_id})).send().await.unwrap().status(),StatusCode::BAD_REQUEST);
    let stage = h.create("revoked.txt", "text/plain", 3).await;
    // Membership is rechecked for status, body admission and final publication.
    sqlx::query("delete from channel_memberships where channel_id=? and user_id=?")
        .bind(h.channel.to_string())
        .bind(&h.grant.user_id)
        .execute(&h.data_pool)
        .await
        .unwrap();
    assert_eq!(
        h.chunk(&stage, 0, b"abc").await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(h.complete(&stage).await.status(), StatusCode::NOT_FOUND);
    h.state
        .native_sessions
        .as_ref()
        .unwrap()
        .revoke(&h.grant.revoke_token)
        .await
        .unwrap();
    assert_eq!(
        h.request(reqwest::Method::GET, CONFIG)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let state = crate::web::native::test_state(AuthService::development(), None).await;
    let app = crate::web::native::routes().with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    assert_eq!(
        reqwest::Client::new()
            .get(format!("http://{address}{CONFIG}"))
            .bearer_auth(&h.grant.access_token)
            .header("x-sproyt-account", &h.grant.user_id)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    server.abort();
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

        let fixtures = std::env::var_os("CARGO_TARGET_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("native-upload-fixtures");
        std::fs::create_dir_all(&fixtures).unwrap();
        let database = fixtures.join(format!("{}.sqlite", Uuid::now_v7()));
        let url = format!("sqlite://{}", database.to_string_lossy().replace('\\', "/"));
        let repository = Arc::new(
            crate::db::SqliteChatRepository::connect(&url)
                .await
                .unwrap(),
        );
        repository.migrate().await.unwrap();
        let store = repository.media_store_for_test(UploadLimits {
            max_file_bytes: 4 * CHUNK_BYTES as u64,
            owner_bytes: 32 * CHUNK_BYTES as u64,
            total_bytes: 128 * CHUNK_BYTES as u64,
        });
        let mut state = crate::web::native::test_state(auth, Some(native)).await;
        state.chat = crate::chat::ChatEngine::start(repository);
        state.media_store = Some(store);
        let actor = state
            .native_sessions
            .as_ref()
            .unwrap()
            .authenticate(&state.auth, &grant.access_token)
            .await
            .unwrap();
        state.chat.ensure_user(actor.user.clone()).await.unwrap();
        let other = principal(config.issuer(), "upload-other", "Upload Other", None, None).unwrap();
        state.chat.ensure_user(other.user.clone()).await.unwrap();
        let channel = state
            .chat
            .open_direct_channel(actor.user.id.clone(), other.user.id.clone())
            .await
            .unwrap()
            .id;
        let other_channel = state
            .chat
            .create_channel(
                other.user.id.clone(),
                ChannelSlug::new("native-upload-other").unwrap(),
                DisplayName::new("Other uploads").unwrap(),
                ChannelKind::Private,
                None,
            )
            .await
            .unwrap()
            .id;
        let data_pool = sqlx::SqlitePool::connect(&url).await.unwrap();
        let app = crate::server::build_router(state.clone(), state.operations.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            origin,
            state,
            native_pool,
            data_pool,
            grant,
            browser_cookie: browser.set_cookie,
            channel,
            other_channel,
            other: other.user.id,
            server,
            provider_server,
        }
    }
    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        reqwest::Client::new()
            .request(method, format!("{}{path}", self.origin))
            .bearer_auth(&self.grant.access_token)
            .header("x-sproyt-account", &self.grant.user_id)
    }
    async fn create(&self, name: &str, mime: &str, size: usize) -> Value {
        let response = self.request(reqwest::Method::POST, &format!("/api/v1/native/channels/{}/media-uploads", self.channel)).json(&json!({"request_id":Uuid::now_v7(),"filename":name,"content_type":mime,"size_bytes":size})).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        response.json().await.unwrap()
    }
    async fn chunk(&self, upload: &Value, index: usize, bytes: &[u8]) -> reqwest::Response {
        self.request(
            reqwest::Method::PUT,
            &format!(
                "/api/v1/native/media-uploads/{}/chunks/{index}",
                upload["id"].as_str().unwrap()
            ),
        )
        .header("x-chunk-sha256", hex::encode(Sha256::digest(bytes)))
        .body(bytes.to_vec())
        .send()
        .await
        .unwrap()
    }
    async fn complete(&self, upload: &Value) -> reqwest::Response {
        self.request(
            reqwest::Method::POST,
            &format!(
                "/api/v1/native/media-uploads/{}/complete",
                upload["id"].as_str().unwrap()
            ),
        )
        .send()
        .await
        .unwrap()
    }
}
