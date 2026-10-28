//! Authorised, transient Maps lookups for an agent's search card.
//!
//! Provider responses never pass through agent context or durable storage.
use super::{
    CircleChatAgents, UserId,
    location::JobLocation,
    maps::{MapsService, Waypoint},
    place_lookup::PlaceLookupDescriptor,
};
use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{future::Future, time::Duration};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MapsLookupError {
    Unavailable,
    Busy,
    NotFound,
    Provider,
    Timeout,
    ConsentRequired,
}

type LookupResult<T> = std::result::Result<T, MapsLookupError>;

trait Authority: Sync {
    type Location: Send + Sync;
    fn describe(&self) -> impl Future<Output = LookupResult<Option<PlaceLookupDescriptor>>> + Send;
    fn location<'a>(
        &'a self,
        descriptor: &'a PlaceLookupDescriptor,
    ) -> impl Future<Output = LookupResult<Option<Self::Location>>> + Send;
    fn claim(&self) -> impl Future<Output = LookupResult<Option<String>>> + Send;
    fn release<'a>(&'a self, token: &'a str) -> impl Future<Output = LookupResult<()>> + Send;
    fn viewer(&self) -> &str;
    fn channel(&self) -> &str;
    fn message(&self) -> &str;
}
trait Provider: Sync {
    type Location: Send + Sync;
    fn search<'a>(
        &'a self,
        query: &'a str,
        location: Option<&'a Self::Location>,
    ) -> impl Future<Output = LookupResult<Value>> + Send;
    fn walk<'a>(
        &'a self,
        origin: Origin<'a, Self::Location>,
        place_id: &'a str,
    ) -> impl Future<Output = LookupResult<Value>> + Send;
}
enum Origin<'a, L> {
    Named(&'a str),
    Shared(&'a L),
}

struct RequestAuthority<'a> {
    agents: &'a CircleChatAgents,
    viewer: &'a UserId,
    viewer_id: String,
    channel: &'a str,
    message: &'a str,
}
impl Authority for RequestAuthority<'_> {
    type Location = JobLocation;
    async fn describe(&self) -> LookupResult<Option<PlaceLookupDescriptor>> {
        self.agents
            .place_lookup_for_message(self.viewer, self.channel, self.message)
            .await
            .map_err(|_| MapsLookupError::Unavailable)
    }
    async fn location(&self, d: &PlaceLookupDescriptor) -> LookupResult<Option<JobLocation>> {
        self.agents
            .place_lookup_location(d)
            .await
            .map_err(|_| MapsLookupError::NotFound)
    }
    async fn claim(&self) -> LookupResult<Option<String>> {
        self.agents
            .claim_place_lookup(self.viewer)
            .await
            .map_err(|_| MapsLookupError::Unavailable)
    }
    async fn release(&self, token: &str) -> LookupResult<()> {
        self.agents
            .release_place_lookup(self.viewer, token)
            .await
            .map_err(|_| MapsLookupError::Unavailable)
    }
    fn viewer(&self) -> &str {
        &self.viewer_id
    }
    fn channel(&self) -> &str {
        self.channel
    }
    fn message(&self) -> &str {
        self.message
    }
}
impl Provider for MapsService {
    type Location = JobLocation;
    async fn search(&self, query: &str, location: Option<&JobLocation>) -> LookupResult<Value> {
        self.search_places(query, location)
            .await
            .map_err(|_| MapsLookupError::Provider)
    }
    async fn walk(&self, origin: Origin<'_, JobLocation>, place_id: &str) -> LookupResult<Value> {
        let origin = match origin {
            Origin::Named(s) => Waypoint::Named(s),
            Origin::Shared(l) => Waypoint::Shared(l),
        };
        self.walking_route(origin, Waypoint::PlaceId(place_id))
            .await
            .map_err(|_| MapsLookupError::Provider)
    }
}

#[cfg(test)]
tokio::task_local! {
    pub(crate) static TEST_PROVIDER_CALLS: std::sync::Arc<std::sync::atomic::AtomicUsize>;
}
#[cfg(test)]
struct TestProvider(std::sync::Arc<std::sync::atomic::AtomicUsize>);
#[cfg(test)]
impl Provider for TestProvider {
    type Location = JobLocation;
    async fn search(&self, _: &str, _: Option<&JobLocation>) -> LookupResult<Value> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(json!({"summary":"mock","places":[{"id":"mock-id"}],"contentToken":"opaque"}))
    }
    async fn walk(&self, _: Origin<'_, JobLocation>, _: &str) -> LookupResult<Value> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(json!({"routes":[]}))
    }
}

