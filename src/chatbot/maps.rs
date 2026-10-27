//! Bounded, stateless Google Maps Grounding Lite transport.
//!
//! This module does not publish, cache, log, or persist Google content. Callers
//! must enforce consent immediately before a request and use an authorised,
//! non-durable presentation with the supplied attribution and source links.
use std::time::Duration;

use reqwest::{Client, Url, header::HeaderValue};
use serde_json::{Value, json};

use super::location::JobLocation;

const ENDPOINT: &str = "https://mapstools.googleapis.com/mcp";
const MAX_RESPONSE: usize = 256 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_QUERY_CHARS: usize = 240;
const MAX_PLACES: usize = 50;

/// A shared position can only enter this API through a job-bound grant. The
/// caller must recheck its authority just before the Google request; holding
/// this reference alone does not keep a grant alive.
pub(super) enum Waypoint<'a> {
    Named(&'a str),
    PlaceId(&'a str),
    Shared(&'a JobLocation),
}

/// Never derive Debug for a value containing a credential.
#[derive(Clone)]
pub(super) struct MapsService {
    http: Client,
    endpoint: Url,
    api_key: HeaderValue,
}

impl MapsService {
    pub(super) fn from_env() -> std::result::Result<Option<Self>, &'static str> {
        let key = match std::env::var("SPROYT_GOOGLE_MAPS_API_KEY") {
            Ok(key) if !key.trim().is_empty() => key,
            Ok(_) | Err(std::env::VarError::NotPresent) => return Ok(None),
            Err(_) => return Err("maps_configuration"),
        };
        Self::new(
            &key,
            Url::parse(ENDPOINT).map_err(|_| "maps_configuration")?,
        )
        .map(Some)
    }

    fn new(key: &str, endpoint: Url) -> std::result::Result<Self, &'static str> {
        if key.trim() != key || key.len() < 16 || key.len() > 256 {
            return Err("maps_configuration");
        }
        let mut api_key = HeaderValue::from_str(key).map_err(|_| "maps_configuration")?;
        api_key.set_sensitive(true);
        let http = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| "maps_configuration")?;
        Ok(Self {
            http,
            endpoint,
            api_key,
        })
    }

    /// Return the original structured result, preserving summary, citations,
    /// provider links, tokens and unknown future fields without rewriting.
    /// Bias may only be sourced from the exact authorised location grant.
    pub(super) async fn search_places(
        &self,
        query: &str,
        shared: Option<&JobLocation>,
    ) -> std::result::Result<Value, &'static str> {
        let query = checked_text(query, MAX_QUERY_CHARS)?;
        let mut arguments = json!({"text_query":query,"language_code":"no"});
        if let Some(shared) = shared {
            let point = coordinates(shared)?;
            arguments["location_bias"] = json!({"circle":{"center":point}});
        }
        let result = self.call("search_places", arguments).await?;
        validate_search(&result)?;
        Ok(result)
    }

    /// WALK is mandatory here. UI must display Google's walking route warning
    /// and route attribution alongside the result.
    pub(super) async fn walking_route(
        &self,
        origin: Waypoint<'_>,
        destination: Waypoint<'_>,
    ) -> std::result::Result<Value, &'static str> {
        let result = self
            .call(
                "compute_routes",
                json!({
                    "origin": waypoint(origin)?,
                    "destination": waypoint(destination)?,
                    "travel_mode": "WALK"
                }),
            )
            .await?;
        validate_routes(&result)?;
        Ok(result)
    }

    async fn call(&self, name: &str, arguments: Value) -> std::result::Result<Value, &'static str> {
        let mut response = self
            .http
            .post(self.endpoint.clone())
            .header("x-goog-api-key", self.api_key.clone())
            .header("accept", "application/json, text/event-stream")
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
                "params":{"name":name,"arguments":arguments}}))
            .send()
            .await
            .map_err(|_| "maps_transport")?;
        if !response.status().is_success() {
            return Err("maps_status");
        }
        if response
            .content_length()
            .is_some_and(|size| size > MAX_RESPONSE as u64)
        {
            return Err("maps_response_too_large");
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| "maps_transport")? {
            if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE {
                return Err("maps_response_too_large");
            }
            bytes.extend_from_slice(&chunk);
        }
        decode_response(&bytes)
    }
}

fn checked_text(value: &str, max: usize) -> std::result::Result<&str, &'static str> {
    let text = value.trim();
    if text.is_empty() || text.chars().count() > max || text.chars().any(char::is_control) {
        return Err("maps_invalid_argument");
    }
    Ok(text)
}

fn coordinates(shared: &JobLocation) -> std::result::Result<Value, &'static str> {
    let context = shared.context();
    let latitude = context["latitude"].as_f64().ok_or("maps_location")?;
    let longitude = context["longitude"].as_f64().ok_or("maps_location")?;
    if !latitude.is_finite()
        || !longitude.is_finite()
        || !(-90.0..=90.0).contains(&latitude)
        || !(-180.0..=180.0).contains(&longitude)
    {
        return Err("maps_location");
    }
    Ok(json!({"latitude": latitude, "longitude": longitude}))
}

fn waypoint(point: Waypoint<'_>) -> std::result::Result<Value, &'static str> {
    match point {
        Waypoint::Named(name) => Ok(json!({"address":checked_text(name, MAX_QUERY_CHARS)?})),
        Waypoint::PlaceId(id) => {
            let id = checked_text(id, 256)?;
            if !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
            {
                return Err("maps_invalid_argument");
            }
            Ok(json!({"place_id":id}))
        }
        Waypoint::Shared(shared) => Ok(json!({"lat_lng":coordinates(shared)?})),
    }
}

fn decode_response(bytes: &[u8]) -> std::result::Result<Value, &'static str> {
    // The stateless endpoint ordinarily returns JSON. Also accept a single
    // SSE message, as the MCP HTTP transport permits either representation.
    let raw = std::str::from_utf8(bytes).map_err(|_| "maps_invalid_response")?;
    let input = if raw.trim_start().starts_with('{') {
        raw.to_owned()
    } else {
        let data: Vec<_> = raw
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(str::trim_start)
            .collect();
        if data.is_empty() || data.len() > 16 {
            return Err("maps_invalid_response");
        }
        data.join("\n")
    };
    let envelope: Value = serde_json::from_str(&input).map_err(|_| "maps_invalid_response")?;
    if envelope["jsonrpc"] != "2.0"
        || envelope["id"] != 1
        || envelope.get("error").is_some()
        || envelope["result"]["isError"] == true
    {
        return Err("maps_tool_error");
    }
    let result = &envelope["result"];
    if let Some(structured) = result.get("structuredContent").filter(|v| v.is_object()) {
        return Ok(structured.clone());
    }
    let content = result["content"]
        .as_array()
        .ok_or("maps_invalid_response")?;
    if content.len() != 1 || content[0]["type"] != "text" {
        return Err("maps_invalid_response");
    }
    let raw = content[0]["text"].as_str().ok_or("maps_invalid_response")?;
    let value: Value = serde_json::from_str(raw).map_err(|_| "maps_invalid_response")?;
    if !value.is_object() {
        return Err("maps_invalid_response");
    }
    Ok(value)
}

fn google_url(value: &str) -> bool {
    if value.len() > 4096 || value.chars().any(char::is_control) {
        return false;
    }
    let Ok(url) = Url::parse(value) else {
        return false;
    };
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none_or(|port| port == 443)
        && matches!(
            url.host_str(),
            Some("google.com" | "www.google.com" | "maps.google.com" | "maps.app.goo.gl")
        )
}