impl CircleChatAgents {
    /// Fetch fresh provider output only for a currently authorised card viewer.
    pub(crate) async fn run_place_lookup(
        &self,
        viewer: &UserId,
        channel: &str,
        message_id: &str,
        allow_shared_location: bool,
        expected_preview_binding: &str,
    ) -> LookupResult<Value> {
        let authority = RequestAuthority {
            agents: self,
            viewer,
            viewer_id: viewer.to_string(),
            channel,
            message: message_id,
        };
        #[cfg(test)]
        if let Ok(calls) = TEST_PROVIDER_CALLS.try_with(Clone::clone) {
            return run(
                &authority,
                &TestProvider(calls),
                allow_shared_location,
                Some(expected_preview_binding),
            )
            .await;
        }
        let service = MapsService::from_env()
            .map_err(|_| MapsLookupError::Unavailable)?
            .ok_or(MapsLookupError::Unavailable)?;
        run(
            &authority,
            &service,
            allow_shared_location,
            Some(expected_preview_binding),
        )
        .await
    }

    pub(crate) async fn run_place_route(
        &self,
        viewer: &UserId,
        channel: &str,
        message_id: &str,
        token: &str,
        allow_shared_location: bool,
    ) -> LookupResult<Value> {
        let signer = RouteSigner::from_env()?.ok_or(MapsLookupError::Unavailable)?;
        let authority = RequestAuthority {
            agents: self,
            viewer,
            viewer_id: viewer.to_string(),
            channel,
            message: message_id,
        };
        #[cfg(test)]
        if let Ok(calls) = TEST_PROVIDER_CALLS.try_with(Clone::clone) {
            return run_route(
                &authority,
                &TestProvider(calls),
                &signer,
                token,
                allow_shared_location,
            )
            .await;
        }
        let service = MapsService::from_env()
            .map_err(|_| MapsLookupError::Unavailable)?
            .ok_or(MapsLookupError::Unavailable)?;
        run_route(&authority, &service, &signer, token, allow_shared_location).await
    }
}

async fn check<A: Authority>(
    auth: &A,
    expected: &PlaceLookupDescriptor,
) -> LookupResult<Option<A::Location>> {
    let actual = auth.describe().await?.ok_or(MapsLookupError::NotFound)?;
    if &actual != expected
        || (expected.origin_mode == "shared"
            && (expected.source_author_id != auth.viewer() || expected.share_id.is_none()))
    {
        return Err(MapsLookupError::NotFound);
    }
    let location = auth.location(expected).await?;
    if expected.origin_mode == "shared" && location.is_none() {
        return Err(MapsLookupError::NotFound);
    }
    Ok(location)
}

async fn run<A: Authority, P: Provider<Location = A::Location>>(
    auth: &A,
    provider: &P,
    allow_shared_location: bool,
    expected_preview_binding: Option<&str>,
) -> LookupResult<Value> {
    let descriptor = auth.describe().await?.ok_or(MapsLookupError::NotFound)?;
    if expected_preview_binding.is_some_and(|expected| expected != descriptor.preview_binding()) {
        return Err(MapsLookupError::NotFound);
    }
    if descriptor.provider_query().is_none() {
        return Err(MapsLookupError::NotFound);
    }
    if descriptor.origin_mode == "shared" && !allow_shared_location {
        return Err(MapsLookupError::ConsentRequired);
    }
    // Authorisation precedes the shared, database-backed rate limit claim.
    check(auth, &descriptor).await?;
    let token = auth.claim().await?.ok_or(MapsLookupError::Busy)?;
    let outcome =
        match tokio::time::timeout(Duration::from_secs(50), lookup(auth, provider, &descriptor))
            .await
        {
            Ok(result) => result,
            Err(_) => Err(MapsLookupError::Timeout),
        };
    // Also release after errors and timeouts; database lease expires on process loss.
    auth.release(&token).await?;
    outcome
}