fn validate_attribution(value: &Value) -> std::result::Result<(), &'static str> {
    let title = value["title"].as_str().ok_or("maps_attribution")?;
    checked_text(title, 512).map_err(|_| "maps_attribution")?;
    let url = value["url"].as_str().ok_or("maps_attribution")?;
    if !google_url(url) {
        return Err("maps_attribution");
    }
    Ok(())
}

fn validate_search(value: &Value) -> std::result::Result<(), &'static str> {
    let summary = value["summary"].as_str().ok_or("maps_invalid_response")?;
    if summary.len() > MAX_RESPONSE {
        return Err("maps_response_too_large");
    }
    let places = value["places"].as_array().ok_or("maps_invalid_response")?;
    if places.len() > MAX_PLACES {
        return Err("maps_response_too_large");
    }
    for place in places {
        let id = place["id"].as_str().ok_or("maps_invalid_response")?;
        checked_text(id, 256).map_err(|_| "maps_invalid_response")?;
        validate_attribution(&place["attribution"])?;
        let links = place["googleMapsLinks"]
            .as_object()
            .ok_or("maps_invalid_response")?;
        if !links
            .get("placeUrl")
            .and_then(Value::as_str)
            .is_some_and(google_url)
        {
            return Err("maps_attribution");
        }
        // Fail closed before a UI can render any provider-supplied URL.
        for link in links.values() {
            if !link.as_str().is_some_and(google_url) {
                return Err("maps_attribution");
            }
        }
    }
    Ok(())
}

fn validate_routes(value: &Value) -> std::result::Result<(), &'static str> {
    let routes = value["routes"].as_array().ok_or("maps_invalid_response")?;
    if routes.len() > 3 {
        return Err("maps_invalid_response");
    }
    for route in routes {
        if route["distanceMeters"].as_u64().is_none()
            || !route["duration"]
                .as_str()
                .is_some_and(|s| s.len() <= 64 && s.ends_with('s'))
        {
            return Err("maps_invalid_response");
        }
        validate_attribution(&route["attribution"])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use axum::{Json, Router, extract::State, http::HeaderMap, routing::post};

    fn source() -> Value {
        json!({"title":"Google Maps","url":"https://www.google.com/maps/place/example"})
    }
    fn search_data() -> Value {
        json!({"summary":"Original summary [0]", "places":[{
            "id":"ChIJ-example_12", "place":"places/ChIJ-example_12",
            "location":{"latitude":55.0,"longitude":12.0},
            "attribution":source(), "googleMapsLinks":{"placeUrl":"https://www.google.com/maps/place/example"}
        }],"contentToken":"provider-token"})
    }
    fn route_data() -> Value {
        json!({"routes":[{"distanceMeters":1460,"duration":"1200s", "attribution":source(), "retrievalToken":"provider-token"}]})
    }
    #[derive(Default)]
    struct TestState {
        requests: Mutex<Vec<Value>>,
        headers: Mutex<Vec<HeaderMap>>,
    }

    async fn mock(
        State(state): State<Arc<TestState>>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> Json<Value> {
        state.requests.lock().unwrap().push(body.clone());
        state.headers.lock().unwrap().push(headers);
        let data = match body["params"]["name"].as_str() {
            Some("search_places") => search_data(),
            Some("compute_routes") => route_data(),
            _ => json!({}),
        };
        Json(json!({"jsonrpc":"2.0","id":1,"result":{"structuredContent":data}}))
    }

    #[tokio::test]
    async fn bounded_stateless_transport_preserves_sources_and_uses_walk() {
        let state = Arc::new(TestState::default());
        let app = Router::new()
            .route("/mcp", post(mock))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = MapsService::new(
            "local-test-key-123456789",
            Url::parse(&format!("http://{address}/mcp")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            client
                .search_places("craft beer in Malmö", None)
                .await
                .unwrap(),
            search_data()
        );
        assert_eq!(
            client
                .walking_route(
                    Waypoint::Named("Malmö C"),
                    Waypoint::PlaceId("ChIJ-example_12")
                )
                .await
                .unwrap(),
            route_data()
        );
        let requests = state.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0]["method"], "tools/call");
        assert_eq!(requests[0]["params"]["name"], "search_places");
        assert_eq!(
            requests[0]["params"]["arguments"]["text_query"],
            "craft beer in Malmö"
        );
        assert!(
            requests[0]["params"]["arguments"]
                .get("location_bias")
                .is_none()
        );
        assert_eq!(requests[1]["params"]["name"], "compute_routes");
        assert_eq!(requests[1]["params"]["arguments"]["travel_mode"], "WALK");
        assert_eq!(
            requests[1]["params"]["arguments"]["destination"]["place_id"],
            "ChIJ-example_12"
        );
        assert_eq!(
            state.headers.lock().unwrap()[0]["x-goog-api-key"],
            "local-test-key-123456789"
        );
        server.abort();
    }

    #[test]
    fn unsafe_links_and_malformed_source_data_are_rejected() {
        for url in [
            "http://www.google.com/maps/x",
            "https://www.google.com.evil.test/x",
            "https://www.google.com@evil.test/x",
            "https://evil.test/x",
            "javascript:alert(1)",
            "https://www.google.com:444/maps/x",
        ] {
            let mut data = search_data();
            data["places"][0]["googleMapsLinks"]["placeUrl"] = json!(url);
            assert!(validate_search(&data).is_err(), "accepted unsafe URL");
        }
        let mut missing_source = search_data();
        missing_source["places"][0]["attribution"] = Value::Null;
        assert!(validate_search(&missing_source).is_err());
        let mut missing_route_source = route_data();
        missing_route_source["routes"][0]["attribution"]["url"] = json!("https://evil.test/");
        assert!(validate_routes(&missing_route_source).is_err());
        assert!(waypoint(Waypoint::PlaceId("x/../evil")).is_err());
        assert!(checked_text("   ", 240).is_err());
        assert!(checked_text("x\ny", 240).is_err());
    }

    #[test]
    fn mcp_json_and_sse_preserve_unmodified_provider_content() {
        let envelope = json!({"jsonrpc":"2.0","id":1,"result":{"structuredContent":search_data()}});
        assert_eq!(
            decode_response(envelope.to_string().as_bytes()).unwrap(),
            search_data()
        );
        let sse = format!("event: message\ndata: {envelope}\n\n");
        assert_eq!(decode_response(sse.as_bytes()).unwrap(), search_data());
        let fallback = json!({"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":search_data().to_string()}]}});
        assert_eq!(
            decode_response(fallback.to_string().as_bytes()).unwrap(),
            search_data()
        );
        assert!(decode_response(br#"{"jsonrpc":"2.0","id":1,"error":{"code":-1}}"#).is_err());
        assert!(decode_response(br#"{"jsonrpc":"2.0","id":2,"result":{}}"#).is_err());
        assert!(decode_response(b"not json").is_err());
    }

    #[tokio::test]
    async fn oversized_bodies_and_redirects_fail_without_forwarding_the_key() {
        async fn oversized() -> String {
            "x".repeat(MAX_RESPONSE + 1)
        }
        async fn redirect() -> axum::response::Redirect {
            axum::response::Redirect::temporary("https://example.com/")
        }
        let app = Router::new()
            .route("/huge", post(oversized))
            .route("/redirect", post(redirect));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let key = "local-test-key-123456789";
        let huge =
            MapsService::new(key, Url::parse(&format!("http://{address}/huge")).unwrap()).unwrap();
        let redirect = MapsService::new(
            key,
            Url::parse(&format!("http://{address}/redirect")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            huge.search_places("Copenhagen bars", None)
                .await
                .unwrap_err(),
            "maps_response_too_large"
        );
        // 3xx isn't a success response even when the redirect body is parseable.
        assert!(
            redirect
                .search_places("Copenhagen bars", None)
                .await
                .is_err()
        );
        server.abort();
    }
}