async fn lookup<A: Authority, P: Provider<Location = A::Location>>(
    auth: &A,
    provider: &P,
    d: &PlaceLookupDescriptor,
) -> LookupResult<Value> {
    let query = d.provider_query().ok_or(MapsLookupError::NotFound)?;
    let location = check(auth, d).await?;
    let search = provider.search(&query, location.as_ref()).await?;
    check(auth, d).await?;
    let mut route_tokens = Vec::new();
    if d.origin_mode != "none"
        && let Some(signer) = RouteSigner::from_env()?
        && let Some(places) = search["places"].as_array()
    {
        for (index, place) in places.iter().enumerate() {
            let id = place["id"].as_str().ok_or(MapsLookupError::Provider)?;
            route_tokens.push(json!({"index":index,"token":signer.sign(auth,d,id)?}));
        }
    }
    check(auth, d).await?;
    Ok(json!({"search":search,"walking":null,"route_tokens":route_tokens}))
}

// Encrypted and authenticated, ephemeral, and scoped to the exact request and
// identity. No coordinates or provider content are persisted in server storage.
struct RouteSigner(Aes256Gcm);
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RouteClaims {
    viewer: String,
    channel: String,
    message: String,
    descriptor_hash: String,
    place_id: String,
    exp: i64,
}
impl RouteSigner {
    fn from_env() -> LookupResult<Option<Self>> {
        #[cfg(test)]
        if TEST_PROVIDER_CALLS.try_with(|_| ()).is_ok() {
            return Ok(Some(Self(
                Aes256Gcm::new_from_slice(&[42u8; 32]).map_err(|_| MapsLookupError::Unavailable)?,
            )));
        }
        let key = match std::env::var("SPROYT_PLACE_ROUTE_SIGNING_KEY") {
            Ok(value) if !value.is_empty() => value,
            Ok(_) | Err(std::env::VarError::NotPresent) => return Ok(None),
            Err(_) => return Err(MapsLookupError::Unavailable),
        };
        let bytes = URL_SAFE_NO_PAD
            .decode(key)
            .map_err(|_| MapsLookupError::Unavailable)?;
        if bytes.len() != 32 {
            return Err(MapsLookupError::Unavailable);
        }
        Ok(Some(Self(
            Aes256Gcm::new_from_slice(&bytes).map_err(|_| MapsLookupError::Unavailable)?,
        )))
    }
    fn hash(d: &PlaceLookupDescriptor) -> LookupResult<String> {
        let bytes = serde_json::to_vec(d).map_err(|_| MapsLookupError::Unavailable)?;
        Ok(URL_SAFE_NO_PAD.encode(Sha256::digest(bytes)))
    }
    fn sign<A: Authority>(
        &self,
        auth: &A,
        d: &PlaceLookupDescriptor,
        place_id: &str,
    ) -> LookupResult<String> {
        if place_id.is_empty() || place_id.len() > 256 {
            return Err(MapsLookupError::Provider);
        }
        let claims = RouteClaims {
            viewer: auth.viewer().into(),
            channel: auth.channel().into(),
            message: auth.message().into(),
            descriptor_hash: Self::hash(d)?,
            place_id: place_id.into(),
            exp: chrono::Utc::now().timestamp() + 300,
        };
        let plaintext = serde_json::to_vec(&claims).map_err(|_| MapsLookupError::Unavailable)?;
        let mut nonce = [0u8; 12];
        getrandom::fill(&mut nonce).map_err(|_| MapsLookupError::Unavailable)?;
        let ciphertext = self
            .0
            .encrypt(Nonce::from_slice(&nonce), plaintext.as_ref())
            .map_err(|_| MapsLookupError::Unavailable)?;
        let mut result = nonce.to_vec();
        result.extend(ciphertext);
        Ok(URL_SAFE_NO_PAD.encode(result))
    }
    fn verify<A: Authority>(
        &self,
        auth: &A,
        d: &PlaceLookupDescriptor,
        token: &str,
    ) -> LookupResult<String> {
        if token.len() > 4096 {
            return Err(MapsLookupError::NotFound);
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(token)
            .map_err(|_| MapsLookupError::NotFound)?;
        if bytes.len() < 28 {
            return Err(MapsLookupError::NotFound);
        }
        let plaintext = self
            .0
            .decrypt(Nonce::from_slice(&bytes[..12]), &bytes[12..])
            .map_err(|_| MapsLookupError::NotFound)?;
        let claims: RouteClaims =
            serde_json::from_slice(&plaintext).map_err(|_| MapsLookupError::NotFound)?;
        let now = chrono::Utc::now().timestamp();
        if claims.exp <= now
            || claims.exp > now + 300
            || claims.viewer != auth.viewer()
            || claims.channel != auth.channel()
            || claims.message != auth.message()
            || claims.descriptor_hash != Self::hash(d)?
            || claims.place_id.is_empty()
            || claims.place_id.len() > 256
        {
            return Err(MapsLookupError::NotFound);
        }
        Ok(claims.place_id)
    }
}
async fn run_route<A: Authority, P: Provider<Location = A::Location>>(
    auth: &A,
    provider: &P,
    signer: &RouteSigner,
    route_token: &str,
    allow_shared_location: bool,
) -> LookupResult<Value> {
    let d = auth.describe().await?.ok_or(MapsLookupError::NotFound)?;
    let destination = signer.verify(auth, &d, route_token)?;
    if d.origin_mode == "shared" && !allow_shared_location {
        return Err(MapsLookupError::ConsentRequired);
    }
    if d.origin_mode != "shared" && d.origin_mode != "named" {
        return Err(MapsLookupError::NotFound);
    }
    check(auth, &d).await?;
    let lease = auth.claim().await?.ok_or(MapsLookupError::Busy)?;
    let operation = async {
        let loc = check(auth, &d).await?;
        let origin = match d.origin_mode.as_str() {
            "named" => Origin::Named(d.named_origin.as_deref().ok_or(MapsLookupError::NotFound)?),
            "shared" => Origin::Shared(loc.as_ref().ok_or(MapsLookupError::NotFound)?),
            _ => return Err(MapsLookupError::NotFound),
        };
        let route = provider.walk(origin, &destination).await?;
        check(auth, &d).await?;
        // A token cannot outlive its valid period even if a call was slow.
        signer.verify(auth, &d, route_token)?;
        Ok(
            json!({"route":route,"warning":"Gangruter er i beta og kan mangle fortau eller gangvegar."}),
        )
    };
    let outcome = match tokio::time::timeout(Duration::from_secs(50), operation).await {
        Ok(result) => result,
        Err(_) => Err(MapsLookupError::Timeout),
    };
    auth.release(&lease).await?;
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    struct Auth {
        descriptor: Mutex<Option<PlaceLookupDescriptor>>,
        position: AtomicBool,
        claimed: AtomicBool,
        released: AtomicUsize,
        viewer: &'static str,
        channel: &'static str,
        message: &'static str,
    }
    impl Authority for Auth {
        type Location = u8;
        async fn describe(&self) -> LookupResult<Option<PlaceLookupDescriptor>> {
            Ok(self.descriptor.lock().unwrap().clone())
        }
        async fn location(&self, d: &PlaceLookupDescriptor) -> LookupResult<Option<u8>> {
            Ok((d.origin_mode == "shared" && self.position.load(Ordering::SeqCst)).then_some(42))
        }
        async fn claim(&self) -> LookupResult<Option<String>> {
            Ok(self
                .claimed
                .load(Ordering::SeqCst)
                .then(|| "lease".to_owned()))
        }
        async fn release(&self, _: &str) -> LookupResult<()> {
            self.released.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn viewer(&self) -> &str {
            self.viewer
        }
        fn channel(&self) -> &str {
            self.channel
        }
        fn message(&self) -> &str {
            self.message
        }
    }
    struct Mock {
        auth: Arc<Auth>,
        revoke: bool,
        fail: bool,
        calls: AtomicUsize,
    }
    impl Provider for Mock {
        type Location = u8;
        async fn search(&self, _: &str, loc: Option<&u8>) -> LookupResult<Value> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(loc, Some(&42));
            if self.revoke {
                self.auth.position.store(false, Ordering::SeqCst);
            }
            if self.fail {
                return Err(MapsLookupError::Provider);
            }
            Ok(json!({"summary":"unchanged","places":[],"contentToken":"opaque"}))
        }
        async fn walk(&self, _: Origin<'_, u8>, _: &str) -> LookupResult<Value> {
            Err(MapsLookupError::Provider)
        }
    }
    fn setup(viewer: &'static str, revoke: bool, fail: bool) -> (Arc<Auth>, Mock) {
        let d = PlaceLookupDescriptor {
            job_id: "job".into(),
            agent_id: "agent".into(),
            source_message_id: "source".into(),
            source_author_id: "author".into(),
            share_id: Some("share".into()),
            query: "bars".into(),
            origin_mode: "shared".into(),
            named_origin: None,
        };
        let auth = Arc::new(Auth {
            descriptor: Mutex::new(Some(d)),
            position: AtomicBool::new(true),
            claimed: AtomicBool::new(true),
            released: AtomicUsize::new(0),
            viewer,
            channel: "channel",
            message: "reply",
        });
        let mock = Mock {
            auth: auth.clone(),
            revoke,
            fail,
            calls: AtomicUsize::new(0),
        };
        (auth, mock)
    }
    #[tokio::test]
    async fn preserves_result_and_releases_lease() {
        let (auth, mock) = setup("author", false, false);
        let result = run(&*auth, &mock, true, None).await.unwrap();
        assert_eq!(
            result["search"],
            json!({"summary":"unchanged","places":[],"contentToken":"opaque"})
        );
        assert_eq!(auth.released.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn revoked_location_prevents_result_and_releases_lease() {
        let (auth, mock) = setup("author", true, false);
        assert_eq!(
            run(&*auth, &mock, true, None).await.unwrap_err(),
            MapsLookupError::NotFound
        );
        assert_eq!(auth.released.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn another_viewer_never_sends_location_to_provider() {
        let (auth, mock) = setup("another", false, false);
        assert_eq!(
            run(&*auth, &mock, true, None).await.unwrap_err(),
            MapsLookupError::NotFound
        );
        assert_eq!(mock.calls.load(Ordering::SeqCst), 0);
        assert_eq!(auth.released.load(Ordering::SeqCst), 0);
    }
    #[tokio::test]
    async fn provider_failure_releases_lease() {
        let (auth, mock) = setup("author", false, true);
        assert_eq!(
            run(&*auth, &mock, true, None).await.unwrap_err(),
            MapsLookupError::Provider
        );
        assert_eq!(auth.released.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn rate_limit_never_calls_provider() {
        let (auth, mock) = setup("author", false, false);
        auth.claimed.store(false, Ordering::SeqCst);
        assert_eq!(
            run(&*auth, &mock, true, None).await.unwrap_err(),
            MapsLookupError::Busy
        );
        assert_eq!(mock.calls.load(Ordering::SeqCst), 0);
    }
    #[tokio::test]
    async fn explicit_google_location_consent_is_required() {
        let (auth, mock) = setup("author", false, false);
        assert_eq!(
            run(&*auth, &mock, false, None).await.unwrap_err(),
            MapsLookupError::ConsentRequired
        );
        assert_eq!(mock.calls.load(Ordering::SeqCst), 0);
        assert_eq!(auth.released.load(Ordering::SeqCst), 0);
    }
    #[tokio::test]
    async fn changed_message_or_membership_discards_result() {
        struct RevokingProvider(Arc<Auth>);
        impl Provider for RevokingProvider {
            type Location = u8;
            async fn search(&self, _: &str, _: Option<&u8>) -> LookupResult<Value> {
                *self.0.descriptor.lock().unwrap() = None;
                Ok(json!({"summary":"should never be returned","places":[]}))
            }
            async fn walk(&self, _: Origin<'_, u8>, _: &str) -> LookupResult<Value> {
                Err(MapsLookupError::Provider)
            }
        }
        let (auth, _) = setup("author", false, false);
        let provider = RevokingProvider(auth.clone());
        assert_eq!(
            run(&*auth, &provider, true, None).await.unwrap_err(),
            MapsLookupError::NotFound
        );
        assert_eq!(auth.released.load(Ordering::SeqCst), 1);
    }
    struct RouteMock {
        calls: AtomicUsize,
        revoke: bool,
        auth: Arc<Auth>,
    }
    impl Provider for RouteMock {
        type Location = u8;
        async fn search(&self, _: &str, _: Option<&u8>) -> LookupResult<Value> {
            Err(MapsLookupError::Provider)
        }
        async fn walk(&self, origin: Origin<'_, u8>, id: &str) -> LookupResult<Value> {
            assert!(matches!(origin, Origin::Shared(&42)));
            assert_eq!(id, "google-place-id");
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.revoke {
                self.auth.position.store(false, Ordering::SeqCst);
            }
            Ok(
                json!({"routes":[{"distanceMeters":125,"attribution":{"url":"https://www.google.com/maps/route","title":"Google Maps"}}]}),
            )
        }
    }
    #[tokio::test]
    async fn route_tokens_are_encrypted_scoped_and_require_consent() {
        let signer = RouteSigner(Aes256Gcm::new_from_slice(&[19; 32]).unwrap());
        let (auth, _) = setup("author", false, false);
        let d = auth.descriptor.lock().unwrap().clone().unwrap();
        let token = signer.sign(&*auth, &d, "google-place-id").unwrap();
        assert!(
            !String::from_utf8_lossy(&URL_SAFE_NO_PAD.decode(&token).unwrap())
                .contains("google-place-id")
        );
        assert_eq!(
            signer.verify(&*auth, &d, &token).unwrap(),
            "google-place-id"
        );
        let (other, _) = setup("another", false, false);
        assert_eq!(
            signer.verify(&*other, &d, &token).unwrap_err(),
            MapsLookupError::NotFound
        );
        // A distinct request path must not reuse a previously issued token.
        let replacement = Auth {
            descriptor: Mutex::new(Some(d.clone())),
            position: AtomicBool::new(true),
            claimed: AtomicBool::new(true),
            released: AtomicUsize::new(0),
            viewer: "author",
            channel: "other",
            message: "reply",
        };
        assert_eq!(
            signer.verify(&replacement, &d, &token).unwrap_err(),
            MapsLookupError::NotFound
        );
        let mut changed = d.clone();
        changed.share_id = Some("replaced".into());
        assert_eq!(
            signer.verify(&*auth, &changed, &token).unwrap_err(),
            MapsLookupError::NotFound
        );
        assert_eq!(
            signer.verify(&*auth, &d, "nonsense").unwrap_err(),
            MapsLookupError::NotFound
        );
        let mock = RouteMock {
            calls: AtomicUsize::new(0),
            revoke: false,
            auth: auth.clone(),
        };
        assert_eq!(
            run_route(&*auth, &mock, &signer, &token, false)
                .await
                .unwrap_err(),
            MapsLookupError::ConsentRequired
        );
        assert_eq!(mock.calls.load(Ordering::SeqCst), 0);
        let result = run_route(&*auth, &mock, &signer, &token, true)
            .await
            .unwrap();
        assert_eq!(result["route"]["routes"][0]["distanceMeters"], 125);
        assert!(result["warning"].as_str().unwrap().contains("beta"));
        assert_eq!(mock.calls.load(Ordering::SeqCst), 1);
        let revoke = RouteMock {
            calls: AtomicUsize::new(0),
            revoke: true,
            auth: auth.clone(),
        };
        assert_eq!(
            run_route(&*auth, &revoke, &signer, &token, true)
                .await
                .unwrap_err(),
            MapsLookupError::NotFound
        );
        assert_eq!(auth.released.load(Ordering::SeqCst), 2);
    }
    #[test]
    fn expired_route_token_is_rejected() {
        let signer = RouteSigner(Aes256Gcm::new_from_slice(&[11; 32]).unwrap());
        let (auth, _) = setup("author", false, false);
        let d = auth.descriptor.lock().unwrap().clone().unwrap();
        let claims = RouteClaims {
            viewer: "author".into(),
            channel: "channel".into(),
            message: "reply".into(),
            descriptor_hash: RouteSigner::hash(&d).unwrap(),
            place_id: "id".into(),
            exp: chrono::Utc::now().timestamp() - 1,
        };
        let nonce = [3u8; 12];
        let encrypted = signer
            .0
            .encrypt(
                Nonce::from_slice(&nonce),
                serde_json::to_vec(&claims).unwrap().as_ref(),
            )
            .unwrap();
        let mut bytes = nonce.to_vec();
        bytes.extend(encrypted);
        assert_eq!(
            signer
                .verify(&*auth, &d, &URL_SAFE_NO_PAD.encode(bytes))
                .unwrap_err(),
            MapsLookupError::NotFound
        );
    }
    #[tokio::test]
    async fn stale_preview_is_rejected_before_claim_or_provider() {
        let (auth, mock) = setup("author", false, false);
        let binding = auth
            .descriptor
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .preview_binding();
        auth.descriptor.lock().unwrap().as_mut().unwrap().share_id = Some("replaced".into());
        assert_eq!(
            run(&*auth, &mock, true, Some(&binding)).await.unwrap_err(),
            MapsLookupError::NotFound
        );
        assert_eq!(mock.calls.load(Ordering::SeqCst), 0);
        assert_eq!(auth.released.load(Ordering::SeqCst), 0);
    }
}
